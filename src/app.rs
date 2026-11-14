//! Top-level application state, routing and action handling.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{Datelike, NaiveDate};
use eframe::egui::{self, Key};
use photovault::archive::import::{self, DupLocation, ImportResult};
use photovault::archive::integrity::{self, IntegrityReport};
use photovault::archive::model::STATE_DIR;
use photovault::archive::{Archive, FolderKind, TrashBatch};
use photovault::backup::{self, BackupOptions, BackupPlan, Manifest};
use photovault::config::{AppConfig, ThemeMode};
use photovault::search::SearchFilters;
use photovault::t;
use uuid::Uuid;

use crate::jobs::{DupReply, JobOutput, JobRunner};
use crate::theme;
use crate::thumbs::ThumbCache;
use crate::ui::toasts::{Toast, ToastKind};

/// Which page the central panel shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Home,
    Collection(Uuid),
    Search,
    Stats,
    Map,
    /// Developer-only bidi comparison page.
    BidiTest,
}

/// Sidebar filter chips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidebarFilter {
    All,
    Favorites,
    Tag(String),
}

/// New collection dialog state.
pub struct NewCollectionDialog {
    pub title: String,
    pub date: jiff::civil::Date,
    pub from_metadata: Option<bool>,
    pub files: Vec<PathBuf>,
    pub unsupported: Vec<PathBuf>,
    pub by_day: BTreeMap<NaiveDate, Vec<PathBuf>>,
    pub split: bool,
}

/// Backup dialog state.
pub struct BackupDialog {
    pub plan: BackupPlan,
    pub incremental_plan: Option<BackupPlan>,
    pub base: Option<Manifest>,
    pub dest: PathBuf,
    pub verify: bool,
    pub incremental: bool,
}

/// Restore dialog state.
pub struct RestoreDialog {
    pub zip: PathBuf,
    pub manifest: Manifest,
    pub dest: Option<PathBuf>,
}

/// Integrity report dialog state.
pub struct IntegrityDialog {
    pub report: IntegrityReport,
    pub confirm_hashes: bool,
}

/// Duplicate question asked by a running import.
pub struct DupDialog {
    pub file: PathBuf,
    pub existing: DupLocation,
    pub reply: DupReply,
    pub apply_all: bool,
}

/// Import summary.
pub struct ImportSummary {
    pub imported: usize,
    pub duplicates: Vec<PathBuf>,
    pub unsupported: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub cancelled: bool,
}

/// Modal dialogs (one at a time).
pub enum Dialog {
    NewCollection(Box<NewCollectionDialog>),
    ConfirmDelete {
        id: Uuid,
        typed: String,
    },
    ConfirmRemove {
        id: Uuid,
        names: Vec<String>,
    },
    Backup(Box<BackupDialog>),
    BackupDone {
        path: PathBuf,
        verified: bool,
    },
    Restore(Box<RestoreDialog>),
    RestoreDone {
        message: String,
    },
    ImportSummary(ImportSummary),
    Integrity(Box<IntegrityDialog>),
    Message {
        title: String,
        body: String,
        is_error: bool,
    },
    AdoptArchive {
        root: PathBuf,
        folders: usize,
        name: String,
    },
    CreateArchive {
        path: PathBuf,
        name: String,
    },
    Settings {
        trash_size: Option<u64>,
    },
    ArchiveSettings {
        name: String,
    },
    Shortcuts,
}

/// Photo viewer state.
pub struct ViewerState {
    pub id: Uuid,
    pub index: usize,
    pub zoom: f32,
    pub pan: egui::Vec2,
    pub move_target: Option<Uuid>,
    /// When the current photo appeared (fade-in).
    pub shown_at: Instant,
}

/// Slideshow state.
pub struct SlideshowState {
    pub id: Uuid,
    pub index: usize,
    pub playing: bool,
    pub last_switch: Instant,
}

/// Autosave bookkeeping for the collection editor.
#[derive(Default)]
pub struct EditState {
    /// Collections with unsaved changes → time of the last edit.
    pub dirty: HashMap<Uuid, Instant>,
    /// Collections whose folder should be renamed on the next save.
    pub sync_folder: HashSet<Uuid>,
    pub last_error: Option<String>,
    pub tag_input: String,
    pub notes_expanded: bool,
    /// RTL notes/titles are shown as bidi-correct text until clicked.
    pub notes_editing: bool,
    pub title_editing: bool,
    /// Focus the field on the next frame (after switching to edit mode).
    pub focus_notes: bool,
    pub focus_title: bool,
}

/// Update-checker state.
#[derive(Default)]
pub struct UpdateState {
    /// A check is running in the background.
    pub checking: bool,
    /// When the last check was started (drives the 24-hour interval).
    pub checked_at: Option<Instant>,
    /// A release newer than this build, if one was found.
    pub available: Option<photovault::update::Release>,
    /// Why the last check failed, if it did.
    pub error: Option<String>,
    /// Installer downloaded during this session.
    pub downloaded: Option<PathBuf>,
    /// Install the update as soon as the download finishes.
    pub install_after_download: bool,
    /// The installer is being applied in the background.
    pub installing: bool,
    /// The availability toast was already shown for `available`.
    pub notified: bool,
}

/// Deferred actions emitted while drawing.
pub enum Action {
    Select(Uuid),
    ShowView(View),
    OpenViewer {
        id: Uuid,
        index: usize,
    },
    NewCollection,
    ImportFiles,
    ImportFolder,
    ImportPaths(Vec<PathBuf>),
    AddPhotos(Uuid),
    Backup,
    Restore,
    OpenSettings,
    ToggleTheme,
    SetCover {
        id: Uuid,
        file: String,
    },
    RemovePhotos {
        id: Uuid,
        names: Vec<String>,
    },
    RemovePhotosConfirmed {
        id: Uuid,
        names: Vec<String>,
    },
    AskDeleteCollection(Uuid),
    DeleteCollection(Uuid),
    MovePhotos {
        from: Uuid,
        to: Uuid,
        names: Vec<String>,
    },
    Reorder {
        id: Uuid,
        file: String,
        to: usize,
    },
    Open(PathBuf),
    Reveal(PathBuf),
    OpenUrl(String),
    MarkDirty(Uuid),
    SyncFolder(Uuid),
    Undo(u64),
    CheckIntegrity,
    Rescan,
    CloseArchive,
    OpenArchive(PathBuf),
    PickArchiveFolder,
    CreateArchiveAt(PathBuf, String),
    ExportZip(Uuid),
    ExportHtml(Uuid),
    Slideshow(Uuid),
    AdoptFolder(PathBuf),
    ArchiveSettings,
    CheckForUpdates,
    DownloadUpdate {
        asset: photovault::update::ReleaseAsset,
        /// Install and restart after downloading instead of only saving it.
        install: bool,
    },
    InstallUpdate(PathBuf),
    Toast(String, ToastKind),
}

