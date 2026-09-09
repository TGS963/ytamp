//! Ten peaking biquads before rodio's volume/resampling, at the decoded rate.
//! Coefficients follow the RBJ/W3C Audio EQ Cookbook (peaking EQ, Q = 1.4):
//! https://www.w3.org/TR/audio-eq-cookbook/
//! The audio callback never waits for settings or allocates. Coefficient,
//! preamp and bypass changes ramp over 20 ms; channel histories stay separate.
use crate::core::equalizer::{FREQUENCIES, Parameters};
use rodio::{ChannelCount, SampleRate, Source, source::SeekError};
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Clone, Default)]
pub struct Control(Arc<RwLock<(Parameters, f32)>>);
impl Control {
    pub fn set(&self, parameters: Parameters) {
        self.0.write().unwrap_or_else(|e| e.into_inner()).0 = parameters.normalized();
    }
    pub fn set_balance(&self, value: f32) {
        self.0.write().unwrap_or_else(|e| e.into_inner()).1 = if value.is_finite() {
            value.clamp(-1., 1.)
        } else {
            0.
        };
    }
}

#[derive(Clone, Copy)]
struct Coefficients([f64; 5]);
impl Coefficients {
    const IDENTITY: Self = Self([1., 0., 0., 0., 0.]);
    fn peak(frequency: f64, rate: f64, gain_db: f32) -> Self {
        // Unrepresentable bands are bypassed rather than folded below Nyquist.
        if frequency >= rate * 0.49 || gain_db == 0. {
            return Self::IDENTITY;
        }
        let a = 10_f64.powf(gain_db as f64 / 40.);
        let omega = std::f64::consts::TAU * frequency / rate;
        let alpha = omega.sin() / (2. * 1.4);
        let a0 = 1. + alpha / a;
        Self([
            (1. + alpha * a) / a0,
            -2. * omega.cos() / a0,
            (1. - alpha * a) / a0,
            -2. * omega.cos() / a0,
            (1. - alpha / a) / a0,
        ])
    }
}
#[derive(Clone, Copy, Default)]
struct History {
    x1: f64,
    x2: f64,
    y1: f64,
    y2: f64,
}
impl History {
    fn process(&mut self, input: f64, c: Coefficients) -> f64 {
        let [b0, b1, b2, a1, a2] = c.0;
        let output = b0 * input + b1 * self.x1 + b2 * self.x2 - a1 * self.y1 - a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = if output.abs() < 1e-30 { 0. } else { output };
        output
    }
}

pub struct Equalized<S> {
    inner: S,
    control: Control,
    parameters: Parameters,
    balance: f64,
    target_balance: f64,
    balance_ramp: usize,
    coefficients: [Coefficients; 10],
    target: [Coefficients; 10],
    histories: Vec<[History; 10]>,
    frame: Vec<f32>,
    dry: Vec<f32>,
    cursor: usize,
    poll: usize,
    ramp: usize,
    ramp_frames: usize,
    gain: f64,
    target_gain: f64,
    wet: f64,
    target_wet: f64,
    limiter: f64,
    release: f64,
}
impl<S: Source> Equalized<S> {
    pub fn new(inner: S, control: Control) -> Self {
        let (parameters, balance) = *control.0.read().unwrap_or_else(|e| e.into_inner());
        let rate = inner.sample_rate().get() as f64;
        let channels = inner.channels().get() as usize;
        let coefficients = std::array::from_fn(|i| {
            Coefficients::peak(FREQUENCIES[i], rate, parameters.bands_db[i])
        });
        let gain = 10_f64.powf(parameters.effective_preamp_db() as f64 / 20.);
        let wet = f64::from(parameters.enabled);
        Self {
            inner,
            control,
            parameters,
            balance: balance as f64,
            target_balance: balance as f64,
            balance_ramp: 0,
            coefficients,
            target: coefficients,
            histories: vec![[History::default(); 10]; channels],
            frame: vec![0.; channels],
            dry: vec![0.; channels],
            cursor: channels,
            poll: 0,
            ramp: 0,
            ramp_frames: (rate * 0.020).max(1.) as usize,
            gain,
            target_gain: gain,
            wet,
            target_wet: wet,
            limiter: 1.,
            release: (-1. / (rate * 0.080)).exp(),
        }
    }
    fn refresh(&mut self) {
        if self.poll > 0 {
            self.poll -= 1;
            return;
        }
        self.poll = 63;
        let next = self.control.0.try_read().ok().map(|p| *p);
        if let Some((_, balance)) = next
            && balance as f64 != self.target_balance
        {
            self.target_balance = balance as f64;
            self.balance_ramp = self.ramp_frames;
        }
        if let Some((p, _)) = next
            && p != self.parameters
        {
            self.parameters = p;
            self.target = std::array::from_fn(|i| {
                Coefficients::peak(
                    FREQUENCIES[i],
                    self.inner.sample_rate().get() as f64,
                    p.bands_db[i],
                )
            });
            self.target_gain = 10_f64.powf(p.effective_preamp_db() as f64 / 20.);
            self.target_wet = f64::from(p.enabled);
            self.ramp = self.ramp_frames;
        }
    }
    fn process_frame(&mut self) {
        self.refresh();
        self.advance_ramps();
        let peak = self.filter_channels();
        self.mix_output(peak);
    }

