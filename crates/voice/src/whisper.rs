//! The Whisper encoder-decoder and greedy decoding, written against candle's
//! CPU tensors. Weight names follow OpenAI's checkpoints, which is what
//! whisper.cpp files contain (`encoder.blocks.0.attn.query.weight`, …).
//! Matrix weights stay quantized when the file is quantized.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use candle_core::quantized::{QMatMul, QTensor};
use candle_core::{DType, Device, Module, Tensor, D};

use crate::ggml::{self, HParams};
use crate::languages::{self, LANGUAGES};
use crate::mel;
use crate::{Error, Result};

/// How to transcribe.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Whisper language code ("en", "te", "hi", "sa", …); `None` detects it.
    pub language: Option<String>,
    /// When detecting, only consider these codes (empty: any language).
    pub detect_among: Vec<String>,
    /// Set to stop early; the text so far is returned.
    pub cancel: Option<Arc<AtomicBool>>,
}

/// The result of a transcription.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// The recognised text, trimmed.
    pub text: String,
    /// The language code used (given or detected).
    pub language: String,
}

/// What a loaded model is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ModelInfo {
    /// "tiny", "base", "small", "medium", "large" or "custom".
    pub size: &'static str,
    /// False for English-only (`.en`) models.
    pub multilingual: bool,
    /// Whether the matrix weights are quantized.
    pub quantized: bool,
}

/// A loaded Whisper model. Loading takes a moment and the model is read-only
/// afterwards, so callers keep one around and share it.
pub struct Whisper {
    hp: HParams,
    filters: Vec<f32>,
    vocab: Vec<Vec<u8>>,
    tok: Specials,
    info: ModelInfo,
    encoder: Encoder,
    decoder: Decoder,
}

/// Special token ids, computed from the vocabulary size as whisper.cpp does.
#[derive(Debug, Clone, Copy)]
struct Specials {
    eot: u32,
    sot: u32,
    transcribe: u32,
    no_timestamps: u32,
    timestamp_begin: u32,
    n_languages: usize,
}

impl Specials {
    fn new(n_vocab: usize) -> Self {
        let multi = usize::from(n_vocab >= 51_865);
        let n_languages = (n_vocab - 51_765 - multi) * multi;
        let dt = if multi == 1 { n_languages as u32 - 98 } else { 0 };
        Specials {
            eot: 50_256 + multi as u32,
            sot: 50_257 + multi as u32,
            transcribe: 50_358 + dt,
            no_timestamps: 50_362 + dt,
            timestamp_begin: 50_363 + dt,
            n_languages,
        }
    }

    fn language(&self, code: &str) -> Option<u32> {
        let i = languages::index(code)?;
        (i < self.n_languages).then(|| self.sot + 1 + i as u32)
    }
}

struct Linear {
    w: QMatMul,
    b: Option<Tensor>,
}

impl Linear {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let y = self.w.forward(x)?;
        Ok(match &self.b {
            Some(b) => y.broadcast_add(b)?,
            None => y,
        })
    }
}

struct Norm {
    w: Tensor,
    b: Tensor,
}

impl Norm {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let mean = x.mean_keepdim(D::Minus1)?;
        let xc = x.broadcast_sub(&mean)?;
        let var = xc.sqr()?.mean_keepdim(D::Minus1)?;
        let xn = xc.broadcast_div(&var.affine(1.0, 1e-5)?.sqrt()?)?;
        Ok(xn.broadcast_mul(&self.w)?.broadcast_add(&self.b)?)
    }
}

struct Attention {
    query: Linear,
    key: Linear,
    value: Linear,
    out: Linear,
    heads: usize,
}

struct Block {
    attn_ln: Norm,
    attn: Attention,
    cross: Option<(Norm, Attention)>,
    mlp_ln: Norm,
    mlp1: Linear,
    mlp2: Linear,
}

struct Encoder {
    conv1: (Tensor, Tensor),
    conv2: (Tensor, Tensor),
    pos: Tensor,
    blocks: Vec<Block>,
    ln_post: Norm,
}

struct Decoder {
    embed: Tensor,
    pos: Tensor,
    blocks: Vec<Block>,
    ln: Norm,
}

/// Per-layer keys/values: self-attention grows by one row per token, the
/// cross-attention pair is computed once from the audio.
struct Cache {
    selfs: Vec<Option<(Tensor, Tensor)>>,
    cross: Vec<(Tensor, Tensor)>,
}

struct Weights {
    t: HashMap<String, QTensor>,
}

