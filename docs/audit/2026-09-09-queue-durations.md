# Queue duration fix

Decoded audio durations did not update the track copies in the queue. Some discovery tracks also lacked duration metadata. The classic playlist rendered those missing values as `0:00`.

The reducer now copies decoded durations to all queued occurrences of the same video. A background OAuth request fills missing durations in batches of up to 50 unique video IDs. It preserves known durations, ignores stale account results, and stops after three failed attempts. A notice explains a lookup failure. Saved queues retain resolved durations.

## Verification

- The original reducer-to-playlist test failed with `0:00` instead of `7:30` before the fix, then passed.
- All 401 library tests pass. All-target tests and Clippy pass. Formatting, map links, and whitespace checks pass.
- Added checks cover duplicate entries, batch continuation, saved queue restoration, decoder precedence, bounded retries, and stale sessions.
- Live OAuth lookups returned Amethyst at 451 seconds, Fell Asleep in the Sun at 232 seconds, and msn at 233 seconds.
- Native playback reported slightly shorter durations. The [running app screenshot](evidence/durations/queue.png) shows these measured values in every affected row. No affected row shows zero or an unknown duration.
- The running app saved all five formerly missing durations to its session file.
- Changed production functions have cyclomatic complexity at most 10.

YouTube can omit metadata for unavailable videos or live streams. The app does not invent a duration for those cases.

## Release checks

Routine CI now uses one Linux job. The separate [release workflow validation](https://github.com/TGS963/ytamp/actions/runs/34323836191) passed for Windows, Linux, and universal macOS at commit `3fbc1a8`. The macOS job built and checked both ARM64 and x86-64 slices. This manual run uploaded packages without publishing a release. These package builds preceded the queue fix.

The downloaded macOS archive also passed local inspection: both `x86_64` and `arm64` slices are present, the minimum OS version is 14.0, and the app includes the expected icon.

The [routine CI run for the queue fix](https://github.com/TGS963/ytamp/actions/runs/34326467057) passed at commit `951b6b5`.
