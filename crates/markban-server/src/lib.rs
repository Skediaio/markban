//! The markban server: several boards in one data directory, tokens with board access, a JSON
//! command API and an append-only log per board (spec section "Server mode").
//!
//! Every change goes through [`markban_core::fs::run`]: all or nothing, under the board lock,
//! with compare and swap. The server adds authentication, access per board and the log; it has
//! no board logic of its own.

pub mod access;

use access::{Access, User};
use axum::Router;
use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use markban_core::command::Command;
use markban_core::fs::{ApplyError, EXIT_INVALID, RunError};
use markban_core::view::{self, BoardView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

/// Largest request body the API accepts.
pub const MAX_BODY: usize = 1024 * 1024;
/// Most commands one `run` request may carry.
pub const MAX_COMMANDS: usize = 1000;
/// Default and largest `limit` of the log endpoint.
pub const LOG_DEFAULT: usize = 50;
pub const LOG_MAX: usize = 1000;
/// Default listen address of `markban serve`.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7171";

/// What a server needs to run.
#[derive(Debug, Clone)]
pub struct Config {
    /// The data directory (`access.toml`, `boards/`, `log/`).
    pub data: PathBuf,
    /// Host names accepted in the `Host` header besides loopback (see [`host_allowed`]).
    pub allowed_hosts: Vec<String>,
}

// ---- Host header guard ----

/// The host name (lower case, IPv6 keeps its brackets) of a `Host` header value, when the value
/// is well formed: `name`, or `name:port`, or `[v6]`, or `[v6]:port`.
fn host_name(host: &str) -> Option<String> {
    let (name, port) = if host.starts_with('[') {
        let end = host.find(']')?;
        let (name, tail) = host.split_at(end + 1);
        (name, tail.strip_prefix(':'))
    } else {
        match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        }
    };
    let port_ok = port.is_none_or(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    (port_ok && !name.is_empty()).then(|| name.to_ascii_lowercase())
}

/// Whether a request's `Host` header may be served: loopback names (any port), or an entry of
/// `extra` (a host name, or `name:port`). This blocks DNS rebinding.
#[must_use]
pub fn host_allowed(host: &str, extra: &[String]) -> bool {
    let Some(name) = host_name(host) else {
        return false;
    };
    let full = host.to_ascii_lowercase();
    matches!(name.as_str(), "localhost" | "127.0.0.1" | "[::1]")
        || extra.iter().any(|e| *e == name || *e == full)
}

/// Parses a comma separated host list (as in `MARKBAN_ALLOWED_HOSTS`) into lower-case entries.
#[must_use]
pub fn parse_hosts(list: &str) -> Vec<String> {
    list.split(',')
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
}

/// The request's `Host` header, or the URI authority.
#[must_use]
pub fn request_host(request: &Request) -> Option<String> {
    request
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().authority().map(ToString::to_string))
}

async fn host_guard(State(state): State<Arc<Server>>, request: Request, next: Next) -> Response {
    if request_host(&request).is_some_and(|h| host_allowed(&h, &state.config.allowed_hosts)) {
        next.run(request).await
    } else {
        ApiError::new(
            StatusCode::FORBIDDEN,
            "unknown Host header; set MARKBAN_ALLOWED_HOSTS on the server to allow it",
        )
        .into_response()
    }
}

// ---- errors ----

/// An API failure: an HTTP status and the JSON body `{"error", "code", "index"?}`, where `code`
/// is the CLI exit code (1 invalid, 3 contention, 4 partial) and `index` the failing command.
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    body: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    error: String,
    code: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
}

impl ApiError {
    fn new(status: StatusCode, error: impl Into<String>) -> Self {
        Self::with_code(status, EXIT_INVALID, error)
    }