/// The application.
pub struct PhotoVaultApp {
    pub config: AppConfig,
    pub config_dirty: bool,
    pub archive: Option<Archive>,
    pub view: View,
    pub sidebar_filter: SidebarFilter,
    pub collapsed_years: HashSet<i32>,
    pub selection: HashSet<String>,
    pub anchor: Option<usize>,
    pub edit: EditState,
    pub thumbs: ThumbCache,
    pub jobs: JobRunner,
    pub dialog: Option<Dialog>,
    pub dup_question: Option<DupDialog>,
    pub viewer: Option<ViewerState>,
    pub slideshow: Option<SlideshowState>,
    pub toasts: Vec<Toast>,
    next_toast: u64,
    pub actions: Vec<Action>,
    pub update: UpdateState,
    pub search_query: String,
    pub search_filters: SearchFilters,
    pub search_from: jiff::civil::Date,
    pub search_to: jiff::civil::Date,
    pub search_use_from: bool,
    pub search_use_to: bool,
    pub focus_search: bool,
    pub reminder_dismissed: bool,
    pub fullscreen: bool,
    /// Text shown on the welcome screen while an archive opens.
    pub opening: Option<PathBuf>,
    pub new_archive_name: String,
    pub new_archive_path: PathBuf,
    pub dropped_hover: bool,
    /// Bumped on every mutation; invalidates cached search results.
    pub generation: u64,
    pub search_cache: Option<(
        String,
        SearchFilters,
        u64,
        Vec<photovault::search::SearchHit>,
    )>,
    /// Developer aid: `PHOTOVAULT_SCREENSHOT=out.png` saves a screenshot and exits.
    dev_shot: Option<DevShot>,
    /// View shown by the current transition animation.
    pub anim_view: Option<View>,
    pub view_changed_at: Instant,
    /// Visuals at the start of a theme cross-fade.
    pub theme_from: Option<(egui::Visuals, Instant)>,
    /// First-run introduction overlay.
    pub intro: Option<crate::ui::intro::IntroState>,
    /// When the welcome screen appeared (entrance animation).
    pub welcome_at: Instant,
    /// Last known inner window size (saved on exit).
    window_size: Option<[f32; 2]>,
}

/// State of the developer screenshot hook.
struct DevShot {
    path: PathBuf,
    view: String,
    frame: u32,
}

/// Converts a chrono date to the date picker's type.
pub fn to_jiff(d: NaiveDate) -> jiff::civil::Date {
    jiff::civil::Date::new(d.year() as i16, d.month() as i8, d.day() as i8).unwrap_or_default()
}

/// Converts the date picker's date to chrono.
pub fn from_jiff(d: jiff::civil::Date) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year() as i32, d.month() as u32, d.day() as u32).unwrap_or_default()
}

/// Today in local time.
pub fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn last_manifest_path(root: &Path) -> PathBuf {
    root.join(STATE_DIR).join("last-backup-manifest.json")
}

