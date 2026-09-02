//! Disk cache for the library: the playlist list, liked songs, and
//! each opened playlist's track list.
//!
//! The app shows the last session's library at once, then a network
//! refresh replaces it. Each list lives in its own JSON file in the
//! platform cache directory (see `stream::disk_cache` for the same
//! pattern, applied there to audio bytes). A version field makes an
//! old file shape a clean miss instead of a bad parse.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::core::model::{Playlist, PlaylistId, Track};

/// Bumped when the snapshot shape changes, so a file from an older
/// build counts as a miss rather than a failed parse.
const CACHE_VERSION: u32 = 3;

/// The longest playlist id this cache accepts into a file name.
const MAX_PLAYLIST_ID_LEN: usize = 128;

#[derive(Serialize, Deserialize)]
struct Snapshot<T> {
    version: u32,
    data: T,
}

pub fn load_playlists() -> Option<Vec<Playlist>> {
    read_snapshot(&library_dir()?.join("playlists.json"))
}

pub fn save_playlists(playlists: &[Playlist]) {
    save_named("playlists.json", playlists);
}

pub fn load_liked() -> Option<Vec<Track>> {
    read_snapshot(&library_dir()?.join("liked.json"))
}

pub fn save_liked(tracks: &[Track]) {
    save_named("liked.json", tracks);
}

pub fn load_playlist_tracks(id: &PlaylistId) -> Option<Vec<Track>> {
    let dir = library_dir()?;
    read_snapshot(&playlist_tracks_path(&dir, id)?)
}

pub fn save_playlist_tracks(id: &PlaylistId, tracks: &[Track]) {
    let Some(dir) = library_dir() else { return };
    let Some(path) = playlist_tracks_path(&dir, id) else {
        return;
    };
    write_snapshot(&dir, &path, tracks);
}

/// Deletes the whole library cache. Sign-out calls this, so no
/// account's library leaks into the next sign-in.
pub fn clear() {
    if let Some(dir) = library_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
}

fn save_named<T: Serialize>(file_name: &str, data: T) {
    let Some(dir) = library_dir() else { return };
    let path = dir.join(file_name);
    write_snapshot(&dir, &path, data);
}

/// The snapshot at `path`, or `None` on a missing, corrupt, or
/// version-mismatched file. A corrupt or stale file is deleted, so
/// the next write starts clean.
fn read_snapshot<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let json = fs::read_to_string(path).ok()?;
    let snapshot = match serde_json::from_str::<Snapshot<T>>(&json) {
        Ok(snapshot) => snapshot,
        Err(error) => return discard(path, &format!("corrupt ({error})")),
    };
    if snapshot.version != CACHE_VERSION {
        return discard(path, "an old version");
    }
    Some(snapshot.data)
}

/// Logs a miss, deletes the file that caused it, and returns `None`.
fn discard<T>(path: &Path, reason: &str) -> Option<T> {
    log::warn!(
        "library cache at {} is {reason}; discarding it",
        path.display()
    );
    let _ = fs::remove_file(path);
    None
}

/// Writes `data` to `path` under `dir` as a versioned snapshot. A
/// failure only logs a warning: the caller already holds the data in
/// memory, so a lost write does not lose the library.
fn write_snapshot<T: Serialize>(dir: &Path, path: &Path, data: T) {
    let snapshot = Snapshot {
        version: CACHE_VERSION,
        data,
    };
    let Ok(json) = serde_json::to_string(&snapshot) else {
        log::warn!("library cache snapshot for {} did not serialize", path.display());
        return;
    };
    if let Err(error) = write_atomic(dir, path, json.as_bytes()) {
        log::warn!("could not write library cache at {}: {error}", path.display());
    }
}

/// Writes `bytes` to a temporary file in `dir`, then renames it to
/// `path`. The rename is atomic, so a reader never sees a partial
/// file.
fn write_atomic(dir: &Path, path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".{}.tmp", fastrand::u64(..)));
    fs::write(&temp, bytes)?;
    fs::rename(&temp, path)
}

/// True when `id` is safe to use as a file name: not empty, not too
/// long, and built only from letters, digits, underscores, and
/// hyphens. This rejects path separators and `.` outright, so a
/// crafted playlist id can never escape the cache directory.
fn is_valid_playlist_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_PLAYLIST_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The cache file path for a playlist's track list, or `None` when
/// the id does not have a safe shape.
fn playlist_tracks_path(dir: &Path, id: &PlaylistId) -> Option<PathBuf> {
    is_valid_playlist_id(&id.0).then(|| dir.join(format!("playlist-{}.json", id.0)))
}

/// The platform cache directory for the library, or `None` when the
/// system exposes no home directory.
fn library_dir() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ytamp")?;
    Some(dirs.cache_dir().join("library"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::TrackId;

    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ytamp-library-cache-test-{label}-{}",
            fastrand::u64(..)
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    fn track(id: &str) -> Track {
        Track {
            id: TrackId(id.to_string()),
            title: id.to_string(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn valid_playlist_id_accepts_the_usual_shape() {
        assert!(is_valid_playlist_id("PL1234567890abcdef"));
        assert!(is_valid_playlist_id("LM"));
    }

    #[test]
    fn valid_playlist_id_rejects_path_separators_and_dots() {
        assert!(!is_valid_playlist_id(""));
        assert!(!is_valid_playlist_id("../../etc/passwd"));
        assert!(!is_valid_playlist_id("a/b"));
        assert!(!is_valid_playlist_id("a\\b"));
        assert!(!is_valid_playlist_id("a.json"));
        assert!(!is_valid_playlist_id(&"x".repeat(MAX_PLAYLIST_ID_LEN + 1)));
    }

    #[test]
    fn playlist_tracks_path_rejects_an_unsafe_id() {
        let dir = PathBuf::from("/cache/library");
        let id = PlaylistId("../escape".to_string());
        assert!(playlist_tracks_path(&dir, &id).is_none());
    }

    #[test]
    fn write_then_read_round_trips_a_snapshot() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("liked.json");
        write_snapshot(&dir, &path, vec![track("a"), track("b")]);

        let loaded: Option<Vec<Track>> = read_snapshot(&path);
        assert_eq!(loaded, Some(vec![track("a"), track("b")]));

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_missing_file_is_a_miss() {
        let dir = temp_dir("missing");
        let loaded: Option<Vec<Track>> = read_snapshot(&dir.join("nope.json"));
        assert_eq!(loaded, None);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_json_is_a_silent_miss_and_deletes_the_file() {
        let dir = temp_dir("corrupt");
        let path = dir.join("liked.json");
        fs::write(&path, b"not json").expect("write garbage");

        let loaded: Option<Vec<Track>> = read_snapshot(&path);
        assert_eq!(loaded, None);
        assert!(!path.exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_version_mismatch_is_a_miss_and_deletes_the_file() {
        let dir = temp_dir("stale-version");
        let path = dir.join("liked.json");
        let stale = Snapshot {
            version: CACHE_VERSION + 1,
            data: vec![track("a")],
        };
        fs::write(&path, serde_json::to_string(&stale).unwrap()).expect("write stale");

        let loaded: Option<Vec<Track>> = read_snapshot(&path);
        assert_eq!(loaded, None);
        assert!(!path.exists());

        fs::remove_dir_all(&dir).ok();
    }
}
