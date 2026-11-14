//! Background job runner. Jobs run on their own thread, report progress via a
//! [`JobCtl`] and send results back over a channel. The UI thread polls the
//! channel each frame and is woken with `request_repaint`.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use photovault::archive::import::{
    CollectedFiles, DateSuggestion, DupDecision, DupLocation, ImportResult,
};
use photovault::archive::integrity::IntegrityReport;
use photovault::archive::{Archive, CollectionEntry};
use photovault::backup::{BackupPlan, Manifest, RestoreReport};
use photovault::progress::JobCtl;

/// Reply channel for a duplicate question.
pub type DupReply = Sender<Option<(DupDecision, bool)>>;

/// Messages sent from jobs to the UI.
pub enum JobOutput {
    Opened {
        root: PathBuf,
        result: Result<Archive, String>,
        notice: Option<String>,
    },
    Analyzed {
        collected: CollectedFiles,
        suggestion: Option<DateSuggestion>,
        target: Option<uuid::Uuid>,
    },
    Imported {
        results: Vec<Result<ImportResult, String>>,
        unsupported: Vec<PathBuf>,
    },
    BackupPlanned {
        result: Result<BackupPlan, String>,
        base: Option<Manifest>,
        full_plan: Option<BackupPlan>,
    },
    BackupDone(Result<(PathBuf, Manifest, bool), String>),
    RestorePreview(Result<(PathBuf, Manifest), String>),
    Restored {
        result: Result<RestoreReport, String>,
        incremental: bool,
    },
    Integrity(IntegrityReport),
    Adopted(Result<CollectionEntry, String>),
    Exported(Result<PathBuf, String>),
    TrashSize(u64),
    /// Not final: the import job is waiting for an answer.
    DupQuestion {
        file: PathBuf,
        existing: DupLocation,
        reply: DupReply,
    },
}

impl JobOutput {
    fn is_final(&self) -> bool {
        !matches!(self, JobOutput::DupQuestion { .. })
    }
}

/// A job that is currently running.
pub struct RunningJob {
    pub id: u64,
    pub title: String,
    pub ctl: JobCtl,
    /// Shows the blocking progress modal.
    pub modal: bool,
    pub cancellable: bool,
}

/// Owns all running jobs.
pub struct JobRunner {
    pub running: Vec<RunningJob>,
    tx: Sender<(u64, JobOutput)>,
    rx: Receiver<(u64, JobOutput)>,
    next_id: u64,
}

impl Default for JobRunner {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            running: Vec::new(),
            tx,
            rx,
            next_id: 1,
        }
    }
}

/// Handle given to a job to send intermediate messages.
#[derive(Clone)]
pub struct JobSender {
    id: u64,
    tx: Sender<(u64, JobOutput)>,
    ctx: egui::Context,
}

impl JobSender {
    /// Sends a non-final message (e.g. a question) to the UI.
    pub fn send(&self, out: JobOutput) {
        let _ = self.tx.send((self.id, out));
        self.ctx.request_repaint();
    }
}

impl JobRunner {
    /// Starts a job.
    pub fn spawn(
        &mut self,
        ctx: &egui::Context,
        title: &str,
        modal: bool,
        cancellable: bool,
        f: impl FnOnce(&JobCtl, &JobSender) -> JobOutput + Send + 'static,
    ) {
        let id = self.next_id;
        self.next_id += 1;
        let repaint_ctx = ctx.clone();
        let ctl = JobCtl::with_notify(move || repaint_ctx.request_repaint());
        let sender = JobSender {
            id,
            tx: self.tx.clone(),
            ctx: ctx.clone(),
        };
        let job_ctl = ctl.clone();
        let spawn = std::thread::Builder::new()
            .name(format!("job-{id}"))
            .spawn(move || {
                let out = f(&job_ctl, &sender);
                let _ = sender.tx.send((sender.id, out));
                sender.ctx.request_repaint();
            });
        if let Err(e) = spawn {
            log::error!("failed to spawn job thread: {e}");
            return;
        }
        self.running.push(RunningJob {
            id,
            title: title.to_owned(),
            ctl,
            modal,
            cancellable,
        });
    }

    /// Collects finished results and questions.
    pub fn poll(&mut self) -> Vec<JobOutput> {
        let mut out = Vec::new();
        while let Ok((id, msg)) = self.rx.try_recv() {
            if msg.is_final() {
                self.running.retain(|j| j.id != id);
            }
            out.push(msg);
        }
        out
    }

    /// The job showing the progress modal, if any.
    pub fn modal_job(&self) -> Option<&RunningJob> {
        self.running.iter().find(|j| j.modal)
    }

    /// Whether any job is running.
    pub fn busy(&self) -> bool {
        self.running.iter().any(|j| j.modal)
    }
}
