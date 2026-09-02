//! A growing byte buffer for one audio download.
//!
//! A source writes chunks as they arrive. A decoder reads the same
//! bytes through `std::io::Read`, at whatever pace it wants, even
//! before the download ends. A read past the downloaded region
//! blocks until a chunk arrives or the download ends.
//!
//! The buffer is a small state machine (`Inner`) behind a
//! `Mutex` and a `Condvar`. The state transitions are plain data
//! logic, so the read and seek decisions are pure functions the unit
//! tests drive directly, with no thread involved.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex};

use bytes::Bytes;

/// The buffer's state as a source fills it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BufferStatus {
    Filling,
    Complete,
    Failed(String),
}

struct Inner {
    bytes: Storage,
    expected_len: Option<u64>,
    status: BufferStatus,
    /// True once a source has pushed at least one non-empty chunk.
    /// A chain of sources reads this to tell "failed before any
    /// bytes" from "failed after some bytes".
    delivered_any: bool,
    /// True once a caller has taken the one writer. Guards the
    /// single-writer rule at run time.
    writer_taken: bool,
}

/// The bytes grow in a `Vec` while the download runs. On completion
/// they freeze into `Bytes` once, so every later handout is a
/// reference-count bump and never a copy of a whole song.
enum Storage {
    Growing(Vec<u8>),
    Frozen(Bytes),
}

impl Storage {
    fn as_slice(&self) -> &[u8] {
        match self {
            Storage::Growing(bytes) => bytes,
            Storage::Frozen(bytes) => bytes,
        }
    }

    fn len(&self) -> usize {
        self.as_slice().len()
    }

    fn append(&mut self, chunk: &[u8]) {
        if let Storage::Growing(bytes) = self {
            bytes.extend_from_slice(chunk);
        }
    }

    fn freeze(&mut self) {
        if let Storage::Growing(bytes) = self {
            *self = Storage::Frozen(Bytes::from(std::mem::take(bytes)));
        }
    }

    fn to_bytes(&self) -> Bytes {
        match self {
            Storage::Growing(bytes) => Bytes::from(bytes.clone()),
            Storage::Frozen(bytes) => bytes.clone(),
        }
    }
}

type Shared = Arc<(Mutex<Inner>, Condvar)>;

/// A shared, growing byte buffer for one audio download. Cheap to
/// clone: every clone shares the same bytes and status.
#[derive(Clone)]
pub struct AudioBuffer {
    shared: Shared,
}

impl AudioBuffer {
    /// A new, empty buffer. `expected_len` is the final size in
    /// bytes, when a source knows it ahead of time.
    pub fn new(expected_len: Option<u64>) -> Self {
        Self {
            shared: Arc::new((
                Mutex::new(Inner {
                    bytes: Storage::Growing(Vec::new()),
                    expected_len,
                    status: BufferStatus::Filling,
                    delivered_any: false,
                    writer_taken: false,
                }),
                Condvar::new(),
            )),
        }
    }

    /// A buffer that already holds its full bytes. A disk cache hit,
    /// or a memory cache hit, builds a buffer this way.
    pub fn from_complete(bytes: Bytes) -> Self {
        let len = bytes.len() as u64;
        let inner = Inner {
            bytes: Storage::Frozen(bytes),
            expected_len: Some(len),
            status: BufferStatus::Complete,
            delivered_any: true,
            writer_taken: true,
        };
        Self {
            shared: Arc::new((Mutex::new(inner), Condvar::new())),
        }
    }

    /// The one writer for this buffer. Panics on a second call: a
    /// buffer takes bytes from a single source of truth, so a second
    /// writer is a programming error, not a runtime condition to
    /// handle.
    pub fn writer(&self) -> BufferWriter {
        let (mutex, _) = &*self.shared;
        let mut inner = mutex.lock().expect("buffer mutex poisoned");
        assert!(!inner.writer_taken, "AudioBuffer::writer called twice");
        inner.writer_taken = true;
        BufferWriter {
            shared: self.shared.clone(),
            is_primary: true,
        }
    }

    /// A new reader over this buffer, starting at position zero. Many
    /// readers may read the same buffer at once.
    pub fn reader(&self) -> BufferReader {
        BufferReader {
            shared: self.shared.clone(),
            position: 0,
        }
    }

