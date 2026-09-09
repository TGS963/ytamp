# ytamp system map

This map routes changes to four live object clusters and four real flows.
The Rust source remains the behavior authority. Map cards cite source paths.

| Change | Open |
| --- | --- |
| Account, library, search, or discovery | `objects/account-catalog/` and `processes/sign-in.md` or `processes/home-loading.md` |
| Queue, audio, stream, or effects | `objects/playback/` and `processes/playback.md` |
| Main window, pages, controls, or navigation | `objects/default-ui/` and the affected process card |
| Winamp window or `.wsz` skin | `objects/classic-skins/` and `processes/skin-loading.md` |
| Change impact lookup | `effects/CONTEXT.md` |
| Check links and generated twins | `python3 scripts/check-map.py --check` |

Read one cluster card, then follow its `See` links into `src/`. Do not treat this map as a second specification.