impl Weights {
    fn take(&mut self, name: &str) -> Result<QTensor> {
        self.t.remove(name).ok_or_else(|| Error::new(format!("the model file has no tensor '{name}'")))
    }

    fn tensor(&mut self, name: &str) -> Result<Tensor> {
        Ok(self.take(name)?.dequantize(&Device::Cpu)?.to_dtype(DType::F32)?)
    }

    /// A bias or norm weight, flattened to one dimension.
    fn vector(&mut self, name: &str) -> Result<Tensor> {
        Ok(self.tensor(name)?.flatten_all()?)
    }

    fn linear(&mut self, name: &str, bias: bool) -> Result<Linear> {
        let w = QMatMul::from_qtensor(self.take(&format!("{name}.weight"))?)?;
        let b = if bias { Some(self.vector(&format!("{name}.bias"))?) } else { None };
        Ok(Linear { w, b })
    }

    fn norm(&mut self, name: &str) -> Result<Norm> {
        Ok(Norm { w: self.vector(&format!("{name}.weight"))?, b: self.vector(&format!("{name}.bias"))? })
    }

    fn attention(&mut self, name: &str, heads: usize) -> Result<Attention> {
        Ok(Attention {
            query: self.linear(&format!("{name}.query"), true)?,
            key: self.linear(&format!("{name}.key"), false)?,
            value: self.linear(&format!("{name}.value"), true)?,
            out: self.linear(&format!("{name}.out"), true)?,
            heads,
        })
    }

    fn block(&mut self, name: &str, heads: usize, cross: bool) -> Result<Block> {
        Ok(Block {
            attn_ln: self.norm(&format!("{name}.attn_ln"))?,
            attn: self.attention(&format!("{name}.attn"), heads)?,
            cross: if cross {
                Some((self.norm(&format!("{name}.cross_attn_ln"))?, self.attention(&format!("{name}.cross_attn"), heads)?))
            } else {
                None
            },
            mlp_ln: self.norm(&format!("{name}.mlp_ln"))?,
            mlp1: self.linear(&format!("{name}.mlp.0"), true)?,
            mlp2: self.linear(&format!("{name}.mlp.2"), true)?,
        })
    }

    fn conv(&mut self, name: &str) -> Result<(Tensor, Tensor)> {
        let w = self.tensor(&format!("{name}.weight"))?;
        let b = self.vector(&format!("{name}.bias"))?;
        let n = b.dim(0)?;
        Ok((w, b.reshape((1, n, 1))?))
    }
}

/// Reads only the header of a model file: fast enough to check a file the
/// user picked without loading the weights.
pub fn probe(path: &Path) -> Result<ModelInfo> {
    use std::io::Read as _;
    let mut f = std::fs::File::open(path)
        .map_err(|e| Error::new(format!("cannot open the speech model {}: {e}", path.display())))?;
    let mut head = [0u8; 48];
    f.read_exact(&mut head).map_err(|_| Error::new("not a whisper.cpp model file (ggml-*.bin)"))?;
    let word = |i: usize| u32::from_le_bytes([head[i * 4], head[i * 4 + 1], head[i * 4 + 2], head[i * 4 + 3]]) as usize;
    if word(0) != 0x6767_6d6c {
        return Err(Error::new("not a whisper.cpp model file (ggml-*.bin)"));
    }
    let (n_vocab, layers, state, ftype) = (word(1), word(5), word(3), word(11));
    if !(51_864..=51_866).contains(&n_vocab) || word(2) != 1500 {
        return Err(Error::new("unsupported Whisper model parameters"));
    }
    Ok(ModelInfo { size: size_name(layers, state), multilingual: n_vocab >= 51_865, quantized: ftype % 1000 > 1 })
}

fn size_name(layers: usize, state: usize) -> &'static str {
    match (layers, state) {
        (4, 384) => "tiny",
        (6, 512) => "base",
        (12, 768) => "small",
        (24, 1024) => "medium",
        (32, 1280) => "large",
        _ => "custom",
    }
}

/// Upper bound on tokens produced for one 30-second window.
const MAX_TOKENS: usize = 224;

impl Whisper {
    /// Loads a whisper.cpp model file.
    pub fn load(path: &Path) -> Result<Self> {
        let f = std::fs::File::open(path)
            .map_err(|e| Error::new(format!("cannot open the speech model {}: {e}", path.display())))?;
        let len = f.metadata()?.len();
        Self::from_reader(f, len)
    }