    /// The full bytes, once the buffer reached `Complete`. `None`
    /// while the buffer is filling or failed.
    pub fn complete_bytes(&self) -> Option<Bytes> {
        let (mutex, _) = &*self.shared;
        let inner = mutex.lock().expect("buffer mutex poisoned");
        matches!(inner.status, BufferStatus::Complete).then(|| inner.bytes.to_bytes())
    }

    /// The buffer's current status.
    pub fn status(&self) -> BufferStatus {
        let (mutex, _) = &*self.shared;
        mutex.lock().expect("buffer mutex poisoned").status.clone()
    }

    /// The byte count downloaded so far.
    /// The full length in bytes, when known: the complete length, or
    /// the length a source announced ahead of time.
    pub fn known_len(&self) -> Option<u64> {
        let (mutex, _) = &*self.shared;
        let inner = mutex.lock().expect("buffer mutex poisoned");
        end_from_state(inner.bytes.len() as u64, inner.expected_len, &inner.status)
    }

    pub fn downloaded_len(&self) -> u64 {
        let (mutex, _) = &*self.shared;
        mutex.lock().expect("buffer mutex poisoned").bytes.len() as u64
    }

    /// Waits, off the calling task, for the buffer to reach
    /// `Complete` or `Failed`. Returns the full bytes on success.
    pub async fn wait_complete(&self) -> Result<Bytes, String> {
        let shared = self.shared.clone();
        match tokio::task::spawn_blocking(move || block_until_end(&shared)).await {
            Ok(result) => result,
            Err(join_error) => Err(format!("the wait task ended early: {join_error}")),
        }
    }
}

/// Blocks the calling thread until `shared` reaches `Complete` or
/// `Failed`, then reports the outcome. Runs on a blocking thread, so
/// the condvar wait never stalls an async worker.
fn block_until_end(shared: &Shared) -> Result<Bytes, String> {
    let (mutex, condvar) = &**shared;
    let mut inner = mutex.lock().expect("buffer mutex poisoned");
    loop {
        match &inner.status {
            BufferStatus::Complete => return Ok(inner.bytes.to_bytes()),
            BufferStatus::Failed(message) => return Err(message.clone()),
            BufferStatus::Filling => {
                inner = condvar.wait(inner).expect("buffer mutex poisoned");
            }
        }
    }
}

/// The write half of an `AudioBuffer`. A source pushes chunks as they
/// arrive, then calls `finish` or `fail` exactly once.
pub struct BufferWriter {
    shared: Shared,
    /// True for the writer `AudioBuffer::writer` returned. False for
    /// an internal attempt handle a resolver chain hands to one
    /// source at a time. Only the primary writer's drop marks the
    /// buffer failed, so one source's early failure does not end a
    /// chain that still has sources left to try.
    is_primary: bool,
}

impl BufferWriter {
    /// A second handle onto the same buffer, for a resolver chain to
    /// hand to one source attempt. Dropping this handle without a
    /// call to `finish` or `fail` does nothing: only the primary
    /// writer's drop is a safety net.
    pub(crate) fn share(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            is_primary: false,
        }
    }

    /// Appends `chunk` to the buffer and wakes any reader waiting for
    /// more bytes. A no-op for an empty chunk.
    pub fn push(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let (mutex, condvar) = &*self.shared;
        let mut inner = mutex.lock().expect("buffer mutex poisoned");
        inner.bytes.append(chunk);
        inner.delivered_any = true;
        condvar.notify_all();
    }

    /// Records the final size, once a source learns it after the
    /// buffer already started filling (a content-length header, for
    /// example).
    pub fn set_expected_len(&self, expected_len: u64) {
        let (mutex, _) = &*self.shared;
        mutex.lock().expect("buffer mutex poisoned").expected_len = Some(expected_len);
    }

    /// True once this buffer has received at least one non-empty
    /// chunk, from any writer handle.
    pub fn delivered_any(&self) -> bool {
        let (mutex, _) = &*self.shared;
        mutex.lock().expect("buffer mutex poisoned").delivered_any
    }

    /// Marks the buffer complete. A reader at the end now sees end of
    /// file instead of a block.
    pub fn finish(self) {
        self.set_status(BufferStatus::Complete);
    }

    /// Marks the buffer failed. A reader at the end, or blocked at
    /// the end, now sees an error instead of a block.
    pub fn fail(self, message: String) {
        self.set_status(BufferStatus::Failed(message));
    }

    fn set_status(&self, status: BufferStatus) {
        let (mutex, condvar) = &*self.shared;
        let mut inner = mutex.lock().expect("buffer mutex poisoned");
        if status == BufferStatus::Complete {
            inner.bytes.freeze();
        }
        inner.status = status;
        condvar.notify_all();
    }
}

