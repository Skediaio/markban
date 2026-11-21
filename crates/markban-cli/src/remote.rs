//! Remote mode: the same commands, sent to a markban server's API (spec "Server mode").

use markban_core::command::Command;
use markban_core::fs::EXIT_INVALID;
use markban_core::view::{BoardView, CardDetail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
use std::time::Duration;

/// Largest response the client reads.
const MAX_RESPONSE: u64 = 64 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(60);

/// A failure: the one-line message, the exit code and, for a refused command, its index.
#[derive(Debug)]
pub struct RemoteError {
    pub message: String,
    pub code: u8,
    pub index: Option<usize>,
}

impl From<String> for RemoteError {
    fn from(message: String) -> Self {
        Self {
            message,
            code: EXIT_INVALID,
            index: None,
        }
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
    code: Option<u8>,
    index: Option<usize>,
}

#[derive(Deserialize)]
struct RunReply {
    outcomes: Vec<OutcomeReply>,
    /// `"failed"` when the server applied the commands but could not log them.
    log: Option<String>,
}

#[derive(Deserialize)]
struct OutcomeReply {
    id: String,
}

/// One entry of `GET /api/boards`.
#[derive(Debug, serde::Serialize, Deserialize)]
pub struct BoardEntry {
    pub id: String,
    pub title: String,
}

/// A server, a token and the board in use.
pub struct Remote {
    agent: ureq::Agent,
    /// Whether proxy variables from the environment (`HTTPS_PROXY`, `ALL_PROXY`...) apply.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "the agent holds the setting; tests read it")
    )]
    uses_env_proxy: bool,
    base: String,
    token: String,
    board: Option<String>,
}

