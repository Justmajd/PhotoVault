//! Modal dialogs: new collection, confirmations, backup, restore, progress,
//! duplicates, import summary, integrity report and more.

use eframe::egui::{self, RichText};
use photovault::archive::import::DupDecision;
use photovault::backup::BackupKind;
use photovault::fsutil::human_bytes;
use photovault::t;

use crate::app::{Action, Dialog, PhotoVaultApp, from_jiff};
use crate::theme;

const W: f32 = 460.0;

fn modal(ctx: &egui::Context, id: &str, add: impl FnOnce(&mut egui::Ui)) -> bool {
    egui::Modal::new(egui::Id::unique(id))
        .show(ctx, |ui| {
            ui.set_width(W);
            ui.set_max_width(W);
            ui.with_layout(crate::ui::column_layout(), add);
        })
        .should_close()
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(20.0).strong());
    ui.add_space(6.0);
}

fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    let color = theme::muted(ui);
    crate::ui::bidi::para(ui, &text.into(), 14.5, Some(color));
}

fn buttons_right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.add_space(14.0);
    crate::ui::end_row(ui, add);
}

fn file_list(ui: &mut egui::Ui, id: &str, label: String, files: &[std::path::PathBuf]) {
    if files.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(label)
        .id_salt(id)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .show(ui, |ui| {
                    for f in files {
                        ui.label(RichText::new(f.display().to_string()).small());
                    }
                });
        });
}