impl Drop for BufferWriter {
    /// A primary writer dropped mid-fill means the download ended
    /// without a call to `finish` or `fail`, most likely a panic or a
    /// cancellation. The buffer fails, so a reader never blocks
    /// forever. An attempt handle from `share` never triggers this:
    /// its owner (a resolver chain) still holds the primary writer.
    fn drop(&mut self) {
        if !self.is_primary {
            return;
        }
        let (mutex, condvar) = &*self.shared;
        let mut inner = mutex.lock().expect("buffer mutex poisoned");
        if matches!(inner.status, BufferStatus::Filling) {
            inner.status = BufferStatus::Failed("the download ended early".to_string());
            condvar.notify_all();
        }
    }
}

/// The read half of an `AudioBuffer`. Implements `Read` and `Seek`,
/// so a decoder reads it exactly like a file, even while it fills.
pub struct BufferReader {
    shared: Shared,
    position: u64,
}

impl Read for BufferReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let (mutex, condvar) = &*self.shared;
        let mut inner = mutex.lock().expect("buffer mutex poisoned");
        loop {
            match decide_read(inner.bytes.len() as u64, self.position, &inner.status) {
                ReadDecision::Ready => {
                    let start = self.position as usize;
                    let read = copy_available(&inner.bytes.as_slice()[start..], buf);
                    self.position += read as u64;
                    return Ok(read);
                }
                ReadDecision::Eof => return Ok(0),
                ReadDecision::Failed(message) => {
                    return Err(io::Error::new(io::ErrorKind::Other, message));
                }
                ReadDecision::Blocked => {
                    inner = condvar.wait(inner).expect("buffer mutex poisoned");
                }
            }
        }
    }
}

/// What a read at `position` should do, given `available` downloaded
/// bytes and the buffer's `status`. A pure decision, so the read loop
/// only needs to act on it.
enum ReadDecision {
    /// Bytes exist at `position`; copy from there.
    Ready,
    /// The buffer ended cleanly at or before `position`.
    Eof,
    /// The buffer failed before it reached `position`.
    Failed(String),
    /// No bytes yet at `position`; wait for more or a status change.
    Blocked,
}

fn decide_read(available: u64, position: u64, status: &BufferStatus) -> ReadDecision {
    if position < available {
        return ReadDecision::Ready;
    }
    match status {
        BufferStatus::Filling => ReadDecision::Blocked,
        BufferStatus::Complete => ReadDecision::Eof,
        BufferStatus::Failed(message) => ReadDecision::Failed(message.clone()),
    }
}

/// Copies as much of `available` into `buf` as fits either slice.
/// Returns the copied byte count.
fn copy_available(available: &[u8], buf: &mut [u8]) -> usize {
    let copy_len = available.len().min(buf.len());
    buf[..copy_len].copy_from_slice(&available[..copy_len]);
    copy_len
}

impl Seek for BufferReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let target = match position {
            SeekFrom::Start(offset) => offset,
            SeekFrom::Current(delta) => offset_by(self.position, delta)?,
            SeekFrom::End(delta) => offset_by(self.known_end()?, delta)?,
        };
        self.position = target;
        Ok(target)
    }
}