/// Percent-encodes everything but unreserved characters, for one path segment.
fn encode_segment(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                char::from(b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Whether a URL host (IPv6 in brackets) is a loopback name or address.
fn is_loopback(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6
            .parse::<std::net::Ipv6Addr>()
            .is_ok_and(|ip| ip.is_loopback());
    }
    host.parse::<std::net::Ipv4Addr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// The checked parts of a server URL.
struct ServerUrl {
    /// The URL without a trailing `/`, scheme in lower case.
    base: String,
    https: bool,
    host: String,
}

/// Checks a server URL: `http://` or `https://`, a host, and no user name or password (which
/// would be sent in clear and shown in errors; they are never echoed here).
fn parse_url(url: &str) -> Result<ServerUrl, String> {
    let bad_scheme = || {
        "MARKBAN_URL must start with http:// or https://, e.g. https://board.example".to_string()
    };
    let (scheme, rest) = url.split_once("://").ok_or_else(bad_scheme)?;
    let scheme = scheme.to_ascii_lowercase();
    let https = match scheme.as_str() {
        "https" => true,
        "http" => false,
        _ => return Err(bad_scheme()),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        return Err(
            "MARKBAN_URL must not contain a user name or password; put the token in MARKBAN_TOKEN"
                .into(),
        );
    }
    let host = if authority.starts_with('[') {
        authority
            .find(']')
            .and_then(|end| authority.get(..=end))
            .unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host.is_empty() || host == "[]" {
        return Err("MARKBAN_URL has no host, e.g. https://board.example".into());
    }
    let base = format!("{scheme}://{rest}")
        .trim_end_matches('/')
        .to_string();
    Ok(ServerUrl {
        base,
        https,
        host: host.to_string(),
    })
}

impl Remote {
    /// Plain `http://` is only accepted for a loopback host unless `allow_insecure_http`: the
    /// token would otherwise cross the network in clear. Loopback `http://` never goes through
    /// a proxy from the environment; every other URL honours `HTTPS_PROXY`, `HTTP_PROXY`,
    /// `ALL_PROXY` and `NO_PROXY` (ureq's rules).
    ///
    /// # Errors
    /// A URL that is not `http://` or `https://`, has no host, carries credentials, or is plain
    /// `http://` to a remote host without the escape hatch.
    pub fn new(
        url: &str,
        token: String,
        board: Option<String>,
        allow_insecure_http: bool,
    ) -> Result<Self, String> {
        let url = parse_url(url)?;
        let loopback = is_loopback(&url.host);
        if !url.https && !loopback && !allow_insecure_http {
            return Err(format!(
                "refusing to send the token in clear to {} over http://; use https://, or pass --allow-insecure-http (MARKBAN_ALLOW_INSECURE_HTTP=1) on a trusted network",
                url.host
            ));
        }
        let uses_env_proxy = url.https || !loopback;
        let mut config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(TIMEOUT));
        if !uses_env_proxy {
            config = config.proxy(None);
        }
        let agent = config.build().new_agent();
        Ok(Self {
            agent,
            uses_env_proxy,
            base: url.base,
            token,
            board,
        })
    }

    /// `/api/boards/<id>` of the board in use.
    fn board_path(&self) -> Result<String, RemoteError> {
        let Some(board) = self.board.as_deref() else {
            return Err(String::from(
                "no board given; pass -b <id> or set MARKBAN_BOARD (a board id in remote mode); list them with `markban boards`",
            )
            .into());
        };
        if !markban_server::access::valid_name(board) {
            return Err(format!(
                "'{}' is not a board id; ids use lowercase letters, digits and single '-'; list them with `markban boards`",
                board.escape_debug()
            )
            .into());
        }
        Ok(format!("/api/boards/{board}"))
    }

    fn send(&self, path: &str, body: Option<String>) -> Result<String, RemoteError> {
        let url = format!("{}{path}", self.base);
        let auth = format!("Bearer {}", self.token);
        let response = match body {
            None => self.agent.get(&url).header("Authorization", &auth).call(),
            Some(body) => self
                .agent
                .post(&url)
                .header("Authorization", &auth)
                .header("Content-Type", "application/json")
                .send(body),
        }
        .map_err(|e| format!("cannot reach {}: {e}", self.base))?;
        let status = response.status().as_u16();
        let text = response
            .into_body()
            .with_config()
            .limit(MAX_RESPONSE)
            .read_to_string()
            .map_err(|e| format!("cannot read the answer from {}: {e}", self.base))?;
        if (200..300).contains(&status) {
            return Ok(text);
        }
        match serde_json::from_str::<ErrorBody>(&text) {
            Ok(error) => Err(RemoteError {
                message: error.error,
                code: error
                    .code
                    .filter(|c| matches!(c, 1 | 3 | 4))
                    .unwrap_or(EXIT_INVALID),
                index: error.index,
            }),
            Err(_) => Err(format!(
                "{} answered HTTP {status}; is MARKBAN_URL a markban server?",
                self.base
            )
            .into()),
        }
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, RemoteError> {
        let text = self.send(path, None)?;
        serde_json::from_str(&text)
            .map_err(|e| format!("unexpected answer from {}: {e}", self.base).into())
    }

    /// The board view, as `ls --json` prints it.
    ///
    /// # Errors
    /// [`RemoteError`].
    pub fn view(&self) -> Result<BoardView, RemoteError> {
        self.get(&self.board_path()?)
    }

    /// One card.
    ///
    /// # Errors
    /// [`RemoteError`].
    pub fn card(&self, id: &str) -> Result<CardDetail, RemoteError> {
        let path = format!("{}/cards/{}", self.board_path()?, encode_segment(id));
        self.get(&path)
    }

    /// Runs `cmds` on the server, all or nothing, and returns the affected ids.
    ///
    /// # Errors
    /// [`RemoteError`]; `index` names the refused command.
    pub fn run(&self, cmds: &[Command]) -> Result<Vec<String>, RemoteError> {
        let path = format!("{}/run", self.board_path()?);
        let body = serde_json::to_string(cmds).map_err(|e| format!("cannot encode JSON: {e}"))?;
        let text = self.send(&path, Some(body))?;
        let reply: RunReply = serde_json::from_str(&text)
            .map_err(|e| format!("unexpected answer from {}: {e}", self.base))?;
        if reply.log.is_some() {
            eprintln!("markban: applied, but the server could not write its log");
        }
        Ok(reply.outcomes.into_iter().map(|o| o.id).collect())
    }

    /// The boards the token may use.
    ///
    /// # Errors
    /// [`RemoteError`].
    pub fn boards(&self) -> Result<Vec<BoardEntry>, RemoteError> {
        self.get("/api/boards")
    }

    /// The last `count` log entries, exactly as stored.
    ///
    /// # Errors
    /// [`RemoteError`].
    pub fn log(&self, count: usize) -> Result<Vec<Box<RawValue>>, RemoteError> {
        let count = count.min(markban_server::LOG_MAX);
        self.get(&format!("{}/log?limit={count}", self.board_path()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_percent_encoded() {
        assert_eq!(encode_segment("a-b_c.d~9"), "a-b_c.d~9");
        assert_eq!(encode_segment("../x"), "..%2Fx");
        assert_eq!(encode_segment("a b?#%"), "a%20b%3F%23%25");
        assert_eq!(encode_segment("é"), "%C3%A9");
    }

    fn remote(url: &str, insecure: bool) -> Result<Remote, String> {
        Remote::new(url, "t".into(), None, insecure)
    }

    #[test]
    fn only_http_urls_are_accepted() {
        assert!(remote("http://127.0.0.1:1/", false).is_ok());
        assert!(remote("https://example.com", false).is_ok());
        for bad in [
            "",
            "ftp://x",
            "127.0.0.1:7171",
            "file:///etc",
            "http://",
            "https://:80",
        ] {
            assert!(remote(bad, true).is_err(), "{bad}");
        }
    }

    #[test]
    fn plain_http_needs_a_loopback_host_or_the_escape_hatch() {
        for ok in [
            "http://127.0.0.1:7171",
            "http://127.4.5.6",
            "http://localhost:1",
            "http://LOCALHOST",
            "http://[::1]:7171/",
            "http://[0:0:0:0:0:0:0:1]",
            "https://board.example",
            "https://10.0.0.1:443",
        ] {
            assert!(remote(ok, false).is_ok(), "{ok}");
        }
        for bad in [
            "http://board.example",
            "http://10.0.0.1:7171",
            "http://128.0.0.1",
            "http://[::2]",
            "http://localhost.evil.example",
            "http://127.0.0.1.evil.example",
            "HTTP://board.example",
        ] {
            let err = remote(bad, false).err().unwrap_or_default();
            assert!(err.contains("--allow-insecure-http"), "{bad}: {err}");
            assert!(!err.contains('\n'));
            assert!(remote(bad, true).is_ok(), "{bad}");
        }
    }

    #[test]
    fn credentials_in_the_url_are_refused_and_never_echoed() {
        for bad in [
            "http://u:secret@127.0.0.1:7171",
            "https://secret@board.example",
            "https://u:secret@board.example/path",
            "http://127.0.0.1@secret.example",
        ] {
            let err = remote(bad, true).err().unwrap_or_default();
            assert!(err.contains("user name or password"), "{bad}: {err}");
            assert!(!err.contains("secret"), "{err}");
        }
        // An `@` after the host is part of the path, not credentials.
        assert!(remote("https://board.example/a@b", false).is_ok());
    }

    #[test]
    fn loopback_http_ignores_proxies_and_https_keeps_them() {
        let local = remote("http://127.0.0.1:7171", false).unwrap();
        assert!(!local.uses_env_proxy);
        let tls = remote("https://board.example", false).unwrap();
        assert!(tls.uses_env_proxy);
    }
}