    fn with_code(status: StatusCode, code: u8, error: impl Into<String>) -> Self {
        Self {
            status,
            body: ErrorBody {
                error: error.into().replace(['\n', '\r'], " "),
                code,
                index: None,
            },
        }
    }

    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "missing or invalid token; send `Authorization: Bearer <token>` (MARKBAN_TOKEN)",
        )
    }

    /// The one answer for a board that does not exist, is not a valid id or is not the user's:
    /// they must be indistinguishable.
    fn no_board() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "no such board for this token; list yours with `markban boards`",
        )
    }

    fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal server error; the server log has details",
        )
    }

    /// Maps a failed run. Messages never carry server paths.
    fn from_run(error: RunError) -> Self {
        let code = error.exit_code();
        let status = match code {
            3 => StatusCode::CONFLICT,
            4 => StatusCode::INTERNAL_SERVER_ERROR,
            _ => match &error {
                RunError::Load(_) | RunError::Apply(ApplyError::Io { .. }) => {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
                _ => StatusCode::BAD_REQUEST,
            },
        };
        let (message, index) = match error {
            RunError::Command { index, source } => (source.to_string(), Some(index)),
            RunError::Locked { .. } => (
                "board is locked by another markban process; try again shortly".to_string(),
                None,
            ),
            RunError::Load(e) => {
                eprintln!("markban serve: {e}");
                ("cannot read the board on the server".to_string(), None)
            }
            RunError::NoBoard { reason, .. } => (format!("no usable board: {reason}"), None),
            other => (other.to_string(), None),
        };
        let mut api = Self::with_code(status, code, message);
        api.body.index = index;
        api
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = serde_json::to_string(&self.body)
            .unwrap_or_else(|_| r#"{"error":"internal server error","code":1}"#.to_string());
        let mut response = (self.status, json_headers(), body).into_response();
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        response
    }
}

fn json_headers() -> [(axum::http::HeaderName, HeaderValue); 1] {
    [(CONTENT_TYPE, HeaderValue::from_static("application/json"))]
}

fn json_ok(body: String) -> Response {
    (StatusCode::OK, json_headers(), body).into_response()
}

fn to_json<T: Serialize>(value: &T) -> Result<String, ApiError> {
    serde_json::to_string(value).map_err(|_| ApiError::internal())
}

// ---- state ----

struct Server {
    config: Config,
    /// One lock per board, so runs on a board (and their log lines) never interleave.
    locks: Mutex<BTreeMap<String, Arc<Mutex<()>>>>,
}

impl Server {
    /// The user of the request's bearer token. `access.toml` is read on every request, so admin
    /// changes take effect at once; an unreadable file refuses everyone.
    fn authenticate(&self, headers: &HeaderMap) -> Result<User, ApiError> {
        let token = bearer(headers).ok_or_else(ApiError::unauthorized)?;
        let access = Access::load(&self.config.data).map_err(|e| {
            eprintln!("markban serve: {e}");
            ApiError::internal()
        })?;
        access
            .authenticate(token)
            .cloned()
            .ok_or_else(ApiError::unauthorized)
    }

    /// The directory of board `id` when `user` may use it; otherwise the same 404 as a missing
    /// board.
    fn board(&self, user: &User, id: &str) -> Result<PathBuf, ApiError> {
        if !user.may_use(id) {
            return Err(ApiError::no_board());
        }
        access::board_dir(&self.config.data, id).ok_or_else(ApiError::no_board)
    }

    fn board_lock(&self, id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(locks.entry(id.to_string()).or_default())
    }
}

/// The token of `Authorization: Bearer <token>` (scheme case-insensitive, exactly one space).
fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer")
        && !token.is_empty()
        && !token.contains(|c: char| c.is_whitespace()))
    .then_some(token)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|_| ApiError::internal())?
}

// ---- handlers ----

#[derive(Serialize)]
struct BoardEntry {
    id: String,
    title: String,
}

async fn list_boards(
    State(state): State<Arc<Server>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let user = state.authenticate(&headers)?;
    let data = state.config.data.clone();
    let boards = blocking(move || {
        access::list_boards(&data).map_err(|e| {
            eprintln!("markban serve: {e}");
            ApiError::internal()
        })
    })
    .await?;
    let visible: Vec<BoardEntry> = boards
        .into_iter()
        .filter(|(id, _)| user.may_use(id))
        .map(|(id, title)| BoardEntry { id, title })
        .collect();
    Ok(json_ok(to_json(&visible)?))
}

/// Loads a board as `markban ls --json` shows it: the view with every load diagnostic.
fn load_board(dir: &Path) -> Result<(markban_core::board::Board, BoardView), ApiError> {
    let (board, diagnostics) = markban_core::fs::load(dir).map_err(|e| {
        eprintln!("markban serve: {e}");
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "cannot read the board on the server",
        )
    })?;
    let Some(board) = board else {
        let reason = diagnostics
            .first()
            .map_or_else(|| "board.md is unusable".to_string(), ToString::to_string);
        return Err(ApiError::new(StatusCode::BAD_REQUEST, reason));
    };
    let mut board_view = view::view(&board);
    board_view.diagnostics = diagnostics;
    Ok((board, board_view))
}

fn path_board(path: Result<UrlPath<String>, PathRejection>) -> Result<String, ApiError> {
    path.map(|UrlPath(id)| id).map_err(|_| ApiError::no_board())
}

