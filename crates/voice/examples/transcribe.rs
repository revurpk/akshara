//! Transcribes a WAV file: `cargo run --release -p office-voice --example
//! transcribe -- ggml-base.bin speech.wav [language]`.

use std::time::Instant;

use office_voice::{read_wav, resample, Options, Whisper, SAMPLE_RATE};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [model, wav, rest @ ..] = args.as_slice() else {
        eprintln!("usage: transcribe MODEL.bin AUDIO.wav [language]");
        std::process::exit(2);
    };
    let t = Instant::now();
    let model = Whisper::load(model.as_ref())?;
    eprintln!("loaded {:?} in {:.2?}", model.info(), t.elapsed());
    let (pcm, rate) = read_wav(&std::fs::read(wav)?)?;
    let pcm = resample(&pcm, rate, SAMPLE_RATE);
    let t = Instant::now();
    let opts = Options { language: rest.first().cloned(), ..Options::default() };
    let out = model.transcribe(&pcm, &opts)?;
    eprintln!("transcribed {:.1} s of audio in {:.2?}", pcm.len() as f32 / SAMPLE_RATE as f32, t.elapsed());
    println!("[{}] {}", out.language, out.text);
    Ok(())
}
