//! Offline speech-to-text for Akshara Office.
//!
//! The engine runs OpenAI's Whisper models on the CPU, reading the single-file
//! model format used by whisper.cpp (`ggml-base.bin`, `ggml-small-q5_1.bin`,
//! …). The file carries the hyper-parameters, the mel filter bank, the
//! vocabulary and the weights, so nothing else needs to be installed and
//! nothing is downloaded: the user picks a model file once in Settings.
//!
//! Microphone capture (feature `capture`, on by default; Windows, macOS and
//! Linux) records the default input device through cpal and converts it to
//! 16 kHz mono for the model. Elsewhere [`Recorder::start`] reports that
//! recording is not available.
//!
//! ```no_run
//! use office_voice::{Options, Whisper};
//! let model = Whisper::load(std::path::Path::new("ggml-base.bin"))?;
//! let pcm = vec![0.0f32; 16_000]; // one second of 16 kHz mono audio
//! let t = model.transcribe(&pcm, &Options { language: Some("te".into()), ..Options::default() })?;
//! println!("{} ({})", t.text, t.language);
//! # Ok::<(), office_voice::Error>(())
//! ```

mod audio;
#[cfg_attr(not(all(feature = "capture", any(target_os = "linux", target_os = "windows", target_os = "macos"))), path = "no_capture.rs")]
mod capture;
mod ggml;
mod languages;
mod mel;
mod whisper;

pub use audio::{read_wav, resample, SAMPLE_RATE};
pub use capture::{Recorder, CAN_RECORD, MAX_RECORDING_SECONDS};
pub use languages::{language_name, LANGUAGES, VOICE_LANGUAGES};
pub use whisper::{probe, ModelInfo, Options, Transcript, Whisper};

use std::fmt;

/// Why loading a model, recording or transcribing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl Error {
    pub(crate) fn new(msg: impl Into<String>) -> Self {
        Error(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<candle_core::Error> for Error {
    fn from(e: candle_core::Error) -> Self {
        Error(format!("speech engine: {e}"))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;
