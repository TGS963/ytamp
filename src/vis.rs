//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino),
//! src/vis.rs.
//!
//! The shared audio tap, and Winamp's classic spectrum analyser and
//! oscilloscope math: a 512-point Hann-windowed FFT into 19 bars, with
//! Winamp's own fall and peak physics, cross-checked against Webamp's
//! `VisPainter.ts` and `FFTNullsoft.ts`.
//!
//! ytamp plays through one `rodio::Player` at a time, so one
//! process-wide tap is enough. `AudioTap::shared` hands the same
//! instance to the playing source and to the visualiser view, with no
//! channel or event between them.

use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Mono samples kept in the ring. The tap does not track the playing
/// track's real sample rate, so this is a generous half second at a
/// nominal 96 kHz, not an exact count.
const KEPT: usize = 48_000;
/// How far behind the newest sample a read looks, so a frame shows
/// what the speaker plays, not what the decoder just queued. 150 ms
/// at the same nominal rate as `KEPT`.
pub const LAG: usize = 7_200;
/// Samples that go into one spectrum.
pub const FFT_SAMPLES: usize = 512;
const SPECTRUM_BINS: usize = FFT_SAMPLES / 2;
/// Samples the scope reads, one column every seventh.
pub const SCOPE_SAMPLES: usize = 576;
/// The visualiser's width and height in skin pixels.
pub const COLUMNS: usize = 75;
pub const ROWS: u8 = 16;
/// The bars, each three columns wide with one gap between them.
pub const BARS: usize = 19;
/// The tallest a bar gets.
const MAX_HEIGHT: f32 = 15.0;
/// How far a bar falls each step, and how fast peaks pick up speed.
const FALLOFF: f32 = 12.0 / 16.0;
const PEAK_FALLOFF: f32 = 1.1;
/// How often the bars move. Winamp drew its analyser sixty times a
/// second on a timer of its own, so a fast or slow frame rate never
/// changed how quickly the bars fell; a frame that comes sooner than
/// this shows the bars where they were.
pub const STEP: Duration = Duration::from_micros(16_667);
/// Converts the tap's channel mean to Winamp's channel sum.
const CHANNEL_SUM: f32 = 2.0;
/// Winamp's own scale on every magnitude.
const SPEC_SCALE: f32 = 0.5;

/// The last half second of decoded audio, shared between the playing
/// `StreamingSource` and the visualiser view. `StreamingSource::next`
/// pushes every real sample it plays; silence played during a stall
/// never enters the ring.
pub struct AudioTap {
    ring: Mutex<Ring>,
}

/// The mono samples, plus a partial frame still waiting for the rest
/// of its channels.
struct Ring {
    samples: VecDeque<f32>,
    frame_sum: f32,
    frame_len: u32,
}

impl Default for Ring {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(KEPT),
            frame_sum: 0.0,
            frame_len: 0,
        }
    }
}

impl std::fmt::Debug for AudioTap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AudioTap")
    }
}

impl Default for AudioTap {
    fn default() -> Self {
        Self {
            ring: Mutex::new(Ring::default()),
        }
    }
}

impl AudioTap {
    /// The process-wide tap. One player, one output: a second output
    /// device would need a tap of its own, but ytamp never opens two.
    pub fn shared() -> Arc<Self> {
        static TAP: LazyLock<Arc<AudioTap>> = LazyLock::new(|| Arc::new(AudioTap::default()));
        TAP.clone()
    }

    /// Adds one interleaved sample. Once `channels` of them have
    /// arrived, their mean joins the ring as one mono sample.
    pub fn push(&self, sample: f32, channels: u32) {
        let mut ring = self.ring.lock().unwrap_or_else(|poison| poison.into_inner());
        push_frame(&mut ring, sample, channels.max(1));
    }

    /// Adds a batch of mono samples under one lock. The audio callback
    /// calls this every few dozen frames instead of once per sample,
    /// so it takes the lock about a thousand times a second at most.
    pub fn push_mono(&self, samples: &[f32]) {
        let mut ring = self.ring.lock().unwrap_or_else(|poison| poison.into_inner());
        for sample in samples {
            if ring.samples.len() == KEPT {
                ring.samples.pop_front();
            }
            ring.samples.push_back(*sample);
        }
    }

    /// The `count` samples ending `lag` samples before the newest,
    /// with silence where there are fewer than that.
    pub fn window(&self, count: usize, lag: usize) -> Vec<f32> {
        let ring = self.ring.lock().unwrap_or_else(|poison| poison.into_inner());
        window_from(&ring.samples, count, lag)
    }

