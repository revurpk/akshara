//! Voice input and AI chat operations. Both run in background threads: the
//! UI starts a session or job, polls it, and stops or cancels it, so no call
//! blocks the UI for longer than a moment.
//!
//! - `voice.*`: microphone recording and offline Whisper transcription. The
//!   model is loaded once per path and kept, so later dictations start fast.
//! - `ai.*`: chat with a local Ollama server (loopback only, see office-ai).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use office_ai::{Endpoint, Message};
use office_odf::OdfError;
use office_voice::{Options, Recorder, Whisper};
use serde_json::{json, Map, Value};

type R<T> = Result<T, OdfError>;

fn bad(msg: impl Into<String>) -> OdfError {
    OdfError::Invalid(msg.into())
}

fn str_arg<'a>(a: &'a Map<String, Value>, key: &str) -> R<&'a str> {
    a.get(key).and_then(Value::as_str).ok_or_else(|| bad(format!("missing argument '{key}'")))
}

fn id_arg(a: &Map<String, Value>, key: &str) -> R<u64> {
    a.get(key).and_then(Value::as_u64).ok_or_else(|| bad(format!("missing argument '{key}'")))
}

fn next_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Most chat messages / characters accepted in one request.
const MAX_MESSAGES: usize = 500;
const MAX_CHAT_CHARS: usize = 1 << 20;

/// Handles `voice.*` and `ai.*`; `None` for other ops.
pub fn dispatch(op: &str, a: &Map<String, Value>) -> Option<R<Value>> {
    Some(match op {
        "voice.available" => Ok(json!({ "record": office_voice::CAN_RECORD, "languages": office_voice::VOICE_LANGUAGES })),
        "voice.model_info" => {
            str_arg(a, "path").and_then(|p| office_voice::probe(Path::new(p)).map(|i| json!(i)).map_err(|e| bad(e.0)))
        }
        "voice.start" => voice_start(a),
        "voice.transcribe_file" => voice_file(a),
        "voice.status" => voice_status(a),
        "voice.stop" => voice_stop(a),
        "voice.cancel" => voice_cancel(a),
        "ai.models" => (|| {
            let ep = endpoint(a)?;
            let models = office_ai::list_models(&ep).map_err(ai_err)?;
            Ok(json!({ "url": ep.url(), "models": models }))
        })(),
        "ai.chat" => ai_chat(a),
        "ai.poll" => ai_poll(a),
        "ai.cancel" => ai_cancel(a),
        _ => return None,
    })
}

// ---- voice -------------------------------------------------------------------

enum Phase {
    Recording(Recorder),
    Transcribing,
    Done { text: String, language: String },
    Failed(String),
}

struct Session {
    phase: Phase,
    model: PathBuf,
    options: Options,
    cancel: Arc<AtomicBool>,
}

fn sessions() -> &'static Mutex<HashMap<u64, Session>> {
    static S: OnceLock<Mutex<HashMap<u64, Session>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The loaded model and the file it came from.
type ModelCache = Mutex<Option<(PathBuf, Arc<Whisper>)>>;

/// Returns the model for `path`, loading it (once) if needed.
fn model(path: &Path) -> Result<Arc<Whisper>, String> {
    static CACHE: OnceLock<ModelCache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    // Holding the lock while loading makes concurrent callers wait for one
    // load instead of reading the file twice.
    let mut c = lock(cache);
    if let Some((p, m)) = c.as_ref() {
        if p == path {
            return Ok(Arc::clone(m));
        }
    }
    *c = None; // free the previous model first
    let m = Arc::new(Whisper::load(path).map_err(|e| e.0)?);
    *c = Some((path.to_path_buf(), Arc::clone(&m)));
    Ok(m)
}

fn options(a: &Map<String, Value>) -> Options {
    let language = a.get("language").and_then(Value::as_str).filter(|l| !l.is_empty() && *l != "auto").map(str::to_string);
    let detect_among = a
        .get("among")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_else(|| office_voice::VOICE_LANGUAGES.iter().map(|s| s.to_string()).collect());
    Options { language, detect_among, cancel: None }
}

