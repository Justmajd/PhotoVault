//! Progress reporting and cancellation for long-running background jobs.
//!
//! The library never depends on the UI toolkit: a job reports through a
//! [`JobCtl`], and the UI installs a `notify` callback (typically
//! `ctx.request_repaint()`) to be woken up when progress changes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Snapshot of a job's progress.
#[derive(Clone, Debug)]
pub struct Progress {
    /// Short description of the current phase ("Copying", "Verifying", …).
    pub phase: String,
    /// The item currently being worked on (usually a file name).
    pub current: String,
    /// Items finished.
    pub done: u64,
    /// Items in total (0 if unknown).
    pub total: u64,
    /// Bytes processed.
    pub bytes_done: u64,
    /// Bytes in total (0 if unknown).
    pub bytes_total: u64,
    /// When the job started.
    pub started: Instant,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            phase: String::new(),
            current: String::new(),
            done: 0,
            total: 0,
            bytes_done: 0,
            bytes_total: 0,
            started: Instant::now(),
        }
    }
}

impl Progress {
    /// Fraction in `0..=1`, preferring the byte count when known.
    pub fn fraction(&self) -> f32 {
        if self.bytes_total > 0 {
            (self.bytes_done as f64 / self.bytes_total as f64).clamp(0.0, 1.0) as f32
        } else if self.total > 0 {
            (self.done as f64 / self.total as f64).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }

    /// Items per second since the job started.
    pub fn items_per_sec(&self) -> f64 {
        let secs = self.started.elapsed().as_secs_f64();
        if secs > 0.2 {
            self.done as f64 / secs
        } else {
            0.0
        }
    }

    /// Bytes per second since the job started.
    pub fn bytes_per_sec(&self) -> f64 {
        let secs = self.started.elapsed().as_secs_f64();
        if secs > 0.2 {
            self.bytes_done as f64 / secs
        } else {
            0.0
        }
    }
}

type Notify = Arc<dyn Fn() + Send + Sync>;

/// Shared handle for progress + cancellation. Cheap to clone.
#[derive(Clone)]
pub struct JobCtl {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<Progress>>,
    notify: Option<Notify>,
    last_notify: Arc<Mutex<Instant>>,
}

impl Default for JobCtl {
    fn default() -> Self {
        Self::new()
    }
}

impl JobCtl {
    /// A control handle without a wake-up callback (tests, CLI).
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(Progress::default())),
            notify: None,
            last_notify: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// A control handle that calls `notify` (rate-limited) when progress changes.
    pub fn with_notify(notify: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            notify: Some(Arc::new(notify)),
            ..Self::new()
        }
    }

    /// Request cancellation.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.wake(true);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn lock(&self) -> MutexGuard<'_, Progress> {
        self.progress.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Current progress snapshot.
    pub fn snapshot(&self) -> Progress {
        self.lock().clone()
    }

    /// Mutates the progress and wakes the UI (rate-limited to ~30 Hz).
    pub fn update(&self, f: impl FnOnce(&mut Progress)) {
        f(&mut self.lock());
        self.wake(false);
    }

    /// Starts a new phase, resetting counters.
    pub fn begin_phase(&self, phase: &str, total: u64, bytes_total: u64) {
        self.update(|p| {
            p.phase = phase.to_owned();
            p.current.clear();
            p.done = 0;
            p.total = total;
            p.bytes_done = 0;
            p.bytes_total = bytes_total;
            p.started = Instant::now();
        });
        self.wake(true);
    }

    /// Sets the item currently being processed.
    pub fn set_current(&self, current: &str) {
        self.update(|p| current.clone_into(&mut p.current));
    }

    /// Marks one item done.
    pub fn inc_done(&self) {
        self.update(|p| p.done += 1);
    }

    /// Adds processed bytes.
    pub fn add_bytes(&self, n: u64) {
        self.update(|p| p.bytes_done += n);
    }

    fn wake(&self, force: bool) {
        let Some(notify) = &self.notify else { return };
        let mut last = self.last_notify.lock().unwrap_or_else(|e| e.into_inner());
        if force || last.elapsed() >= Duration::from_millis(33) {
            *last = Instant::now();
            drop(last);
            notify();
        }
    }

    /// Unconditionally wake the UI.
    pub fn notify_now(&self) {
        self.wake(true);
    }
}