pub fn show(app: &mut PhotoVaultApp, ctx: &egui::Context) {
    let accent = app.accent();

    // 1. Duplicate question from a running import (highest priority).
    if let Some(q) = app.dup_question.as_mut() {
        let mut answer: Option<DupDecision> = None;
        modal(ctx, "dup_question", |ui| {
            title(ui, t!("dup.title"));
            ui.label(t!(
                "dup.body",
                file = q.file.file_name().unwrap_or_default().to_string_lossy(),
                title = q.existing.collection_title
            ));
            muted(ui, t!("dup.existing", file = q.existing.file_name));
            ui.add_space(8.0);
            ui.checkbox(&mut q.apply_all, t!("dup.apply_all"));
            buttons_right(ui, |ui| {
                if ui.button(t!("dup.show")).clicked() {
                    answer = Some(DupDecision::ShowExisting);
                }
                if ui.button(t!("dup.import")).clicked() {
                    answer = Some(DupDecision::ImportAnyway);
                }
                if theme::primary_button(ui, accent, t!("dup.skip")).clicked() {
                    answer = Some(DupDecision::Skip);
                }
            });
        });
        if let Some(a) = answer
            && let Some(q) = app.dup_question.take()
        {
            let _ = q.reply.send(Some((a, q.apply_all)));
        }
        return;
    }

    // 2. Progress modal for the running job.
    if let Some(job) = app.jobs.modal_job() {
        let p = job.ctl.snapshot();
        let title_text = job.title.clone();
        let cancellable = job.cancellable;
        let ctl = job.ctl.clone();
        modal(ctx, "progress", |ui| {
            title(ui, &title_text);
            if !p.phase.is_empty() {
                muted(ui, p.phase.clone());
            }
            ui.add_space(6.0);
            let frac = p.fraction();
            let bar = egui::ProgressBar::new(frac)
                .show_percentage()
                .animate(frac == 0.0);
            ui.add(bar);
            ui.add_space(4.0);
            let current: String = p.current.chars().take(70).collect();
            ui.label(RichText::new(current).small());
            let mut stats = Vec::new();
            if p.total > 0 {
                stats.push(format!("{} / {}", p.done, p.total));
            }
            if p.items_per_sec() > 0.0 {
                stats.push(t!(
                    "progress.files_per_sec",
                    n = format!("{:.1}", p.items_per_sec())
                ));
            }
            if p.bytes_per_sec() > 0.0 {
                stats.push(format!("{}/s", human_bytes(p.bytes_per_sec() as u64)));
            }
            let parts: Vec<&str> = stats.iter().map(String::as_str).collect();
            muted(ui, crate::ui::bidi::join(&parts, " · "));
            if cancellable {
                buttons_right(ui, |ui| {
                    let cancelling = ctl.is_cancelled();
                    if ui
                        .add_enabled(
                            !cancelling,
                            egui::Button::new(if cancelling {
                                t!("progress.cancelling")
                            } else {
                                t!("progress.cancel")
                            }),
                        )
                        .clicked()
                    {
                        ctl.cancel();
                    }
                });
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        return;
    }

    let Some(mut dialog) = app.dialog.take() else {
        return;
    };
    let mut keep = true;
    match &mut dialog {
        Dialog::NewCollection(d) => {
            let mut create = false;
            let close = modal(ctx, "new_collection", |ui| {
                title(ui, t!("new.title"));
                if !d.files.is_empty() {
                    muted(ui, t!("new.files", n = d.files.len()));
                }
                ui.label(t!("new.name"));
                let r = ui.add(
                    crate::ui::field(&mut d.title, false, 14.5)
                        .hint_text(crate::ui::hint(t!("collection.title_hint"), 14.5))
                        .desired_width(f32::INFINITY),
                );
                if !r.has_focus()
                    && d.title.is_empty()
                    && !ui.ctx().memory(|m| m.focused().is_some())
                {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    create = true;
                }
                ui.add_space(6.0);
                crate::ui::row(ui, |ui| {
                    ui.label(t!("new.date"));
                    ui.add_enabled_ui(!d.split, |ui| {
                        ui.add(egui_extras::DatePickerButton::new(&mut d.date).id_salt("new_date"));
                    });
                });
                match d.from_metadata {
                    Some(true) => muted(ui, t!("new.suggested_meta")),
                    Some(false) => muted(ui, t!("new.suggested_mtime")),
                    None => {}
                }
                if d.by_day.len() > 1 {
                    ui.add_space(6.0);
                    ui.checkbox(&mut d.split, t!("new.split", n = d.by_day.len()));
                    if d.split {
                        let days: Vec<String> = d
                            .by_day
                            .iter()
                            .take(6)
                            .map(|(day, f)| format!("{} ({})", day.format("%Y-%m-%d"), f.len()))
                            .collect();
                        muted(
                            ui,
                            days.join(", ") + if d.by_day.len() > 6 { " …" } else { "" },
                        );
                    }
                }
                if !d.unsupported.is_empty() {
                    muted(ui, t!("new.unsupported", n = d.unsupported.len()));
                }
                buttons_right(ui, |ui| {
                    let label = if d.files.is_empty() {
                        t!("new.create")
                    } else {
                        t!("new.create_import")
                    };
                    if theme::primary_button(ui, accent, label).clicked() {
                        create = true;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
            if create {
                if let Dialog::NewCollection(d) = std::mem::replace(&mut dialog, Dialog::Shortcuts)
                {
                    app.confirm_new_collection(ctx, *d);
                }
                return;
            }
        }
        Dialog::ConfirmDelete { id, typed } => {
            let id = *id;
            let name = app
                .archive
                .as_ref()
                .and_then(|a| a.get(id))
                .map(|c| c.data.title.clone())
                .unwrap_or_default();
            let n = app
                .archive
                .as_ref()
                .and_then(|a| a.get(id))
                .map(|c| c.data.photos.len())
                .unwrap_or(0);
            let close = modal(ctx, "confirm_delete", |ui| {
                title(ui, t!("delete.title"));
                crate::ui::bidi::para(ui, &t!("delete.body", title = name, n = n), 14.5, None);
                muted(ui, t!("delete.trash_note"));
                ui.add_space(8.0);
                crate::ui::bidi::para(ui, &t!("delete.type", title = name), 14.5, None);
                ui.add(crate::ui::field(typed, false, 14.5).desired_width(f32::INFINITY));
                buttons_right(ui, |ui| {
                    let ok = typed.trim() == name.trim();
                    if theme::danger_button(ui, ok, t!("delete.confirm")).clicked() {
                        app.actions.push(Action::DeleteCollection(id));
                        keep = false;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::ConfirmRemove { id, names } => {
            let close = modal(ctx, "confirm_remove", |ui| {
                title(ui, &t!("remove.title", n = names.len()));
                muted(ui, t!("remove.body"));
                let mut dont_ask = !app.config.confirm_before_delete;
                if ui.checkbox(&mut dont_ask, t!("remove.dont_ask")).changed() {
                    app.config.confirm_before_delete = !dont_ask;
                    app.config_dirty = true;
                }
                buttons_right(ui, |ui| {
                    if theme::danger_button(ui, true, t!("remove.confirm")).clicked() {
                        app.actions.push(Action::RemovePhotosConfirmed {
                            id: *id,
                            names: names.clone(),
                        });
                        keep = false;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::Backup(d) => {
            let mut start = false;
            let close = modal(ctx, "backup", |ui| {
                title(ui, t!("backup.title"));
                let plan = if d.incremental {
                    d.incremental_plan.as_ref().unwrap_or(&d.plan)
                } else {
                    &d.plan
                };
                ui.label(t!(
                    "backup.contents",
                    collections = d.plan.collections,
                    photos = d.plan.photos,
                    size = human_bytes(d.plan.total_bytes)
                ));
                if d.incremental {
                    ui.label(
                        RichText::new(t!(
                            "backup.changed",
                            n = plan.included_files,
                            size = human_bytes(plan.included_bytes)
                        ))
                        .color(accent),
                    );
                }
                ui.add_space(8.0);
                ui.label(t!("backup.destination"));
                ui.add(
                    egui::Label::new(RichText::new(d.dest.display().to_string()).small()).wrap(),
                );
                if ui.button(t!("backup.change")).clicked() {
                    let mut dlg = rfd::FileDialog::new().add_filter("Zip", &["zip"]);
                    if let Some(name) = d.dest.file_name() {
                        dlg = dlg.set_file_name(name.to_string_lossy());
                    }
                    if let Some(parent) = d.dest.parent() {
                        dlg = dlg.set_directory(parent);
                    }
                    if let Some(p) = dlg.save_file() {
                        d.dest = if p.extension().is_some() {
                            p
                        } else {
                            p.with_extension("zip")
                        };
                    }
                }
                ui.add_space(6.0);
                ui.checkbox(&mut d.verify, t!("backup.verify"));
                ui.add_enabled_ui(d.incremental_plan.is_some(), |ui| {
                    ui.checkbox(&mut d.incremental, t!("backup.incremental"))
                        .on_disabled_hover_text(t!("backup.incremental_unavailable"));
                });
                if d.incremental
                    && let Some(b) = &d.base
                {
                    muted(
                        ui,
                        t!(
                            "backup.incremental_base",
                            date = b
                                .created_at
                                .with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M")
                        ),
                    );
                }
                muted(ui, t!("backup.note"));
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("backup.start")).clicked() {
                        start = true;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
            if start {
                if let Dialog::Backup(d) = std::mem::replace(&mut dialog, Dialog::Shortcuts) {
                    app.run_backup(ctx, *d);
                }
                return;
            }
        }
        Dialog::BackupDone { path, verified } => {
            let close = modal(ctx, "backup_done", |ui| {
                title(ui, t!("backup.done_title"));
                ui.label(if *verified {
                    t!("backup.done_verified")
                } else {
                    t!("backup.done")
                });
                muted(ui, path.display().to_string());
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.ok")).clicked() {
                        keep = false;
                    }
                    if ui.button(t!("viewer.show_in_folder")).clicked() {
                        app.actions.push(Action::Reveal(path.clone()));
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::Restore(d) => {
            let mut start = false;
            let current_id = app.archive.as_ref().map(|a| a.meta.archive_id);
            let close = modal(ctx, "restore", |ui| {
                title(ui, t!("restore.title"));
                let m = &d.manifest;
                egui::Grid::new("restore_preview")
                    .num_columns(2)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new(t!("restore.archive")).color(theme::muted(ui)));
                        ui.label(&m.archive_name);
                        ui.end_row();
                        ui.label(RichText::new(t!("restore.created")).color(theme::muted(ui)));
                        ui.label(
                            m.created_at
                                .with_timezone(&chrono::Local)
                                .format("%Y-%m-%d %H:%M")
                                .to_string(),
                        );
                        ui.end_row();
                        ui.label(RichText::new(t!("restore.source_os")).color(theme::muted(ui)));
                        ui.label(&m.source_os);
                        ui.end_row();
                        ui.label(RichText::new(t!("restore.contents")).color(theme::muted(ui)));
                        ui.label(t!(
                            "backup.contents",
                            collections = m.stats.collections,
                            photos = m.stats.photos,
                            size = human_bytes(m.stats.total_bytes)
                        ));
                        ui.end_row();
                        ui.label(RichText::new(t!("restore.kind")).color(theme::muted(ui)));
                        ui.label(if m.kind == BackupKind::Incremental {
                            t!("restore.kind_incremental")
                        } else {
                            t!("restore.kind_full")
                        });
                        ui.end_row();
                    });
                ui.add_space(10.0);
                let can_start = if m.kind == BackupKind::Incremental {
                    let matches = current_id == Some(m.archive_id);
                    if matches {
                        crate::ui::bidi::para(ui, t!("restore.incremental_apply"), 14.5, None);
                    } else {
                        crate::ui::bidi::para(
                            ui,
                            t!("restore.incremental_wrong"),
                            14.5,
                            Some(theme::AMBER),
                        );
                    }
                    matches
                } else {
                    ui.label(t!("restore.destination"));
                    if let Some(dest) = &d.dest {
                        ui.add(
                            egui::Label::new(RichText::new(dest.display().to_string()).small())
                                .wrap(),
                        );
                    }
                    if ui.button(t!("restore.choose")).clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title(t!("restore.choose"))
                            .pick_folder()
                    {
                        let resolved = photovault::backup::resolve_destination(&dir);
                        d.dest = Some(resolved);
                    }
                    if let Some(dest) = &d.dest
                        && dest.file_name().is_some_and(|n| {
                            n.to_string_lossy()
                                .starts_with(photovault::backup::restore::RESTORED_SUBFOLDER)
                        })
                    {
                        muted(ui, t!("restore.subfolder_note"));
                    }
                    muted(ui, t!("restore.verify_note"));
                    d.dest.is_some()
                };
                buttons_right(ui, |ui| {
                    if ui
                        .add_enabled(
                            can_start,
                            egui::Button::new(RichText::new(t!("restore.start")).strong()),
                        )
                        .clicked()
                    {
                        start = true;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
            if start {
                if let Dialog::Restore(d) = std::mem::replace(&mut dialog, Dialog::Shortcuts) {
                    app.run_restore(ctx, *d);
                }
                return;
            }
        }
        Dialog::RestoreDone { message } => {
            let close = modal(ctx, "restore_done", |ui| {
                title(ui, t!("restore.done_title"));
                ui.label(RichText::new(message.as_str()).color(theme::SUCCESS));
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.ok")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::ImportSummary(s) => {
            let close = modal(ctx, "import_summary", |ui| {
                title(
                    ui,
                    if s.cancelled {
                        t!("summary.cancelled")
                    } else {
                        t!("summary.title")
                    },
                );
                ui.label(
                    RichText::new(t!("summary.imported", n = s.imported)).color(theme::SUCCESS),
                );
                if !s.duplicates.is_empty() {
                    ui.label(t!("summary.duplicates", n = s.duplicates.len()));
                }
                if !s.unsupported.is_empty() {
                    ui.label(t!("summary.unsupported", n = s.unsupported.len()));
                }
                if !s.failed.is_empty() {
                    ui.colored_label(theme::DANGER, t!("summary.failed", n = s.failed.len()));
                }
                file_list(
                    ui,
                    "dups",
                    t!("summary.show_duplicates").to_owned(),
                    &s.duplicates,
                );
                file_list(
                    ui,
                    "unsup",
                    t!("summary.show_unsupported").to_owned(),
                    &s.unsupported,
                );
                if !s.failed.is_empty() {
                    egui::CollapsingHeader::new(t!("summary.show_failed")).show(ui, |ui| {
                        for (p, e) in &s.failed {
                            ui.label(RichText::new(format!("{}: {e}", p.display())).small());
                        }
                    });
                }
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.ok")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::Integrity(d) => {
            let close = modal(ctx, "integrity", |ui| {
                title(ui, t!("integrity.title"));
                let r = &d.report;
                if r.is_clean() {
                    ui.label(RichText::new(t!("integrity.clean", n = r.ok)).color(theme::SUCCESS));
                } else {
                    ui.label(t!(
                        "integrity.summary",
                        ok = r.ok,
                        modified = r.modified.len(),
                        missing = r.missing.len(),
                        orphans = r.orphans.len()
                    ));
                }
                let list =
                    |ui: &mut egui::Ui,
                     id: &str,
                     label: String,
                     items: &[photovault::archive::integrity::IntegrityItem]| {
                        if items.is_empty() {
                            return;
                        }
                        egui::CollapsingHeader::new(label)
                            .id_salt(id)
                            .show(ui, |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(140.0)
                                    .show(ui, |ui| {
                                        for it in items {
                                            ui.label(
                                                RichText::new(crate::ui::bidi::vis(&format!(
                                                    "{} / {}",
                                                    it.collection_title, it.file_name
                                                )))
                                                .small(),
                                            );
                                        }
                                    });
                            });
                    };
                list(
                    ui,
                    "mod",
                    t!("integrity.modified", n = r.modified.len()),
                    &r.modified,
                );
                list(
                    ui,
                    "miss",
                    t!("integrity.missing", n = r.missing.len()),
                    &r.missing,
                );
                list(
                    ui,
                    "orph",
                    t!("integrity.orphans", n = r.orphans.len()),
                    &r.orphans,
                );
                ui.add_space(8.0);
                let mut fixed = None;
                if let Some(archive) = app.archive.as_mut() {
                    crate::ui::row_wrapped(ui, |ui| {
                        if !r.orphans.is_empty() && ui.button(t!("integrity.adopt")).clicked() {
                            fixed = Some(
                                archive
                                    .fix_adopt_orphans(&r.orphans)
                                    .map(|n| ("orphans", n)),
                            );
                        }
                        if !r.missing.is_empty() && ui.button(t!("integrity.drop")).clicked() {
                            fixed =
                                Some(archive.fix_drop_missing(&r.missing).map(|n| ("missing", n)));
                        }
                        if !r.modified.is_empty() {
                            if d.confirm_hashes {
                                if theme::danger_button(ui, true, t!("integrity.confirm_hashes"))
                                    .clicked()
                                {
                                    fixed = Some(
                                        archive
                                            .fix_update_hashes(&r.modified)
                                            .map(|n| ("modified", n)),
                                    );
                                }
                            } else if ui
                                .button(t!("integrity.update_hashes"))
                                .on_hover_text(t!("integrity.update_hashes_tip"))
                                .clicked()
                            {
                                d.confirm_hashes = true;
                            }
                        }
                    });
                }
                match fixed {
                    Some(Ok((kind, n))) => {
                        app.actions.push(Action::Toast(
                            t!("integrity.fixed", n = n),
                            crate::ui::toasts::ToastKind::Success,
                        ));
                        match kind {
                            "orphans" => d.report.orphans.clear(),
                            "missing" => d.report.missing.clear(),
                            _ => {
                                d.report.ok += d.report.modified.len();
                                d.report.modified.clear();
                                d.confirm_hashes = false;
                            }
                        }
                    }
                    Some(Err(e)) => app.actions.push(Action::Toast(
                        photovault::bidi::visual_string(&e.to_string()),
                        crate::ui::toasts::ToastKind::Error,
                    )),
                    None => {}
                }
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.close")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::Message {
            title: t,
            body,
            is_error,
        } => {
            let close = modal(ctx, "message", |ui| {
                title(ui, t);
                if *is_error {
                    crate::ui::bidi::para(ui, body, 14.5, Some(theme::DANGER));
                } else {
                    crate::ui::bidi::para(ui, body, 14.5, None);
                }
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.ok")).clicked() {
                        keep = false;
                    }
                    if *is_error && ui.button(t!("common.copy")).clicked() {
                        ui.ctx().copy_text(body.clone());
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::AdoptArchive {
            root,
            folders,
            name,
        } => {
            let close = modal(ctx, "adopt_archive", |ui| {
                title(ui, t!("adopt.title"));
                crate::ui::bidi::para(ui, &t!("adopt.body", n = *folders), 14.5, None);
                muted(ui, root.display().to_string());
                ui.add_space(6.0);
                ui.label(t!("welcome.archive_name"));
                ui.add(crate::ui::field(name, false, 14.5).desired_width(f32::INFINITY));
                muted(ui, t!("adopt.note"));
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("adopt.confirm")).clicked() {
                        app.adopt_archive(ctx, root.clone(), name.clone());
                        keep = false;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::CreateArchive { path, name } => {
            let close = modal(ctx, "create_archive", |ui| {
                title(ui, t!("create.title"));
                muted(ui, path.display().to_string());
                ui.label(t!("welcome.archive_name"));
                ui.add(crate::ui::field(name, false, 14.5).desired_width(f32::INFINITY));
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("welcome.create_button")).clicked() {
                        app.actions
                            .push(Action::CreateArchiveAt(path.clone(), name.clone()));
                        keep = false;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::Settings { trash_size } => {
            let close = modal(ctx, "settings", |ui| {
                crate::ui::settings::show(app, ui, *trash_size, &mut keep);
            });
            if close {
                keep = false;
            }
        }
        Dialog::ArchiveSettings { name } => {
            let close = modal(ctx, "archive_settings", |ui| {
                title(ui, t!("archive_settings.title"));
                ui.label(t!("welcome.archive_name"));
                ui.add(crate::ui::field(name, false, 14.5).desired_width(f32::INFINITY));
                if let Some(a) = &app.archive {
                    muted(ui, t!("archive_settings.location", path = a.root.display()));
                    muted(
                        ui,
                        t!(
                            "archive_settings.created",
                            date = a.meta.created_at.format("%Y-%m-%d")
                        ),
                    );
                    muted(ui, t!("archive_settings.id", id = a.meta.archive_id));
                }
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.save")).clicked() {
                        if let Some(a) = app.archive.as_mut() {
                            a.meta.name = name.trim().to_owned();
                            if let Err(e) = a.save_meta() {
                                app.actions.push(Action::Toast(
                                    photovault::bidi::visual_string(&e.to_string()),
                                    crate::ui::toasts::ToastKind::Error,
                                ));
                            }
                        }
                        keep = false;
                    }
                    if ui.button(t!("common.cancel")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
        Dialog::Shortcuts => {
            let close = modal(ctx, "shortcuts", |ui| {
                title(ui, t!("shortcuts.title"));
                let cmd = if cfg!(target_os = "macos") {
                    "⌘"
                } else {
                    "Ctrl"
                };
                egui::Grid::new("shortcuts_grid")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        for (k, v) in [
                            (format!("{cmd}+N"), t!("shortcuts.new")),
                            (format!("{cmd}+I"), t!("shortcuts.import")),
                            (format!("{cmd}+F"), t!("shortcuts.search")),
                            (format!("{cmd}+B"), t!("shortcuts.backup")),
                            (format!("{cmd}+A"), t!("shortcuts.select_all")),
                            (
                                format!("{cmd}+{}", t!("shortcuts.click")),
                                t!("shortcuts.toggle"),
                            ),
                            (
                                format!("Shift+{}", t!("shortcuts.click")),
                                t!("shortcuts.range"),
                            ),
                            ("Del".to_owned(), t!("shortcuts.remove")),
                            ("← →".to_owned(), t!("shortcuts.navigate")),
                            ("F".to_owned(), t!("shortcuts.fullscreen")),
                            ("Esc".to_owned(), t!("shortcuts.close")),
                            ("Space".to_owned(), t!("shortcuts.pause")),
                            ("?".to_owned(), t!("shortcuts.help")),
                        ] {
                            ui.label(RichText::new(k).monospace().strong());
                            ui.label(v);
                            ui.end_row();
                        }
                    });
                buttons_right(ui, |ui| {
                    if theme::primary_button(ui, accent, t!("common.close")).clicked() {
                        keep = false;
                    }
                });
            });
            if close {
                keep = false;
            }
        }
    }
    if keep && app.dialog.is_none() {
        app.dialog = Some(dialog);
    }
    let _ = from_jiff;
}