fn voice_start(a: &Map<String, Value>) -> R<Value> {
    let path = PathBuf::from(str_arg(a, "model")?);
    office_voice::probe(&path).map_err(|e| bad(e.0))?;
    let recorder = Recorder::start().map_err(|e| bad(e.0))?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut opts = options(a);
    opts.cancel = Some(Arc::clone(&cancel));
    let id = next_id();
    // Only one dictation at a time: a new one cancels older sessions.
    {
        let mut s = lock(sessions());
        for old in s.values() {
            old.cancel.store(true, Ordering::Relaxed);
        }
        s.retain(|_, v| matches!(v.phase, Phase::Transcribing));
        s.insert(id, Session { phase: Phase::Recording(recorder), model: path.clone(), options: opts, cancel });
    }
    // Load the model while the user speaks.
    std::thread::spawn(move || {
        let _ = model(&path);
    });
    Ok(json!({ "session": id }))
}

fn voice_file(a: &Map<String, Value>) -> R<Value> {
    let path = PathBuf::from(str_arg(a, "model")?);
    office_voice::probe(&path).map_err(|e| bad(e.0))?;
    let audio = str_arg(a, "path")?;
    let limits = office_odf::Limits { max_archive_size: 512 << 20, ..Default::default() };
    let bytes = office_odf::read_file_limited(Path::new(audio), &limits)?;
    let (pcm, rate) = office_voice::read_wav(&bytes).map_err(|e| bad(e.0))?;
    let pcm = office_voice::resample(&pcm, rate, office_voice::SAMPLE_RATE);
    let cancel = Arc::new(AtomicBool::new(false));
    let mut opts = options(a);
    opts.cancel = Some(Arc::clone(&cancel));
    let id = next_id();
    lock(sessions()).insert(id, Session { phase: Phase::Transcribing, model: path.clone(), options: opts.clone(), cancel });
    std::thread::spawn(move || transcribe(id, path, pcm, opts));
    Ok(json!({ "session": id }))
}

fn transcribe(id: u64, path: PathBuf, pcm: Vec<f32>, opts: Options) {
    let result = model(&path).and_then(|m| m.transcribe(&pcm, &opts).map_err(|e| e.0));
    let mut s = lock(sessions());
    if let Some(session) = s.get_mut(&id) {
        session.phase = match result {
            Ok(t) => Phase::Done { text: t.text, language: t.language },
            Err(e) => Phase::Failed(e),
        };
    }
}

fn voice_status(a: &Map<String, Value>) -> R<Value> {
    let id = id_arg(a, "session")?;
    let mut s = lock(sessions());
    let session = s.get(&id).ok_or_else(|| bad("no such voice session"))?;
    let v = match &session.phase {
        Phase::Recording(r) => match r.error() {
            Some(e) => json!({ "state": "error", "error": format!("recording failed: {e}") }),
            None => json!({ "state": "recording", "level": r.level(), "seconds": r.seconds() }),
        },
        Phase::Transcribing => json!({ "state": "transcribing" }),
        Phase::Done { text, language } => json!({ "state": "done", "text": text, "language": language }),
        Phase::Failed(e) => json!({ "state": "error", "error": e }),
    };
    // Finished sessions are reported once, then forgotten.
    if matches!(session.phase, Phase::Done { .. } | Phase::Failed(_)) {
        s.remove(&id);
    }
    Ok(v)
}

fn voice_stop(a: &Map<String, Value>) -> R<Value> {
    let id = id_arg(a, "session")?;
    let (recorder, path, opts) = {
        let mut s = lock(sessions());
        let session = s.get_mut(&id).ok_or_else(|| bad("no such voice session"))?;
        match std::mem::replace(&mut session.phase, Phase::Transcribing) {
            Phase::Recording(r) => (r, session.model.clone(), session.options.clone()),
            other => {
                session.phase = other;
                return Ok(Value::Null);
            }
        }
    };
    std::thread::spawn(move || match recorder.finish() {
        Ok(pcm) => transcribe(id, path, pcm, opts),
        Err(e) => {
            if let Some(session) = lock(sessions()).get_mut(&id) {
                session.phase = Phase::Failed(e.0);
            }
        }
    });
    Ok(Value::Null)
}

