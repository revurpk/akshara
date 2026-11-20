//! Stand-in recorder for platforms without microphone support in Akshara.

use crate::{Error, Result};

/// Whether this build can record from a microphone.
pub const CAN_RECORD: bool = false;

/// Recording stops by itself after this long.
pub const MAX_RECORDING_SECONDS: u32 = 10 * 60;

/// A recording in progress (never created on this platform).
pub struct Recorder(());

impl Recorder {
    /// Always fails: recording is not available on this platform.
    pub fn start() -> Result<Self> {
        Err(Error::new("voice recording is not available on this device"))
    }

    /// The latest input peak level.
    pub fn level(&self) -> f32 {
        0.0
    }

    /// Seconds recorded so far.
    pub fn seconds(&self) -> f32 {
        0.0
    }

    /// An error reported while recording.
    pub fn error(&self) -> Option<String> {
        None
    }

    /// Stops recording.
    pub fn finish(self) -> Result<Vec<f32>> {
        Ok(Vec::new())
    }
}
