//! Credential storage: one JSON file in the platform config directory.
//!
//! The file holds the Cookie header the user pasted and the
//! X-Goog-AuthUser account index. It is secret material, so it gets
//! owner-only permissions on Unix and its content never appears in a
//! log. A legacy plain cookies.txt loads as account 0.

use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;

use crate::core::effect::Credentials;

pub fn load_credentials() -> Option<Credentials> {
    let dir = config_dir()?;
    if let Ok(json) = fs::read_to_string(dir.join("auth.json"))
        && let Ok(credentials) = serde_json::from_str::<Credentials>(&json)
        && !credentials.cookies.trim().is_empty()
    {
        return Some(credentials);
    }
    load_legacy_cookies(&dir)
}

fn load_legacy_cookies(dir: &std::path::Path) -> Option<Credentials> {
    let text = fs::read_to_string(dir.join("cookies.txt")).ok()?;
    let cookies = text.trim();
    (!cookies.is_empty()).then(|| Credentials {
        cookies: cookies.to_string(),
        authuser: "0".to_string(),
    })
}

pub fn save_credentials(credentials: &Credentials) -> io::Result<()> {
    let Some(dir) = config_dir() else {
        return Err(io::Error::other("no config directory on this system"));
    };
    fs::create_dir_all(&dir)?;
    let json = serde_json::to_string(credentials).map_err(io::Error::other)?;
    let path = dir.join("auth.json");
    let temp = path.with_extension("tmp");
    fs::write(&temp, json)?;
    restrict_to_owner(&temp)?;
    fs::rename(&temp, &path)
}

/// Removes the stored credentials, the legacy cookie file included.
pub fn delete_credentials() -> io::Result<()> {
    let Some(dir) = config_dir() else {
        return Ok(());
    };
    for name in ["auth.json", "cookies.txt"] {
        let path = dir.join(name);
        if path.exists() {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn config_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ytamp")?;
    Some(dirs.config_dir().to_path_buf())
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
