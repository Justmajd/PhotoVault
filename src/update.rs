//! Update checks against GitHub Releases.
//!
//! The app asks the GitHub API for the latest published release, compares its
//! version with the running build and can download the installer for the
//! current platform. It never replaces the running binary itself: installers
//! are saved to the downloads folder and opened by the user. That keeps
//! package-manager installs (apt, dnf, pacman) under the package manager's
//! control.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::progress::JobCtl;

/// Repository whose releases are checked.
const API_LATEST: &str = "https://api.github.com/repos/Justmajd/PhotoVault/releases/latest";
const USER_AGENT: &str = concat!("PhotoVault/", env!("CARGO_PKG_VERSION"));
/// Upper bound for the release JSON; it is normally a few kilobytes.
const MAX_JSON: u64 = 2 * 1024 * 1024;

/// A published release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading `v`, e.g. `0.1.2`.
    pub version: String,
    /// HTML page of the release on GitHub.
    pub url: String,
    /// Release notes in Markdown.
    pub body: String,
    /// RFC 3339 publication timestamp, when GitHub provides it.
    pub published_at: Option<String>,
    pub assets: Vec<ReleaseAsset>,
}

/// One downloadable file attached to a release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("could not reach GitHub: {0}")]
    Http(#[from] minreq::Error),
    #[error("GitHub returned HTTP {0}")]
    HttpStatus(u16),
    #[error("invalid release JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("release tag {0:?} is not a version")]
    InvalidVersion(String),
    #[error("release URL is not HTTPS")]
    InsecureUrl,
    #[error("unsafe asset file name")]
    UnsafeFileName,
    #[error("cancelled")]
    Cancelled,
    #[error("download is incomplete: expected {expected} bytes, got {got}")]
    SizeMismatch { expected: u64, got: u64 },
    #[error("this update cannot be installed automatically")]
    UnsupportedInstall,
    #[error("update installer failed: {0}")]
    Installer(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// What the app has to do after staging an update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    /// The running copy was replaced; start `exe` and quit.
    Restart { exe: PathBuf },
    /// An installer was started; quit and let it finish.
    InstallerStarted,
}

/// Whether `latest` is a newer version than `current`.
///
/// Both may have a leading `v`; pre-release and build suffixes are ignored
/// (the GitHub `latest` endpoint does not return pre-releases).
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

fn parse_version(version: &str) -> Option<[u64; 3]> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let core = version.split(['-', '+']).next().unwrap_or(version);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some([major, minor, patch])
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Fetches the latest release from GitHub.
pub fn fetch_latest() -> Result<Release, UpdateError> {
    let response = minreq::get(API_LATEST)
        .with_header("User-Agent", USER_AGENT)
        .with_header("Accept", "application/vnd.github+json")
        .with_timeout(20)
        .send_lazy()?;
    if response.status_code != 200 {
        return Err(UpdateError::HttpStatus(response.status_code));
    }
    let mut body = String::new();
    response.take(MAX_JSON).read_to_string(&mut body)?;
    let api: ApiRelease = serde_json::from_str(&body)?;
    let version = api
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_owned();
    if parse_version(&version).is_none() {
        return Err(UpdateError::InvalidVersion(api.tag_name));
    }
    if !api.html_url.starts_with("https://") {
        return Err(UpdateError::InsecureUrl);
    }
    let assets = api
        .assets
        .into_iter()
        .filter(|a| a.browser_download_url.starts_with("https://"))
        .map(|a| ReleaseAsset {
            name: a.name,
            url: a.browser_download_url,
            size: a.size,
        })
        .collect();
    Ok(Release {
        version,
        url: api.html_url,
        body: api.body,
        published_at: api.published_at,
        assets,
    })
}

/// The installer that fits the running platform, if the release has one.
pub fn asset_for_current_platform(release: &Release) -> Option<&ReleaseAsset> {
    let appimage = std::env::var_os("APPIMAGE").is_some();
    asset_for(release, std::env::consts::OS, appimage)
}

fn asset_for<'a>(release: &'a Release, os: &str, appimage: bool) -> Option<&'a ReleaseAsset> {
    let suffixes: &[&str] = match os {
        "macos" => &[".dmg"],
        "windows" => &["setup.exe", ".msi"],
        "linux" if appimage => &[".appimage"],
        _ => &[],
    };
    suffixes.iter().find_map(|suffix| {
        release
            .assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(suffix))
    })
}