    /// Loads a model from `len` bytes of `r`.
    pub fn from_reader(r: impl Read, len: u64) -> Result<Self> {
        let file = ggml::read(r, len)?;
        let hp = file.hp;
        let quantized = file
            .tensors
            .get("encoder.blocks.0.attn.query.weight")
            .is_some_and(|t| !matches!(t.dtype(), candle_core::quantized::GgmlDType::F16 | candle_core::quantized::GgmlDType::F32));
        let mut w = Weights { t: file.tensors };
        let encoder = Encoder {
            conv1: w.conv("encoder.conv1")?,
            conv2: w.conv("encoder.conv2")?,
            pos: w.tensor("encoder.positional_embedding")?,
            blocks: (0..hp.n_audio_layer)
                .map(|i| w.block(&format!("encoder.blocks.{i}"), hp.n_audio_head, false))
                .collect::<Result<_>>()?,
            ln_post: w.norm("encoder.ln_post")?,
        };
        let decoder = Decoder {
            embed: w.tensor("decoder.token_embedding.weight")?,
            pos: w.tensor("decoder.positional_embedding")?,
            blocks: (0..hp.n_text_layer)
                .map(|i| w.block(&format!("decoder.blocks.{i}"), hp.n_text_head, true))
                .collect::<Result<_>>()?,
            ln: w.norm("decoder.ln")?,
        };
        if encoder.pos.dims() != [hp.n_audio_ctx, hp.n_audio_state]
            || decoder.embed.dims() != [hp.n_vocab, hp.n_text_state]
            || decoder.pos.dim(0)? < hp.n_text_ctx
        {
            return Err(Error::new("the model file's tensors do not match its header"));
        }
        let size = size_name(hp.n_audio_layer, hp.n_audio_state);
        let tok = Specials::new(hp.n_vocab);
        let info = ModelInfo { size, multilingual: tok.n_languages > 0, quantized };
        Ok(Whisper { hp, filters: file.filters, vocab: file.vocab, tok, info, encoder, decoder })
    }

    /// What kind of model this is.
    pub fn info(&self) -> &ModelInfo {
        &self.info
    }

