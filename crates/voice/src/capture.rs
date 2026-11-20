//! Microphone capture through cpal. The stream lives on its own thread (cpal
//! streams are not `Send` on every platform); the callback mixes to mono and
//! appends to a shared buffer, and keeps a running input level for the UI.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};

use crate::audio::{resample, SAMPLE_RATE};
use crate::{Error, Result};

/// Whether this build can record from a microphone.
pub const CAN_RECORD: bool = true;

/// Recording stops by itself after this long.
pub const MAX_RECORDING_SECONDS: u32 = 10 * 60;

#[derive(Default)]
struct Shared {
    samples: Mutex<Vec<f32>>,
    /// Peak level of the latest callback, as `f32` bits.
    level: AtomicU32,
    failed: Mutex<Option<String>>,
    full: AtomicBool,
}

/// A recording in progress. Dropping it discards the audio.
pub struct Recorder {
    shared: Arc<Shared>,
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<()>>,
    rate: u32,
}

impl Recorder {
    /// Starts recording from the default input device.
    pub fn start() -> Result<Self> {
        let shared = Arc::new(Shared::default());
        let (stop, stopped) = mpsc::channel::<()>();
        let (ready, started) = mpsc::channel::<Result<u32>>();
        let s = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("akshara-mic".into())
            .spawn(move || {
                let stream = match open(&s) {
                    Ok((stream, rate)) => {
                        let _ = ready.send(Ok(rate));
                        stream
                    }
                    Err(e) => {
                        let _ = ready.send(Err(e));
                        return;
                    }
                };
                let _ = stopped.recv_timeout(Duration::from_secs(MAX_RECORDING_SECONDS.into()));
                drop(stream);
            })
            .map_err(|e| Error::new(format!("cannot start recording: {e}")))?;
        let rate = started
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| Error::new("the microphone did not respond"))??;
        Ok(Recorder { shared, stop, thread: Some(thread), rate })
    }

    /// The latest input peak level, 0.0 to 1.0.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::Relaxed))
    }

    /// Seconds recorded so far.
    pub fn seconds(&self) -> f32 {
        let n = self.shared.samples.lock().map(|s| s.len()).unwrap_or(0);
        n as f32 / self.rate.max(1) as f32
    }

    /// An error reported by the audio device while recording, if any.
    pub fn error(&self) -> Option<String> {
        self.shared.failed.lock().ok().and_then(|f| f.clone())
    }

    /// Stops recording and returns the audio as 16 kHz mono.
    pub fn finish(mut self) -> Result<Vec<f32>> {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        if let Some(e) = self.error() {
            return Err(Error::new(format!("recording failed: {e}")));
        }
        let pcm = std::mem::take(&mut *self.shared.samples.lock().map_err(|_| Error::new("recording failed"))?);
        Ok(resample(&pcm, self.rate, SAMPLE_RATE))
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(shared: &Arc<Shared>) -> Result<(cpal::Stream, u32)> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| Error::new("no microphone was found"))?;
    let config = device
        .default_input_config()
        .map_err(|e| Error::new(format!("the microphone cannot be used: {e}")))?;
    let rate = config.sample_rate();
    let channels = config.channels().max(1) as usize;
    let stream_config = config.config();
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, stream_config, channels, rate, shared, |s| s),
        SampleFormat::I16 => build::<i16>(&device, stream_config, channels, rate, shared, |s| s as f32 / 32_768.0),
        SampleFormat::U16 => build::<u16>(&device, stream_config, channels, rate, shared, |s| (s as f32 - 32_768.0) / 32_768.0),
        SampleFormat::I32 => build::<i32>(&device, stream_config, channels, rate, shared, |s| s as f32 / 2_147_483_648.0),
        SampleFormat::U8 => build::<u8>(&device, stream_config, channels, rate, shared, |s| (s as f32 - 128.0) / 128.0),
        SampleFormat::F64 => build::<f64>(&device, stream_config, channels, rate, shared, |s| s as f32),
        other => return Err(Error::new(format!("unsupported microphone sample format {other}"))),
    }?;
    stream.play().map_err(|e| Error::new(format!("cannot start the microphone: {e}")))?;
    Ok((stream, rate))
}

fn build<T: SizedSample + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    channels: usize,
    rate: u32,
    shared: &Arc<Shared>,
    to_f32: fn(T) -> f32,
) -> Result<cpal::Stream> {
    let s = Arc::clone(shared);
    let e = Arc::clone(shared);
    let limit = rate as usize * MAX_RECORDING_SECONDS as usize;
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut peak = 0.0f32;
                if let Ok(mut buf) = s.samples.lock() {
                    if buf.len() >= limit {
                        s.full.store(true, Ordering::Relaxed);
                        return;
                    }
                    for frame in data.chunks(channels) {
                        let v = frame.iter().map(|&x| to_f32(x)).sum::<f32>() / channels as f32;
                        peak = peak.max(v.abs());
                        buf.push(v);
                    }
                }
                s.level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
            },
            move |err| {
                if let Ok(mut f) = e.failed.lock() {
                    f.get_or_insert_with(|| err.to_string());
                }
            },
            None,
        )
        .map_err(|e| Error::new(format!("cannot open the microphone: {e}")))
}