    pub fn clear(&self) {
        let mut ring = self.ring.lock().unwrap_or_else(|poison| poison.into_inner());
        ring.samples.clear();
        ring.frame_sum = 0.0;
        ring.frame_len = 0;
    }
}

/// One interleaved sample's contribution to the ring: accumulates
/// into the open frame, and closes the frame into one mono sample once
/// every channel of it has arrived.
fn push_frame(ring: &mut Ring, sample: f32, channels: u32) {
    ring.frame_sum += sample;
    ring.frame_len += 1;
    if ring.frame_len < channels {
        return;
    }
    let mono = ring.frame_sum / channels as f32;
    ring.frame_sum = 0.0;
    ring.frame_len = 0;
    if ring.samples.len() == KEPT {
        ring.samples.pop_front();
    }
    ring.samples.push_back(mono);
}

/// The pure windowing math behind `AudioTap::window`, tested on its
/// own without a lock.
fn window_from(samples: &VecDeque<f32>, count: usize, lag: usize) -> Vec<f32> {
    let end = samples.len().saturating_sub(lag);
    let start = end.saturating_sub(count);
    let mut out = vec![0.0; count];
    let taken = end - start;
    for (slot, sample) in out[count - taken..].iter_mut().zip(samples.range(start..end)) {
        *slot = *sample;
    }
    out
}

/// The classic analyser's FFT, as Winamp's own source has it: 512
/// samples under a Hann window, 256 magnitudes out, each halved.
struct Fft {
    bit_reversed: Vec<usize>,
    envelope: Vec<f32>,
    twiddles: Vec<(f32, f32)>,
    real: Vec<f32>,
    imaginary: Vec<f32>,
}

impl Fft {
    fn new() -> Self {
        let n = FFT_SAMPLES;
        let mut bit_reversed: Vec<usize> = (0..n).collect();
        let mut j = 0;
        for i in 0..n {
            if j > i {
                bit_reversed.swap(i, j);
            }
            let mut m = n >> 1;
            while m >= 1 && j >= m {
                j -= m;
                m >>= 1;
            }
            j += m;
        }
        let envelope = (0..n)
            .map(|i| {
                let phase = i as f32 / n as f32 * std::f32::consts::TAU;
                0.5 + 0.5 * (phase - std::f32::consts::FRAC_PI_2).sin()
            })
            .collect();
        let mut twiddles = Vec::new();
        let mut size = 2;
        while size <= n {
            let theta = -std::f32::consts::TAU / size as f32;
            twiddles.push((theta.cos(), theta.sin()));
            size <<= 1;
        }
        Self {
            bit_reversed,
            envelope,
            twiddles,
            real: vec![0.0; n],
            imaginary: vec![0.0; n],
        }
    }

    fn spectrum(&mut self, wave: &[f32], out: &mut [f32; SPECTRUM_BINS]) {
        let n = FFT_SAMPLES;
        for i in 0..n {
            let from = self.bit_reversed[i];
            self.real[i] = wave.get(from).copied().unwrap_or(0.0) * self.envelope[from];
            self.imaginary[i] = 0.0;
        }
        let mut size = 2;
        let mut stage = 0;
        while size <= n {
            let (wpr, wpi) = self.twiddles[stage];
            let (mut wr, mut wi) = (1.0f32, 0.0f32);
            let half = size >> 1;
            for m in 0..half {
                let mut i = m;
                while i < n {
                    let j = i + half;
                    let tr = wr * self.real[j] - wi * self.imaginary[j];
                    let ti = wr * self.imaginary[j] + wi * self.real[j];
                    self.real[j] = self.real[i] - tr;
                    self.imaginary[j] = self.imaginary[i] - ti;
                    self.real[i] += tr;
                    self.imaginary[i] += ti;
                    i += size;
                }
                let previous = wr;
                wr = wr * wpr - wi * wpi;
                wi = wi * wpr + previous * wpi;
            }
            size <<= 1;
            stage += 1;
        }
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = (self.real[i] * self.real[i] + self.imaginary[i] * self.imaginary[i]).sqrt()
                * SPEC_SCALE;
        }
    }
}

/// One bar of the analyser, in rows from the bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bar {
    /// How tall the bar is, 0 to 15.
    pub height: u8,
    /// Where the peak mark sits, as the height of a bar whose top row
    /// it would be, 1 to 16; `None` while it is out of sight.
    pub peak: Option<u8>,
}