    /// Transcribes 16 kHz mono samples. Audio longer than 30 seconds is
    /// processed in 30-second windows.
    pub fn transcribe(&self, pcm: &[f32], opts: &Options) -> Result<Transcript> {
        let lang = match &opts.language {
            Some(code) => {
                if !self.info.multilingual && code != "en" {
                    return Err(Error::new("this speech model only understands English; choose a multilingual model"));
                }
                code.clone()
            }
            None if !self.info.multilingual => "en".to_string(),
            None => String::new(),
        };
        let mut lang = lang;
        let mut text = String::new();
        let mut start = 0;
        while start < pcm.len() {
            if cancelled(opts) {
                break;
            }
            let end = (start + mel::CHUNK).min(pcm.len());
            let chunk = &pcm[start..end];
            start = end;
            // Skip windows that are (nearly) silent: Whisper invents text
            // for silence.
            if chunk.len() < 1600 || rms(chunk) < 1e-3 {
                continue;
            }
            let features = mel::log_mel(chunk, &self.filters, self.hp.n_mels);
            let audio = self.encode(&features)?;
            let mut cache = self.cache(&audio)?;
            if lang.is_empty() {
                lang = self.detect(&mut cache, &opts.detect_among)?;
            }
            let piece = self.decode(&mut cache, &lang, opts)?;
            let piece = piece.trim();
            if !piece.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(piece);
            }
        }
        if lang.is_empty() {
            lang = opts.detect_among.first().cloned().unwrap_or_else(|| "en".into());
        }
        Ok(Transcript { text, language: lang })
    }

    fn encode(&self, features: &[f32]) -> Result<Tensor> {
        let e = &self.encoder;
        let x = Tensor::from_slice(features, (1, self.hp.n_mels, mel::FRAMES), &Device::Cpu)?;
        let x = x.conv1d(&e.conv1.0, 1, 1, 1, 1)?.broadcast_add(&e.conv1.1)?.gelu_erf()?;
        let x = x.conv1d(&e.conv2.0, 1, 2, 1, 1)?.broadcast_add(&e.conv2.1)?.gelu_erf()?;
        // (1, state, 1500) -> (1500, state)
        let mut x = x.squeeze(0)?.t()?.contiguous()?.broadcast_add(&e.pos)?;
        for b in &e.blocks {
            let h = b.attn_ln.forward(&x)?;
            let q = b.attn.query.forward(&h)?;
            let k = b.attn.key.forward(&h)?;
            let v = b.attn.value.forward(&h)?;
            x = (x + b.attn.out.forward(&attend(&q, &k, &v, b.attn.heads, None)?)?)?;
            x = (&x + mlp(b, &x)?)?;
        }
        e.ln_post.forward(&x)
    }

    fn cache(&self, audio: &Tensor) -> Result<Cache> {
        let mut cross = Vec::with_capacity(self.decoder.blocks.len());
        for b in &self.decoder.blocks {
            let Some((_, a)) = &b.cross else { return Err(Error::new("decoder block without cross-attention")) };
            cross.push((a.key.forward(audio)?, a.value.forward(audio)?));
        }
        Ok(Cache { selfs: vec![None; self.decoder.blocks.len()], cross })
    }

    /// Runs `tokens` (positions `offset..`) through the decoder; returns the
    /// logits for the last position.
    fn step(&self, cache: &mut Cache, tokens: &[u32], offset: usize) -> Result<Vec<f32>> {
        let d = &self.decoder;
        let n = tokens.len();
        if offset + n > self.hp.n_text_ctx {
            return Err(Error::new("transcription too long for the model"));
        }
        let ids = Tensor::new(tokens, &Device::Cpu)?;
        let mut x = d.embed.index_select(&ids, 0)?.broadcast_add(&d.pos.narrow(0, offset, n)?)?;
        let mask = if n > 1 {
            let total = offset + n;
            let m: Vec<f32> = (0..n)
                .flat_map(|i| (0..total).map(move |j| if j > offset + i { f32::NEG_INFINITY } else { 0.0 }))
                .collect();
            Some(Tensor::from_vec(m, (n, total), &Device::Cpu)?)
        } else {
            None
        };
        for (l, b) in d.blocks.iter().enumerate() {
            let h = b.attn_ln.forward(&x)?;
            let q = b.attn.query.forward(&h)?;
            let mut k = b.attn.key.forward(&h)?;
            let mut v = b.attn.value.forward(&h)?;
            if let Some((pk, pv)) = &cache.selfs[l] {
                k = Tensor::cat(&[pk, &k], 0)?;
                v = Tensor::cat(&[pv, &v], 0)?;
            }
            x = (x + b.attn.out.forward(&attend(&q, &k, &v, b.attn.heads, mask.as_ref())?)?)?;
            cache.selfs[l] = Some((k, v));
            if let Some((ln, a)) = &b.cross {
                let h = ln.forward(&x)?;
                let q = a.query.forward(&h)?;
                let (ck, cv) = &cache.cross[l];
                x = (x + a.out.forward(&attend(&q, ck, cv, a.heads, None)?)?)?;
            }
            x = (&x + mlp(b, &x)?)?;
        }
        let last = d.ln.forward(&x.narrow(0, n - 1, 1)?)?;
        let logits = last.matmul(&d.embed.t()?)?;
        Ok(logits.flatten_all()?.to_vec1::<f32>()?)
    }

    fn detect(&self, cache: &mut Cache, among: &[String]) -> Result<String> {
        let logits = self.step(cache, &[self.tok.sot], 0)?;
        cache.selfs.iter_mut().for_each(|s| *s = None);
        let candidates: Vec<&str> = if among.is_empty() {
            LANGUAGES[..self.tok.n_languages].to_vec()
        } else {
            among.iter().map(String::as_str).collect()
        };
        let best = candidates
            .iter()
            .filter_map(|&c| self.tok.language(c).map(|t| (c, logits[t as usize])))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        Ok(best.map_or("en", |b| b.0).to_string())
    }

    fn decode(&self, cache: &mut Cache, lang: &str, opts: &Options) -> Result<String> {
        let mut prompt = vec![self.tok.sot];
        if self.info.multilingual {
            let l = self.tok.language(lang).ok_or_else(|| Error::new(format!("the model does not know language '{lang}'")))?;
            prompt.extend([l, self.tok.transcribe]);
        }
        prompt.push(self.tok.no_timestamps);
        let mut out: Vec<u32> = Vec::new();
        let mut logits = self.step(cache, &prompt, 0)?;
        let mut pos = prompt.len();
        let space = self.space_token();
        while out.len() < MAX_TOKENS && pos < self.hp.n_text_ctx && !cancelled(opts) {
            // Only text tokens or end-of-text; no blank or end at the start.
            let first = out.is_empty();
            let next = logits
                .iter()
                .enumerate()
                .filter(|&(t, _)| {
                    let t = t as u32;
                    (t < self.tok.eot || (t == self.tok.eot && !first)) && !(first && Some(t) == space)
                })
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(self.tok.eot, |(t, _)| t as u32);
            if next == self.tok.eot || next >= self.tok.timestamp_begin {
                break;
            }
            out.push(next);
            if repeating(&out) {
                break;
            }
            logits = self.step(cache, &[next], pos)?;
            pos += 1;
        }
        trim_repeats(&mut out);
        let bytes: Vec<u8> = out.iter().filter_map(|&t| self.vocab.get(t as usize)).flatten().copied().collect();
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    fn space_token(&self) -> Option<u32> {
        self.vocab.iter().position(|w| w == b" ").map(|i| i as u32)
    }
}