impl BufferReader {
    /// The buffer's total length, when known: the downloaded length
    /// once `Complete`, or the announced `expected_len` otherwise.
    /// `Unsupported` when neither is known yet, since a seek from the
    /// end needs a length to seek from.
    fn known_end(&self) -> io::Result<u64> {
        let (mutex, _) = &*self.shared;
        let inner = mutex.lock().expect("buffer mutex poisoned");
        end_from_state(inner.bytes.len() as u64, inner.expected_len, &inner.status).ok_or_else(
            || {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "the stream end is not known yet",
                )
            },
        )
    }
}

fn end_from_state(
    downloaded: u64,
    expected_len: Option<u64>,
    status: &BufferStatus,
) -> Option<u64> {
    match status {
        BufferStatus::Complete => Some(downloaded),
        _ => expected_len,
    }
}

/// `base` moved by the signed `delta`, as `SeekFrom::Current` and
/// `SeekFrom::End` need. An underflow is an invalid seek target.
fn offset_by(base: u64, delta: i64) -> io::Result<u64> {
    base.checked_add_signed(delta)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek target underflows"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn read_blocks_then_returns_bytes_a_writer_pushes_on_another_thread() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        let mut reader = buffer.reader();

        let pusher = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            writer.push(b"hello");
            writer.finish();
        });

        let mut out = [0u8; 5];
        reader.read_exact(&mut out).expect("read succeeds");
        assert_eq!(&out, b"hello");
        pusher.join().expect("writer thread panicked");
    }

    #[test]
    fn read_on_a_complete_buffer_past_the_end_returns_zero() {
        let buffer = AudioBuffer::from_complete(Bytes::from_static(b"abc"));
        let mut reader = buffer.reader();
        let mut out = [0u8; 3];
        reader.read_exact(&mut out).expect("reads the three bytes");

        let mut extra = [0u8; 1];
        assert_eq!(reader.read(&mut extra).expect("eof, not an error"), 0);
    }

    #[test]
    fn read_on_a_failed_buffer_returns_an_error() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        writer.fail("network gone".to_string());

        let mut reader = buffer.reader();
        let mut out = [0u8; 1];
        let error = reader.read(&mut out).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "network gone");
    }

    #[test]
    fn a_seek_past_downloaded_bytes_then_blocks_until_they_arrive() {
        let buffer = AudioBuffer::new(Some(10));
        let writer = buffer.writer();
        let mut reader = buffer.reader();
        reader
            .seek(SeekFrom::Start(5))
            .expect("seek is not bounded by downloaded bytes");

        let pusher = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            writer.push(b"0123456789");
            writer.finish();
        });

        let mut out = [0u8; 5];
        reader
            .read_exact(&mut out)
            .expect("read succeeds once bytes arrive");
        assert_eq!(&out, b"56789");
        pusher.join().expect("writer thread panicked");
    }

    #[test]
    fn a_dropped_writer_that_never_finished_fails_the_buffer() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        drop(writer);
        assert_eq!(
            buffer.status(),
            BufferStatus::Failed("the download ended early".to_string())
        );
    }

    #[test]
    fn a_dropped_attempt_handle_does_not_fail_the_buffer() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        drop(writer.share());
        assert_eq!(buffer.status(), BufferStatus::Filling);
        writer.finish();
    }

    #[test]
    fn seek_from_end_needs_a_known_length() {
        let buffer = AudioBuffer::new(None);
        let mut reader = buffer.reader();
        let error = reader.seek(SeekFrom::End(0)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn seek_from_end_uses_the_expected_length() {
        let buffer = AudioBuffer::new(Some(100));
        let mut reader = buffer.reader();
        assert_eq!(reader.seek(SeekFrom::End(-10)).expect("seek"), 90);
    }

    #[test]
    fn decide_read_reports_ready_when_bytes_cover_the_position() {
        assert!(matches!(
            decide_read(10, 5, &BufferStatus::Filling),
            ReadDecision::Ready
        ));
    }

    #[test]
    fn decide_read_reports_eof_at_the_end_of_a_complete_buffer() {
        assert!(matches!(
            decide_read(10, 10, &BufferStatus::Complete),
            ReadDecision::Eof
        ));
    }

    #[test]
    fn decide_read_reports_blocked_at_the_end_of_a_filling_buffer() {
        assert!(matches!(
            decide_read(10, 10, &BufferStatus::Filling),
            ReadDecision::Blocked
        ));
    }
}
