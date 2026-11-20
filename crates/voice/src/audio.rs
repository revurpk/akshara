//! Sample-rate conversion and WAV reading.

use crate::{Error, Result};

/// The sample rate Whisper expects.
pub const SAMPLE_RATE: u32 = 16_000;

/// Converts mono samples from `from` Hz to `to` Hz. Each output sample is the
/// mean of the input samples it covers when downsampling (a box low-pass,
/// enough for speech), and a linear interpolation when upsampling.
pub fn resample(pcm: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || pcm.is_empty() || from == 0 || to == 0 {
        return pcm.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let n = ((pcm.len() as f64) / ratio).floor() as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let centre = (i as f64 + 0.5) * ratio;
        if ratio > 1.0 {
            let a = (centre - ratio / 2.0).max(0.0) as usize;
            let b = ((centre + ratio / 2.0).ceil() as usize).clamp(a + 1, pcm.len());
            out.push(pcm[a..b].iter().sum::<f32>() / (b - a) as f32);
        } else {
            let x = (centre - 0.5).max(0.0);
            let j = (x as usize).min(pcm.len() - 1);
            let f = (x - j as f64) as f32;
            let next = pcm[(j + 1).min(pcm.len() - 1)];
            out.push(pcm[j] * (1.0 - f) + next * f);
        }
    }
    out
}

/// Reads a PCM WAV file (8/16/24/32-bit integer or 32-bit float, any channel
/// count) as mono samples; returns them with the file's sample rate.
pub fn read_wav(data: &[u8]) -> Result<(Vec<f32>, u32)> {
    let bad = || Error::new("not a PCM WAV file");
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(bad());
    }
    let u16_at = |i: usize| u16::from_le_bytes([data[i], data[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
    let mut pos = 12;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let len = u32_at(pos + 4) as usize;
        let body = pos + 8;
        let end = body.saturating_add(len).min(data.len());
        if id == b"fmt " && end - body >= 16 {
            let mut format = u16_at(body);
            // WAVE_FORMAT_EXTENSIBLE: the real format starts the sub-format GUID.
            if format == 0xFFFE && end - body >= 26 {
                format = u16_at(body + 24);
            }
            fmt = Some((format, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            let (format, channels, rate, bits) = fmt.ok_or_else(bad)?;
            let channels = channels.max(1) as usize;
            let bytes = (bits as usize).div_ceil(8);
            if bytes == 0 || bytes > 4 {
                return Err(bad());
            }
            let float = format == 3;
            let frame = bytes * channels;
            let body = &data[body..end];
            let mut out = Vec::with_capacity(body.len() / frame);
            for f in body.chunks_exact(frame) {
                let mut sum = 0.0f32;
                for c in f.chunks_exact(bytes) {
                    sum += match (bytes, float) {
                        (4, true) => f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                        (1, _) => (c[0] as f32 - 128.0) / 128.0,
                        (2, _) => i16::from_le_bytes([c[0], c[1]]) as f32 / 32_768.0,
                        (3, _) => (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0,
                        _ => i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0,
                    };
                }
                out.push(sum / channels as f32);
            }
            return Ok((out, rate));
        }
        pos = body.saturating_add(len + (len & 1));
    }
    Err(bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_length_and_level() {
        let pcm = vec![0.5f32; 48_000];
        let out = resample(&pcm, 48_000, 16_000);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-6));
        let up = resample(&out, 16_000, 44_100);
        assert_eq!(up.len(), 44_100);
    }

    #[test]
    fn wav_round_trip() {
        let samples: [i16; 4] = [0, 16_384, -16_384, 32_767];
        let mut w = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        w.extend(16u32.to_le_bytes());
        w.extend(1u16.to_le_bytes()); // PCM
        w.extend(1u16.to_le_bytes()); // mono
        w.extend(8_000u32.to_le_bytes());
        w.extend(16_000u32.to_le_bytes());
        w.extend(2u16.to_le_bytes());
        w.extend(16u16.to_le_bytes());
        w.extend(b"data");
        w.extend(8u32.to_le_bytes());
        for s in samples {
            w.extend(s.to_le_bytes());
        }
        let (pcm, rate) = read_wav(&w).expect("wav");
        assert_eq!(rate, 8_000);
        assert_eq!(pcm.len(), 4);
        assert!((pcm[1] - 0.5).abs() < 1e-4);
        assert!(read_wav(b"RIFF").is_err());
    }
}
