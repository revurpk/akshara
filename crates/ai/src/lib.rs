//! A small client for a local [Ollama](https://ollama.com) server, used by
//! Akshara's AI chat to draft documents with models that run on the user's
//! own computer.
//!
//! Akshara does not use the network. The one exception is this client, which
//! the user turns on in Settings, and which only ever connects to the
//! loopback interface: [`Endpoint::parse`] refuses every host except
//! `localhost`, `127.0.0.1` and `[::1]`, and the address is resolved without
//! DNS. It speaks plain HTTP/1.1 over a `TcpStream`, with no TLS, proxy or
//! redirect support.
//!
//! ```no_run
//! use office_ai::{chat, list_models, Endpoint, Message};
//! let ep = Endpoint::parse("http://127.0.0.1:11434")?;
//! let models = list_models(&ep)?;
//! let reply = chat(&ep, &models[0].name, &[Message::user("Write a haiku")], |piece| {
//!     print!("{piece}");
//!     true // keep going
//! })?;
//! # Ok::<(), office_ai::Error>(())
//! ```

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The address Ollama listens on by default.
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

/// Largest response body accepted (the model list or one chat stream).
pub const MAX_RESPONSE: usize = 16 << 20;

/// Why a request failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The address is not a loopback HTTP URL.
    NotLocal(String),
    /// Nothing is listening: Ollama is not installed or not running.
    NotRunning,
    /// The server answered with an error.
    Server(String),
    /// The connection failed or the reply could not be read.
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotLocal(u) => write!(f, "only a local Ollama server can be used (http://127.0.0.1:PORT), not '{u}'"),
            Error::NotRunning => f.write_str("Ollama is not running on this computer"),
            Error::Server(m) => write!(f, "Ollama: {m}"),
            Error::Io(m) => write!(f, "connection to Ollama failed: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        match e.kind() {
            std::io::ErrorKind::ConnectionRefused => Error::NotRunning,
            _ => Error::Io(e.to_string()),
        }
    }
}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// A validated loopback server address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    addr: SocketAddr,
}

impl Endpoint {
    /// Parses `http://HOST[:PORT][/]` where HOST is `localhost`, `127.0.0.1`
    /// (or any `127.x.y.z`) or `[::1]`. The port defaults to 11434.
    pub fn parse(url: &str) -> Result<Endpoint> {
        let bad = || Error::NotLocal(url.chars().take(200).collect());
        let rest = url.trim().strip_prefix("http://").ok_or_else(bad)?;
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        if rest.contains(['/', '@', '?', '#']) {
            return Err(bad());
        }
        let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
            let (h, p) = v6.split_once(']').ok_or_else(bad)?;
            (h, p.strip_prefix(':'))
        } else {
            match rest.rsplit_once(':') {
                Some((h, p)) => (h, Some(p)),
                None => (rest, None),
            }
        };
        let port: u16 = match port {
            Some(p) => p.parse().ok().filter(|&p| p != 0).ok_or_else(bad)?,
            None => 11434,
        };
        let ip: IpAddr = if host.eq_ignore_ascii_case("localhost") {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            host.parse().map_err(|_| bad())?
        };
        if !ip.is_loopback() {
            return Err(bad());
        }
        Ok(Endpoint { addr: SocketAddr::new(ip, port) })
    }

    /// The URL in canonical form.
    pub fn url(&self) -> String {
        match self.addr.ip() {
            IpAddr::V6(_) => format!("http://[::1]:{}", self.addr.port()),
            IpAddr::V4(v4) => format!("http://{v4}:{}", self.addr.port()),
        }
    }

    fn host_header(&self) -> String {
        match self.addr.ip() {
            IpAddr::V6(v6) if v6 == Ipv6Addr::LOCALHOST => format!("[::1]:{}", self.addr.port()),
            ip => format!("{ip}:{}", self.addr.port()),
        }
    }
}

/// One chat message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// "system", "user" or "assistant".
    pub role: String,
    /// The text.
    pub content: String,
}

