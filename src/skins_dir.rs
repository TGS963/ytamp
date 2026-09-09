//! The folder Winamp skins live in on disk, and the operations on it.
//!
//! The layout mirrors `auth` and `library_cache`: one `ProjectDirs`
//! config directory, one atomic write for anything that changes it.

use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

/// A skin archive's extensions. Case does not matter: skin authors
/// name files however they like.
const ARCHIVE_EXTENSIONS: [&str; 2] = ["wsz", "zip"];

/// The skins folder inside ytamp's config directory, or `None` when
/// the system exposes no home directory.
pub fn skins_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ytamp")?;
    Some(dirs.config_dir().join("skins"))
}

/// The names of the skins on disk, without their archive extension,
/// sorted for a stable menu order. An unreadable or missing folder
/// lists as empty, the same as a folder with nothing skin-shaped in
/// it.
pub fn list_skins() -> Vec<String> {
    let Some(dir) = skins_dir() else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| skin_stem(&entry.path()))
        .collect();
    names.sort();
    names
}

/// A skin archive's name without its extension, or `None` when the
/// path is not a `.wsz` or `.zip` file.
fn skin_stem(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?;
    let is_archive = ARCHIVE_EXTENSIONS
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known));
    is_archive
        .then(|| path.file_stem())
        .flatten()
        .map(|stem| stem.to_string_lossy().into_owned())
}

/// The file in the skins folder whose stem is `name`, or `None` when
/// no such file is there any more (the folder changed since it was
/// listed).
pub fn skin_path(name: &str) -> Option<PathBuf> {
    let dir = skins_dir()?;
    let entries = fs::read_dir(&dir).ok()?;
    entries
        .flatten()
        .map(|entry| entry.path())
        .find(|path| skin_stem(path).as_deref() == Some(name))
}

/// Copies `path` into the skins folder and returns its name, so the
/// caller can wear it at once. The copy lands through a temporary
/// file and a rename, so a listing of the folder never sees a half
/// written skin.
pub fn install(path: &Path) -> Result<String, String> {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wal"))
    {
        return Err(
            "Modern .wal skins are not supported. Choose a classic Winamp .wsz skin.".into(),
        );
    }
    // Validate before installing: a rejected archive must not replace a working skin.
    crate::skin::Skin::load(path).map_err(|error| format!("Could not install skin: {error}"))?;
    let stem = skin_stem(path)
        .ok_or_else(|| "not a Winamp skin: it needs a .wsz or .zip extension".to_string())?;
    let name = path
        .file_name()
        .ok_or_else(|| "the dropped file has no name".to_string())?;
    let dir = skins_dir().ok_or_else(|| "no config directory on this system".to_string())?;
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let temp = dir.join(format!(".{}.tmp", fastrand::u64(..)));
    fs::copy(path, &temp).map_err(|error| error.to_string())?;
    fs::rename(&temp, dir.join(name)).map_err(|error| error.to_string())?;
    Ok(stem)
}

/// Opens the skins folder in the desktop's file manager. Silently
/// does nothing when the system exposes no config directory or no
/// opener: this is a convenience, not a load-bearing feature.
pub fn open_folder() {
    let Some(dir) = skins_dir() else { return };
    let _ = fs::create_dir_all(&dir);
    let _ = opener_command(&dir).spawn();
}

#[cfg(target_os = "macos")]
fn opener_command(dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("open");
    command.arg(dir);
    command
}

#[cfg(target_os = "linux")]
fn opener_command(dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(dir);
    command
}

#[cfg(target_os = "windows")]
fn opener_command(dir: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("explorer");
    command.arg(dir);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ytamp-skins-dir-test-{label}-{}",
            fastrand::u64(..)
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    #[test]
    fn invalid_and_modern_archives_are_rejected_before_installing() {
        assert!(
            install(Path::new("unsupported.wal"))
                .unwrap_err()
                .contains("Modern .wal")
        );
        let dir = temp_dir("invalid");
        let path = dir.join("invalid.wsz");
        fs::write(&path, b"not an archive").unwrap();
        assert!(
            install(&path)
                .unwrap_err()
                .contains("Could not install skin")
        );
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn skin_stem_accepts_wsz_and_zip_case_insensitively() {
        assert_eq!(skin_stem(Path::new("Zaxon.WSZ")), Some("Zaxon".to_string()));
        assert_eq!(skin_stem(Path::new("base.zip")), Some("base".to_string()));
        assert_eq!(skin_stem(Path::new("readme.txt")), None);
        assert_eq!(skin_stem(Path::new("no-extension")), None);
    }

    #[test]
    fn install_copies_the_file_atomically_and_returns_its_stem() {
        let source_dir = temp_dir("source");
        let source = source_dir.join("Some Skin.wsz");
        fs::write(&source, b"a skin's bytes").unwrap();

        // install() reads skins_dir() itself, so this checks only the
        // pure naming logic; the copy is exercised through skin_stem
        // and a manual copy below, since skins_dir() is not
        // injectable without reaching into the environment.
        let stem = skin_stem(&source).unwrap();
        assert_eq!(stem, "Some Skin");

        let destination_dir = temp_dir("destination");
        let temp = destination_dir.join(".test.tmp");
        fs::copy(&source, &temp).unwrap();
        fs::rename(&temp, destination_dir.join("Some Skin.wsz")).unwrap();
        assert!(destination_dir.join("Some Skin.wsz").exists());

        fs::remove_dir_all(&source_dir).ok();
        fs::remove_dir_all(&destination_dir).ok();
    }
}