impl PhotoVaultApp {
    /// Creates the app and starts opening the last archive.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut config = AppConfig::load();
        // Developer overrides (used for screenshots).
        match std::env::var("PHOTOVAULT_LANG").as_deref() {
            Ok("ar") => config.language = photovault::config::Language::Ar,
            Ok("en") => config.language = photovault::config::Language::En,
            _ => {}
        }
        match std::env::var("PHOTOVAULT_THEME").as_deref() {
            Ok("light") => config.theme = ThemeMode::Light,
            Ok("dark") => config.theme = ThemeMode::Dark,
            _ => {}
        }
        photovault::i18n::set_language(config.language);
        theme::install_fonts(&cc.egui_ctx);
        theme::apply(&cc.egui_ctx, &config);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let mut app = Self {
            thumbs: ThumbCache::new(&cc.egui_ctx),
            config,
            config_dirty: false,
            archive: None,
            view: View::Home,
            sidebar_filter: SidebarFilter::All,
            collapsed_years: HashSet::new(),
            selection: HashSet::new(),
            anchor: None,
            edit: EditState::default(),
            jobs: JobRunner::default(),
            dialog: None,
            dup_question: None,
            viewer: None,
            slideshow: None,
            toasts: Vec::new(),
            next_toast: 1,
            actions: Vec::new(),
            update: UpdateState::default(),
            search_query: String::new(),
            search_filters: SearchFilters::default(),
            search_from: to_jiff(today() - chrono::Duration::days(365)),
            search_to: to_jiff(today()),
            search_use_from: false,
            search_use_to: false,
            focus_search: false,
            reminder_dismissed: false,
            fullscreen: false,
            opening: None,
            new_archive_name: photovault::i18n::tr_logical("welcome.default_name").to_owned(),
            new_archive_path: photovault::config::default_archive_path(),
            dropped_hover: false,
            generation: 0,
            search_cache: None,
            window_size: None,
            anim_view: None,
            view_changed_at: Instant::now(),
            theme_from: None,
            intro: None,
            welcome_at: Instant::now(),
            dev_shot: std::env::var_os("PHOTOVAULT_SCREENSHOT").map(|p| DevShot {
                path: PathBuf::from(p),
                view: std::env::var("PHOTOVAULT_VIEW").unwrap_or_default(),
                frame: 0,
            }),
        };
        if !app.config.intro_seen {
            app.intro = Some(crate::ui::intro::IntroState::new());
        }
        if let Some(last) = app.config.last_archive.clone()
            && last
                .join(photovault::archive::model::ARCHIVE_FILE)
                .is_file()
        {
            app.open_archive(&cc.egui_ctx, last);
        }
        app
    }

    pub fn accent(&self) -> egui::Color32 {
        theme::accent(&self.config)
    }

    pub fn mark_config_dirty(&mut self) {
        self.config_dirty = true;
    }

    /// Adds a toast notification.
    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) -> u64 {
        let id = self.next_toast;
        self.next_toast += 1;
        self.toasts.push(Toast::new(id, text.into(), kind));
        id
    }

    fn toast_undo(&mut self, text: String, batch: TrashBatch) {
        let id = self.toast(text, ToastKind::Info);
        if let Some(t) = self.toasts.iter_mut().find(|t| t.id == id) {
            t.undo = Some(batch);
            t.secs = 10.0;
        }
    }

    /// Shows an error whose body is raw (logical-order) text, e.g. an I/O error.
    fn error(&mut self, title: &str, body: impl Into<String>) {
        let body = photovault::i18n::display(&body.into());
        self.error_display(title, body);
    }

    /// Shows an error whose body is already display text (from `t!`).
    fn error_display(&mut self, title: &str, body: String) {
        self.dialog = Some(Dialog::Message {
            title: title.to_owned(),
            body,
            is_error: true,
        });
    }

    // ------------------------------------------------------------------
    // Archive lifecycle

    pub fn open_archive(&mut self, ctx: &egui::Context, root: PathBuf) {
        self.flush_saves();
        self.opening = Some(root.clone());
        self.jobs
            .spawn(ctx, t!("job.opening"), true, false, move |ctl, _| {
                let result = Archive::open(&root, Some(ctl)).map_err(|e| e.to_string());
                JobOutput::Opened {
                    root,
                    result,
                    notice: None,
                }
            });
    }

    fn set_archive(&mut self, archive: Archive) {
        self.thumbs.clear();
        self.generation += 1;
        self.config.remember_archive(&archive.root);
        self.mark_config_dirty();
        let scan = archive.last_scan.clone();
        self.archive = Some(archive);
        self.view = View::Home;
        self.selection.clear();
        self.viewer = None;
        self.slideshow = None;
        self.reminder_dismissed = false;
        if !scan.adopted_orphans.is_empty() {
            self.toast(
                t!("toast.adopted_orphans", n = scan.adopted_orphans.len()),
                ToastKind::Info,
            );
        }
        if scan.missing > 0 {
            self.toast(
                t!("toast.missing_files", n = scan.missing),
                ToastKind::Warning,
            );
        }
        for e in scan.errors.iter().take(3) {
            self.toast(photovault::bidi::visual_string(e), ToastKind::Error);
        }
    }

    fn pick_archive_folder(&mut self, ctx: &egui::Context) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title(photovault::i18n::tr_logical("dialog.pick_archive"))
            .pick_folder()
        else {
            return;
        };
        match Archive::inspect_folder(&dir) {
            FolderKind::Archive => self.open_archive(ctx, dir),
            FolderKind::Adoptable { folders } => {
                let name = dir
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::AdoptArchive {
                    root: dir,
                    folders,
                    name,
                });
            }
            FolderKind::Empty => {
                let name = photovault::i18n::tr_logical("welcome.default_name").to_owned();
                self.dialog = Some(Dialog::CreateArchive { path: dir, name });
            }
            FolderKind::Other => self.error_display(
                t!("error.not_archive_title"),
                t!("error.not_archive_body", path = dir.display()),
            ),
        }
    }

    pub fn create_archive(&mut self, ctx: &egui::Context, path: PathBuf, name: String) {
        match Archive::create(&path, name.trim()) {
            Ok(a) => {
                self.set_archive(a);
                self.toast(t!("toast.archive_created"), ToastKind::Success);
            }
            Err(photovault::archive::ArchiveError::AlreadyExists(p)) => self.open_archive(ctx, p),
            Err(e) => self.error(t!("error.create_archive"), e.to_string()),
        }
    }

    pub fn adopt_archive(&mut self, ctx: &egui::Context, root: PathBuf, name: String) {
        self.jobs
            .spawn(ctx, t!("job.adopting"), true, false, move |ctl, _| {
                let result = Archive::adopt_existing(&root, name.trim(), Some(ctl))
                    .map_err(|e| e.to_string());
                JobOutput::Opened {
                    root,
                    result,
                    notice: Some(t!("toast.archive_adopted").to_owned()),
                }
            });
    }

    // ------------------------------------------------------------------
    // Updates

    /// Starts a background check for a newer GitHub release.
    pub fn check_for_updates(&mut self, ctx: &egui::Context) {
        if self.update.checking {
            return;
        }
        self.update.checking = true;
        self.update.checked_at = Some(Instant::now());
        self.update.error = None;
        self.config.last_update_check = Some(chrono::Utc::now().timestamp());
        self.mark_config_dirty();
        self.jobs
            .spawn(ctx, t!("job.update_check"), false, false, |_, _| {
                JobOutput::UpdateChecked(
                    photovault::update::fetch_latest().map_err(|e| e.to_string()),
                )
            });
    }

    /// Checks automatically at most once every 24 hours, if enabled.
    fn maybe_check_updates(&mut self, ctx: &egui::Context) {
        const DAY: i64 = 24 * 60 * 60;
        if !self.config.check_updates || self.dev_shot.is_some() || self.update.checking {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        if self
            .config
            .last_update_check
            .is_none_or(|last| now.saturating_sub(last) >= DAY)
        {
            self.check_for_updates(ctx);
        }
    }

    /// Downloads the installer matching this platform.
    fn download_update(
        &mut self,
        ctx: &egui::Context,
        asset: photovault::update::ReleaseAsset,
        install: bool,
    ) {
        self.update.install_after_download = install;
        self.update.downloaded = None;
        let dir = photovault::update::update_dir();
        self.jobs
            .spawn(ctx, t!("job.update_download"), true, true, move |ctl, _| {
                JobOutput::UpdateDownloaded(
                    photovault::update::download(&asset, &dir, ctl).map_err(|e| e.to_string()),
                )
            });
    }

    /// Applies a downloaded installer in the background.
    fn install_update(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.update.installing = true;
        self.jobs
            .spawn(ctx, t!("job.update_install"), true, false, move |_, _| {
                JobOutput::UpdateInstalled(
                    photovault::update::install(&path).map_err(|e| e.to_string()),
                )
            });
    }

    /// Toast that offers to open the update section in Settings.
    fn toast_update(&mut self, version: &str) {
        let id = self.toast(
            t!("update.available_toast", version = version),
            ToastKind::Info,
        );
        if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) {
            toast.open_settings = true;
            toast.secs = 15.0;
        }
    }

    // ------------------------------------------------------------------
    // Saving

    /// Saves collections whose last edit is older than the debounce interval.
    fn autosave(&mut self, ctx: &egui::Context) {
        let due: Vec<Uuid> = self
            .edit
            .dirty
            .iter()
            .filter(|(_, t)| t.elapsed() >= Duration::from_millis(1000))
            .map(|(id, _)| *id)
            .collect();
        for id in due {
            self.save_now(id, false);
        }
        if !self.edit.dirty.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// Saves a collection immediately.
    pub fn save_now(&mut self, id: Uuid, sync_folder: bool) {
        self.edit.dirty.remove(&id);
        let sync = sync_folder || self.edit.sync_folder.remove(&id);
        let Some(archive) = self.archive.as_mut() else {
            return;
        };
        let old_dir = archive.collection_dir(id);
        match archive.save_collection(id, sync) {
            Ok(outcome) => {
                self.edit.last_error = None;
                if outcome.renamed
                    && let Some(old) = old_dir
                {
                    self.thumbs.forget_dir(&old);
                }
                if let Some(err) = outcome.rename_error {
                    log::warn!("folder rename failed: {err}");
                    self.toast(t!("toast.rename_failed", error = err), ToastKind::Warning);
                }
            }
            Err(e) => {
                log::error!("save failed: {e}");
                self.edit.last_error = Some(e.to_string());
                self.toast(t!("toast.save_failed", error = e), ToastKind::Error);
            }
        }
    }

    /// Saves everything that is pending (on view change, exit, …).
    pub fn flush_saves(&mut self) {
        let ids: Vec<Uuid> = self
            .edit
            .dirty
            .keys()
            .copied()
            .chain(self.edit.sync_folder.iter().copied())
            .collect();
        for id in ids {
            self.save_now(id, false);
        }
    }

    // ------------------------------------------------------------------
    // Import

    fn start_import_paths(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        target: Option<Uuid>,
    ) {
        if self.archive.is_none() || paths.is_empty() {
            return;
        }
        self.jobs
            .spawn(ctx, t!("job.analyzing"), true, true, move |ctl, _| {
                ctl.begin_phase("Analyzing", 0, 0);
                let collected = import::collect_import_files(&paths);
                let suggestion = if target.is_none() {
                    ctl.begin_phase("Reading photo dates", collected.files.len() as u64, 0);
                    import::suggest_date(&collected.files, Some(ctl))
                } else {
                    None
                };
                JobOutput::Analyzed {
                    collected,
                    suggestion,
                    target,
                }
            });
    }

    /// Starts copying files into collections.
    pub fn start_import(
        &mut self,
        ctx: &egui::Context,
        batches: Vec<(Uuid, Vec<PathBuf>)>,
        unsupported: Vec<PathBuf>,
    ) {
        let Some(archive) = self.archive.as_ref() else {
            return;
        };
        let mut requests = Vec::new();
        for (id, files) in batches {
            match archive.import_request(id, files) {
                Ok(r) => requests.push(r),
                Err(e) => {
                    self.error(t!("error.import"), e.to_string());
                    return;
                }
            }
        }
        self.jobs
            .spawn(ctx, t!("job.importing"), true, true, move |ctl, sender| {
                let mut results = Vec::new();
                for mut req in requests {
                    // Hashes imported by earlier batches count as "elsewhere".
                    for r in results
                        .iter()
                        .filter_map(|r: &Result<ImportResult, String>| r.as_ref().ok())
                    {
                        for p in &r.imported {
                            req.hashes_elsewhere
                                .entry(p.sha256.clone())
                                .or_insert_with(|| DupLocation {
                                    collection_id: r.collection_id.unwrap_or_default(),
                                    collection_title: String::new(),
                                    file_name: p.file_name.clone(),
                                });
                        }
                    }
                    let mut ask = |file: &Path, existing: &DupLocation| {
                        let (tx, rx) = std::sync::mpsc::channel();
                        sender.send(JobOutput::DupQuestion {
                            file: file.to_path_buf(),
                            existing: existing.clone(),
                            reply: tx,
                        });
                        loop {
                            match rx.recv_timeout(Duration::from_millis(200)) {
                                Ok(answer) => return answer,
                                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
                                    if !ctl.is_cancelled() =>
                                {
                                    continue;
                                }
                                _ => return None,
                            }
                        }
                    };
                    let r = import::import_files(&req, ctl, &mut ask).map_err(|e| e.to_string());
                    let cancelled = r.as_ref().map(|r| r.cancelled).unwrap_or(false);
                    results.push(r);
                    if cancelled {
                        break;
                    }
                }
                JobOutput::Imported {
                    results,
                    unsupported,
                }
            });
    }

    fn pick_import_files(&mut self, ctx: &egui::Context) {
        let exts: Vec<&str> = import::PREVIEW_EXTS
            .iter()
            .chain(import::ARCHIVE_ONLY_EXTS.iter())
            .copied()
            .collect();
        let upper: Vec<String> = exts.iter().map(|e| e.to_uppercase()).collect();
        let mut all: Vec<&str> = exts.clone();
        all.extend(upper.iter().map(String::as_str));
        if let Some(files) = rfd::FileDialog::new()
            .set_title(photovault::i18n::tr_logical("dialog.pick_photos"))
            .add_filter(photovault::i18n::tr_logical("dialog.photos_filter"), &all)
            .pick_files()
        {
            let target = self.current_collection();
            self.start_import_paths(ctx, files, target);
        }
    }

    fn pick_import_folder(&mut self, ctx: &egui::Context) {
        if let Some(dir) = rfd::FileDialog::new()
            .set_title(photovault::i18n::tr_logical("dialog.pick_folder"))
            .pick_folder()
        {
            let target = self.current_collection();
            self.start_import_paths(ctx, vec![dir], target);
        }
    }

    pub fn current_collection(&self) -> Option<Uuid> {
        match self.view {
            View::Collection(id) if self.archive.as_ref().is_some_and(|a| a.get(id).is_some()) => {
                Some(id)
            }
            _ => None,
        }
    }

    /// Creates the collection(s) from the New Collection dialog and imports files.
    pub fn confirm_new_collection(&mut self, ctx: &egui::Context, d: NewCollectionDialog) {
        let Some(archive) = self.archive.as_mut() else {
            return;
        };
        let title = if d.title.trim().is_empty() {
            photovault::i18n::tr_logical("collection.untitled").to_owned()
        } else {
            d.title.trim().to_owned()
        };
        let mut batches = Vec::new();
        if d.split && d.by_day.len() > 1 {
            for (day, files) in &d.by_day {
                match archive.create_collection(&title, *day) {
                    Ok(id) => batches.push((id, files.clone())),
                    Err(e) => {
                        self.error(t!("error.create_collection"), e.to_string());
                        return;
                    }
                }
            }
        } else {
            match archive.create_collection(&title, from_jiff(d.date)) {
                Ok(id) => batches.push((id, d.files.clone())),
                Err(e) => {
                    self.error(t!("error.create_collection"), e.to_string());
                    return;
                }
            }
        }
        if let Some((first, _)) = batches.first() {
            self.select(*first);
        }
        let has_files = batches.iter().any(|(_, f)| !f.is_empty());
        if has_files {
            self.start_import(ctx, batches, d.unsupported);
        } else {
            self.toast(t!("toast.collection_created"), ToastKind::Success);
        }
    }

    pub fn select(&mut self, id: Uuid) {
        self.flush_saves();
        if self.view != View::Collection(id) {
            self.selection.clear();
            self.anchor = None;
            self.edit.tag_input.clear();
            self.edit.notes_editing = false;
            self.edit.title_editing = false;
        }
        self.view = View::Collection(id);
    }

    // ------------------------------------------------------------------
    // Backup / restore

    fn start_backup(&mut self, ctx: &egui::Context) {
        self.flush_saves();
        let Some(archive) = self.archive.as_ref() else {
            return;
        };
        let root = archive.root.clone();
        let archive_id = archive.meta.archive_id;
        self.jobs
            .spawn(ctx, t!("job.planning"), true, false, move |_, _| {
                let base: Option<Manifest> = std::fs::read(last_manifest_path(&root))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Manifest>(&b).ok())
                    .filter(|m| m.archive_id == archive_id);
                let full = backup::plan_backup(&root, None).map_err(|e| e.to_string());
                let inc = base
                    .as_ref()
                    .and_then(|b| backup::plan_backup(&root, Some(b)).ok());
                JobOutput::BackupPlanned {
                    result: full,
                    base,
                    full_plan: inc,
                }
            });
    }

    pub fn run_backup(&mut self, ctx: &egui::Context, d: BackupDialog) {
        let Some(archive) = self.archive.as_ref() else {
            return;
        };
        let root = archive.root.clone();
        let meta = archive.meta.clone();
        let plan = if d.incremental {
            d.incremental_plan.unwrap_or(d.plan)
        } else {
            d.plan
        };
        let dest = d.dest;
        if let Some(parent) = dest.parent() {
            self.config.last_backup_dir = Some(parent.to_path_buf());
            self.mark_config_dirty();
        }
        let verify = d.verify;
        self.jobs
            .spawn(ctx, t!("job.backup"), true, true, move |ctl, _| {
                let result = backup::create_backup(
                    &root,
                    &meta,
                    &plan,
                    &dest,
                    &BackupOptions { verify },
                    ctl,
                )
                .map(|m| (dest, m, verify))
                .map_err(|e| e.to_string());
                JobOutput::BackupDone(result)
            });
    }

    fn start_restore(&mut self, ctx: &egui::Context) {
        let Some(zip) = rfd::FileDialog::new()
            .set_title(photovault::i18n::tr_logical("dialog.pick_backup"))
            .add_filter("Zip", &["zip", "ZIP"])
            .pick_file()
        else {
            return;
        };
        self.jobs
            .spawn(ctx, t!("job.reading_backup"), true, false, move |_, _| {
                JobOutput::RestorePreview(
                    backup::read_manifest(&zip)
                        .map(|m| (zip, m))
                        .map_err(|e| e.to_string()),
                )
            });
    }

    pub fn run_restore(&mut self, ctx: &egui::Context, d: RestoreDialog) {
        self.flush_saves();
        let incremental = d.manifest.kind == backup::BackupKind::Incremental;
        let zip = d.zip;
        if incremental {
            let Some(archive) = self.archive.as_ref() else {
                return;
            };
            let root = archive.root.clone();
            self.jobs
                .spawn(ctx, t!("job.restore"), true, true, move |ctl, _| {
                    let result =
                        backup::apply_incremental(&zip, &root, ctl).map_err(|e| e.to_string());
                    JobOutput::Restored {
                        result,
                        incremental: true,
                    }
                });
        } else {
            let Some(dest) = d.dest else { return };
            self.jobs
                .spawn(ctx, t!("job.restore"), true, true, move |ctl, _| {
                    let result =
                        backup::restore_backup(&zip, &dest, ctl).map_err(|e| e.to_string());
                    JobOutput::Restored {
                        result,
                        incremental: false,
                    }
                });
        }
    }

    fn start_integrity(&mut self, ctx: &egui::Context) {
        self.flush_saves();
        let Some(archive) = self.archive.as_ref() else {
            return;
        };
        let snapshot = integrity::snapshot(archive);
        self.jobs
            .spawn(ctx, t!("job.integrity"), true, true, move |ctl, _| {
                JobOutput::Integrity(integrity::check(&snapshot, ctl))
            });
    }

    pub fn compute_trash_size(&mut self, ctx: &egui::Context) {
        let Some(archive) = self.archive.as_ref() else {
            return;
        };
        let dir = archive.trash_dir();
        self.jobs.spawn(ctx, "trash", false, false, move |_, _| {
            JobOutput::TrashSize(photovault::fsutil::dir_size(&dir))
        });
    }

    // ------------------------------------------------------------------
    // Job results

    fn handle_jobs(&mut self, ctx: &egui::Context) {
        for out in self.jobs.poll() {
            match out {
                JobOutput::Opened {
                    root,
                    result,
                    notice,
                } => {
                    self.opening = None;
                    match result {
                        Ok(a) => {
                            self.set_archive(a);
                            if let Some(n) = notice {
                                self.toast(n, ToastKind::Success);
                            }
                        }
                        Err(e) => {
                            self.config.recent_archives.retain(|p| p != &root);
                            if self.config.last_archive.as_ref() == Some(&root) {
                                self.config.last_archive = None;
                            }
                            self.mark_config_dirty();
                            self.error(t!("error.open_archive"), e);
                        }
                    }
                }
                JobOutput::Analyzed {
                    collected,
                    suggestion,
                    target,
                } => {
                    if collected.files.is_empty() {
                        let body = if collected.unsupported.is_empty() {
                            t!("error.no_photos_body").to_owned()
                        } else {
                            t!("error.only_unsupported", n = collected.unsupported.len())
                        };
                        self.error_display(t!("error.no_photos_title"), body);
                        continue;
                    }
                    if let Some(id) = target {
                        self.start_import(ctx, vec![(id, collected.files)], collected.unsupported);
                    } else {
                        let (date, from_meta, by_day) = match suggestion {
                            Some(s) => (s.date, Some(s.from_metadata), s.by_day),
                            None => (today(), None, BTreeMap::new()),
                        };
                        self.dialog = Some(Dialog::NewCollection(Box::new(NewCollectionDialog {
                            title: collected.folder_name.clone().unwrap_or_default(),
                            date: to_jiff(date),
                            from_metadata: from_meta,
                            files: collected.files,
                            unsupported: collected.unsupported,
                            by_day,
                            split: false,
                        })));
                    }
                }
                JobOutput::Imported {
                    results,
                    unsupported,
                } => self.finish_import(results, unsupported),
                JobOutput::DupQuestion {
                    file,
                    existing,
                    reply,
                } => {
                    self.dup_question = Some(DupDialog {
                        file,
                        existing,
                        reply,
                        apply_all: false,
                    });
                }
                JobOutput::BackupPlanned {
                    result,
                    base,
                    full_plan,
                } => match result {
                    Ok(plan) => {
                        let dir = self
                            .config
                            .last_backup_dir
                            .clone()
                            .filter(|d| d.is_dir())
                            .or_else(dirs::document_dir)
                            .or_else(dirs::home_dir)
                            .unwrap_or_default();
                        let dest = dir.join(photovault::archive::naming::backup_file_name(
                            chrono::Local::now(),
                        ));
                        self.dialog = Some(Dialog::Backup(Box::new(BackupDialog {
                            plan,
                            incremental_plan: full_plan,
                            base,
                            dest,
                            verify: true,
                            incremental: false,
                        })));
                    }
                    Err(e) => self.error(t!("error.backup"), e),
                },
                JobOutput::BackupDone(result) => match result {
                    Ok((path, manifest, verified)) => {
                        if let Some(archive) = self.archive.as_mut() {
                            archive.meta.last_backup_at = Some(manifest.created_at);
                            if let Err(e) = archive.save_meta() {
                                log::error!("failed to record backup date: {e}");
                            }
                            let state = last_manifest_path(&archive.root);
                            if let Some(parent) = state.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            // Keep the full state so the next backup can be incremental.
                            let mut stored = manifest.clone();
                            if stored.kind == backup::BackupKind::Full {
                                stored.snapshot.clear();
                            }
                            if let Ok(bytes) = serde_json::to_vec(&stored) {
                                let _ = photovault::fsutil::atomic_write(&state, &bytes);
                            }
                        }
                        self.dialog = Some(Dialog::BackupDone { path, verified });
                    }
                    Err(e) if e == "cancelled" => {
                        self.toast(t!("toast.backup_cancelled"), ToastKind::Info);
                    }
                    Err(e) => self.error(t!("error.backup"), e),
                },
                JobOutput::RestorePreview(result) => match result {
                    Ok((zip, manifest)) => {
                        let dest = if manifest.kind == backup::BackupKind::Full {
                            Some(backup::resolve_destination(
                                &photovault::config::default_archive_path().with_file_name(
                                    photovault::i18n::tr_logical("restore.default_folder"),
                                ),
                            ))
                        } else {
                            None
                        };
                        self.dialog = Some(Dialog::Restore(Box::new(RestoreDialog {
                            zip,
                            manifest,
                            dest,
                        })));
                    }
                    Err(e) => self.error(t!("error.restore"), e),
                },
                JobOutput::Restored {
                    result,
                    incremental,
                } => match result {
                    Ok(report) => {
                        let message = t!(
                            "restore.success",
                            collections = report.collections,
                            photos = report.photos,
                            files = report.files
                        );
                        self.dialog = Some(Dialog::RestoreDone { message });
                        let root = report.root.clone();
                        self.open_archive(ctx, root);
                        let _ = incremental;
                    }
                    Err(e) if e == "cancelled" => {
                        self.toast(t!("toast.restore_cancelled"), ToastKind::Info);
                    }
                    Err(e) => {
                        self.error_display(t!("error.restore"), t!("error.restore_body", error = e))
                    }
                },
                JobOutput::Integrity(report) => {
                    if report.cancelled {
                        self.toast(t!("toast.integrity_cancelled"), ToastKind::Info);
                    } else {
                        self.dialog = Some(Dialog::Integrity(Box::new(IntegrityDialog {
                            report,
                            confirm_hashes: false,
                        })));
                    }
                }
                JobOutput::Adopted(result) => match result {
                    Ok(entry) => {
                        let id = entry.data.id;
                        if let Some(a) = self.archive.as_mut() {
                            a.last_scan.unadopted.retain(|p| p != &entry.rel_dir);
                            a.insert_entry(entry);
                        }
                        self.select(id);
                        self.toast(t!("toast.folder_adopted"), ToastKind::Success);
                    }
                    Err(e) => self.error(t!("error.adopt"), e),
                },
                JobOutput::Exported(result) => match result {
                    Ok(path) => {
                        let id = self.toast(t!("toast.exported"), ToastKind::Success);
                        if let Some(t) = self.toasts.iter_mut().find(|t| t.id == id) {
                            t.reveal = Some(path);
                            t.secs = 8.0;
                        }
                    }
                    Err(e) if e == "cancelled" => {}
                    Err(e) => self.error(t!("error.export"), e),
                },
                JobOutput::UpdateChecked(result) => {
                    self.update.checking = false;
                    match result {
                        Ok(release) => {
                            self.update.error = None;
                            if photovault::update::is_newer(
                                photovault::APP_VERSION,
                                &release.version,
                            ) {
                                let is_new = self
                                    .update
                                    .available
                                    .as_ref()
                                    .is_none_or(|r| r.version != release.version);
                                if is_new {
                                    self.update.notified = false;
                                    self.update.downloaded = None;
                                }
                                let version = release.version.clone();
                                self.update.available = Some(release);
                                if is_new && !self.update.notified {
                                    self.update.notified = true;
                                    self.toast_update(&version);
                                }
                            } else {
                                self.update.available = None;
                            }
                        }
                        Err(e) => {
                            log::warn!("update check failed: {e}");
                            self.update.error = Some(e);
                        }
                    }
                }
                JobOutput::UpdateDownloaded(result) => match result {
                    Ok(path) => {
                        self.update.downloaded = Some(path.clone());
                        if self.update.install_after_download {
                            self.update.install_after_download = false;
                            self.install_update(ctx, path);
                        } else {
                            let id = self.toast(t!("update.downloaded"), ToastKind::Success);
                            if let Some(toast) = self.toasts.iter_mut().find(|t| t.id == id) {
                                toast.reveal = Some(path);
                                toast.secs = 15.0;
                            }
                        }
                    }
                    Err(e) if e == "cancelled" => {
                        self.update.install_after_download = false;
                        self.toast(t!("update.cancelled"), ToastKind::Info);
                    }
                    Err(e) => {
                        self.update.install_after_download = false;
                        self.error(t!("update.download_failed"), e);
                    }
                },
                JobOutput::UpdateInstalled(result) => {
                    self.update.installing = false;
                    match result {
                        Ok(outcome) => {
                            // Persist settings and open collections before the
                            // window closes; the new process starts fresh.
                            self.flush_saves();
                            if let Err(e) = self.config.save() {
                                log::warn!("could not save config: {e}");
                            }
                            match photovault::update::restart(&outcome) {
                                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                                Err(e) => self.error(t!("update.install_failed"), e.to_string()),
                            }
                        }
                        Err(e) => self.error(t!("update.install_failed"), e),
                    }
                }
                JobOutput::TrashSize(n) => {
                    if let Some(Dialog::Settings { trash_size }) = &mut self.dialog {
                        *trash_size = Some(n);
                    }
                }
            }
        }
    }

    fn finish_import(
        &mut self,
        results: Vec<Result<ImportResult, String>>,
        unsupported: Vec<PathBuf>,
    ) {
        let mut summary = ImportSummary {
            imported: 0,
            duplicates: Vec::new(),
            unsupported,
            failed: Vec::new(),
            cancelled: false,
        };
        let mut show = None;
        for r in results {
            match r {
                Ok(r) => {
                    summary.imported += r.imported.len();
                    summary.duplicates.extend(r.duplicates_skipped);
                    summary.unsupported.extend(r.unsupported);
                    summary.failed.extend(r.failed);
                    summary.cancelled |= r.cancelled;
                    if show.is_none() {
                        show = r.show_existing;
                    }
                    if let (Some(id), Some(archive)) = (r.collection_id, self.archive.as_mut())
                        && let Err(e) = archive.add_photos(id, r.imported)
                    {
                        summary.failed.push((PathBuf::new(), e.to_string()));
                    }
                }
                Err(e) => summary.failed.push((PathBuf::new(), e)),
            }
        }
        if let Some(loc) = show {
            self.select(loc.collection_id);
            if let Some(idx) = self
                .archive
                .as_ref()
                .and_then(|a| a.get(loc.collection_id))
                .and_then(|c| c.data.photo_index(&loc.file_name))
            {
                self.viewer = Some(ViewerState {
                    id: loc.collection_id,
                    index: idx,
                    zoom: 1.0,
                    pan: egui::Vec2::ZERO,
                    move_target: None,
                    shown_at: Instant::now(),
                });
            }
        }
        self.dialog = Some(Dialog::ImportSummary(summary));
    }

    // ------------------------------------------------------------------
    // Actions

    fn apply_actions(&mut self, ctx: &egui::Context) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            self.apply(ctx, action);
        }
    }

    fn apply(&mut self, ctx: &egui::Context, action: Action) {
        self.generation += 1;
        match action {
            Action::Select(id) => self.select(id),
            Action::ShowView(v) => {
                self.flush_saves();
                self.view = v;
            }
            Action::OpenViewer { id, index } => {
                self.viewer = Some(ViewerState {
                    id,
                    index,
                    zoom: 1.0,
                    pan: egui::Vec2::ZERO,
                    move_target: None,
                    shown_at: Instant::now(),
                });
            }
            Action::NewCollection => {
                if self.archive.is_some() {
                    self.dialog = Some(Dialog::NewCollection(Box::new(NewCollectionDialog {
                        title: String::new(),
                        date: to_jiff(today()),
                        from_metadata: None,
                        files: Vec::new(),
                        unsupported: Vec::new(),
                        by_day: BTreeMap::new(),
                        split: false,
                    })));
                }
            }
            Action::ImportFiles => self.pick_import_files(ctx),
            Action::ImportFolder => self.pick_import_folder(ctx),
            Action::ImportPaths(paths) => {
                let target = self.current_collection();
                self.start_import_paths(ctx, paths, target);
            }
            Action::AddPhotos(id) => {
                self.select(id);
                self.pick_import_files(ctx);
            }
            Action::Backup => self.start_backup(ctx),
            Action::Restore => self.start_restore(ctx),
            Action::OpenSettings => {
                self.dialog = Some(Dialog::Settings { trash_size: None });
                self.compute_trash_size(ctx);
            }
            Action::ToggleTheme => {
                let dark_now = ctx.global_style().visuals.dark_mode;
                self.config.theme = if dark_now {
                    ThemeMode::Light
                } else {
                    ThemeMode::Dark
                };
                theme::apply(ctx, &self.config);
                self.mark_config_dirty();
            }
            Action::SetCover { id, file } => {
                if let Some(c) = self.archive.as_mut().and_then(|a| a.get_mut(id)) {
                    c.data.cover_photo = Some(file);
                    self.save_now(id, false);
                    self.toast(t!("toast.cover_set"), ToastKind::Success);
                }
            }
            Action::RemovePhotos { id, names } => {
                if names.is_empty() {
                    return;
                }
                if self.config.confirm_before_delete {
                    self.dialog = Some(Dialog::ConfirmRemove { id, names });
                } else {
                    self.apply(ctx, Action::RemovePhotosConfirmed { id, names });
                }
            }
            Action::RemovePhotosConfirmed { id, names } => {
                self.save_now(id, false);
                let Some(archive) = self.archive.as_mut() else {
                    return;
                };
                match archive.remove_photos(id, &names) {
                    Ok(batch) => {
                        for n in &names {
                            self.selection.remove(n);
                        }
                        if let Some(v) = &mut self.viewer {
                            let len = archive.get(id).map(|c| c.data.photos.len()).unwrap_or(0);
                            if len == 0 {
                                self.viewer = None;
                            } else {
                                v.index = v.index.min(len - 1);
                            }
                        }
                        self.toast_undo(t!("toast.photos_removed", n = names.len()), batch);
                    }
                    Err(e) => self.error(t!("error.remove"), e.to_string()),
                }
            }
            Action::AskDeleteCollection(id) => {
                self.dialog = Some(Dialog::ConfirmDelete {
                    id,
                    typed: String::new(),
                });
            }
            Action::DeleteCollection(id) => {
                self.edit.dirty.remove(&id);
                self.edit.sync_folder.remove(&id);
                let Some(archive) = self.archive.as_mut() else {
                    return;
                };
                let title = archive
                    .get(id)
                    .map(|c| c.data.title.clone())
                    .unwrap_or_default();
                match archive.delete_collection(id) {
                    Ok(batch) => {
                        self.view = View::Home;
                        self.viewer = None;
                        self.toast_undo(t!("toast.collection_deleted", title = title), batch);
                    }
                    Err(e) => self.error(t!("error.delete"), e.to_string()),
                }
            }
            Action::MovePhotos { from, to, names } => {
                self.save_now(from, false);
                self.save_now(to, false);
                let Some(archive) = self.archive.as_mut() else {
                    return;
                };
                match archive.move_photos(from, to, &names) {
                    Ok(n) => {
                        let title = archive
                            .get(to)
                            .map(|c| c.data.title.clone())
                            .unwrap_or_default();
                        for name in &names {
                            self.selection.remove(name);
                        }
                        if let Some(v) = &mut self.viewer {
                            let len = archive.get(from).map(|c| c.data.photos.len()).unwrap_or(0);
                            if len == 0 {
                                self.viewer = None;
                            } else {
                                v.index = v.index.min(len - 1);
                            }
                        }
                        self.toast(t!("toast.moved", n = n, title = title), ToastKind::Success);
                    }
                    Err(e) => self.error(t!("error.move"), e.to_string()),
                }
            }
            Action::Reorder { id, file, to } => {
                if let Some(archive) = self.archive.as_mut()
                    && let Err(e) = archive.reorder_photo(id, &file, to)
                {
                    self.toast(
                        photovault::bidi::visual_string(&e.to_string()),
                        ToastKind::Error,
                    );
                }
            }
            Action::Open(path) => {
                if let Err(e) = opener::open(&path) {
                    self.toast(t!("toast.open_failed", error = e), ToastKind::Error);
                }
            }
            Action::Reveal(path) => {
                if let Err(e) = opener::reveal(&path) {
                    self.toast(t!("toast.open_failed", error = e), ToastKind::Error);
                }
            }
            Action::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
            Action::MarkDirty(id) => {
                self.edit.dirty.insert(id, Instant::now());
            }
            Action::SyncFolder(id) => self.save_now(id, true),
            Action::Undo(toast_id) => {
                let batch = self
                    .toasts
                    .iter_mut()
                    .find(|t| t.id == toast_id)
                    .and_then(|t| t.undo.take());
                self.toasts.retain(|t| t.id != toast_id);
                if let (Some(batch), Some(archive)) = (batch, self.archive.as_mut()) {
                    let restored_collection = match &batch.kind {
                        photovault::archive::TrashKind::Collection { data, .. } => Some(data.id),
                        photovault::archive::TrashKind::Photos { .. } => None,
                    };
                    match archive.undo_trash(batch) {
                        Ok(()) => {
                            if let Some(id) = restored_collection {
                                self.select(id);
                            }
                            self.toast(t!("toast.restored"), ToastKind::Success);
                        }
                        Err(e) => self.error(t!("error.undo"), e.to_string()),
                    }
                }
            }
            Action::CheckIntegrity => self.start_integrity(ctx),
            Action::Rescan => {
                if let Some(root) = self.archive.as_ref().map(|a| a.root.clone()) {
                    self.open_archive(ctx, root);
                }
            }
            Action::CloseArchive => {
                self.flush_saves();
                self.archive = None;
                self.welcome_at = Instant::now();
                self.view = View::Home;
                self.viewer = None;
                self.slideshow = None;
                self.thumbs.clear();
            }
            Action::OpenArchive(path) => {
                if path
                    .join(photovault::archive::model::ARCHIVE_FILE)
                    .is_file()
                {
                    self.open_archive(ctx, path);
                } else {
                    self.config.recent_archives.retain(|p| p != &path);
                    self.mark_config_dirty();
                    self.error_display(
                        t!("error.open_archive"),
                        t!("error.archive_gone", path = path.display()),
                    );
                }
            }
            Action::PickArchiveFolder => self.pick_archive_folder(ctx),
            Action::CreateArchiveAt(path, name) => self.create_archive(ctx, path, name),
            Action::ExportZip(id) => {
                let Some(archive) = self.archive.as_ref() else {
                    return;
                };
                let Some(entry) = archive.get(id) else { return };
                let dir = archive.root.join(&entry.rel_dir);
                let default = format!("{}.zip", entry.folder_name());
                if let Some(dest) = rfd::FileDialog::new()
                    .set_file_name(&default)
                    .add_filter("Zip", &["zip"])
                    .save_file()
                {
                    self.jobs
                        .spawn(ctx, t!("job.export"), true, true, move |ctl, _| {
                            JobOutput::Exported(
                                photovault::export::export_collection_zip(&dir, &dest, ctl)
                                    .map(|_| dest)
                                    .map_err(|e| e.to_string()),
                            )
                        });
                }
            }
            Action::ExportHtml(id) => {
                let Some(archive) = self.archive.as_ref() else {
                    return;
                };
                let Some(entry) = archive.get(id) else { return };
                let dir = archive.root.join(&entry.rel_dir);
                let data = entry.data.clone();
                if let Some(dest) = rfd::FileDialog::new()
                    .set_title(photovault::i18n::tr_logical("dialog.pick_export_folder"))
                    .pick_folder()
                {
                    self.jobs
                        .spawn(ctx, t!("job.export"), true, true, move |ctl, _| {
                            JobOutput::Exported(
                                photovault::export::export_collection_html(&dir, &data, &dest, ctl)
                                    .map_err(|e| e.to_string()),
                            )
                        });
                }
            }
            Action::Slideshow(id) => {
                let has = self
                    .archive
                    .as_ref()
                    .and_then(|a| a.get(id))
                    .is_some_and(|c| !c.data.photos.is_empty());
                if has {
                    self.slideshow = Some(SlideshowState {
                        id,
                        index: 0,
                        playing: true,
                        last_switch: Instant::now(),
                    });
                    self.set_fullscreen(ctx, true);
                }
            }
            Action::AdoptFolder(rel) => {
                let Some(archive) = self.archive.as_ref() else {
                    return;
                };
                let root = archive.root.clone();
                self.jobs
                    .spawn(ctx, t!("job.adopting"), true, false, move |ctl, _| {
                        JobOutput::Adopted(
                            Archive::adopt_folder(&root, &rel, Some(ctl))
                                .map_err(|e| e.to_string()),
                        )
                    });
            }
            Action::ArchiveSettings => {
                if let Some(a) = &self.archive {
                    self.dialog = Some(Dialog::ArchiveSettings {
                        name: a.meta.name.clone(),
                    });
                }
            }
            Action::CheckForUpdates => self.check_for_updates(ctx),
            Action::DownloadUpdate { asset, install } => self.download_update(ctx, asset, install),
            Action::InstallUpdate(path) => self.install_update(ctx, path),
            Action::Toast(text, kind) => {
                self.toast(text, kind);
            }
        }
    }

    /// Applies the configured theme, cross-fading from the current colors.
    pub fn apply_theme_animated(&mut self, ctx: &egui::Context) {
        if self.config.animations {
            let from = ctx.global_style().visuals.clone();
            self.theme_from = Some((from, Instant::now()));
        }
        theme::apply(ctx, &self.config);
    }

    /// Advances the theme cross-fade (call once per frame).
    fn animate_theme(&mut self, ctx: &egui::Context) {
        let Some((from, start)) = &self.theme_from else {
            return;
        };
        let t = crate::anim::ease_out(crate::anim::progress(ctx, *start, 0.35, true));
        let dark = ctx.theme() == egui::Theme::Dark;
        let target = theme::visuals_for(dark, self.accent());
        if t >= 1.0 {
            self.theme_from = None;
            theme::apply(ctx, &self.config);
        } else {
            ctx.set_visuals_of(ctx.theme(), theme::lerp_visuals(from, &target, t));
        }
    }

    pub fn set_fullscreen(&mut self, ctx: &egui::Context, on: bool) {
        self.fullscreen = on;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(on));
    }

    // ------------------------------------------------------------------
    // Input

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let (hovered, dropped) =
            ctx.input(|i| (!i.raw.hovered_files.is_empty(), i.raw.dropped_files.clone()));
        self.dropped_hover = hovered && self.archive.is_some();
        if dropped.is_empty() || self.archive.is_none() || self.jobs.busy() {
            return;
        }
        let paths: Vec<PathBuf> = dropped
            .into_iter()
            .map(|f| f.path().to_path_buf())
            .filter(|p| !p.as_os_str().is_empty())
            .collect();
        if !paths.is_empty() {
            self.actions.push(Action::ImportPaths(paths));
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if self.archive.is_none()
            || self.jobs.busy()
            || self.dialog.is_some()
            || self.dup_question.is_some()
        {
            return;
        }
        if self.viewer.is_some() || self.slideshow.is_some() {
            return;
        }
        let typing = ctx.egui_wants_keyboard_input();
        let (cmd, keys) = ctx.input(|i| {
            let pressed = |k: Key| i.key_pressed(k);
            (
                i.modifiers.command,
                [
                    pressed(Key::N),
                    pressed(Key::I),
                    pressed(Key::F),
                    pressed(Key::B),
                    pressed(Key::A),
                    pressed(Key::Delete) || (i.modifiers.command && pressed(Key::Backspace)),
                    pressed(Key::Questionmark) || (i.modifiers.shift && pressed(Key::Slash)),
                    pressed(Key::Escape),
                ],
            )
        });
        let [n, i, f, b, a, del, help, esc] = keys;
        if cmd && n {
            self.actions.push(Action::NewCollection);
        }
        if cmd && i {
            self.actions.push(Action::ImportFiles);
        }
        if cmd && f {
            self.focus_search = true;
        }
        if cmd && b {
            self.actions.push(Action::Backup);
        }
        if typing {
            return;
        }
        if let Some(id) = self.current_collection() {
            if cmd
                && a
                && let Some(c) = self.archive.as_ref().and_then(|ar| ar.get(id))
            {
                self.selection = c.data.photos.iter().map(|p| p.file_name.clone()).collect();
            }
            if del && !self.selection.is_empty() {
                let names: Vec<String> = self.selection.iter().cloned().collect();
                self.actions.push(Action::RemovePhotos { id, names });
            }
            if esc {
                self.selection.clear();
            }
        }
        if help {
            self.dialog = Some(Dialog::Shortcuts);
        }
    }

    /// Drives the `PHOTOVAULT_SCREENSHOT` developer hook (used for README screenshots).
    fn dev_screenshot(&mut self, ctx: &egui::Context) {
        let Some(dev) = self.dev_shot.as_mut() else {
            return;
        };
        if self.archive.is_none() && self.opening.is_some() {
            ctx.request_repaint();
            return;
        }
        dev.frame += 1;
        ctx.request_repaint();
        let frame = dev.frame;
        let shot_path = dev.path.clone();
        if frame == 3 {
            let view = dev.view.clone();
            let (kind, arg) = view.split_once(':').unwrap_or((view.as_str(), ""));
            let find = |a: &Archive| {
                a.collections
                    .iter()
                    .find(|c| c.data.title.contains(arg))
                    .map(|c| c.data.id)
            };
            match kind {
                "collection" | "viewer" | "slideshow" => {
                    if let Some(id) = self.archive.as_ref().and_then(find) {
                        self.select(id);
                        if kind == "viewer" {
                            self.viewer = Some(ViewerState {
                                id,
                                index: 0,
                                zoom: 1.0,
                                pan: egui::Vec2::ZERO,
                                move_target: None,
                                shown_at: Instant::now(),
                            });
                        }
                        if kind == "slideshow" {
                            self.slideshow = Some(SlideshowState {
                                id,
                                index: 0,
                                playing: false,
                                last_switch: Instant::now(),
                            });
                        }
                    }
                }
                "stats" => self.view = View::Stats,
                "map" => self.view = View::Map,
                "bidi" => self.view = View::BidiTest,
                "search" => {
                    self.search_query = arg.to_owned();
                    self.view = View::Search;
                }
                "settings" => {
                    self.dialog = Some(Dialog::Settings {
                        trash_size: Some(0),
                    })
                }
                "shortcuts" => self.dialog = Some(Dialog::Shortcuts),
                "backup" => self.actions.push(Action::Backup),
                "integrity" => self.actions.push(Action::CheckIntegrity),
                "welcome" => {
                    self.archive = None;
                    self.welcome_at = Instant::now();
                }
                "intro" => self.intro = Some(crate::ui::intro::IntroState::new()),
                _ => {}
            }
        }
        if frame == 120 {
            let path = shot_path;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                egui::ScreenshotCallback::new(move |img| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                        let _ = buf.save(&path);
                    }
                    std::process::exit(0);
                }),
            ));
        }
    }

    pub fn backup_overdue(&self) -> Option<i64> {
        let archive = self.archive.as_ref()?;
        let days = self.config.backup_reminder_days;
        if days == 0 || archive.collections.is_empty() {
            return None;
        }
        match archive.meta.last_backup_at {
            None => Some(-1),
            Some(t) => {
                let age = (chrono::Utc::now() - t).num_days();
                (age >= days as i64).then_some(age)
            }
        }
    }
}

