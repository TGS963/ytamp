//! OAuth credential storage. Owner-only permissions; tokens never appear in logs.

use std::fs;
use std::io;
use std::path::PathBuf;

use directories::ProjectDirs;

/// Only OAuth is an application sign-in method. Old cookie files are ignored.
pub fn load_auth_method() -> Option<crate::core::effect::AuthMethod> {
    load_oauth_token().map(crate::core::effect::AuthMethod::OAuthToken)
}

pub fn load_oauth_token() -> Option<String> {
    let json = fs::read_to_string(config_dir()?.join("oauth.json")).ok()?;
    (!json.trim().is_empty()).then_some(json)
}

pub fn save_oauth_token(json: &str) -> io::Result<()> {
    let Some(dir) = config_dir() else {
        return Err(io::Error::other("no config directory on this system"));
    };
    fs::create_dir_all(&dir)?;
    let path = dir.join("oauth.json");
    let temp = path.with_extension("tmp");
    fs::write(&temp, json)?;
    restrict_to_owner(&temp)?;
    fs::rename(&temp, &path)
}

/// Removes every stored sign-in: the OAuth token, the credentials,
/// and the legacy cookie file.
pub fn delete_credentials() -> io::Result<()> {
    let Some(dir) = config_dir() else {
        return Ok(());
    };
    for name in ["oauth.json", "auth.json", "cookies.txt"] {
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
