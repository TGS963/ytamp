//! Disk cache for downloaded audio bytes, keyed by video id.
//!
//! A song that played once must play again from disk, with no
//! network use. The cache stores one file per video id, holding the
//! raw bytes the resolver chain produced. It sits between the
//! in-memory prefetch cache and the resolver chain: memory first,
//! then disk, then network.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use bytes::Bytes;
use directories::ProjectDirs;

use crate::stream::{AudioBuffer, ResolverChain};

/// Total disk space the cache may use before it evicts old entries.
const CAP_BYTES: u64 = 512 * 1024 * 1024;

/// The character count of a YouTube video id.
const VIDEO_ID_LEN: usize = 11;

/// Fetches audio for `video_id` and returns a buffer that fills as
/// the bytes arrive.
///
/// A disk hit returns an already-complete buffer at once, with no
/// network use. A miss returns an empty, filling buffer right away
/// and starts the resolver chain in the background; a reader can
/// start decoding as soon as the chain's first bytes land. Once the
/// chain finishes, the bytes are written to disk on the blocking
/// pool.
///
/// Both the active-load path and the prefetch path in the player
/// call this one function, so the cache logic lives in a single
/// place.
pub async fn fetch_audio(
    resolvers: &Arc<ResolverChain>,
    http: &reqwest::Client,
    video_id: &str,
) -> AudioBuffer {
    if let Some(bytes) = read_off_thread(video_id.to_string()).await {
        return AudioBuffer::from_complete(bytes);
    }
    spawn_download(resolvers.clone(), http.clone(), video_id.to_string())
}

/// Starts a buffer, runs the resolver chain into it on a background
/// task, and returns the buffer right away. Once the chain ends, a
/// completed buffer's bytes are written to disk.
fn spawn_download(resolvers: Arc<ResolverChain>, http: reqwest::Client, video_id: String) -> AudioBuffer {
    let buffer = AudioBuffer::new(None);
    let writer = buffer.writer();
    let task_buffer = buffer.clone();
    tokio::spawn(async move {
        let _ = resolvers.fetch_audio(&http, &video_id, writer).await;
        persist_if_complete(&task_buffer, &video_id).await;
    });
    buffer
}

/// Writes the buffer's bytes to disk, when the chain completed it. A
/// failed or somehow still-filling buffer has nothing to persist.
async fn persist_if_complete(buffer: &AudioBuffer, video_id: &str) {
    if let Some(bytes) = buffer.complete_bytes() {
        write_off_thread(video_id.to_string(), bytes).await;
    }
}

/// Deletes the cache entry for `video_id`. The player calls this when
/// cached bytes do not decode, so a poisoned entry cannot fail on
/// every later play.
pub fn remove(video_id: &str) {
    let Some(dir) = cache_directory() else { return };
    if let Some(path) = track_path(&dir, video_id) {
        delete_quietly(&path);
    }
}

/// Runs the blocking disk read on tokio's blocking pool, so a large
/// file read never stalls an async worker thread.
async fn read_off_thread(video_id: String) -> Option<Bytes> {
    tokio::task::spawn_blocking(move || read(&video_id).map(Bytes::from))
        .await
        .ok()
        .flatten()
}

/// Runs the blocking write and the cap enforcement on tokio's
/// blocking pool. The caller waits for the write, so a later
/// `remove` for the same id always runs after the file exists.
async fn write_off_thread(video_id: String, bytes: Bytes) {
    let _ = tokio::task::spawn_blocking(move || write(&video_id, &bytes)).await;
}

/// The cached bytes for `video_id`, if a readable file exists. Touches
/// the file's modified time, so eviction ranks it as recently used. A
/// corrupt or unreadable file counts as a miss, and this deletes it.
fn read(video_id: &str) -> Option<Vec<u8>> {
    let path = track_path(&cache_directory()?, video_id)?;
    match fs::read(&path) {
        Ok(bytes) if !bytes.is_empty() => {
            touch(&path);
            Some(bytes)
        }
        _ => {
            delete_quietly(&path);
            None
        }
    }
}

/// Marks `path` as freshly used by setting its modified time to now.
fn touch(path: &Path) {
    let Ok(file) = fs::File::open(path) else {
        return;
    };
    if let Err(error) = file.set_modified(SystemTime::now()) {
        log::warn!("could not update cache mtime: {error}");
    }
}