impl Message {
    /// A user message.
    pub fn user(content: impl Into<String>) -> Self {
        Message { role: "user".into(), content: content.into() }
    }

    /// A system prompt.
    pub fn system(content: impl Into<String>) -> Self {
        Message { role: "system".into(), content: content.into() }
    }

    /// An assistant reply.
    pub fn assistant(content: impl Into<String>) -> Self {
        Message { role: "assistant".into(), content: content.into() }
    }
}

/// An installed model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelEntry {
    /// The name to request, e.g. "llama3.2:3b".
    pub name: String,
    /// Size on disk in bytes.
    pub size: u64,
    /// e.g. "3.2B", when the server reports it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub parameters: String,
}

/// The system prompt for drafting documents: answers come back as Markdown,
/// which Akshara turns into a formatted Writer document.
pub const DOCUMENT_PROMPT: &str = "You are the writing assistant inside Akshara Office, a word processor. \
Answer in the language the user writes in. When the user asks for a document (a letter, report, essay, notes, \
an agenda, a plan…), reply with the document itself, formatted in Markdown: # headings, paragraphs, - or 1. lists, \
| tables |, **bold** and *italic*. Put diagrams in ```mermaid code blocks (flowchart, sequenceDiagram, classDiagram, \
stateDiagram-v2 or pie). Do not add remarks before or after the document. For other questions, answer briefly.";

/// Lists the models installed on the server (`GET /api/tags`).
pub fn list_models(ep: &Endpoint) -> Result<Vec<ModelEntry>> {
    let mut body = Vec::new();
    request(ep, "GET", "/api/tags", None, Duration::from_secs(10), |chunk| {
        body.extend_from_slice(chunk);
        true
    })?;
    let v: Value = serde_json::from_slice(&body).map_err(|e| Error::Io(format!("unexpected reply: {e}")))?;
    let mut models: Vec<ModelEntry> = v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    Some(ModelEntry {
                        name: m["name"].as_str().or_else(|| m["model"].as_str())?.to_string(),
                        size: m["size"].as_u64().unwrap_or(0),
                        parameters: m["details"]["parameter_size"].as_str().unwrap_or("").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    models.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(models)
}

/// Sends a chat (`POST /api/chat`, streamed) and calls `on_piece` with each
/// piece of the reply as it arrives; returning `false` stops the request.
/// Returns the whole reply.
pub fn chat(ep: &Endpoint, model: &str, messages: &[Message], mut on_piece: impl FnMut(&str) -> bool) -> Result<String> {
    let body = json!({ "model": model, "messages": messages, "stream": true }).to_string();
    let mut reply = String::new();
    let mut line = Vec::new();
    let mut failure: Option<Error> = None;
    let mut stopped = false;
    // Models can take minutes to load into memory before the first token.
    request(ep, "POST", "/api/chat", Some(&body), Duration::from_secs(600), |chunk| {
        for &b in chunk {
            if b != b'\n' {
                line.push(b);
                continue;
            }
            let keep = handle_line(&line, &mut reply, &mut failure, &mut on_piece);
            line.clear();
            if !keep {
                stopped = true;
                return false;
            }
        }
        true
    })?;
    if !stopped && !line.is_empty() {
        handle_line(&line, &mut reply, &mut failure, &mut on_piece);
    }
    match failure {
        Some(e) => Err(e),
        None => Ok(reply),
    }
}

fn handle_line(line: &[u8], reply: &mut String, failure: &mut Option<Error>, on_piece: &mut impl FnMut(&str) -> bool) -> bool {
    let Ok(v) = serde_json::from_slice::<Value>(line) else { return true };
    if let Some(e) = v["error"].as_str() {
        *failure = Some(Error::Server(e.to_string()));
        return false;
    }
    if let Some(piece) = v["message"]["content"].as_str() {
        if !piece.is_empty() {
            reply.push_str(piece);
            if !on_piece(piece) {
                return false;
            }
        }
    }
    !v["done"].as_bool().unwrap_or(false)
}

/// One HTTP/1.1 request; the decoded body is passed to `sink` in pieces
/// (stop by returning `false`). Non-2xx statuses become [`Error::Server`].
fn request(ep: &Endpoint, method: &str, path: &str, body: Option<&str>, read_timeout: Duration, mut sink: impl FnMut(&[u8]) -> bool) -> Result<()> {
    let mut s = TcpStream::connect_timeout(&ep.addr, Duration::from_secs(3))?;
    s.set_read_timeout(Some(read_timeout))?;
    s.set_write_timeout(Some(Duration::from_secs(30)))?;
    let body = body.unwrap_or("");
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Akshara-Office\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ep.host_header(),
        body.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(body.as_bytes())?;
    s.flush()?;

    let mut r = BufReader::new(s);
    let mut status = String::new();
    r.read_line(&mut status)?;
    let code: u16 = status.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| Error::Io("not an HTTP reply".into()))?;
    let mut chunked = false;
    let mut length: Option<usize> = None;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 {
            return Err(Error::Io("connection closed early".into()));
        }
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            if k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            } else if k == "content-length" {
                length = v.parse().ok();
            }
        }
    }

    if !(200..300).contains(&code) {
        let mut body = Vec::new();
        read_body(&mut r, chunked, length, |c| {
            body.extend_from_slice(c);
            body.len() < 64 * 1024
        })?;
        let text = String::from_utf8_lossy(&body);
        let msg = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"].as_str().map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {code}"));
        return Err(Error::Server(msg));
    }
    let mut total = 0usize;
    read_body(&mut r, chunked, length, |c| {
        total += c.len();
        total <= MAX_RESPONSE && sink(c)
    })
}

