//! Whisper's log-mel spectrogram: 25 ms Hann windows every 10 ms over 30 s of
//! 16 kHz audio (3000 frames), projected onto the model's mel filter bank.

const N_FFT: usize = 400;
const HOP: usize = 160;
const BINS: usize = N_FFT / 2 + 1;
/// Samples in one 30-second window.
pub(crate) const CHUNK: usize = 30 * 16_000;
/// Mel frames in one window.
pub(crate) const FRAMES: usize = CHUNK / HOP;

/// Log-mel features for one window: `n_mels` rows of [`FRAMES`] values.
/// `pcm` is at most [`CHUNK`] samples and is padded with silence.
pub(crate) fn log_mel(pcm: &[f32], filters: &[f32], n_mels: usize) -> Vec<f32> {
    let n = pcm.len().min(CHUNK);
    // torch.stft(center=True): reflect-pad by N_FFT/2 on both sides.
    let half = N_FFT / 2;
    let mut padded = vec![0.0f32; CHUNK + N_FFT];
    padded[half..half + n].copy_from_slice(&pcm[..n]);
    for k in 1..=half {
        padded[half - k] = padded[half + k];
        padded[half + CHUNK - 1 + k] = padded[half + CHUNK - 1 - k];
    }

    let window: Vec<f32> =
        (0..N_FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos()).collect();
    let (cos, sin) = twiddles();
    // Frames past the audio (plus one window of slack) are pure silence:
    // their spectrum is zero, so skip the transform for them.
    let live = ((n + N_FFT) / HOP + 1).min(FRAMES);
    let mut power = vec![0.0f32; FRAMES * BINS];
    let mut frame = [0.0f32; N_FFT];
    for f in 0..live {
        let start = f * HOP;
        for (j, s) in frame.iter_mut().enumerate() {
            *s = padded[start + j] * window[j];
        }
        let out = &mut power[f * BINS..(f + 1) * BINS];
        for (k, o) in out.iter_mut().enumerate() {
            let (c, s) = (&cos[k * N_FFT..(k + 1) * N_FFT], &sin[k * N_FFT..(k + 1) * N_FFT]);
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for j in 0..N_FFT {
                re += frame[j] * c[j];
                im += frame[j] * s[j];
            }
            *o = re * re + im * im;
        }
    }

    let mut mel = vec![0.0f32; n_mels * FRAMES];
    let mut max = f32::MIN;
    for m in 0..n_mels {
        let fb = &filters[m * BINS..(m + 1) * BINS];
        for f in 0..FRAMES {
            let p = &power[f * BINS..(f + 1) * BINS];
            let v: f32 = fb.iter().zip(p).map(|(a, b)| a * b).sum();
            let v = v.max(1e-10).log10();
            max = max.max(v);
            mel[m * FRAMES + f] = v;
        }
    }
    for v in &mut mel {
        *v = (v.max(max - 8.0) + 4.0) / 4.0;
    }
    mel
}

fn twiddles() -> (Vec<f32>, Vec<f32>) {
    let mut cos = vec![0.0f32; BINS * N_FFT];
    let mut sin = vec![0.0f32; BINS * N_FFT];
    for k in 0..BINS {
        for j in 0..N_FFT {
            // Reduce k*j modulo N before converting so the angle stays exact.
            let a = 2.0 * std::f64::consts::PI * ((k * j) % N_FFT) as f64 / N_FFT as f64;
            cos[k * N_FFT + j] = a.cos() as f32;
            sin[k * N_FFT + j] = -a.sin() as f32;
        }
    }
    (cos, sin)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tone_lands_in_the_right_bin() {
        // A 1 kHz tone: bin 1000 / (16000 / 400) = 25. Use an identity-like
        // "filter bank" that picks bins 20..30 so the peak row is checked.
        let pcm: Vec<f32> = (0..16_000).map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 16_000.0).sin()).collect();
        let n_mels = 10;
        let mut filters = vec![0.0f32; n_mels * BINS];
        for m in 0..n_mels {
            filters[m * BINS + 20 + m] = 1.0;
        }
        let mel = log_mel(&pcm, &filters, n_mels);
        let frame = 50;
        let peak = (0..n_mels).max_by(|&a, &b| mel[a * FRAMES + frame].total_cmp(&mel[b * FRAMES + frame]));
        assert_eq!(peak, Some(5));
        // Silence after the tone sits at the clamped floor.
        assert!(mel[5 * FRAMES + 2000] < mel[5 * FRAMES + frame]);
    }
}
