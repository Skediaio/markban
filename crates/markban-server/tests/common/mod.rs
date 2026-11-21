#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use markban_server::{Config, access};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A data directory with boards `acme` and `other`, an admin (`*`) and `anna` (only `acme`),
/// served on an ephemeral port.
pub struct Fixture {
    pub data: TempDir,
    pub addr: SocketAddr,
    pub admin: String,
    pub anna: String,
}

impl Fixture {
    pub fn new() -> Self {
        Self::with(|_| {})
    }

    /// A fixture whose server configuration is first changed by `tune`.
    pub fn with(tune: impl FnOnce(&mut Config)) -> Self {
        let data = TempDir::new().unwrap();
        let admin = access::init(data.path(), "alice").unwrap();
        for id in ["acme", "other"] {
            access::add_board(data.path(), id, &format!("Board {id}"), &cols()).unwrap();
        }
        let anna = access::add_user(data.path(), "anna", &["acme".to_string()], false).unwrap();
        let mut config = Config::new(data.path().to_path_buf());
        tune(&mut config);
        let addr = markban_server::spawn(config, "127.0.0.1:0").unwrap();
        Self {
            data,
            addr,
            admin,
            anna,
        }
    }

    pub fn board(&self, id: &str) -> PathBuf {
        self.data.path().join("boards").join(id)
    }

    pub fn log(&self, id: &str) -> String {
        std::fs::read_to_string(self.data.path().join("log").join(format!("{id}.jsonl")))
            .unwrap_or_default()
    }

    pub fn get(&self, path: &str, token: Option<&str>) -> Resp {
        request(self.addr, "GET", path, token, None, None)
    }

    pub fn post(&self, path: &str, token: Option<&str>, body: &str) -> Resp {
        request(self.addr, "POST", path, token, Some(body.as_bytes()), None)
    }
}

pub fn cols() -> Vec<String> {
    ["Todo", "Doing", "Done"].map(String::from).to_vec()
}

#[derive(Debug)]
pub struct Resp {
    pub status: u16,
    pub headers: String,
    pub body: String,
}

impl Resp {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body)
            .unwrap_or_else(|e| panic!("not JSON ({e}): {:?}", self.body))
    }
}

/// A raw HTTP/1.1 request, so tests control every byte of the path.
pub fn request(
    addr: SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&[u8]>,
    host: Option<&str>,
) -> Resp {
    let mut stream = TcpStream::connect(addr).unwrap();
    let host = host.map_or_else(|| addr.to_string(), str::to_string);
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    if let Some(body) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).unwrap();
    if let Some(body) = body {
        // The server may answer (413) and close before the whole body is sent.
        let _ = stream.write_all(body);
    }
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);
    parse_response(&raw)
}

fn parse_response(raw: &[u8]) -> Resp {
    let text = String::from_utf8_lossy(raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {text:?}"));
    let headers = head.to_ascii_lowercase();
    let body = if headers.contains("transfer-encoding: chunked") {
        dechunk(body)
    } else {
        body.to_string()
    };
    Resp {
        status,
        headers,
        body,
    }
}

fn dechunk(mut rest: &str) -> String {
    let mut out = String::new();
    while let Some((size, tail)) = rest.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.push_str(&tail[..n]);
        rest = &tail[n + 2..];
    }
    out
}

/// `board.md` and `cards/*.md` of a board directory, for comparing two boards.
pub fn board_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![(
        "board.md".to_string(),
        std::fs::read(dir.join("board.md")).unwrap(),
    )];
    if let Ok(entries) = std::fs::read_dir(dir.join("cards")) {
        let mut cards: Vec<_> = entries.map(|e| e.unwrap().path()).collect();
        cards.sort();
        for card in cards {
            let name = card.file_name().unwrap().to_string_lossy().into_owned();
            files.push((format!("cards/{name}"), std::fs::read(&card).unwrap()));
        }
    }
    files
}

/// Sends `head` (a request without its blank line; `Host` and `Connection: close` are added)
/// and reads the whole answer.
pub fn raw(addr: SocketAddr, head: &str) -> Resp {
    let mut stream = TcpStream::connect(addr).unwrap();
    let text = format!("{head}Host: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(text.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    parse_response(&out)
}
