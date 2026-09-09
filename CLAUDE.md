# ytamp

This repository contains one Rust crate for a music player.

| Task | Read |
| --- | --- |
| Find the system map | [docs/map/CLAUDE.md](docs/map/CLAUDE.md) |
| Change account or catalog behavior | [`src/core/`](src/core/), [`src/api/`](src/api/), [`src/auth.rs`](src/auth.rs) |
| Change playback or stream behavior | [`src/player/`](src/player/), [`src/stream/`](src/stream/), [`src/runtime/`](src/runtime/) |
| Change the default UI | [`src/ui/`](src/ui/), [`src/theme/`](src/theme/), [`src/app.rs`](src/app.rs) |
| Change classic skins | [`src/skin/`](src/skin/), [`src/ui/winamp/`](src/ui/winamp/), [`assets/skins/`](assets/skins/) |
| Run map checks | `python3 scripts/check-map.py --check` |

The Rust source is the behavior authority. Keep public module paths stable during refactors.