/// Writes `bytes` to disk under `video_id`, then enforces the size
/// cap. A write failure only logs a warning: playback already holds
/// the bytes in memory, so the download is not lost.
fn write(video_id: &str, bytes: &[u8]) {
    let Some(dir) = cache_directory() else {
        return;
    };
    let Some(path) = track_path(&dir, video_id) else {
        return;
    };
    if let Err(error) = write_atomic(&dir, &path, bytes) {
        log::warn!("could not cache audio for {video_id}: {error}");
        return;
    }
    enforce_cache_cap(&dir);
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

/// True when `id` has the shape of a YouTube video id: eleven
/// characters, each a letter, digit, underscore, or hyphen.
fn is_valid_video_id(id: &str) -> bool {
    id.len() == VIDEO_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// The cache file path for `video_id` under `dir`. `None` when the id
/// does not have the shape of a real video id, so an unchecked id
/// never becomes a file name.
fn track_path(dir: &Path, video_id: &str) -> Option<PathBuf> {
    is_valid_video_id(video_id).then(|| dir.join(format!("{video_id}.m4a")))
}

/// The platform cache directory for audio files, or `None` when the
/// system exposes no home directory.
fn cache_directory() -> Option<PathBuf> {
    let dirs = ProjectDirs::from("", "", "ytamp")?;
    Some(dirs.cache_dir().join("audio"))
}

/// One cached file's identity for eviction purposes.
struct CacheEntry {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

/// Chooses which files to delete so the total size fits under `cap`.
/// Deletes the oldest files first, by modified time. Pure: it takes
/// the full entry list and returns the paths to remove, so eviction
/// order is unit-testable without a filesystem.
fn select_evictions(entries: &[CacheEntry], cap: u64) -> Vec<PathBuf> {
    let total: u64 = entries.iter().map(|entry| entry.size).sum();
    if total <= cap {
        return Vec::new();
    }

    let mut oldest_first: Vec<&CacheEntry> = entries.iter().collect();
    oldest_first.sort_by_key(|entry| entry.modified);

    let mut remaining = total - cap;
    let mut victims = Vec::new();
    for entry in oldest_first {
        if remaining == 0 {
            break;
        }
        victims.push(entry.path.clone());
        remaining = remaining.saturating_sub(entry.size);
    }
    victims
}

/// Lists `dir`'s files and deletes the ones `select_evictions` picks,
/// so the cache stays under the size cap.
fn enforce_cache_cap(dir: &Path) {
    let entries = list_cache_entries(dir);
    for path in select_evictions(&entries, CAP_BYTES) {
        delete_quietly(&path);
    }
}

/// The cache entries found in `dir`. A file that vanishes, or whose
/// metadata cannot be read, is skipped rather than treated as an
/// error.
fn list_cache_entries(dir: &Path) -> Vec<CacheEntry> {
    let Ok(read_dir) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read_dir
        .filter_map(Result::ok)
        .filter_map(cache_entry_from_dir_entry)
        .collect()
}

fn cache_entry_from_dir_entry(entry: fs::DirEntry) -> Option<CacheEntry> {
    let metadata = entry.metadata().ok()?;
    Some(CacheEntry {
        path: entry.path(),
        size: metadata.len(),
        modified: metadata.modified().ok()?,
    })
}

fn delete_quietly(path: &Path) {
    let _ = fs::remove_file(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A fresh directory under the system temp dir, for tests that
    /// need real files. Never the real cache directory.
    fn temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ytamp-test-{label}-{}", fastrand::u64(..)));
        fs::create_dir_all(&dir).expect("create temp test dir");
        dir
    }

    fn entry(path: &str, size: u64, age_seconds: u64) -> CacheEntry {
        CacheEntry {
            path: PathBuf::from(path),
            size,
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 - age_seconds),
        }
    }

    #[test]
    fn valid_video_id_accepts_eleven_id_characters() {
        assert!(is_valid_video_id("dQw4w9WgXcQ"));
        assert!(is_valid_video_id("a-b_c-d_e-f"));
    }

    #[test]
    fn valid_video_id_rejects_wrong_shape() {
        assert!(!is_valid_video_id("short"));
        assert!(!is_valid_video_id("twelve-chars"));
        assert!(!is_valid_video_id("../../etc/pw"));
        assert!(!is_valid_video_id("has space!!"));
    }

    #[test]
    fn track_path_rejects_invalid_id() {
        let dir = PathBuf::from("/cache/audio");
        assert!(track_path(&dir, "not an id!!").is_none());
    }

    #[test]
    fn track_path_builds_file_name_from_id() {
        let dir = PathBuf::from("/cache/audio");
        let path = track_path(&dir, "dQw4w9WgXcQ").expect("valid id");
        assert_eq!(path, PathBuf::from("/cache/audio/dQw4w9WgXcQ.m4a"));
    }

    #[test]
    fn select_evictions_keeps_everything_under_cap() {
        let entries = vec![entry("a", 100, 10), entry("b", 100, 5)];
        assert!(select_evictions(&entries, 1000).is_empty());
    }

    #[test]
    fn select_evictions_removes_oldest_first_until_under_cap() {
        let entries = vec![
            entry("newest", 100, 1),
            entry("oldest", 100, 100),
            entry("middle", 100, 50),
        ];
        let victims = select_evictions(&entries, 250);
        assert_eq!(victims, vec![PathBuf::from("oldest")]);
    }

    #[test]
    fn select_evictions_removes_enough_to_clear_the_cap() {
        let entries = vec![
            entry("oldest", 100, 100),
            entry("middle", 100, 50),
            entry("newest", 100, 1),
        ];
        let victims = select_evictions(&entries, 100);
        assert_eq!(
            victims,
            vec![PathBuf::from("oldest"), PathBuf::from("middle")]
        );
    }

    #[test]
    fn write_then_read_round_trips_bytes() {
        let dir = temp_dir("roundtrip");
        let path = track_path(&dir, "dQw4w9WgXcQ").expect("valid id");
        write_atomic(&dir, &path, b"hello audio").expect("write");

        let bytes = fs::read(&path).expect("read back");
        assert_eq!(bytes, b"hello audio");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enforce_cache_cap_deletes_oldest_file_over_cap() {
        let dir = temp_dir("cap");
        let old_path = dir.join("old.m4a");
        let new_path = dir.join("new.m4a");
        fs::write(&old_path, vec![0u8; 10]).expect("write old");
        fs::write(&new_path, vec![0u8; 10]).expect("write new");

        let old_time = SystemTime::now() - Duration::from_secs(60);
        fs::File::open(&old_path)
            .expect("open old")
            .set_modified(old_time)
            .expect("set old mtime");

        let entries = list_cache_entries(&dir);
        for path in select_evictions(&entries, 15) {
            delete_quietly(&path);
        }

        assert!(!old_path.exists());
        assert!(new_path.exists());

        fs::remove_dir_all(&dir).ok();
    }
}