fn voice_cancel(a: &Map<String, Value>) -> R<Value> {
    let id = id_arg(a, "session")?;
    if let Some(session) = lock(sessions()).remove(&id) {
        session.cancel.store(true, Ordering::Relaxed);
        // Dropping a recorder stops the microphone.
    }
    Ok(Value::Null)
}

// ---- AI chat -----------------------------------------------------------------

struct Job {
    text: String,
    done: bool,
    error: Option<String>,
    cancel: Arc<AtomicBool>,
}

fn jobs() -> &'static Mutex<HashMap<u64, Job>> {
    static J: OnceLock<Mutex<HashMap<u64, Job>>> = OnceLock::new();
    J.get_or_init(Default::default)
}

fn endpoint(a: &Map<String, Value>) -> R<Endpoint> {
    let url = a.get("url").and_then(Value::as_str).unwrap_or(office_ai::DEFAULT_URL);
    Endpoint::parse(url).map_err(ai_err)
}

fn ai_err(e: office_ai::Error) -> OdfError {
    match e {
        office_ai::Error::NotLocal(_) => bad(e.to_string()),
        _ => OdfError::Io(std::io::Error::other(e.to_string())),
    }
}

fn ai_chat(a: &Map<String, Value>) -> R<Value> {
    let ep = endpoint(a)?;
    let model = str_arg(a, "model")?.to_string();
    let list = a.get("messages").and_then(Value::as_array).ok_or_else(|| bad("missing argument 'messages'"))?;
    if list.len() > MAX_MESSAGES {
        return Err(OdfError::limit("the conversation is too long"));
    }
    let mut messages = Vec::with_capacity(list.len() + 1);
    if a.get("document").and_then(Value::as_bool).unwrap_or(true) {
        messages.push(Message::system(office_ai::DOCUMENT_PROMPT));
    }
    let mut chars = 0;
    for m in list {
        let role = m.get("role").and_then(Value::as_str).unwrap_or("user");
        if !matches!(role, "user" | "assistant" | "system") {
            return Err(bad(format!("unknown chat role '{role}'")));
        }
        let content = m.get("content").and_then(Value::as_str).unwrap_or("");
        chars += content.len();
        messages.push(Message { role: role.to_string(), content: content.to_string() });
    }
    if chars > MAX_CHAT_CHARS {
        return Err(OdfError::limit("the conversation is too long"));
    }
    let id = next_id();
    let cancel = Arc::new(AtomicBool::new(false));
    lock(jobs()).insert(id, Job { text: String::new(), done: false, error: None, cancel: Arc::clone(&cancel) });
    std::thread::spawn(move || {
        let result = office_ai::chat(&ep, &model, &messages, |piece| {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            match lock(jobs()).get_mut(&id) {
                Some(job) => {
                    job.text.push_str(piece);
                    true
                }
                None => false,
            }
        });
        if let Some(job) = lock(jobs()).get_mut(&id) {
            job.done = true;
            if let Err(e) = result {
                job.error = Some(e.to_string());
            }
        }
    });
    Ok(json!({ "job": id }))
}

fn ai_poll(a: &Map<String, Value>) -> R<Value> {
    let id = id_arg(a, "job")?;
    let mut j = lock(jobs());
    let job = j.get(&id).ok_or_else(|| bad("no such chat request"))?;
    let v = json!({ "text": job.text, "done": job.done, "error": job.error });
    if job.done {
        j.remove(&id);
    }
    Ok(v)
}

fn ai_cancel(a: &Map<String, Value>) -> R<Value> {
    let id = id_arg(a, "job")?;
    if let Some(job) = lock(jobs()).remove(&id) {
        job.cancel.store(true, Ordering::Relaxed);
    }
    Ok(Value::Null)
}
