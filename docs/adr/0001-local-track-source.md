# Local tracks have a distinct media source

## Decision

`Track.source` distinguishes YouTube from `LocalFile { path }`. Import records a
canonical path. `TrackId::local_file` encodes its OS path bytes without loss.
Repeated imports retain separate queue occurrences with the same file identity.
Old serialized tracks default to YouTube, so prior sessions remain readable.

## Boundaries

The local-media module owns inspection, tags, duration, artwork, and LRC reads.
The reducer owns ordered import jobs and cancellation generations. The runtime
runs file work outside the UI thread. The player opens a seekable file and uses
the same supported-audio selector as inspection. Both playback sources feed the
existing sample channel, mixer, equalizer, and visualizer.

Local tracks bypass extraction, download caches, online lyrics, and catalog writes.
The UI hides those actions. Reducer and runtime guards enforce the boundary.
No FFmpeg process or bundled codec runtime is required.

## Tradeoffs

File identity follows its path rather than its content. Moving a file requires
Locate. That action replaces all queue occurrences and reloads the current file
paused. There is no folder index or file watcher. Supported containers can still
contain unsupported codecs. Import validates audio and reads a duration before
queue admission. Packet scans run in a cancellable worker when headers lack one.

See the [import process](../map/processes/local-import.md) for source links.
