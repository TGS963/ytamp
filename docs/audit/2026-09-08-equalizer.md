# Equalizer implementation and verification

The main player and Winamp now share a working 10-band equalizer and preamp.
EQ defaults to bypass. Its On/Auto settings, gains, and up to 32 custom preset
curves persist in the existing session file; old sessions remain compatible.
Built-in curves are Flat, Warm, Vocal, and Bright.

## Audio path

`player::equalizer::Equalized` wraps the decoded source before rodio's volume
control and resampling. Ten peaking biquads use the actual decoded sample rate
and independent channel histories. Filters follow the
[RBJ/W3C Audio EQ Cookbook](https://www.w3.org/TR/audio-eq-cookbook/) peaking
formula with Q = 1.4. This implements the classic controls without claiming
bit-for-bit emulation of Winamp's original DSP.

Gain, coefficient, and bypass changes ramp over 20 ms. Settings are polled once
per 64 frames with a nonblocking lock attempt. The wrapper allocates its frame
buffers/histories once and does not allocate while processing samples.
Automatic headroom subtracts the largest positive band gain from the preamp;
a linked sample-peak protector uses instant attack and 80 ms release. It can
change dynamics under heavy boosts. It is not an intersample true-peak limiter.
Settled bypass preserves the incoming samples exactly. Unrepresentable high
bands are skipped at low sample rates. Seeking resets filter histories.

## Interfaces

Winamp uses the existing skin's EQ sprites, with live sliders, a filter response
graph, effective-preamp line, On/Auto, presets, close, and shade controls. It
stacks between the main player and playlist. Double-click a slider to reset its
gain; Flat resets the whole curve. Closing the panel does not disable EQ.

The main player has an EQ popup with the same state, including preset save and
delete. Preset lists scroll within the viewport. The minimum-width player bar
still fits and song titles retain their left alignment. Balance remains outside
this feature and is marked unavailable.

## Verification

- Full offline all-target test suite: **348 tests pass** (347 library tests and
  the existing audit probe test).
- Strict all-target Clippy, formatting, and diff whitespace checks pass.
- Measured sine-wave gain for every band at -12, +6, and +12 dB, at 44.1, 48,
  and 96 kHz, stays within 0.08 dB of the requested center gain.
- Tests cover flat/bypass sample identity, preamp/headroom, stereo separation,
  linked sample-peak protection, low-rate streams, live preamp/bypass ramps,
  rapid band changes, seeking, session compatibility, and preset replacement,
  deletion, capacity, and persistence.
- A full skinned-view interaction test exercises opening, On/Auto, all ten
  sliders, preamp, shade, and close while preserving enabled state.
- Native macOS playback advanced with EQ active while gains and presets changed.
  Verified custom-preset creation, restart persistence, main/Winamp sharing,
  deletion, and controls at 700-point window width. No subjective audio-fidelity
  assessment or testing on other operating systems was performed.
- `swift scripts/smoke-winamp.swift <pid>` passed three player/EQ/playlist/shade/
  return cycles. [Recorded output](evidence/eq-native-smoke.txt).

The full-suite run also exposed an existing seek race: draining the sample
channel after publishing a seek discarded newly decoded samples. The decoder
now tags samples and end-of-stream with seek generations, drains before the
command, and rejects stale samples. Completion is observed before checking the
sample channel, preserving the last sample. Commands cannot block behind a
stalled decoder. The seek test now requires exactly 4,000 remaining samples,
and a deterministic test covers stale samples and stale end-of-stream.

Screenshots: [classic panel](evidence/eq-panel.png),
[playback and saved preset](evidence/eq-playing-preset.png),
[shade with playlist](evidence/eq-shade-playlist.png),
[restored main controls](evidence/eq-restored.png), and
[restored custom preset](evidence/eq-restored-presets.png).

The final rebuilt app was left paused with the EQ panel open, Flat gains, and EQ
bypassed. The temporary native-test preset was deleted. [Final view](evidence/eq-final.png).