async fn board_view(
    State(state): State<Arc<Server>>,
    headers: HeaderMap,
    path: Result<UrlPath<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let user = state.authenticate(&headers)?;
    let dir = state.board(&user, &path_board(path)?)?;
    let (_, board_view) = blocking(move || load_board(&dir)).await?;
    Ok(json_ok(to_json(&board_view)?))
}

async fn card(
    State(state): State<Arc<Server>>,
    headers: HeaderMap,
    path: Result<UrlPath<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let user = state.authenticate(&headers)?;
    let (board, card) = path.map(|UrlPath(p)| p).map_err(|_| ApiError::no_board())?;
    let dir = state.board(&user, &board)?;
    let (board, _) = blocking(move || load_board(&dir)).await?;
    // The id is only a key into the loaded cards; it never touches the filesystem.
    let detail = view::card(&board, &card).ok_or_else(|| {
        let shown: String = card.chars().take(80).collect();
        ApiError::new(
            StatusCode::NOT_FOUND,
            format!("no card '{shown}'; card ids are the file names in cards/ without .md"),
        )
    })?;
    Ok(json_ok(to_json(&detail)?))
}

#[derive(Serialize)]
struct LogLine<'a> {
    ts: &'a str,
    user: &'a str,
    cmd: &'a Command,
    outcome: &'a str,
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    let now = now.replace_nanosecond(0).unwrap_or(now);
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Appends one line per command. A failure is reported on the server's stderr: the board has
/// already changed, so the request still succeeds.
fn append_log(data: &Path, board: &str, user: &str, cmds: &[Command], outcomes: &[String]) {
    let ts = now_rfc3339();
    let mut text = String::new();
    for (cmd, outcome) in cmds.iter().zip(outcomes) {
        let line = LogLine {
            ts: &ts,
            user,
            cmd,
            outcome,
        };
        if let Ok(json) = serde_json::to_string(&line) {
            text.push_str(&json);
            text.push('\n');
        }
    }
    let path = access::log_path(data, board);
    let written = std::fs::create_dir_all(data.join(access::LOG_DIR)).and_then(|()| {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
        file.sync_data()
    });
    if let Err(e) = written {
        eprintln!("markban serve: cannot append to the log of board '{board}': {e}");
    }
}

#[derive(Serialize)]
struct RunReply {
    outcomes: Vec<markban_core::Outcome>,
}

fn parse_commands(body: Result<Bytes, BytesRejection>) -> Result<Vec<Command>, ApiError> {
    let body = body.map_err(|rejection| {
        let status = rejection.status();
        if status == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::new(
                status,
                format!("request body is larger than {MAX_BODY} bytes; split the commands"),
            )
        } else {
            ApiError::new(StatusCode::BAD_REQUEST, "cannot read the request body")
        }
    })?;
    let cmds: Vec<Command> = serde_json::from_slice(&body).map_err(|e| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "invalid command: {e}; send a JSON array of commands such as [{{\"cmd\":\"card.add\",\"title\":\"T\"}}]"
            ),
        )
    })?;
    if cmds.len() > MAX_COMMANDS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "{} commands in one request; send at most {MAX_COMMANDS}",
                cmds.len()
            ),
        ));
    }
    Ok(cmds)
}

async fn run(
    State(state): State<Arc<Server>>,
    headers: HeaderMap,
    path: Result<UrlPath<String>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, ApiError> {
    let user = state.authenticate(&headers)?;
    let id = path_board(path)?;
    let dir = state.board(&user, &id)?;
    let cmds = parse_commands(body)?;
    let lock = state.board_lock(&id);
    let data = state.config.data.clone();
    let outcomes = blocking(move || {
        let _held = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let outcomes = markban_core::fs::run(&dir, &cmds).map_err(ApiError::from_run)?;
        let ids: Vec<String> = outcomes.iter().map(|o| o.id.clone()).collect();
        append_log(&data, &id, &user.name, &cmds, &ids);
        Ok(outcomes)
    })
    .await?;
    Ok(json_ok(to_json(&RunReply { outcomes })?))
}

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<usize>,
}

/// The last `limit` lines of a log file that are valid JSON, as a JSON array (newest last).
fn read_log(path: &Path, limit: usize) -> Result<String, ApiError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("markban serve: cannot read '{}': {e}", path.display());
            return Err(ApiError::internal());
        }
    };
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| serde_json::from_str::<serde::de::IgnoredAny>(l).is_ok())
        .collect();
    let start = lines.len().saturating_sub(limit);
    Ok(format!("[{}]", lines.get(start..).unwrap_or(&[]).join(",")))
}