fn read_body(r: &mut impl BufRead, chunked: bool, length: Option<usize>, mut sink: impl FnMut(&[u8]) -> bool) -> Result<()> {
    let mut buf = [0u8; 8192];
    if chunked {
        loop {
            let mut size = String::new();
            if r.read_line(&mut size)? == 0 {
                return Ok(());
            }
            let size = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
                .map_err(|_| Error::Io("bad chunked encoding".into()))?;
            if size == 0 {
                return Ok(());
            }
            let mut left = size;
            while left > 0 {
                let cap = left.min(buf.len());
                let n = r.read(&mut buf[..cap])?;
                if n == 0 {
                    return Err(Error::Io("connection closed early".into()));
                }
                left -= n;
                if !sink(&buf[..n]) {
                    return Ok(());
                }
            }
            let mut crlf = String::new();
            r.read_line(&mut crlf)?;
        }
    }
    let mut left = length.unwrap_or(usize::MAX);
    while left > 0 {
        let cap = left.min(buf.len());
                let n = r.read(&mut buf[..cap])?;
        if n == 0 {
            return if length.is_some() { Err(Error::Io("connection closed early".into())) } else { Ok(()) };
        }
        left -= n;
        if !sink(&buf[..n]) {
            return Ok(());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback() {
        for ok in ["http://127.0.0.1:11434", "http://localhost:8080/", "http://[::1]:11434", "http://127.0.0.2", "http://LOCALHOST"] {
            assert!(Endpoint::parse(ok).is_ok(), "{ok}");
        }
        for bad in [
            "https://127.0.0.1:11434",
            "http://192.168.1.5:11434",
            "http://example.com",
            "http://127.0.0.1.nip.io",
            "http://user@127.0.0.1",
            "http://127.0.0.1:11434/api",
            "http://0.0.0.0:11434",
            "http://[::ffff:8.8.8.8]",
            "http://127.0.0.1:0",
            "ftp://127.0.0.1",
            "",
        ] {
            assert!(matches!(Endpoint::parse(bad), Err(Error::NotLocal(_))), "{bad}");
        }
        assert_eq!(Endpoint::parse("http://localhost").map(|e| e.url()), Ok("http://127.0.0.1:11434".into()));
    }
}
