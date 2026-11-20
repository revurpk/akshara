//! Talks to a fake Ollama server on the loopback interface.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

use office_ai::{chat, list_models, Endpoint, Error, Message};

/// Serves one connection with `reply(request_line, body)`.
fn serve(reply: impl Fn(&str, &str) -> String + Send + 'static) -> Endpoint {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = l.local_addr().expect("addr").port();
    thread::spawn(move || {
        let (s, _) = l.accept().expect("accept");
        let mut r = BufReader::new(s.try_clone().expect("clone"));
        let mut first = String::new();
        r.read_line(&mut first).expect("request line");
        let mut len = 0;
        loop {
            let mut h = String::new();
            r.read_line(&mut h).expect("header");
            if h.trim().is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                len = v.trim().parse().expect("length");
            }
        }
        let mut body = vec![0; len];
        r.read_exact(&mut body).expect("body");
        let out = reply(&first, &String::from_utf8_lossy(&body));
        (&s).write_all(out.as_bytes()).expect("write");
    });
    Endpoint::parse(&format!("http://127.0.0.1:{port}")).expect("endpoint")
}

fn chunked(parts: &[&str]) -> String {
    let mut s = String::from("HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nTransfer-Encoding: chunked\r\n\r\n");
    for p in parts {
        s.push_str(&format!("{:x}\r\n{p}\r\n", p.len()));
    }
    s.push_str("0\r\n\r\n");
    s
}

#[test]
fn lists_models() {
    let ep = serve(|req, _| {
        assert!(req.starts_with("GET /api/tags "));
        let body = r#"{"models":[{"name":"qwen2.5:7b","size":4683087332,"details":{"parameter_size":"7.6B"}},{"name":"llama3.2:3b","size":2019393189}]}"#;
        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}", body.len())
    });
    let m = list_models(&ep).expect("models");
    assert_eq!(m.len(), 2);
    assert_eq!(m[0].name, "llama3.2:3b");
    assert_eq!(m[1].parameters, "7.6B");
}

#[test]
fn streams_a_chat() {
    let ep = serve(|req, body| {
        assert!(req.starts_with("POST /api/chat "));
        assert!(body.contains(r#""stream":true"#) && body.contains("namaste"));
        // A line split across chunks, Telugu text, then done.
        chunked(&[
            "{\"message\":{\"role\":\"assistant\",\"content\":\"# Hel\"},\"done\":false}\n{\"message\":{\"role\":\"assi",
            "stant\",\"content\":\"lo \u{c24}\u{c46}\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n",
        ])
    });
    let mut pieces = Vec::new();
    let reply = chat(&ep, "m", &[Message::user("namaste")], |p| {
        pieces.push(p.to_string());
        true
    })
    .expect("chat");
    assert_eq!(reply, "# Hello \u{c24}\u{c46}");
    assert_eq!(pieces.len(), 2);
}

#[test]
fn stopping_early_keeps_the_text_so_far() {
    let ep = serve(|_, _| chunked(&["{\"message\":{\"content\":\"one \"}}\n", "{\"message\":{\"content\":\"two\"}}\n"]));
    let reply = chat(&ep, "m", &[Message::user("x")], |_| false).expect("chat");
    assert_eq!(reply, "one ");
}

#[test]
fn server_errors_are_reported() {
    let ep = serve(|_, _| {
        let body = r#"{"error":"model 'nope' not found"}"#;
        format!("HTTP/1.1 404 Not Found\r\nContent-Length: {}\r\n\r\n{body}", body.len())
    });
    assert_eq!(chat(&ep, "nope", &[Message::user("x")], |_| true), Err(Error::Server("model 'nope' not found".into())));
}

#[test]
fn nothing_listening() {
    let port = TcpListener::bind("127.0.0.1:0").expect("bind").local_addr().expect("addr").port();
    let ep = Endpoint::parse(&format!("http://127.0.0.1:{port}")).expect("endpoint");
    assert_eq!(list_models(&ep), Err(Error::NotRunning));
}