/// Whether this copy is managed by a Linux package manager (or built from
/// source) rather than running as an AppImage. Such copies should be updated
/// through the package manager instead of downloading a second copy.
pub fn managed_by_package_manager() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("APPIMAGE").is_none()
}

/// Folder where downloaded installers are stored.
pub fn update_dir() -> PathBuf {
    dirs::download_dir()
        .unwrap_or_else(crate::config::config_dir)
        .join("PhotoVault Updates")
}

fn safe_file_name(name: &str) -> Option<&str> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return None;
    }
    Some(name)
}

/// Downloads an asset into `dir`, reporting byte progress through `ctl`.
pub fn download(asset: &ReleaseAsset, dir: &Path, ctl: &JobCtl) -> Result<PathBuf, UpdateError> {
    if !asset.url.starts_with("https://") {
        return Err(UpdateError::InsecureUrl);
    }
    let name = safe_file_name(&asset.name).ok_or(UpdateError::UnsafeFileName)?;
    std::fs::create_dir_all(dir)?;
    let final_path = dir.join(name);
    let part_path = dir.join(format!("{name}.part"));

    let mut response = minreq::get(&asset.url)
        .with_header("User-Agent", USER_AGENT)
        .with_header("Accept", "application/octet-stream")
        .with_timeout(120)
        .send_lazy()?;
    if response.status_code != 200 {
        return Err(UpdateError::HttpStatus(response.status_code));
    }

    ctl.begin_phase("", 0, asset.size);
    let mut file = std::io::BufWriter::new(std::fs::File::create(&part_path)?);
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        if ctl.is_cancelled() {
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            return Err(UpdateError::Cancelled);
        }
        let n = response.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        ctl.add_bytes(n as u64);
    }
    file.flush()?;
    let file = file
        .into_inner()
        .map_err(|e| UpdateError::Io(e.into_error()))?;
    file.sync_all()?;
    drop(file);

    if asset.size > 0 {
        let got = std::fs::metadata(&part_path)?.len();
        if got != asset.size {
            let _ = std::fs::remove_file(&part_path);
            return Err(UpdateError::SizeMismatch {
                expected: asset.size,
                got,
            });
        }
    }
    std::fs::rename(&part_path, &final_path)?;
    #[cfg(unix)]
    if name.to_ascii_lowercase().ends_with(".appimage") {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&final_path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(final_path)
}

/// Whether this copy can install updates itself: a macOS `.app` bundle, a
/// Windows build, or a Linux AppImage. Package-managed Linux copies cannot.
pub fn can_self_install() -> bool {
    #[cfg(target_os = "macos")]
    let supported = macos_bundle().is_some();
    #[cfg(target_os = "windows")]
    let supported = true;
    #[cfg(target_os = "linux")]
    let supported = linux_appimage().is_some();
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let supported = false;
    supported
}

/// Installs a downloaded release asset and reports how to restart.
///
/// macOS: mounts the `.dmg`, copies the contained `.app` over the running
/// bundle (keeping a backup until the swap succeeds).
/// Windows: hands the installer to `cmd`, which waits and then reopens the app.
/// Linux: replaces the running AppImage in place.
pub fn install(downloaded: &Path) -> Result<InstallOutcome, UpdateError> {
    #[cfg(target_os = "macos")]
    {
        let bundle = macos_bundle().ok_or(UpdateError::UnsupportedInstall)?;
        install_macos(downloaded, &bundle)
    }
    #[cfg(target_os = "windows")]
    {
        install_windows(downloaded)
    }
    #[cfg(target_os = "linux")]
    {
        install_linux(downloaded)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = downloaded;
        Err(UpdateError::UnsupportedInstall)
    }
}

/// Launches the freshly installed copy.
pub fn restart(outcome: &InstallOutcome) -> Result<(), UpdateError> {
    match outcome {
        InstallOutcome::InstallerStarted => Ok(()),
        InstallOutcome::Restart { exe } => {
            #[cfg(target_os = "macos")]
            if exe.extension().and_then(|e| e.to_str()) == Some("app") {
                let status = std::process::Command::new("open")
                    .arg("-n")
                    .arg(exe)
                    .status()?;
                if status.success() {
                    return Ok(());
                }
                return Err(UpdateError::Installer(
                    "could not reopen the app".to_owned(),
                ));
            }
            std::process::Command::new(exe).spawn()?;
            Ok(())
        }
    }
}

