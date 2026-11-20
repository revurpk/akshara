//! Reader for whisper.cpp model files ("ggml" format, magic `lmgg`).
//!
//! Layout (all little-endian):
//! - magic `0x67676d6c`, then 11 `i32` hyper-parameters;
//! - the mel filter bank: `n_mel`, `n_fft`, then `n_mel * n_fft` `f32`;
//! - the vocabulary: a count, then each token as a `u32` length and bytes;
//! - tensors until the end of the file: `n_dims`, name length, type, the
//!   dimensions (fastest first), the name, then the raw data.
//!
//! Every size is checked against the file length before anything is
//! allocated, so a damaged or hostile file fails cleanly.

use std::collections::HashMap;
use std::io::{BufReader, Read};

use candle_core::quantized::{ggml_file::qtensor_from_ggml, GgmlDType, QTensor};
use candle_core::Device;

use crate::{Error, Result};

const MAGIC: u32 = 0x6767_6d6c;

/// Hyper-parameters from the file header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HParams {
    pub n_vocab: usize,
    pub n_audio_ctx: usize,
    pub n_audio_state: usize,
    pub n_audio_head: usize,
    pub n_audio_layer: usize,
    pub n_text_ctx: usize,
    pub n_text_state: usize,
    pub n_text_head: usize,
    pub n_text_layer: usize,
    pub n_mels: usize,
}

/// A parsed model file.
pub(crate) struct ModelFile {
    pub hp: HParams,
    /// `n_mels` rows of `n_fft` (= 201) weights.
    pub filters: Vec<f32>,
    /// Text tokens as raw bytes, indexed by token id.
    pub vocab: Vec<Vec<u8>>,
    pub tensors: HashMap<String, QTensor>,
}

struct Input<R> {
    r: R,
    left: u64,
}

impl<R: Read> Input<R> {
    fn bytes(&mut self, n: u64) -> Result<Vec<u8>> {
        if n > self.left {
            return Err(Error::new("the model file is truncated or damaged"));
        }
        // Grow the buffer as data arrives instead of trusting `n` up front.
        let mut buf = Vec::with_capacity(n.min(1 << 20) as usize);
        (&mut self.r).take(n).read_to_end(&mut buf)?;
        if buf.len() as u64 != n {
            return Err(Error::new("the model file is truncated or damaged"));
        }
        self.left -= n;
        Ok(buf)
    }

    fn u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn count(&mut self, max: u32) -> Result<usize> {
        let v = self.u32()?;
        if v > max {
            return Err(Error::new("the model file has an impossible size field"));
        }
        Ok(v as usize)
    }
}

/// Reads a whole model from `r`, whose total length is `len` bytes.
pub(crate) fn read(r: impl Read, len: u64) -> Result<ModelFile> {
    let mut i = Input { r: BufReader::with_capacity(1 << 20, r), left: len };
    if len < 4 || i.u32()? != MAGIC {
        return Err(Error::new("not a whisper.cpp model file (ggml-*.bin)"));
    }
    let mut h = [0usize; 11];
    for v in &mut h {
        *v = i.count(1 << 20)?;
    }
    let hp = HParams {
        n_vocab: h[0],
        n_audio_ctx: h[1],
        n_audio_state: h[2],
        n_audio_head: h[3],
        n_audio_layer: h[4],
        n_text_ctx: h[5],
        n_text_state: h[6],
        n_text_head: h[7],
        n_text_layer: h[8],
        n_mels: h[9],
    };
    let ok = (51_864..=51_866).contains(&hp.n_vocab)
        && hp.n_audio_ctx == 1500
        && hp.n_text_ctx >= 8
        && hp.n_audio_head > 0
        && hp.n_text_head > 0
        && hp.n_audio_state % hp.n_audio_head == 0
        && hp.n_text_state % hp.n_text_head == 0
        && hp.n_audio_state == hp.n_text_state
        && (hp.n_mels == 80 || hp.n_mels == 128)
        && hp.n_audio_layer <= 64
        && hp.n_text_layer <= 64;
    if !ok {
        return Err(Error::new("unsupported Whisper model parameters"));
    }

    let n_mel = i.count(256)?;
    let n_fft = i.count(4096)?;
    if n_mel != hp.n_mels || n_fft != 201 {
        return Err(Error::new("unsupported mel filter bank in the model file"));
    }
    let raw = i.bytes((n_mel * n_fft * 4) as u64)?;
    let filters = raw.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();

    let n_words = i.count(hp.n_vocab as u32)?;
    let mut vocab = Vec::with_capacity(n_words);
    for _ in 0..n_words {
        let n = i.count(1024)?;
        vocab.push(i.bytes(n as u64)?);
    }

    let mut tensors = HashMap::new();
    while i.left > 0 {
        let n_dims = i.count(4)?;
        let name_len = i.count(512)?;
        let ty = i.u32()?;
        let dtype = dtype(ty)?;
        let mut dims = Vec::with_capacity(n_dims);
        for _ in 0..n_dims {
            dims.push(i.count(1 << 24)?);
        }
        dims.reverse();
        let name = String::from_utf8_lossy(&i.bytes(name_len as u64)?).into_owned();
        let elems = dims.iter().try_fold(1u64, |a, &d| a.checked_mul(d as u64)).unwrap_or(u64::MAX);
        if dims.is_empty() || elems % dtype.block_size() as u64 != 0 {
            return Err(Error::new(format!("bad tensor shape for '{name}' in the model file")));
        }
        let size = (elems / dtype.block_size() as u64).saturating_mul(dtype.type_size() as u64);
        let data = i.bytes(size)?;
        let t = qtensor_from_ggml(dtype, &data, dims, &Device::Cpu)?;
        tensors.insert(name, t);
    }
    Ok(ModelFile { hp, filters, vocab, tensors })
}

fn dtype(ty: u32) -> Result<GgmlDType> {
    Ok(match ty {
        0 => GgmlDType::F32,
        1 => GgmlDType::F16,
        2 => GgmlDType::Q4_0,
        3 => GgmlDType::Q4_1,
        6 => GgmlDType::Q5_0,
        7 => GgmlDType::Q5_1,
        8 => GgmlDType::Q8_0,
        10 => GgmlDType::Q2K,
        11 => GgmlDType::Q3K,
        12 => GgmlDType::Q4K,
        13 => GgmlDType::Q5K,
        14 => GgmlDType::Q6K,
        _ => return Err(Error::new(format!("unsupported tensor type {ty} in the model file"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        for data in [&b""[..], b"abcd", b"lmgg", &[0x6c, 0x6d, 0x67, 0x67, 1, 2, 3][..]] {
            assert!(read(data, data.len() as u64).is_err());
        }
        // A plausible header that claims more data than exists.
        let mut d = MAGIC.to_le_bytes().to_vec();
        for v in [51_865u32, 1500, 384, 6, 4, 448, 384, 6, 4, 80, 1, 80, 201] {
            d.extend(v.to_le_bytes());
        }
        let e = read(&d[..], d.len() as u64).err().map(|e| e.0).unwrap_or_default();
        assert!(e.contains("truncated"), "{e}");
    }
}
