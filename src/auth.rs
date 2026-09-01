//! Cookie storage: one file in the platform config directory.
//!
//! The file holds the raw Cookie header the user pasted. It is secret
//! material, so it gets owner-only permissions on Unix and its content
//! never appears in a log.

use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;

pub fn load_cookies() -> Option<String> {
    let text = fs::read_to_string(cookie_path()?).ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

pub fn save_cookies(cookies: &str) -> io::Result<()> {
    let Some(path) = cookie_path() else {
        return Err(io::Error::other("no config directory on this system"));
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    fs::write(&temp, cookies)?;
    restrict_to_owner(&temp)?;
    fs::rename(&temp, &path)
}

fn cookie_path() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ytamp")?;
    Some(dirs.config_dir().join("cookies.txt"))
}

#[cfg(unix)]
fn restrict_to_owner(path: &std::path::Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &std::path::Path) -> io::Result<()> {
    Ok(())
}
