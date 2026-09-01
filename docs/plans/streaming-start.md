# Streaming start: play while the download runs

## Goal

A track starts to play as soon as its header and a few seconds of
audio arrive. Today the whole file downloads first. Skips onto a
prefetched track stay instant. Seek, position, track end, the disk
cache, and the prefetch cache keep their behavior.

## Today

- `stream::AudioSource::fetch_audio(http, id) -> Result<Vec<u8>>`.
  `ResolverChain` tries rustypipe, then a yt-dlp subprocess. Both
  return the complete file.
- `stream::disk_cache::fetch_audio` wraps the chain: memory is in the
  player, then disk, then network.
- `player::Engine` (one OS thread) receives complete `Bytes`, decodes
  with `rodio::Decoder::new(Cursor<Bytes>)`, appends to a
  `rodio::Player`. `PrefetchCache` holds two complete `Bytes`.
- rustypipe fails on every track (upstream deobfuscator). Each load
  pays that failure before yt-dlp starts.

## Design

### 1. Shared growing buffer (`src/stream/buffer.rs`)

One buffer per download, shared between a writer and many readers.

- `AudioBuffer` (cheap clone, `Arc` inside): bytes so far, an
  optional expected length, and a status `Filling | Complete |
  Failed(String)`.
- `BufferWriter`: `push(&[u8])`, `finish()`, `fail(String)`. One
  writer per buffer.
- `BufferReader`: `std::io::Read + Seek`. `read` blocks until the
  bytes exist or the buffer ends. A seek past the downloaded region
  blocks in the next `read` until the bytes arrive. A `Failed` buffer
  returns `io::Error` from `read`.
- `AudioBuffer::complete_bytes() -> Option<Bytes>` for the disk cache
  and the memory cache. `AudioBuffer::from_complete(Bytes)` for a
  cache hit.
- Pure state machine plus `Mutex + Condvar`. Unit tests drive a
  writer thread and a reader thread.

### 2. Streaming sources (`src/stream/mod.rs`, `ytdlp.rs`, `rustypipe.rs`)

- `AudioSource::fetch_audio(http, id, writer: BufferWriter)
  -> BoxFuture<Result<(), String>>`. A source pushes chunks as they
  arrive and calls `finish`.
- yt-dlp: spawn with piped stdout, read 64 KiB chunks with
  `tokio::io::AsyncReadExt`, push each. On a non-zero exit before the
  first chunk, return the first stderr line. After the first chunk, a
  failure calls `writer.fail`.
- rustypipe: push each range chunk from `download.rs`.
- `ResolverChain::fetch_audio(http, id, writer)`: a source that fails
  before its first chunk yields to the next source. A source that
  fails after its first chunk fails the buffer. A circuit breaker
  skips a source for the rest of the session after 3 failures with
  no chunk delivered. Log one warning when a source trips.
- `stream::disk_cache::fetch_audio(resolvers, http, id) -> AudioBuffer`.
  Disk hit: `from_complete`. Miss: create a buffer, spawn the chain
  into its writer on tokio, and when the buffer completes, write it to
  disk on the blocking pool. Returns at once, no await on the
  download.

### 3. Player source (`src/player/source.rs`)

`rodio::Decoder` runs the decode inside the audio callback, and a
network stall there means an underrun. The decode moves to its own
thread.

- `StreamingSource`: implements `rodio::Source<Item = f32>`. Holds a
  bounded channel of decoded frames, `channels`, `sample_rate`, and
  `total_duration` from the decoder, and a command channel for seek.
- Decoder thread: `rodio::Decoder::new(BufferReader)` (blocks until
  the header arrives), then loops: apply a pending seek, decode
  samples, send them. Ends on decoder end or a buffer failure.
- `next()` returns a decoded sample, or `0.0` while the channel is
  empty and the decoder is alive (a stall plays silence, the position
  stays honest), or `None` after the decoder ends.
- `try_seek`: send the seek to the decoder thread, drain the channel,
  return `Ok`. rodio adjusts its position counter on `Ok`.
- The decoder thread reports `Ready { channels, sample_rate,
  total_duration }` once, before the first sample. The engine waits
  for `Ready` before it appends the source and reports `TrackStarted`.
- Start threshold: `Decoder::new` blocks on the header. No extra
  buffering before start. A first version can add a small
  pre-roll later if stalls show up.

### 4. Engine changes (`src/player/mod.rs`)

- `PrefetchCache` stores `AudioBuffer` handles, complete or filling.
- `Load`: memory buffer, else `disk_cache::fetch_audio` (returns at
  once). Spawn the decoder thread on the buffer. On `Ready` with the
  current generation: append, play, `TrackStarted`. On a failure
  before `Ready`: `Failed`.
- `Prefetch`: `disk_cache::fetch_audio` into the cache. No decoder.
  The "wait for the active download" rule becomes "wait for the
  active track's `Ready`", since the active track needs bandwidth
  only until it plays comfortably. Keep it simple: start the prefetch
  on `Ready`.
- Adoption of an in-flight prefetch: the buffer is already in the
  cache, so `Load` finds it there. The `promoted_load` field goes
  away.
- A decode failure of a complete buffer deletes the disk entry, as
  today. A decode failure of a filling buffer does not touch disk,
  because the disk write never happened.
- The reducer and the effects do not change. `PlayerEvent` does not
  change.

### 5. Verification

- Unit tests: buffer state machine, reader blocking and seek,
  chain fallback and circuit breaker, `StreamingSource` silence on
  stall and end on close (with a fake decoder or a tiny WAV in
  memory).
- `examples/stream_probe.rs`: fetch one track, print the time to the
  first byte, to `Ready`, and to completion.
- Manual: `cargo run`, play a track, seek forward past the downloaded
  point, skip to a prefetched track, resume a restored session.

## Phases and agents

| Phase | Scope | Files |
|-------|-------|-------|
| A | Growing buffer + streaming sources + chain + disk cache | `src/stream/*` |
| B | `StreamingSource` + engine rework | `src/player/*` |
| C | Probe, manual check, memory doc | `examples/`, docs |

Phase A leaves the player compiling: it keeps a thin
`fetch_complete(...) -> Bytes` adapter that awaits `Complete`, so
the engine changes only in phase B.
