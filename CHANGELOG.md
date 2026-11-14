# Changelog

All notable changes to PhotoVault are documented here. The project follows
[Semantic Versioning](https://semver.org/).

## [0.1.1] — 2026-11-14

### Packaging
- Linux releases now include an `.rpm` built with `cargo-generate-rpm` for
  Fedora, RHEL and openSUSE (dnf/zypper), and a `.pkg.tar.zst` built from
  `packaging/arch/PKGBUILD` in an official Arch Linux container for Arch,
  Manjaro and EndeavourOS (pacman), alongside the existing `.deb` and
  `.AppImage`.
- Both new packages declare GTK3 and libxkbcommon as dependencies.
- README has a per-system install table and package build instructions.

## [0.1.0] — 2026-11-14

First release.

### Archive
- Plain-folder archive (`photovault.json`, `YYYY/YYYY-MM-DD - Title/`,
  `collection.json`, generated `NOTES.md`) with forward-compatible JSON that
  preserves unknown fields.
- Cross-platform name sanitization that preserves Unicode, with collision suffixes.
- Atomic metadata writes; leftover temp files are cleaned on scan.
- Orphan adoption, missing-file flags and adoption of hand-made folders and archives.
- Folder rename/move when the title or date changes, with a retry when the folder is locked.
- Trash with 10-second undo; Empty trash in Settings.

### Import
- File picker, recursive folder import and drag and drop.
- Single-pass copy + SHA-256 with fsync and preserved mtimes; cancel-safe.
- Duplicate handling (same collection: skip; other collection: ask, with "apply to all").
- EXIF date/camera/GPS extraction, date and title suggestions, split per day.

### UI
- egui 0.37 UI: dark/light/system themes, 6 accent colors, timeline
  sidebar, collection editor with debounced auto-save, responsive thumbnail
  grid with multi-select and reordering, lightbox viewer, slideshow, search
  with highlighted snippets and filters, statistics, photo map list, On This
  Day, backup reminders, toasts and keyboard shortcuts.
- Bundled Noto Sans + Noto Sans Arabic.

### Languages, themes and animations
- Full Arabic interface (العربية) with a right-to-left mirrored layout,
  switchable from the welcome screen or Settings.
- A bidi layer on top of egui (Unicode Bidi Algorithm): mixed Arabic/Latin
  text, numbers and brackets display in the correct order, values are isolated
  inside sentences, RTL paragraphs wrap in reading order and are right-aligned.
- Arabic-first editor font for text fields; bidi-correct reading view for
  Arabic notes and titles.
- Animated light/dark theme cross-fade; theme can also follow the system.
- Animated welcome screen, a three-step first-run introduction (replayable
  from Settings), a time-of-day greeting, view transitions, card entrance and
  hover animations, thumbnail fade-in, segmented controls, toggles, toasts and
  viewer zoom/fade. An "Animations" setting turns them off.

### Backup & restore
- Streaming `.zip` backups with `README.txt` and `manifest.json`, stored
  photos and deflated text, zip64, `.partial` + rename, optional verification.
- Incremental backups and applying them during restore.
- Verified restore through a staging folder, with zip-slip protection and
  per-file error reporting.
- Integrity check with one-click fixes.
- Collection export as `.zip` or an offline HTML gallery.

### Tooling
- Unit and integration tests, clippy `-D warnings`, rustfmt.
- cargo-packager configuration and a GitHub Actions workflow producing
  `.dmg`, `.msi`/NSIS, portable `.zip`, `.deb` (apt), `.rpm` (dnf),
  `.pkg.tar.zst` (pacman) and `.AppImage`.

### Known limitations
- While *editing*, a line mixing Arabic with digits or Latin text can show
  those parts out of order (display is correct; see `docs/ui-toolkit-notes.md`).
- Merge-restore into an existing archive (MVP+) is not implemented yet;
  restore into an empty folder or apply incremental backups instead.
- HEIC/RAW/video files are archived and verified, but not previewed.