    fn advance_ramps(&mut self) {
        if self.balance_ramp > 0 {
            self.balance += (self.target_balance - self.balance) / self.balance_ramp as f64;
            self.balance_ramp -= 1;
        }
        if self.ramp > 0 {
            let remaining = self.ramp as f64;
            for (current, target) in self.coefficients.iter_mut().zip(self.target) {
                for (c, t) in current.0.iter_mut().zip(target.0) {
                    *c += (t - *c) / remaining;
                }
            }
            self.gain += (self.target_gain - self.gain) / remaining;
            self.wet += (self.target_wet - self.wet) / remaining;
            self.ramp -= 1;
        }
    }

    fn filter_channels(&mut self) -> f64 {
        let mut peak = 0_f64;
        for (channel, sample) in self.frame.iter_mut().enumerate() {
            let mut value = *sample as f64 * self.gain;
            for (history, coefficients) in self.histories[channel].iter_mut().zip(self.coefficients)
            {
                value = history.process(value, coefficients);
            }
            *sample = value as f32;
            peak = peak.max(value.abs());
        }
        peak
    }

    fn mix_output(&mut self, peak: f64) {
        // Instant attack and an 80 ms release, linked across channels to
        // preserve the stereo image. Inactive in settled bypass.
        let required = if peak > 1. { 1. / peak } else { 1. };
        self.limiter = required.min(1. - (1. - self.limiter) * self.release);
        let stereo = self.frame.len() == 2;
        for (channel, (sample, dry)) in self.frame.iter_mut().zip(&self.dry).enumerate() {
            let processed = (*sample as f64 * self.limiter).clamp(-1., 1.);
            // Balance attenuates one stereo channel, independently of EQ bypass.
            // Mono and multichannel sources retain their original channel layout.
            let gain = if !stereo {
                1.
            } else if channel == 0 {
                1. - self.balance.max(0.)
            } else {
                1. + self.balance.min(0.)
            };
            *sample = ((*dry as f64 * (1. - self.wet) + processed * self.wet) * gain) as f32;
        }
    }
}
impl<S: Source> Iterator for Equalized<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.cursor == self.frame.len() {
            for (sample, dry) in self.frame.iter_mut().zip(&mut self.dry) {
                *sample = self.inner.next()?;
                *dry = *sample;
            }
            self.process_frame();
            self.cursor = 0;
        }
        let sample = self.frame[self.cursor];
        self.cursor += 1;
        Some(sample)
    }
}
impl<S: Source> Source for Equalized<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner
            .current_span_len()
            .map(|n| n + self.frame.len() - self.cursor)
    }
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(position)?;
        self.histories.fill([History::default(); 10]);
        self.cursor = self.frame.len();
        self.limiter = 1.;
        Ok(())
    }
}