async fn log(
    State(state): State<Arc<Server>>,
    headers: HeaderMap,
    path: Result<UrlPath<String>, PathRejection>,
    query: Result<Query<LogQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
    let user = state.authenticate(&headers)?;
    let id = path_board(path)?;
    state.board(&user, &id)?;
    let Query(query) = query.map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("`limit` must be a whole number from 0 to {LOG_MAX}"),
        )
    })?;
    let limit = query.limit.unwrap_or(LOG_DEFAULT).min(LOG_MAX);
    let file = access::log_path(&state.config.data, &id);
    let body = blocking(move || read_log(&file, limit)).await?;
    Ok(json_ok(body))
}

async fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "no such endpoint; the API lives under /api/boards",
    )
}

/// The API router: `/api/boards…` behind the `Host` guard, with the body limit.
pub fn router(config: Config) -> Router {
    let state = Arc::new(Server {
        config,
        locks: Mutex::new(BTreeMap::new()),
    });
    Router::new()
        .route("/api/boards", get(list_boards))
        .route("/api/boards/{board}", get(board_view))
        .route("/api/boards/{board}/cards/{card}", get(card))
        .route("/api/boards/{board}/run", post(run))
        .route("/api/boards/{board}/log", get(log))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            host_guard,
        ))
        .with_state(state)
}

fn check_data(config: &Config) -> Result<(), String> {
    Access::load(&config.data).map(|_| ())
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start the async runtime: {e}"))
}

fn bind(listen: &str) -> Result<std::net::TcpListener, String> {
    let listener = std::net::TcpListener::bind(listen)
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    Ok(listener)
}

async fn serve_on(listener: std::net::TcpListener, config: Config) -> Result<(), String> {
    let listener =
        tokio::net::TcpListener::from_std(listener).map_err(|e| format!("cannot listen: {e}"))?;
    axum::serve(listener, router(config))
        .await
        .map_err(|e| format!("server stopped: {e}"))
}

/// Serves the API on `listen` until the process ends. Checks `access.toml` first.
///
/// # Errors
/// An unusable data directory, an address that cannot be bound, or a server failure.
pub fn serve(config: Config, listen: &str) -> Result<(), String> {
    check_data(&config)?;
    let listener = bind(listen)?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    eprintln!(
        "markban: serving '{}' on http://{addr}",
        config.data.display()
    );
    runtime()?.block_on(serve_on(listener, config))
}

/// Starts the server on a background thread and returns its address (use port 0 for a free
/// port). The server runs until the process ends. For tests and embedding.
///
/// # Errors
/// An unusable data directory or an address that cannot be bound.
pub fn spawn(config: Config, listen: &str) -> Result<SocketAddr, String> {
    check_data(&config)?;
    let listener = bind(listen)?;
    let addr = listener
        .local_addr()
        .map_err(|e| format!("cannot listen on {listen}: {e}"))?;
    let runtime = runtime()?;
    std::thread::spawn(move || {
        if let Err(e) = runtime.block_on(serve_on(listener, config)) {
            eprintln!("markban: {e}");
        }
    });
    Ok(addr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_check_allows_loopback_only_by_default() {
        for ok in [
            "localhost",
            "localhost:8080",
            "LOCALHOST:3000",
            "127.0.0.1",
            "127.0.0.1:8080",
            "[::1]",
            "[::1]:8080",
        ] {
            assert!(host_allowed(ok, &[]), "{ok}");
        }
        for bad in [
            "",
            "evil.example",
            "evil.example:8080",
            "localhost.evil.example",
            "127.0.0.1.evil.example",
            "localhost:80:80",
            "[::1",
            "::1",
        ] {
            assert!(!host_allowed(bad, &[]), "{bad}");
        }
    }

    #[test]
    fn host_check_honours_the_extra_list() {
        let extra = parse_hosts(" board.lan, Other.Example ,,");
        assert!(host_allowed("board.lan", &extra));
        assert!(host_allowed("board.lan:8080", &extra));
        assert!(host_allowed("other.example:1", &extra));
        assert!(!host_allowed("evil.example", &extra));
    }

    #[test]
    fn bearer_parsing_is_strict() {
        let headers = |v: &str| {
            let mut h = HeaderMap::new();
            if let Ok(v) = HeaderValue::from_str(v) {
                h.insert(AUTHORIZATION, v);
            }
            h
        };
        assert_eq!(bearer(&headers("Bearer abc")), Some("abc"));
        assert_eq!(bearer(&headers("bearer abc")), Some("abc"));
        for bad in [
            "",
            "Bearer",
            "Bearer ",
            "Bearer  abc",
            "Basic abc",
            "abc",
            "Bearer a b",
        ] {
            assert_eq!(bearer(&headers(bad)), None, "{bad}");
        }
    }
}