/// The `.app` bundle containing the running executable, if any.
#[cfg(target_os = "macos")]
fn macos_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundle = exe.ancestors().find(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    })?;
    // Apps launched straight from a mounted disk image or Downloads are
    // translocated to a read-only shadow path; updating that copy would be
    // pointless. Ask the user to move the app to /Applications first.
    if bundle
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return None;
    }
    Some(bundle.to_path_buf())
}

/// The running AppImage, when this is an AppImage build.
#[cfg(target_os = "linux")]
fn linux_appimage() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("APPIMAGE")?);
    path.is_file().then_some(path)
}

#[cfg(target_os = "macos")]
fn install_macos(dmg: &Path, bundle: &Path) -> Result<InstallOutcome, UpdateError> {
    if !dmg
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("dmg"))
    {
        return Err(UpdateError::UnsupportedInstall);
    }
    let mount = std::env::temp_dir().join(format!("photovault-mount-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&mount);
    std::fs::create_dir_all(&mount)?;

    let attach = std::process::Command::new("hdiutil")
        .args([
            "attach",
            "-nobrowse",
            "-noautoopen",
            "-readonly",
            "-noverify",
        ])
        .arg("-mountpoint")
        .arg(&mount)
        .arg(dmg)
        .output()?;
    if !attach.status.success() {
        let _ = std::fs::remove_dir_all(&mount);
        return Err(UpdateError::Installer(format!(
            "could not mount the disk image: {}",
            String::from_utf8_lossy(&attach.stderr).trim()
        )));
    }

    let result = copy_bundle_from_mount(&mount, bundle);
    let _ = std::process::Command::new("hdiutil")
        .args(["detach", "-force"])
        .arg(&mount)
        .output();
    let _ = std::fs::remove_dir_all(&mount);
    result?;
    Ok(InstallOutcome::Restart {
        exe: bundle.to_path_buf(),
    })
}

/// Copies the single `.app` in `mount` over `bundle`, restoring the old bundle
/// if the final rename fails.
#[cfg(target_os = "macos")]
fn copy_bundle_from_mount(mount: &Path, bundle: &Path) -> Result<(), UpdateError> {
    let source = std::fs::read_dir(mount)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        })
        .ok_or_else(|| UpdateError::Installer("no app bundle in the disk image".to_owned()))?;

    let parent = bundle.parent().ok_or(UpdateError::UnsupportedInstall)?;
    let pid = std::process::id();
    let stem = bundle
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "PhotoVault.app".to_owned());
    let stage = parent.join(format!(".{stem}.new-{pid}"));
    let backup = parent.join(format!(".{stem}.old-{pid}"));
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_dir_all(&backup);

    let copy = std::process::Command::new("ditto")
        .arg(&source)
        .arg(&stage)
        .output()?;
    if !copy.status.success() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err(UpdateError::Installer(format!(
            "could not copy the new app: {}",
            String::from_utf8_lossy(&copy.stderr).trim()
        )));
    }

    std::fs::rename(bundle, &backup)?;
    if let Err(e) = std::fs::rename(&stage, bundle) {
        let _ = std::fs::rename(&backup, bundle);
        let _ = std::fs::remove_dir_all(&stage);
        return Err(UpdateError::Io(e));
    }
    if let Err(e) = std::fs::remove_dir_all(&backup) {
        log::warn!("could not remove the previous app bundle: {e}");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_windows(installer: &Path) -> Result<InstallOutcome, UpdateError> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = std::env::current_exe()?;
    let extension = installer
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let run = if extension == "msi" {
        format!("msiexec /i \"{}\" /qb /norestart", installer.display())
    } else {
        format!("\"{}\" /S", installer.display())
    };
    // The app must exit before the installer replaces its files, so a shell
    // waits for the installer and then starts the new version.
    let script = format!("{run} && start \"\" \"{}\"", exe.display());
    std::process::Command::new("cmd")
        .arg("/C")
        .arg(&script)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(InstallOutcome::InstallerStarted)
}

