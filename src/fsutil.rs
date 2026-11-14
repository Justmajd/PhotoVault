//! Small file-system helpers shared by the archive and backup modules.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use crate::progress::JobCtl;

/// Suffix used for every temporary file PhotoVault writes. Anything ending in
/// this suffix is garbage left behind by an interrupted write and is safe to
/// delete on the next scan.
pub const TEMP_SUFFIX: &str = ".pvtmp";

/// Buffer size used for streaming copies and hashing.
pub const IO_BUF: usize = 1024 * 1024;

/// Returns `true` for names that PhotoVault treats as hidden (dot files, OS junk).
pub fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case("Thumbs.db")
        || name.eq_ignore_ascii_case("desktop.ini")
        || name == "$RECYCLE.BIN"
}

/// Builds the temporary path used while writing `target`.
pub fn temp_path_for(target: &Path) -> PathBuf {
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = uuid::Uuid::new_v4().simple().to_string();
    target.with_file_name(format!(".{name}.{}{TEMP_SUFFIX}", &unique[..8]))
}

/// Returns `true` if `name` is a leftover temporary file.
pub fn is_temp_name(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(TEMP_SUFFIX)
}

/// Writes `bytes` to `path` atomically: temp file → fsync → rename → fsync dir.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = temp_path_for(path);
    let result = (|| {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)?;
        if let Some(parent) = path.parent() {
            sync_dir(parent);
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Best-effort fsync of a directory so that a rename is durable (Unix only).
pub fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    {
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
    }
}

/// Hashes a whole file with SHA-256 and returns the lowercase hex digest.
pub fn sha256_file(path: &Path, ctl: Option<&JobCtl>) -> io::Result<String> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; IO_BUF];
    loop {
        if let Some(ctl) = ctl
            && ctl.is_cancelled()
        {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        if let Some(ctl) = ctl {
            ctl.add_bytes(n as u64);
        }
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Hashes an in-memory byte slice.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Streams `src` into `dst` (a new file) while hashing it in a single pass.
/// The destination is fsynced. Returns `(sha256, bytes)`.
///
/// On cancellation or error the partially written destination is removed.
pub fn copy_and_hash(src: &Path, dst: &Path, ctl: Option<&JobCtl>) -> io::Result<(String, u64)> {
    let result = (|| {
        let mut input = File::open(src)?;
        let mut output = File::create_new(dst)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; IO_BUF];
        let mut total = 0u64;
        loop {
            if let Some(ctl) = ctl
                && ctl.is_cancelled()
            {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            output.write_all(&buf[..n])?;
            total += n as u64;
            if let Some(ctl) = ctl {
                ctl.add_bytes(n as u64);
            }
        }
        output.sync_all()?;
        Ok((hex::encode(hasher.finalize()), total))
    })();
    if result.is_err() {
        let _ = fs::remove_file(dst);
    }
    result
}

/// Modified time of a file as Unix seconds (0 if unavailable).
pub fn mtime_secs(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Sets the modified (and access) time of `path` to `secs` since the epoch.
pub fn set_mtime_secs(path: &Path, secs: i64) -> io::Result<()> {
    let t = filetime::FileTime::from_unix_time(secs, 0);
    filetime::set_file_times(path, t, t)
}

/// Total size of all regular files below `dir`.
pub fn dir_size(dir: &Path) -> u64 {
    walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}

/// Human readable byte size (`1.2 GB`).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Converts a relative path to a `/`-separated string.
pub fn to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Moves `src` to `dst`, falling back to copy + delete across devices.
pub fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if src.is_file() && e.kind() != io::ErrorKind::NotFound => {
            fs::copy(src, dst)?;
            if let Ok(meta) = fs::metadata(src) {
                let _ = set_mtime_secs(dst, mtime_secs(&meta));
            }
            fs::remove_file(src)
        }
        Err(e) => Err(e),
    }
}