impl eframe::App for PhotoVaultApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_jobs(&ctx);
        self.maybe_check_updates(&ctx);
        self.thumbs.begin_frame(&ctx);
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);
        self.autosave(&ctx);
        self.dev_screenshot(&ctx);
        self.animate_theme(&ctx);
        let animations = self.config.animations;
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique("animations_enabled"), animations));
        if !self.fullscreen
            && let Some(r) = ctx.input(|i| i.viewport().inner_rect)
        {
            self.window_size = Some([r.width(), r.height()]);
        }

        if self.archive.is_none() {
            crate::ui::welcome::show(self, ui);
        } else if self.slideshow.is_some() {
            crate::ui::slideshow::show(self, ui);
        } else if self.viewer.is_some() {
            crate::ui::viewer::show(self, ui);
        } else {
            crate::ui::main_layout(self, ui);
        }
        crate::ui::dialogs::show(self, &ctx);
        crate::ui::intro::show(self, &ctx);
        crate::ui::toasts::show(self, &ctx);
        if self.dropped_hover {
            crate::ui::drop_overlay(self, &ctx);
        }
        self.apply_actions(&ctx);

        if self.config_dirty {
            self.config_dirty = false;
            if let Err(e) = self.config.save() {
                log::warn!("could not save config: {e}");
            }
        }
    }

    fn on_exit(&mut self) {
        self.flush_saves();
        if let Some(size) = self.window_size {
            self.config.window_size = Some(size);
        }
        if let Err(e) = self.config.save() {
            log::warn!("could not save config: {e}");
        }
    }
}