#[cfg(target_os = "linux")]
fn install_linux(downloaded: &Path) -> Result<InstallOutcome, UpdateError> {
    use std::os::unix::fs::PermissionsExt;

    let target = linux_appimage().ok_or(UpdateError::UnsupportedInstall)?;
    let target = std::fs::canonicalize(&target).unwrap_or(target);
    let parent = target.parent().ok_or(UpdateError::UnsupportedInstall)?;
    let stem = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "PhotoVault.AppImage".to_owned());
    let stage = parent.join(format!(".{stem}.new-{}", std::process::id()));
    let _ = std::fs::remove_file(&stage);

    std::fs::copy(downloaded, &stage)?;
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755))?;
    if let Err(e) = std::fs::rename(&stage, &target) {
        let _ = std::fs::remove_file(&stage);
        return Err(UpdateError::Io(e));
    }
    Ok(InstallOutcome::Restart { exe: target })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(names: &[&str]) -> Release {
        Release {
            version: "0.2.0".to_owned(),
            url: "https://github.com/Justmajd/PhotoVault/releases/tag/v0.2.0".to_owned(),
            body: String::new(),
            published_at: None,
            assets: names
                .iter()
                .map(|name| ReleaseAsset {
                    name: (*name).to_owned(),
                    url: format!(
                        "https://github.com/Justmajd/PhotoVault/releases/download/v0.2.0/{name}"
                    ),
                    size: 1,
                })
                .collect(),
        }
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.1.1", "0.1.2"));
        assert!(is_newer("0.1.1", "v0.2.0"));
        assert!(is_newer("0.9.9", "1.0.0"));
        assert!(!is_newer("0.1.1", "0.1.1"));
        assert!(!is_newer("0.2.0", "0.1.9"));
        assert!(!is_newer("0.1.1", "not a version"));
    }

    #[test]
    fn accepts_partial_versions() {
        assert!(is_newer("0.1", "0.1.1"));
        assert_eq!(parse_version("v1.2"), Some([1, 2, 0]));
    }

    #[test]
    fn picks_the_platform_asset() {
        let r = release(&[
            "photovault-0.2.0-1-x86_64.pkg.tar.zst",
            "photovault-0.2.0-1.x86_64.rpm",
            "PhotoVault_0.2.0_amd64.deb",
            "PhotoVault_0.2.0_amd64.AppImage",
            "PhotoVault_0.2.0_aarch64.dmg",
            "PhotoVault_0.2.0_x64-setup.exe",
            "PhotoVault_0.2.0_x64.msi",
        ]);
        assert_eq!(
            asset_for(&r, "linux", true).map(|a| a.name.as_str()),
            Some("PhotoVault_0.2.0_amd64.AppImage")
        );
        assert!(asset_for(&r, "linux", false).is_none());
        assert_eq!(
            asset_for(&r, "macos", false).map(|a| a.name.as_str()),
            Some("PhotoVault_0.2.0_aarch64.dmg")
        );
        assert_eq!(
            asset_for(&r, "windows", false).map(|a| a.name.as_str()),
            Some("PhotoVault_0.2.0_x64-setup.exe")
        );
    }

    #[test]
    fn rejects_unsafe_names() {
        assert!(safe_file_name("../evil").is_none());
        assert!(safe_file_name("a/b").is_none());
        assert!(safe_file_name("..").is_none());
        assert_eq!(
            safe_file_name("PhotoVault.AppImage"),
            Some("PhotoVault.AppImage")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn replaces_an_app_bundle_from_a_dmg() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("photovault-install-{stamp}"));
        let old_app = root.join("PhotoVault.app");
        let new_app = root.join("image").join("PhotoVault.app");
        std::fs::create_dir_all(old_app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(new_app.join("Contents/MacOS")).unwrap();
        std::fs::write(old_app.join("Contents/MacOS/version"), "old").unwrap();
        std::fs::write(new_app.join("Contents/MacOS/version"), "new").unwrap();
        let dmg = root.join("PhotoVault.dmg");

        let created = std::process::Command::new("hdiutil")
            .args(["create", "-quiet", "-volname", "PhotoVault Test"])
            .arg("-srcfolder")
            .arg(root.join("image"))
            .args(["-ov", "-format", "UDZO"])
            .arg(&dmg)
            .output()
            .unwrap();
        assert!(
            created.status.success(),
            "hdiutil create failed: {}",
            String::from_utf8_lossy(&created.stderr)
        );

        let outcome = install_macos(&dmg, &old_app).unwrap();
        assert_eq!(
            outcome,
            InstallOutcome::Restart {
                exe: old_app.clone()
            }
        );
        assert_eq!(
            std::fs::read_to_string(old_app.join("Contents/MacOS/version")).unwrap(),
            "new"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
