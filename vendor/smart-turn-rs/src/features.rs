use anyhow::{Result, bail};
use ndarray::{Array2, Array3, Axis, s};
use once_cell::sync::Lazy;
use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use std::sync::Arc;

pub const SAMPLE_RATE: usize = 16_000;
const N_FFT: usize = 400;
const HOP_LENGTH: usize = 160;
const N_MELS: usize = 80;
const MEL_FLOOR: f32 = 1e-10;
const MAX_FREQUENCY: f32 = 8000.0;

static WINDOW: Lazy<Vec<f32>> = Lazy::new(|| hann_window(N_FFT));
static MEL_FILTERS: Lazy<Array2<f32>> = Lazy::new(build_mel_filters);
static FFT: Lazy<Arc<dyn Fft<f32>>> = Lazy::new(|| {
    let mut planner = FftPlanner::<f32>::new();
    planner.plan_fft_forward(N_FFT)
});

pub fn log_mel_spectrogram(audio: &[f32]) -> Result<Array3<f32>> {
    let padded = reflect_pad(audio, N_FFT / 2)?;
    let power_spec = stft_power(&padded);
    let mel_spec = MEL_FILTERS.dot(&power_spec);
    if mel_spec.len_of(Axis(1)) < 2 {
        bail!("Not enough frames to match Whisper's expectations");
    }
    let mut mel_spec = mel_spec.slice(s![.., ..-1]).to_owned();
    apply_dynamic_range(&mut mel_spec);
    Ok(mel_spec.insert_axis(Axis(0)))
}

fn stft_power(padded_audio: &[f32]) -> Array2<f32> {
    let num_frames = 1 + (padded_audio.len() - N_FFT) / HOP_LENGTH;
    let freq_bins = (N_FFT / 2) + 1;
    let mut spec = Array2::<f32>::zeros((freq_bins, num_frames));
    let fft = FFT.as_ref();
    let mut buffer = vec![Complex32::new(0.0, 0.0); N_FFT];
    let mut offset = 0;

    for frame_idx in 0..num_frames {
        for i in 0..N_FFT {
            buffer[i].re = padded_audio[offset + i] * WINDOW[i];
            buffer[i].im = 0.0;
        }
        fft.process(&mut buffer);
        for bin in 0..freq_bins {
            spec[(bin, frame_idx)] = buffer[bin].norm_sqr();
        }
        offset += HOP_LENGTH;
    }

    spec
}

fn apply_dynamic_range(spec: &mut Array2<f32>) {
    let mut max_val = f32::NEG_INFINITY;
    for value in spec.iter_mut() {
        let logged = value.max(MEL_FLOOR).log10();
        *value = logged;
        if logged > max_val {
            max_val = logged;
        }
    }
    let floor = max_val - 8.0;
    for value in spec.iter_mut() {
        let clamped = if *value < floor { floor } else { *value };
        *value = (clamped + 4.0) * 0.25;
    }
}

fn hann_window(length: usize) -> Vec<f32> {
    let mut window = Vec::with_capacity(length);
    for n in 0..length {
        let value = (std::f32::consts::PI * n as f32 / (length - 1) as f32)
            .sin()
            .powi(2);
        window.push(value);
    }
    window
}

fn reflect_pad(signal: &[f32], pad: usize) -> Result<Vec<f32>> {
    if pad == 0 {
        return Ok(signal.to_vec());
    }
    if signal.len() <= pad {
        bail!("Signal must be longer than pad length");
    }

    let mut padded = Vec::with_capacity(signal.len() + 2 * pad);
    padded.extend(signal[1..=pad].iter().rev().copied());
    padded.extend_from_slice(signal);
    let tail_start = signal.len() - pad - 1;
    let tail_end = signal.len() - 1;
    padded.extend(signal[tail_start..tail_end].iter().rev().copied());
    Ok(padded)
}

fn build_mel_filters() -> Array2<f32> {
    let freq_bins = (N_FFT / 2) + 1;
    let fft_freqs = linspace(0.0, (SAMPLE_RATE as f32) / 2.0, freq_bins);
    let mel_points = linspace(0.0, hz_to_mel(MAX_FREQUENCY), N_MELS + 2);
    let filter_freqs: Vec<f32> = mel_points.iter().map(|&mel| mel_to_hz(mel)).collect();

    let mut filters = Array2::<f32>::zeros((N_MELS, freq_bins));
    for mel_index in 0..N_MELS {
        let left = filter_freqs[mel_index];
        let center = filter_freqs[mel_index + 1];
        let right = filter_freqs[mel_index + 2];

        for (bin, &freq) in fft_freqs.iter().enumerate() {
            let weight = if freq >= left && freq <= center {
                (freq - left) / (center - left)
            } else if freq >= center && freq <= right {
                (right - freq) / (right - center)
            } else {
                0.0
            };
            filters[(mel_index, bin)] = weight.max(0.0);
        }
    }

    for m in 0..N_MELS {
        let enorm = 2.0 / (filter_freqs[m + 2] - filter_freqs[m]);
        for bin in 0..freq_bins {
            filters[(m, bin)] *= enorm;
        }
    }

    filters
}

fn linspace(start: f32, end: f32, points: usize) -> Vec<f32> {
    if points < 2 {
        return vec![start];
    }
    let step = (end - start) / (points as f32 - 1.0);
    (0..points).map(|i| start + i as f32 * step).collect()
}

fn hz_to_mel(freq: f32) -> f32 {
    const F_SP: f32 = 200.0 / 3.0;
    const MIN_LOG_HZ: f32 = 1000.0;
    const MIN_LOG_MEL: f32 = MIN_LOG_HZ / F_SP;
    let log_step = 6.4f32.ln() / 27.0;

    if freq < MIN_LOG_HZ {
        freq / F_SP
    } else {
        MIN_LOG_MEL + (freq / MIN_LOG_HZ).ln() / log_step
    }
}

fn mel_to_hz(mel: f32) -> f32 {
    const F_SP: f32 = 200.0 / 3.0;
    const MIN_LOG_HZ: f32 = 1000.0;
    const MIN_LOG_MEL: f32 = MIN_LOG_HZ / F_SP;
    let log_step = 6.4f32.ln() / 27.0;

    if mel < MIN_LOG_MEL {
        mel * F_SP
    } else {
        MIN_LOG_HZ * f32::exp(log_step * (mel - MIN_LOG_MEL))
    }
}