/// The spectrum analyser's memory: where each bar and its peak are.
pub struct Analyser {
    fft: Fft,
    spectrum: [f32; SPECTRUM_BINS],
    /// Where each bar is, falling at a fixed rate towards the sound.
    falloff: [f32; BARS],
    /// Each peak, in 256ths of a row, and how fast it is dropping.
    peaks: [i32; BARS],
    peak_speed: [f32; BARS],
    /// When the bars last moved, and where they were.
    last_step: Option<Instant>,
    bars: [Bar; BARS],
}

impl Default for Analyser {
    fn default() -> Self {
        Self {
            fft: Fft::new(),
            spectrum: [0.0; SPECTRUM_BINS],
            falloff: [0.0; BARS],
            peaks: [0; BARS],
            peak_speed: [0.0; BARS],
            last_step: None,
            bars: [Bar::default(); BARS],
        }
    }
}

impl Analyser {
    /// One frame: the spectrum of `samples` (512 of them, mono, -1 to
    /// 1) moves the bars, and the bars are returned.
    pub fn step(&mut self, samples: &[f32], now: Instant) -> [Bar; BARS] {
        // Keep the step's own beat when frames come a little early or
        // late, and never owe more than one step after a long gap.
        let due = self.last_step.map_or(now, |last| last + STEP);
        if now + Duration::from_millis(1) < due {
            return self.bars;
        }
        self.last_step = Some(due.max(now - STEP));
        let wave: Vec<f32> = samples.iter().map(|sample| sample * CHANNEL_SUM).collect();
        self.fft.spectrum(&wave, &mut self.spectrum);
        let columns = self.bands();
        let mut bars = [Bar::default(); BARS];
        for (bar, slot) in bars.iter_mut().enumerate() {
            let chunk = 4 * bar;
            let sound =
                (columns[chunk] + columns[chunk + 1] + columns[chunk + 2] + columns[chunk + 3])
                    / 4.0;
            // Winamp kept the target as a whole number of rows.
            let target = sound.min(MAX_HEIGHT).trunc();
            let falloff = &mut self.falloff[bar];
            *falloff -= FALLOFF;
            if *falloff <= target {
                *falloff = target;
            }
            let peak = &mut self.peaks[bar];
            if *peak <= (*falloff * 256.0).round() as i32 {
                *peak = (*falloff * 256.0) as i32;
                self.peak_speed[bar] = 3.0;
            }
            let peak_row = *peak / 256;
            *peak -= self.peak_speed[bar].round() as i32;
            self.peak_speed[bar] *= PEAK_FALLOFF;
            if *peak <= 0 {
                *peak = 0;
            }
            slot.height = falloff.round() as u8;
            slot.peak = (peak_row >= 1).then_some((peak_row + 1) as u8);
        }
        self.bars = bars;
        bars
    }

    /// Whether every bar and peak has come to rest, so nothing moves
    /// until there is sound again.
    pub fn settled(&self) -> bool {
        self.falloff.iter().all(|f| *f <= 0.0) && self.peaks.iter().all(|p| *p == 0)
    }

    /// Winamp's own bands, from its published source: seventy-five
    /// spans a semitone apart (`2^(x/12)`), each summing its share of
    /// the 256 bins, the fractional edges read through a Hermite
    /// curve, the sum clipped at 255. Fifteen of those 255 fill the
    /// display, which is why the classic analyser always looked
    /// alive. One extra silent column pads the last bar's group of
    /// four.
    fn bands(&self) -> [f32; COLUMNS + 1] {
        let bla = 255.0 / 2f32.powf(75.0 / 12.0);
        let warp = |x: f32| (2f32.powf(x / 12.0) - 1.0) * bla;
        let sample = |index: usize| self.spectrum.get(index).copied().unwrap_or(0.0);
        let hermite = |x: f32, y0: f32, y1: f32, y2: f32, y3: f32| {
            let c1 = 0.5 * (y2 - y0);
            let c3 = 1.5 * (y1 - y2) + 0.5 * (y3 - y0);
            let c2 = y0 - y1 + c1 - c3;
            ((c3 * x + c2) * x + c1) * x + y1
        };
        let mut columns = [0.0; COLUMNS + 1];
        let mut next = warp(0.0) + 1.0;
        for (x, column) in columns.iter_mut().take(COLUMNS).enumerate() {
            let low = next;
            next = warp(x as f32 + 1.0) + 1.0;
            let mut value = 0.0f32;
            let mut bin = low.floor() as usize;
            let end = (next.floor() as usize).min(SPECTRUM_BINS - 1);
            let mut fraction = low;
            let mut mult = (bin as f32 + 1.0) - low;
            let mut herm = true;
            loop {
                if bin == end {
                    mult = next - fraction;
                    herm = true;
                }
                if herm {
                    value += hermite(
                        fraction - bin as f32,
                        sample(bin.saturating_sub(1)),
                        sample(bin),
                        sample(bin + 1),
                        sample(bin + 2),
                    ) * mult;
                } else {
                    value += sample(bin);
                }
                herm = false;
                bin += 1;
                if bin > end {
                    break;
                }
                fraction = bin as f32;
            }
            *column = value.min(255.0);
        }
        columns
    }
}

