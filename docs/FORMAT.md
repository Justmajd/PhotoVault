# PhotoVault file formats

This document specifies every file PhotoVault reads or writes, so that an
archive or backup can be understood and recovered by anyone without the app.
All JSON is UTF-8, pretty-printed and written atomically
(temp file → fsync → rename).

## Versioning rules

* Every format carries an integer `format_version` (the *major* version).
* Readers **must ignore unknown fields** and **must preserve them** when
  rewriting a file (PhotoVault keeps them in an internal `extra` map).
* Missing optional fields take the defaults listed below.
* A reader **must refuse** a file whose `format_version` is greater than the
  newest version it understands, with a message asking the user to update.
* Adding optional fields does not change the version. Removing or changing the
  meaning of a field bumps `format_version`.

Current versions: archive `1`, collection `1`, backup `1`.

---

## 1. Archive layout

```
<Archive Root>/
├── photovault.json                 archive marker (§2)
├── YYYY/
│   └── YYYY-MM-DD - <Title>/       one folder per collection
│       ├── collection.json         source of truth (§3)
│       ├── NOTES.md                generated mirror (§4), never read back
│       └── <photo files>           original files, byte-for-byte copies
├── .trash/<timestamp>/…            removed files (excluded from backups)
└── .photovault-state/…             app state, e.g. last backup manifest (excluded)
```

* A folder is recognized as a year folder when its name is exactly four digits.
* Any file or folder whose name starts with `.` is ignored by scans and backups.
* Files named `.<name>.<8 hex>.pvtmp` are leftovers of an interrupted write and
  are deleted on the next scan.

### Folder naming

`YYYY-MM-DD - <Sanitized Title>`, inside `YYYY/`.

Sanitization of the title:

1. Control characters are removed (tabs/newlines count as spaces).
2. `< > : " / \ | ? *` are removed and act as word separators.
3. Runs of whitespace collapse to one space.
4. The result is cut to 80 characters (Unicode scalar values, never mid-character).
5. Leading/trailing dots and spaces are trimmed (Windows forbids trailing ones).
6. Empty → `Untitled`.
7. Windows device names (`CON PRN AUX NUL COM1–9 LPT1–9`, case-insensitive,
   also with an extension) get `_` appended to the stem: `CON` → `CON_`.

Unicode (Arabic, emoji, accents) is preserved exactly. On a name collision,
` (2)`, ` (3)`, … is appended (before the extension for files). Imported file
names are sanitized the same way (120-character stem limit).

## 2. `photovault.json`

| Field | Type | Required | Notes |
|---|---|---|---|
| `format` | string | yes | always `"photovault-archive"` |
| `format_version` | integer | yes | `1` |
| `archive_id` | UUID string | yes | stable identity; used to match incremental backups |
| `name` | string | yes | display name |
| `created_at` | RFC 3339 UTC | yes | |
| `app_version` | string | no | version that created the archive |
| `last_backup_at` | RFC 3339 UTC | no | set after a successful backup |

```json
{
  "format": "photovault-archive",
  "format_version": 1,
  "archive_id": "8d0c3f9e-3a0e-4bb2-9a55-5c2f86d1e5b1",
  "name": "My Photo Archive",
  "created_at": "2026-11-14T18:30:00Z",
  "app_version": "0.1.0",
  "last_backup_at": "2026-11-14T21:30:00Z"
}
```

## 3. `collection.json`

| Field | Type | Default | Notes |
|---|---|---|---|
| `format_version` | integer | `1` | |
| `id` | UUID | — (required) | duplicated ids (copied folders) are re-assigned on scan |
| `title` | string | `""` | |
| `date` | `YYYY-MM-DD` | — (required) | |
| `notes` | string | `""` | free text, unlimited length |
| `tags` | string[] | `[]` | |
| `favorite` | bool | `false` | |
| `cover_photo` | string | absent | a `file_name` from `photos` |
| `created_at`, `updated_at` | RFC 3339 UTC | now | |
| `photos` | Photo[] | `[]` | order = display order |

Photo:

