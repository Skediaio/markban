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

impl Remote {
    /// # Errors
    /// A URL that is not `http://` or `https://`.
    pub fn new(url: &str, token: String, board: Option<String>) -> Result<Self, String> {
        let base = url.trim_end_matches('/').to_string();
        if !(base.starts_with("http://") || base.starts_with("https://")) {
            return Err(
                "MARKBAN_URL must start with http:// or https://, e.g. http://127.0.0.1:7171"
                    .into(),
            );
        }
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(TIMEOUT))
            .build()
            .new_agent();
        Ok(Self {
            agent,
            base,
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

    #[test]
    fn only_http_urls_are_accepted() {
        assert!(Remote::new("http://127.0.0.1:1/", "t".into(), None).is_ok());
        assert!(Remote::new("https://example.com", "t".into(), None).is_ok());
        for bad in ["", "ftp://x", "127.0.0.1:7171", "file:///etc"] {
            assert!(Remote::new(bad, "t".into(), None).is_err(), "{bad}");
        }
    }
}
