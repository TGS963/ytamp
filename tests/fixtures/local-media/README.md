# Local-media fixtures

All `tone*` files and `tagged-aac.mp4` were generated on 2026-09-09 with FFmpeg's
`lavfi sine` source: 440 Hz, 48 kHz, 0.12 or 0.25 seconds. The MP4 uses FFmpeg's
native AAC encoder and carries the title, artist, and album asserted by the
tests. They contain no third-party audio or artwork. FFmpeg is GPL-licensed;
these generated sine-wave fixture files are project test data.

`video-first-aac.mp4` has a 16x16 black H.264 video stream before its AAC audio
stream, with video marked default. `no-audio.mp4` contains only that video stream;
`corrupt.mp3` is intentionally plain text. They test supported-audio selection and
expected failures without invoking FFmpeg at test time.