| Field | Type | Default | Notes |
|---|---|---|---|
| `file_name` | string | — (required) | name inside the collection folder |
| `original_name` | string | `""` | name at import time |
| `sha256` | hex string | `""` | SHA-256 of the file contents |
| `size_bytes` | integer | `0` | |
| `taken_at` | `YYYY-MM-DDTHH:MM:SS` | absent | EXIF `DateTimeOriginal`, local time, no zone |
| `camera` | string | absent | EXIF make + model |
| `gps` | `{ "lat": f64, "lon": f64 }` | absent | decimal degrees |
| `width`, `height` | integer | absent | displayed size (EXIF orientation applied) |
| `caption` | string | `""` | |
| `favorite` | bool | `false` | |
| `added_at` | RFC 3339 UTC | now | |

### Scan rules

* A media file in the folder that is not listed is an **orphan**: it is hashed,
  appended to `photos` and the JSON is saved.
* A listed file that does not exist is flagged **missing** in the UI; the entry
  is kept until the user drops it (Integrity check).
* A `YYYY/…` folder with media but no `collection.json` can be **adopted**: the
  date and title are parsed from `YYYY-MM-DD - Title`, otherwise taken from the
  earliest photo date and the folder name.

## 4. `NOTES.md`

Regenerated on every save, never read back:

```
# <title>
Date: YYYY-MM-DD
Tags: a, b            (omitted without tags)

<notes>

## Photos
- <file_name> — <caption>   (" — caption" omitted when empty)
```

## 5. Backup zip

```
PhotoVault-Backup-YYYY-MM-DD_HHMM.zip
├── README.txt        plain-English recovery instructions
├── archive/…         mirror of the archive root (minus dot folders / temp files)
└── manifest.json     written last
```

* Entry names always use `/` and are UTF-8 (the zip UTF-8 flag is set for
  non-ASCII names).
* Media and other binary files are `Stored`; `.json .md .txt .csv .xmp` are
  `Deflated`.
* Entries ≥ 4 GiB use zip64 (`large_file`); the archive itself switches to
  zip64 automatically when needed.
* The zip is written to `<dest>.partial` and renamed when complete.

### `manifest.json`

| Field | Type | Notes |
|---|---|---|
| `format` | string | `"photovault-backup"` |
| `format_version` | integer | `1` |
| `app_version` | string | |
| `created_at` | RFC 3339 UTC | |
| `source_os` | string | `windows`, `macos`, `linux` |
| `archive_id`, `archive_name` | | copied from `photovault.json` |
| `stats` | `{collections, photos, total_bytes}` | of the whole archive |
| `files` | ManifestFile[] | every file **contained in this zip** under `archive/` |
| `kind` | `"full"` \| `"incremental"` | default `"full"` |
| `base_created_at` | RFC 3339 UTC | incremental only: previous backup in the chain |
| `snapshot` | ManifestFile[] | incremental only: every file of the archive at backup time |

ManifestFile: `{ "path": "archive/2026/…/IMG_0412.jpg", "size": 4821331, "sha256": "hex", "mtime": 1731600000 }`
(`mtime` in Unix seconds).

### Restore rules

1. `format` must be `photovault-backup`; a newer `format_version` is refused.
2. Every entry name is validated **before anything is written**: no absolute
   paths, drive letters, backslashes, NUL bytes or `..` components, and
   `ZipFile::enclosed_name()` must accept it (zip-slip protection).
3. Entries are extracted to `<dest>/.photovault-restoring/`, hashed while
   streaming and compared with the manifest (size + SHA-256). An entry under
   `archive/` that is not in the manifest is an error, as is a manifest file
   missing from the zip.
4. If any file fails, the staging folder is deleted, the destination is left as
   it was and every failing path is reported.
5. Otherwise file mtimes are restored and the top-level items are renamed into
   the destination.

### Incremental backups

An incremental backup stores only files whose size or mtime changed since the
previous backup (whose manifest is kept in `.photovault-state/`). To restore:
restore the full backup into an empty folder, then apply each incremental zip
in order onto that archive. Applying replaces changed files and moves files
that are not in `snapshot` to `.trash/` (nothing is deleted outright).
