//! Export a single collection as a standalone zip or a static HTML gallery.

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::archive::import::{MediaKind, media_kind};
use crate::archive::model::{COLLECTION_FILE, Collection, NOTES_FILE};
use crate::archive::naming;
use crate::backup::{BackupError, Result};
use crate::fsutil::{self, IO_BUF};
use crate::progress::JobCtl;

/// Exports one collection folder as a zip (`<folder>/…`).
pub fn export_collection_zip(collection_dir: &Path, dest: &Path, ctl: &JobCtl) -> Result<()> {
    let folder = collection_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = PathBuf::from(format!("{}.partial", dest.display()));
    let result = (|| {
        let file = File::create(&partial).map_err(BackupError::io(&partial))?;
        let mut zip = ZipWriter::new(BufWriter::new(file));
        let files: Vec<PathBuf> = fs::read_dir(collection_dir)
            .map_err(BackupError::io(collection_dir))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .filter(|e| !fsutil::is_hidden_name(&e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        let total: u64 = files
            .iter()
            .filter_map(|f| fs::metadata(f).ok())
            .map(|m| m.len())
            .sum();
        ctl.begin_phase("Exporting", files.len() as u64, total);
        let mut buf = vec![0u8; IO_BUF];
        for f in files {
            if ctl.is_cancelled() {
                return Err(BackupError::Cancelled);
            }
            let name = f
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            ctl.set_current(&name);
            let size = fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
            let text = name.ends_with(".json") || name.ends_with(".md");
            let opts = SimpleFileOptions::default()
                .compression_method(if text {
                    CompressionMethod::Deflated
                } else {
                    CompressionMethod::Stored
                })
                .large_file(size >= u32::MAX as u64);
            zip.start_file(format!("{folder}/{name}"), opts)?;
            let mut input = File::open(&f).map_err(BackupError::io(&f))?;
            loop {
                let n = input.read(&mut buf).map_err(BackupError::io(&f))?;
                if n == 0 {
                    break;
                }
                zip.write_all(&buf[..n])
                    .map_err(BackupError::io(&partial))?;
                ctl.add_bytes(n as u64);
            }
            ctl.inc_done();
        }
        let w = zip.finish()?;
        let file = w
            .into_inner()
            .map_err(|e| BackupError::io(&partial)(e.into_error()))?;
        file.sync_all().map_err(BackupError::io(&partial))?;
        Ok(())
    })();
    match result {
        Ok(()) => fs::rename(&partial, dest).map_err(BackupError::io(dest)),
        Err(e) => {
            let _ = fs::remove_file(&partial);
            Err(e)
        }
    }
}

/// Escapes text for HTML.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn notes_html(notes: &str) -> String {
    notes
        .split("\n\n")
        .filter(|p| !p.trim().is_empty())
        .map(|p| format!("<p>{}</p>", html_escape(p.trim()).replace('\n', "<br>")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Exports a static, offline HTML gallery into `dest_parent/<folder name>/`.
/// Returns the path of `index.html`.
pub fn export_collection_html(
    collection_dir: &Path,
    data: &Collection,
    dest_parent: &Path,
    ctl: &JobCtl,
) -> Result<PathBuf> {
    let base = naming::collection_folder_name(data.date, &data.title);
    let name = naming::unique_name(dest_parent, &base, &Default::default(), false);
    let out = dest_parent.join(name);
    let images = out.join("images");
    fs::create_dir_all(&images).map_err(BackupError::io(&images))?;
    ctl.begin_phase(
        "Exporting gallery",
        data.photos.len() as u64,
        data.total_bytes(),
    );
    let mut tiles = String::new();
    for p in &data.photos {
        if ctl.is_cancelled() {
            return Err(BackupError::Cancelled);
        }
        ctl.set_current(&p.file_name);
        let src = collection_dir.join(&p.file_name);
        if src.is_file() {
            let dst = images.join(&p.file_name);
            fs::copy(&src, &dst).map_err(BackupError::io(&src))?;
            ctl.add_bytes(p.size_bytes);
        }
        let href = format!("images/{}", url_escape(&p.file_name));
        let caption = html_escape(&p.caption);
        let file = html_escape(&p.file_name);
        if media_kind(Path::new(&p.file_name)) == MediaKind::Previewable {
            tiles.push_str(&format!(
                "<figure><a href=\"{href}\"><img loading=\"lazy\" src=\"{href}\" alt=\"{file}\"></a><figcaption>{caption}</figcaption></figure>\n"
            ));
        } else {
            let ext = html_escape(
                &crate::archive::import::extension_lower(Path::new(&p.file_name)).to_uppercase(),
            );
            tiles.push_str(&format!(
                "<figure><a class=\"file\" href=\"{href}\"><span>{ext}</span>{file}</a><figcaption>{caption}</figcaption></figure>\n"
            ));
        }
        ctl.inc_done();
    }
    for extra in [COLLECTION_FILE, NOTES_FILE] {
        let src = collection_dir.join(extra);
        if src.is_file() {
            let _ = fs::copy(&src, out.join(extra));
        }
    }
    let tags = if data.tags.is_empty() {
        String::new()
    } else {
        format!(
            "<div class=\"tags\">{}</div>",
            data.tags
                .iter()
                .map(|t| format!("<span>{}</span>", html_escape(t)))
                .collect::<String>()
        )
    };
    let html = format!(
        r#"<!doctype html>
<html lang="und" dir="auto">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light dark; --bg:#f6f5f3; --fg:#1d1d1f; --muted:#6b6b70; --card:#fff; --accent:#5b8def; }}
@media (prefers-color-scheme: dark) {{ :root {{ --bg:#16171a; --fg:#ececf1; --muted:#9a9aa3; --card:#212227; }} }}
body {{ margin:0; background:var(--bg); color:var(--fg); font:16px/1.6 system-ui, -apple-system, "Segoe UI", "Noto Sans", "Noto Sans Arabic", sans-serif; }}
main {{ max-width:1200px; margin:0 auto; padding:32px 16px 64px; }}
h1 {{ margin:0 0 4px; font-size:2rem; unicode-bidi:plaintext; }}
.date {{ color:var(--muted); }}
.tags span {{ display:inline-block; margin:8px 6px 0 0; padding:2px 10px; border-radius:999px; background:var(--card); color:var(--accent); font-size:.85rem; }}
.notes {{ margin:24px 0 32px; padding:20px 24px; background:var(--card); border-radius:14px; unicode-bidi:plaintext; }}
.notes p {{ unicode-bidi:plaintext; text-align:start; }}
.grid {{ display:grid; grid-template-columns:repeat(auto-fill, minmax(220px, 1fr)); gap:14px; }}
figure {{ margin:0; background:var(--card); border-radius:12px; overflow:hidden; }}
figure img {{ display:block; width:100%; aspect-ratio:1; object-fit:cover; }}
figcaption {{ padding:6px 10px; font-size:.85rem; color:var(--muted); unicode-bidi:plaintext; min-height:1em; }}
a.file {{ display:flex; flex-direction:column; align-items:center; justify-content:center; aspect-ratio:1; color:var(--fg); text-decoration:none; word-break:break-all; padding:8px; text-align:center; }}
a.file span {{ font-weight:700; color:var(--accent); margin-bottom:6px; }}
</style>
</head>
<body><main>
<h1 dir="auto">{title}</h1>
<div class="date">{date} · {count} photos</div>
{tags}
<section class="notes" dir="auto">{notes}</section>
<section class="grid">
{tiles}</section>
</main></body></html>
"#,
        title = html_escape(&data.title),
        date = data.date.format("%Y-%m-%d"),
        count = data.photos.len(),
        notes = notes_html(&data.notes),
    );
    let index = out.join("index.html");
    fsutil::atomic_write(&index, html.as_bytes()).map_err(BackupError::io(&index))?;
    Ok(index)
}

fn url_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