/// Small-signal filter response for the UI graph, excluding preamp/limiting.
pub fn response_db(bands: [f32; 10], rate: u32, frequency: f64) -> f64 {
    let omega = std::f64::consts::TAU * frequency / rate.max(1) as f64;
    let magnitude = |c0: f64, c1: f64, c2: f64| {
        let real = c0 + c1 * omega.cos() + c2 * (2. * omega).cos();
        let imag = c1 * omega.sin() + c2 * (2. * omega).sin();
        real * real + imag * imag
    };
    FREQUENCIES
        .into_iter()
        .zip(bands)
        .map(|(f, gain)| {
            let [b0, b1, b2, a1, a2] = Coefficients::peak(f, rate as f64, gain).0;
            10. * (magnitude(b0, b1, b2) / magnitude(1., a1, a2)).log10()
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;
    fn source(
        data: Vec<f32>,
        channels: u16,
        rate: u32,
        p: Parameters,
    ) -> (Equalized<SamplesBuffer>, Control) {
        let control = Control::default();
        control.set(p);
        (
            Equalized::new(
                SamplesBuffer::new(channels.try_into().unwrap(), rate.try_into().unwrap(), data),
                control.clone(),
            ),
            control,
        )
    }
    fn tone(rate: u32, frequency: f64, amplitude: f64) -> Vec<f32> {
        (0..rate)
            .map(|i| {
                (amplitude * (std::f64::consts::TAU * frequency * i as f64 / rate as f64).sin())
                    as f32
            })
            .collect()
    }
    fn rms(samples: &[f32]) -> f64 {
        (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    }
    #[test]
    fn balance_routes_stereo_independently_of_eq_and_ramps_live_changes() {
        for enabled in [false, true] {
            for balance in [-1., 0., 1.] {
                let control = Control::default();
                control.set(Parameters {
                    enabled,
                    ..Parameters::default()
                });
                control.set_balance(balance);
                let input = [0.25, -0.5].repeat(2048);
                let output = Equalized::new(
                    SamplesBuffer::new(2.try_into().unwrap(), 48000.try_into().unwrap(), input),
                    control,
                )
                .collect::<Vec<_>>();
                for frame in output.chunks_exact(2) {
                    assert_eq!(frame[0], 0.25 * (1. - balance.max(0.)));
                    assert_eq!(frame[1], -0.5 * (1. + balance.min(0.)));
                }
            }
        }
        let (mut eq, control) = source(vec![0.5; 10000], 2, 48000, Parameters::default());
        assert_eq!(eq.next(), Some(0.5));
        assert_eq!(eq.next(), Some(0.5));
        control.set_balance(1.);
        let output = eq.collect::<Vec<_>>();
        let left: Vec<_> = output
            .chunks_exact(2)
            .map(|f| {
                assert_eq!(f[1], 0.5);
                f[0]
            })
            .collect();
        assert!(
            left.windows(2)
                .all(|w| w[1] <= w[0] && (w[1] - w[0]).abs() < 0.001)
        );
        assert!(left[1100..].iter().all(|v| *v == 0.));
    }
    #[test]
    fn balance_leaves_mono_and_multichannel_unchanged_and_sanitizes_values() {
        for channels in [1, 6] {
            let control = Control::default();
            control.set_balance(-1.);
            let input = vec![0.25; channels as usize * 100];
            let output = Equalized::new(
                SamplesBuffer::new(
                    channels.try_into().unwrap(),
                    48000.try_into().unwrap(),
                    input.clone(),
                ),
                control,
            )
            .collect::<Vec<_>>();
            assert_eq!(output, input);
        }
        let control = Control::default();
        for (input, expected) in [(f32::NAN, 0.), (f32::INFINITY, 0.), (-9., -1.), (9., 1.)] {
            control.set_balance(input);
            assert_eq!(control.0.read().unwrap().1, expected);
        }
    }
    #[test]
    fn bypass_and_enabled_flat_preserve_the_samples() {
        let input = tone(48000, 997., 0.7);
        for p in [
            Parameters {
                bands_db: [12.; 10],
                preamp_db: 12.,
                ..Parameters::default()
            },
            Parameters {
                enabled: true,
                ..Parameters::default()
            },
        ] {
            let (eq, _) = source(input.clone(), 1, 48000, p);
            assert_eq!(eq.collect::<Vec<_>>(), input);
        }
    }
    #[test]
    fn each_band_has_the_requested_center_gain_at_common_sample_rates() {
        for rate in [44100, 48000, 96000] {
            for (band, frequency) in FREQUENCIES.into_iter().enumerate() {
                for gain in [-12., 6., 12.] {
                    let mut p = Parameters {
                        enabled: true,
                        auto_headroom: false,
                        ..Parameters::default()
                    };
                    p.bands_db[band] = gain;
                    let input = tone(rate, frequency, 0.01);
                    let (eq, _) = source(input.clone(), 1, rate, p);
                    let output: Vec<_> = eq.collect();
                    let start = rate as usize / 2;
                    let measured = 20. * (rms(&output[start..]) / rms(&input[start..])).log10();
                    assert!(
                        (measured - gain as f64).abs() < 0.08,
                        "{rate} Hz / {frequency} Hz: {measured} instead of {gain}"
                    );
                }
            }
        }
    }
    #[test]
    fn preamp_and_automatic_headroom_change_measured_gain() {
        let input = tone(48000, 1000., 0.01);
        let mut p = Parameters {
            enabled: true,
            auto_headroom: false,
            preamp_db: -6.,
            ..Parameters::default()
        };
        p.bands_db[4] = 6.;
        for (auto, expected) in [(false, 0.), (true, -6.)] {
            p.auto_headroom = auto;
            let (eq, _) = source(input.clone(), 1, 48000, p);
            let output: Vec<_> = eq.collect();
            let measured = 20. * (rms(&output[24000..]) / rms(&input[24000..])).log10();
            assert!((measured - expected).abs() < 0.05);
        }
    }
    #[test]
    fn channel_histories_are_independent_and_peak_protection_is_linked() {
        let input: Vec<_> = tone(48000, 1000., 0.9)
            .into_iter()
            .flat_map(|x| [x, 0.])
            .collect();
        let p = Parameters {
            enabled: true,
            auto_headroom: false,
            preamp_db: 12.,
            bands_db: [12.; 10],
        };
        let (eq, _) = source(input, 2, 48000, p);
        let output: Vec<_> = eq.collect();
        assert!(output.iter().all(|x| x.is_finite() && x.abs() <= 1.));
        assert!(output.chunks_exact(2).all(|frame| frame[1] == 0.));
        let input: Vec<_> = tone(48000, 1000., 0.9)
            .into_iter()
            .flat_map(|x| [x, x * 0.5])
            .collect();
        let (eq, _) = source(input, 2, 48000, p);
        assert!(
            eq.collect::<Vec<_>>()
                .chunks_exact(2)
                .all(|f| (f[1] - f[0] * 0.5).abs() < 1e-6)
        );
    }
    #[test]
    fn low_rate_streams_skip_bands_above_nyquist() {
        let mut p = Parameters {
            enabled: true,
            auto_headroom: false,
            ..Parameters::default()
        };
        p.bands_db[9] = 12.;
        let input = tone(8000, 1000., 0.3);
        let (eq, _) = source(input.clone(), 1, 8000, p);
        assert_eq!(eq.collect::<Vec<_>>(), input);
    }
    #[test]
    fn live_preamp_and_bypass_ramp_without_an_abrupt_step() {
        let p = Parameters {
            enabled: true,
            auto_headroom: false,
            ..Parameters::default()
        };
        let (mut eq, control) = source(vec![0.1; 12000], 1, 48000, p);
        for _ in 0..1000 {
            eq.next();
        }
        control.set(Parameters {
            preamp_db: 12.,
            ..p
        });
        let output: Vec<_> = eq.by_ref().take(2000).collect();
        assert!(output.windows(2).all(|w| (w[1] - w[0]).abs() < 0.001));
        assert!((output[1999] - 0.398107).abs() < 1e-5);
        control.set(Parameters {
            enabled: false,
            preamp_db: 12.,
            ..p
        });
        let output: Vec<_> = eq.take(2000).collect();
        assert!(output.windows(2).all(|w| (w[1] - w[0]).abs() < 0.001));
        assert_eq!(output[1999], 0.1);
    }
    #[test]
    fn rapid_band_changes_remain_finite_and_settle_to_the_final_response() {
        let p = Parameters {
            enabled: true,
            auto_headroom: false,
            ..Parameters::default()
        };
        let input = tone(48000, 1000., 0.01);
        let (mut eq, control) = source(input.clone(), 1, 48000, p);
        let mut previous = 0.;
        for i in 0..12000 {
            if i % 300 == 0 {
                control.set(Parameters {
                    bands_db: [if i % 600 == 0 { 12. } else { -12. }; 10],
                    ..p
                });
            }
            let sample = eq.next().unwrap();
            assert!(sample.is_finite() && sample.abs() <= 1.);
            assert!((sample - previous).abs() < 0.1, "discontinuity at {i}");
            previous = sample;
        }
        control.set(p);
        let rest: Vec<_> = eq.collect();
        assert!(
            rest[12000..]
                .iter()
                .zip(&input[24000..])
                .all(|(a, b)| (a - b).abs() < 1e-5)
        );
    }
    #[test]
    fn seeking_clears_filter_history_and_preserves_source_metadata() {
        let mut input = vec![0.; 48000];
        input[0] = 0.1;
        let p = Parameters {
            enabled: true,
            bands_db: [6.; 10],
            ..Parameters::default()
        };
        let (mut eq, _) = source(input, 1, 48000, p);
        assert_eq!(eq.channels().get(), 1);
        assert_eq!(eq.sample_rate().get(), 48000);
        assert_eq!(eq.total_duration(), Some(Duration::from_secs(1)));
        eq.next();
        eq.try_seek(Duration::from_millis(500)).unwrap();
        assert!(eq.take(1000).all(|sample| sample == 0.));
    }
}