/// The oscilloscope's trace: a row (0 at the top) for each column,
/// from every seventh of the samples, the wave's centre at row 7.
pub fn scope(samples: &[f32]) -> [u8; COLUMNS] {
    let mut rows = [7u8; COLUMNS];
    for (column, row) in rows.iter_mut().enumerate() {
        let sample = samples.get(column * 7).copied().unwrap_or(0.0);
        let byte = (sample * 128.0 + 128.0).round().clamp(0.0, 255.0);
        let y = (byte / 16.0 * 2.0).round() - 9.0;
        *row = y.clamp(0.0, f32::from(ROWS) - 1.0) as u8;
    }
    rows
}

/// Which of the scope's five colours a row is drawn in: brightest at
/// the centre, darker towards the edges.
pub fn scope_shade(row: u8) -> usize {
    match row {
        14.. => 4,
        12..=13 => 3,
        10..=11 => 2,
        8..=9 => 1,
        6..=7 => 0,
        4..=5 => 1,
        2..=3 => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, amplitude: f32, count: usize) -> Vec<f32> {
        const NOMINAL_RATE: f32 = 44_100.0;
        (0..count)
            .map(|i| amplitude * (i as f32 / NOMINAL_RATE * hz * std::f32::consts::TAU).sin())
            .collect()
    }

    /// `AudioTap::shared` hands out one process-wide instance, so a
    /// test that wants a tap of its own builds one directly: parallel
    /// tests must never share the same ring.
    fn fresh_tap() -> AudioTap {
        AudioTap::default()
    }

    #[test]
    fn the_tap_mixes_stereo_down_and_pads_with_silence() {
        let tap = fresh_tap();
        // Three stereo frames: (0.5, -0.5), (1.0, 0.0), (0.2, 0.2),
        // each channel's mean the mono sample it becomes.
        for sample in [0.5, -0.5, 1.0, 0.0, 0.2, 0.2] {
            tap.push(sample, 2);
        }
        assert_eq!(tap.window(5, 0), [0.0, 0.0, 0.0, 0.5, 0.2]);
        assert_eq!(tap.window(2, 1), [0.0, 0.5]);
        tap.clear();
        assert_eq!(tap.window(2, 0), [0.0, 0.0]);
    }

    #[test]
    fn the_tap_keeps_at_most_its_capacity() {
        let tap = fresh_tap();
        for _ in 0..(KEPT + 100) {
            tap.push(0.25, 1);
        }
        let ring = tap.ring.lock().unwrap();
        assert_eq!(ring.samples.len(), KEPT);
    }

    #[test]
    fn window_from_reads_the_count_ending_lag_before_the_newest() {
        let samples: VecDeque<f32> = (1..=10).map(|n| n as f32).collect();
        assert_eq!(window_from(&samples, 3, 0), [8.0, 9.0, 10.0]);
        assert_eq!(window_from(&samples, 3, 2), [6.0, 7.0, 8.0]);
        // Fewer samples than asked for pad with silence at the front.
        assert_eq!(window_from(&samples, 12, 0), {
            let mut out = vec![0.0; 2];
            out.extend((1..=10).map(|n| n as f32));
            out
        });
        // A lag past the end of the ring is all silence.
        assert_eq!(window_from(&samples, 3, 20), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn the_spectrum_peaks_where_the_tone_is() {
        const NOMINAL_RATE: f32 = 44_100.0;
        let mut fft = Fft::new();
        let mut out = [0.0; SPECTRUM_BINS];
        let wave: Vec<f32> = sine(1000.0, 0.5, FFT_SAMPLES)
            .into_iter()
            .map(|s| s * CHANNEL_SUM)
            .collect();
        fft.spectrum(&wave, &mut out);
        let loudest = (0..SPECTRUM_BINS)
            .max_by(|a, b| out[*a].total_cmp(&out[*b]))
            .unwrap();
        let expected = (1000.0 / NOMINAL_RATE * FFT_SAMPLES as f32).round() as usize;
        assert!(
            loudest.abs_diff(expected) <= 1,
            "peak at bin {loudest}, tone at {expected}"
        );
    }

    /// A sine at a known frequency lights the bar its semitone band
    /// covers.
    #[test]
    fn a_sine_lights_the_expected_bar() {
        let mut analyser = Analyser::default();
        let loud = sine(1000.0, 0.9, FFT_SAMPLES);
        let bars = analyser.step(&loud, Instant::now());
        let loudest = (0..BARS).max_by_key(|&i| bars[i].height).unwrap();
        // Winamp's semitone warp puts a 1 kHz tone, at a nominal
        // 44.1 kHz sample rate, at bar 6: solving `warp(x) + 1 = bin`
        // for the FFT bin nearest 1 kHz gives x near 25, and each bar
        // spans four of those columns.
        assert!(
            (5..=7).contains(&loudest),
            "1 kHz lit bar {loudest}, expected close to bar 6"
        );
        assert!(bars[loudest].height > 0);
    }

    /// Frames one step apart, the way a 60 Hz loop delivers them.
    fn clock() -> impl FnMut() -> Instant {
        let mut at = Instant::now();
        move || {
            at += STEP;
            at
        }
    }

    #[test]
    fn a_fast_frame_rate_leaves_the_bars_alone() {
        let mut analyser = Analyser::default();
        let loud = sine(1000.0, 0.5, FFT_SAMPLES);
        let silence = vec![0.0; FFT_SAMPLES];
        let start = Instant::now();
        let bars = analyser.step(&loud, start);
        // Frames a millisecond apart do not move the bars: they show
        // where they were, however often the window paints.
        for i in 1..12 {
            let again = analyser.step(&silence, start + Duration::from_millis(i));
            assert_eq!(
                again.iter().map(|b| b.height).collect::<Vec<_>>(),
                bars.iter().map(|b| b.height).collect::<Vec<_>>()
            );
        }
        let moved = analyser.step(&silence, start + STEP);
        assert!(
            moved
                .iter()
                .zip(bars.iter())
                .any(|(after, before)| after.height < before.height)
        );
    }

    #[test]
    fn bars_rise_with_sound_and_fall_without() {
        const NOMINAL_RATE: f32 = 44_100.0;
        let mut analyser = Analyser::default();
        let mut tick = clock();
        let loud: Vec<f32> = (0..FFT_SAMPLES)
            .map(|i| {
                let t = i as f32 / NOMINAL_RATE * std::f32::consts::TAU;
                0.3 * ((t * 100.0).sin() + (t * 800.0).sin() + (t * 5000.0).sin())
            })
            .collect();
        let bars = analyser.step(&loud, tick());
        let tallest = bars.iter().map(|bar| bar.height).max().unwrap();
        assert!(tallest > 0, "no bar rose to the sound");
        assert!(tallest <= 15);
        assert!(!analyser.settled());

        let silence = vec![0.0; FFT_SAMPLES];
        let after = analyser.step(&silence, tick());
        let lower = bars
            .iter()
            .zip(after.iter())
            .all(|(before, after)| after.height <= before.height);
        assert!(lower, "bars rose in silence");
        // The peak hangs above the bar it came from.
        let with_peak = after.iter().find(|bar| bar.peak.is_some()).unwrap();
        assert!(with_peak.peak.unwrap() > with_peak.height);
        for _ in 0..400 {
            analyser.step(&silence, tick());
        }
        assert!(analyser.settled());
        assert!(
            analyser
                .step(&silence, tick())
                .iter()
                .all(|bar| bar.height == 0 && bar.peak.is_none())
        );
    }

    #[test]
    fn the_scope_rests_at_the_middle_and_stays_inside() {
        assert!(scope(&[0.0; SCOPE_SAMPLES]).iter().all(|row| *row == 7));
        let loud = scope(&sine(440.0, 1.0, SCOPE_SAMPLES));
        assert!(loud.iter().all(|row| *row < ROWS));
        assert!(loud.iter().any(|row| *row != 7));
        assert_eq!(scope_shade(7), 0);
        assert_eq!(scope_shade(0), 3);
        assert_eq!(scope_shade(15), 4);
    }
}