fn mlp(b: &Block, x: &Tensor) -> Result<Tensor> {
    let h = b.mlp_ln.forward(x)?;
    b.mlp2.forward(&b.mlp1.forward(&h)?.gelu_erf()?)
}

/// Multi-head scaled dot-product attention on (rows, state) matrices.
fn attend(q: &Tensor, k: &Tensor, v: &Tensor, heads: usize, mask: Option<&Tensor>) -> Result<Tensor> {
    let (nq, state) = q.dims2()?;
    let nk = k.dim(0)?;
    let dh = state / heads;
    let scale = (dh as f64).powf(-0.25);
    let split = |t: &Tensor, n: usize| -> Result<Tensor> {
        Ok(t.reshape((n, heads, dh))?.transpose(0, 1)?.contiguous()?.affine(scale, 0.0)?)
    };
    let q = split(q, nq)?;
    let kt = split(k, nk)?.transpose(1, 2)?.contiguous()?;
    let v = v.reshape((nk, heads, dh))?.transpose(0, 1)?.contiguous()?;
    let mut w = q.matmul(&kt)?;
    if let Some(m) = mask {
        w = w.broadcast_add(m)?;
    }
    let max = w.max_keepdim(D::Minus1)?;
    let e = w.broadcast_sub(&max)?.exp()?;
    let w = e.broadcast_div(&e.sum_keepdim(D::Minus1)?)?;
    Ok(w.matmul(&v)?.transpose(0, 1)?.contiguous()?.reshape((nq, state))?)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

fn cancelled(opts: &Options) -> bool {
    opts.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed))
}

/// True when the tail of `t` is one short pattern repeated many times, the
/// usual way greedy Whisper decoding gets stuck.
fn repeating(t: &[u32]) -> bool {
    (1..=12).any(|p| {
        let reps = if p <= 2 { 8 } else { 4 };
        t.len() >= p * reps && (1..reps).all(|r| t[t.len() - p * r - p..t.len() - p * r] == t[t.len() - p..])
    })
}

/// Drops the repeated tail found by [`repeating`], keeping one copy.
fn trim_repeats(t: &mut Vec<u32>) {
    for p in 1..=12 {
        let reps = if p <= 2 { 8 } else { 4 };
        if t.len() >= p * reps && (1..reps).all(|r| t[t.len() - p * r - p..t.len() - p * r] == t[t.len() - p..]) {
            let mut n = t.len();
            while n >= 2 * p && t[n - 2 * p..n - p] == t[n - p..n] {
                n -= p;
            }
            t.truncate(n);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_tokens() {
        let m = Specials::new(51_865);
        assert_eq!((m.eot, m.sot, m.transcribe, m.no_timestamps, m.timestamp_begin), (50_257, 50_258, 50_359, 50_363, 50_364));
        assert_eq!(m.language("en"), Some(50_259));
        assert_eq!(m.language("te"), Some(50_299));
        assert_eq!(m.language("yue"), None);
        let v3 = Specials::new(51_866);
        assert_eq!((v3.transcribe, v3.no_timestamps), (50_360, 50_364));
        assert_eq!(v3.language("yue"), Some(50_358));
        let en = Specials::new(51_864);
        assert_eq!((en.eot, en.sot, en.no_timestamps, en.n_languages), (50_256, 50_257, 50_362, 0));
    }

    #[test]
    fn repetition_guard() {
        let mut t = vec![1, 2, 3, 4, 5, 6, 4, 5, 6, 4, 5, 6, 4, 5, 6];
        assert!(repeating(&t));
        trim_repeats(&mut t);
        assert_eq!(t, [1, 2, 3, 4, 5, 6]);
        assert!(!repeating(&[1, 2, 3, 1, 2, 3]));
    }
}
