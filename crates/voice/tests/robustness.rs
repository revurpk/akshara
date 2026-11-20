//! Damaged or hostile model and audio files fail cleanly.

use office_voice::{read_wav, resample, Whisper};
use proptest::prelude::*;

/// A whisper.cpp header followed by `rest`.
fn with_header(rest: &[u8]) -> Vec<u8> {
    let mut d = 0x6767_6d6cu32.to_le_bytes().to_vec();
    for v in [51_865u32, 1500, 384, 6, 4, 448, 384, 6, 4, 80, 1] {
        d.extend(v.to_le_bytes());
    }
    d.extend_from_slice(rest);
    d
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn model_reader_never_panics(rest in prop::collection::vec(any::<u8>(), 0..4096)) {
        let data = with_header(&rest);
        prop_assert!(Whisper::from_reader(&data[..], data.len() as u64).is_err());
        prop_assert!(Whisper::from_reader(&rest[..], rest.len() as u64).is_err());
    }

    #[test]
    fn wav_reader_never_panics(data in prop::collection::vec(any::<u8>(), 0..2048)) {
        let mut wav = b"RIFF\0\0\0\0WAVE".to_vec();
        wav.extend(&data);
        let _ = read_wav(&wav);
        let _ = read_wav(&data);
    }

    #[test]
    fn resampling_lengths(n in 0usize..5000, from in 8000u32..96_000, to in 8000u32..48_000) {
        let out = resample(&vec![0.25; n], from, to);
        let expected = (n as f64 * to as f64 / from as f64).floor() as usize;
        prop_assert!(out.len().abs_diff(expected) <= 1);
        prop_assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-5));
    }
}

#[test]
fn huge_size_fields_do_not_allocate() {
    // Filter bank claims 80 x 201 floats but the file ends: no allocation of
    // the claimed size, just an error.
    let data = with_header(&[80, 0, 0, 0, 201, 0, 0, 0]);
    let e = Whisper::from_reader(&data[..], data.len() as u64).err().map(|e| e.0).unwrap_or_default();
    assert!(e.contains("truncated"), "{e}");
    // A tensor claiming 2^24 x 2^24 elements.
    assert!(Whisper::from_reader(&data[..], u64::MAX).is_err());
}
