//! The markban server: several boards in one data directory, tokens with board access, a JSON
//! command API and an append-only log per board (spec section "Server mode").
//!
//! Every change goes through [`markban_core::fs::run_guarded`]: all or nothing, under the board
//! lock, with compare and swap. The server adds authentication, access per board, resource
//! limits and the log; it has no board logic of its own.

pub mod access;

use access::{Access, User};
use axum::body::Bytes;
use axum::extract::rejection::{BytesRejection, PathRejection, QueryRejection};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, Request, State};
use axum::http::header::{AUTHORIZATION, CONTENT_TYPE, HOST, WWW_AUTHENTICATE};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Router};
use hyper_util::rt::{TokioIo, TokioTimer};
use hyper_util::service::TowerToHyperService;
use markban_core::board::Board;
use markban_core::command::Command;
use markban_core::fs::{ApplyError, EXIT_CONTENDED, EXIT_INVALID, EXIT_PARTIAL, RunError};
use markban_core::view::{self, BoardView};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tokio::sync::Semaphore;

/// Largest request body the API accepts.
pub const MAX_BODY: usize = 1024 * 1024;
/// Most commands one `run` request may carry.
pub const MAX_COMMANDS: usize = 1000;
/// Default and largest `limit` of the log endpoint.
pub const LOG_DEFAULT: usize = 50;
pub const LOG_MAX: usize = 1000;
/// Longest text field (in bytes) a log line keeps; longer ones are cut with a note.
pub const LOG_TEXT_MAX: usize = 2048;
/// Default listen address of `markban serve`.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7171";

/// Resource limits of a server (spec section "Server mode"). [`Limits::default`] gives the
/// documented defaults.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Most cards a board may hold after a run through the API.
    pub max_cards: usize,
    /// Largest card file, in bytes, a run through the API may write.
    pub max_card_bytes: usize,
    /// Requests handled at once; more are answered 503 at once.
    pub max_concurrent: usize,
    /// Blocking file-system jobs at once, over all boards.
    pub max_blocking: usize,
    /// Open connections at once; more wait in the listen queue.
    pub max_connections: usize,
    /// Longest time for one request, body included.
    pub request_timeout: Duration,
    /// Longest time for a client to send the request headers (also the keep-alive idle time).
    pub header_timeout: Duration,
    /// Size at which a board log is rotated to `<id>.jsonl.1`.
    pub log_rotate_bytes: u64,
}

/// Default most cards per board.
pub const DEFAULT_MAX_CARDS: usize = 5000;
/// Default largest card file written through the API.
pub const DEFAULT_MAX_CARD_BYTES: usize = 256 * 1024;
/// Default requests handled at once.
pub const DEFAULT_MAX_CONCURRENT: usize = 64;
/// Default longest time for one request, in seconds.
pub const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 30;

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_cards: DEFAULT_MAX_CARDS,
            max_card_bytes: DEFAULT_MAX_CARD_BYTES,
            max_concurrent: DEFAULT_MAX_CONCURRENT,
            max_blocking: 16,
            max_connections: 256,
            request_timeout: Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
            header_timeout: Duration::from_secs(10),
            log_rotate_bytes: 16 * 1024 * 1024,
        }
    }
}

/// What a server needs to run.
#[derive(Debug, Clone)]
pub struct Config {
    /// The data directory (`access.toml`, `boards/`, `log/`).
    pub data: PathBuf,
    /// Host names accepted in the `Host` header; empty means loopback only (see
    /// [`host_allowed`]).
    pub allowed_hosts: Vec<String>,
    pub limits: Limits,
}

impl Config {
    /// A configuration for `data` with loopback hosts only and the default limits.
    #[must_use]
    pub fn new(data: PathBuf) -> Self {
        Self {
            data,
            allowed_hosts: Vec::new(),
            limits: Limits::default(),
        }
    }
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

/// Whether a request's `Host` header may be served. With an empty `extra`: loopback names (any
/// port). Otherwise only the entries of `extra` (a host name, or `name:port`), loopback
/// included only when listed. This blocks DNS rebinding.
#[must_use]
pub fn host_allowed(host: &str, extra: &[String]) -> bool {
    let Some(name) = host_name(host) else {
        return false;
    };
    if extra.is_empty() {
        return matches!(name.as_str(), "localhost" | "127.0.0.1" | "[::1]");
    }
    let full = host.to_ascii_lowercase();
    extra.iter().any(|e| *e == name || *e == full)
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
    /// One lock per board, taken asynchronously before any blocking work, so runs on a board
    /// (and their log lines) never interleave and waiting runs hold no blocking thread.
    locks: Mutex<BTreeMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Blocking file-system jobs at once, over all boards.
    blocking: Arc<Semaphore>,
    /// Requests in flight.
    requests: Arc<Semaphore>,
    /// The parsed `access.toml` and the stamp of the file it came from.
    access: Mutex<Option<(access::Stamp, Arc<Access>)>>,
}

impl Server {
    /// The current `access.toml`. Parsed again whenever its size, modification time or inode
    /// changes, so admin changes take effect at once; an unreadable file refuses everyone.
    /// Blocking: call it inside [`Server::blocking`].
    fn current_access(&self) -> Result<Arc<Access>, String> {
        let stamp = access::access_stamp(&self.config.data)?;
        let mut cache = self.access.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, parsed)) = cache.as_ref()
            && *cached == stamp
        {
            return Ok(Arc::clone(parsed));
        }
        // Stamped before reading: what is cached is never older than its stamp.
        let parsed = Arc::new(Access::load(&self.config.data).inspect_err(|_| *cache = None)?);
        *cache = Some((stamp, Arc::clone(&parsed)));
        Ok(parsed)
    }

    /// The directory of board `id` when `user` may use it; otherwise the same 404 as a missing
    /// board.
    fn board(&self, user: &User, id: &str) -> Result<PathBuf, ApiError> {
        if !user.may_use(id) {
            return Err(ApiError::no_board());
        }
        access::board_dir(&self.config.data, id).ok_or_else(ApiError::no_board)
    }

    fn board_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(locks.entry(id.to_string()).or_default())
    }

    /// Runs `work` on the blocking pool once one of the global blocking slots is free. The slot
    /// is released when `work` ends, even when the request was abandoned meanwhile.
    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let permit = Arc::clone(&self.blocking)
            .acquire_owned()
            .await
            .map_err(|_| ApiError::internal())?;
        tokio::task::spawn_blocking(move || {
            let result = work();
            drop(permit);
            result
        })
        .await
        .map_err(|_| ApiError::internal())?
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

// ---- middleware ----

/// Admission: at most `max_concurrent` requests at once (more are 503 with code 3 at once), and
/// each one, body included, within `request_timeout`.
async fn gate(State(state): State<Arc<Server>>, request: Request, next: Next) -> Response {
    let Ok(_permit) = Arc::clone(&state.requests).try_acquire_owned() else {
        return ApiError::with_code(
            StatusCode::SERVICE_UNAVAILABLE,
            EXIT_CONTENDED,
            "the server is busy; try again shortly",
        )
        .into_response();
    };
    let writes = request.method() != Method::GET;
    let limit = state.config.limits.request_timeout;
    match tokio::time::timeout(limit, next.run(request)).await {
        Ok(response) => response,
        Err(_) if writes => ApiError::with_code(
            StatusCode::SERVICE_UNAVAILABLE,
            EXIT_PARTIAL,
            format!(
                "request timed out after {} ms; it may still have been applied: check `markban log` and the board before retrying",
                limit.as_millis()
            ),
        )
        .into_response(),
        Err(_) => ApiError::with_code(
            StatusCode::SERVICE_UNAVAILABLE,
            EXIT_CONTENDED,
            format!(
                "request timed out after {} ms; try again shortly",
                limit.as_millis()
            ),
        )
        .into_response(),
    }
}

/// Authentication from the headers alone, before any handler reads the body: the user goes into
/// the request extensions.
async fn authenticate(
    State(state): State<Arc<Server>>,
    mut request: Request,
    next: Next,
) -> Response {
    let Some(token) = bearer(request.headers()).map(str::to_owned) else {
        return ApiError::unauthorized().into_response();
    };
    let server = Arc::clone(&state);
    let user = state
        .blocking(move || {
            let access = server.current_access().map_err(|e| {
                eprintln!("markban serve: {e}");
                ApiError::internal()
            })?;
            access
                .authenticate(&token)
                .cloned()
                .ok_or_else(ApiError::unauthorized)
        })
        .await;
    match user {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

// ---- handlers ----

#[derive(Serialize)]
struct BoardEntry {
    id: String,
    title: String,
}

async fn list_boards(
    State(state): State<Arc<Server>>,
    Extension(user): Extension<User>,
) -> Result<Response, ApiError> {
    let data = state.config.data.clone();
    let boards = state
        .blocking(move || {
            access::list_boards_where(&data, |id| user.may_use(id)).map_err(|e| {
                eprintln!("markban serve: {e}");
                ApiError::internal()
            })
        })
        .await?;
    let visible: Vec<BoardEntry> = boards
        .into_iter()
        .map(|(id, title)| BoardEntry { id, title })
        .collect();
    Ok(json_ok(to_json(&visible)?))
}

/// Loads a board as `markban ls --json` shows it: the view with every load diagnostic.
fn load_board(dir: &Path) -> Result<(Board, BoardView), ApiError> {
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
    Extension(user): Extension<User>,
    path: Result<UrlPath<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let dir = state.board(&user, &path_board(path)?)?;
    let (_, board_view) = state.blocking(move || load_board(&dir)).await?;
    Ok(json_ok(to_json(&board_view)?))
}

async fn card(
    State(state): State<Arc<Server>>,
    Extension(user): Extension<User>,
    path: Result<UrlPath<(String, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let (board, card) = path.map(|UrlPath(p)| p).map_err(|_| ApiError::no_board())?;
    let dir = state.board(&user, &board)?;
    let (board, _) = state.blocking(move || load_board(&dir)).await?;
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

// ---- the log ----

#[derive(Serialize)]
struct LogLine<'a> {
    ts: &'a str,
    user: &'a str,
    cmd: &'a Command,
    outcome: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    written: Option<&'a [String]>,
}

fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    let now = now.replace_nanosecond(0).unwrap_or(now);
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

/// Cuts `text` to at most [`LOG_TEXT_MAX`] bytes (on a character boundary) plus a note with
/// its full length.
fn clip(text: &mut String) {
    let total = text.len();
    if total <= LOG_TEXT_MAX {
        return;
    }
    let mut end = LOG_TEXT_MAX;
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    text.truncate(end);
    text.push_str(&format!("…(truncated, {total} bytes)"));
}

/// The command as the log keeps it: long text fields clipped, so the log is an audit trail and
/// not a copy of the board.
fn for_log(cmd: &Command) -> Command {
    let mut cmd = cmd.clone();
    let clip_opt = |text: &mut Option<String>| {
        if let Some(text) = text {
            clip(text);
        }
    };
    match &mut cmd {
        Command::BoardInit { title, columns } => {
            clip(title);
            columns.iter_mut().for_each(clip);
        }
        Command::CardAdd {
            title, description, ..
        } => {
            clip(title);
            clip_opt(description);
        }
        Command::CardEdit {
            title,
            description,
            append,
            ..
        } => {
            clip_opt(title);
            clip_opt(description);
            clip_opt(append);
        }
        Command::ColumnAdd { name, .. } | Command::ColumnRename { name, .. } => clip(name),
        _ => {}
    }
    cmd
}

/// Appends one line per command to the board's log, rotating it to `<id>.jsonl.1` first when it
/// has reached `rotate_at` bytes.
fn append_log(
    data: &Path,
    board: &str,
    user: &str,
    cmds: &[Command],
    outcomes: &[&str],
    written: Option<&[String]>,
    rotate_at: u64,
) -> std::io::Result<()> {
    let ts = now_rfc3339();
    let mut text = String::new();
    for (cmd, outcome) in cmds.iter().zip(outcomes) {
        let cmd = for_log(cmd);
        let line = LogLine {
            ts: &ts,
            user,
            cmd: &cmd,
            outcome,
            written,
        };
        if let Ok(json) = serde_json::to_string(&line) {
            text.push_str(&json);
            text.push('\n');
        }
    }
    access::create_private_dir(&data.join(access::LOG_DIR))?;
    let path = access::log_path(data, board);
    if let Ok(meta) = std::fs::symlink_metadata(&path)
        && meta.is_file()
        && meta.len() >= rotate_at
    {
        std::fs::rename(&path, access::rotated_log_path(&path))?;
    }
    let mut file = access::private_file()
        .create(true)
        .append(true)
        .open(&path)?;
    file.write_all(text.as_bytes())?;
    file.sync_data()
}

// ---- running commands ----

#[derive(Serialize)]
struct RunReply {
    outcomes: Vec<markban_core::Outcome>,
    /// `"failed"` when the board changed but the log line could not be written.
    #[serde(skip_serializing_if = "Option::is_none")]
    log: Option<&'static str>,
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

/// The per-board limits: checked on the board a run would produce, before anything is written.
/// A board may always shrink, and only card files the run changes are measured.
fn check_limits(limits: &Limits, before: Option<&Board>, after: &Board) -> Result<(), String> {
    let had = before.map_or(0, |b| b.cards.len());
    let cards = after.cards.len();
    if cards > limits.max_cards && cards > had {
        return Err(format!(
            "the board would hold {cards} cards; this server allows at most {} cards per board; remove cards first",
            limits.max_cards
        ));
    }
    for (id, card) in &after.cards {
        if before.and_then(|b| b.cards.get(id)) == Some(card) {
            continue;
        }
        let size = card.to_bytes().len();
        if size > limits.max_card_bytes {
            return Err(format!(
                "card '{id}' would be {size} bytes; this server allows at most {} bytes per card file; shorten its text",
                limits.max_card_bytes
            ));
        }
    }
    Ok(())
}

async fn run(
    State(state): State<Arc<Server>>,
    Extension(user): Extension<User>,
    path: Result<UrlPath<String>, PathRejection>,
    body: Result<Bytes, BytesRejection>,
) -> Result<Response, ApiError> {
    let id = path_board(path)?;
    let dir = state.board(&user, &id)?;
    let cmds = parse_commands(body)?;
    // Waiting for the board happens here, on the async side, holding no blocking thread.
    let held = state.board_lock(&id).lock_owned().await;
    let data = state.config.data.clone();
    let limits = state.config.limits.clone();
    let reply = state
        .blocking(move || {
            let _held = held;
            run_and_log(
                &data,
                &id,
                &user.name,
                &cmds,
                limits.log_rotate_bytes,
                || {
                    markban_core::fs::run_guarded(&dir, &cmds, &|before, after| {
                        check_limits(&limits, before, after)
                    })
                },
            )
        })
        .await?;
    Ok(json_ok(to_json(&reply)?))
}

/// Runs `cmds` with `runner` and logs the result: one line per command after a success, and
/// after a partial run one line per command with `"outcome":"partial"` and the written paths.
/// A log failure after a success still answers success, with `"log":"failed"`.
fn run_and_log(
    data: &Path,
    id: &str,
    user: &str,
    cmds: &[Command],
    rotate_at: u64,
    runner: impl FnOnce() -> Result<Vec<markban_core::Outcome>, RunError>,
) -> Result<RunReply, ApiError> {
    match runner() {
        Ok(outcomes) => {
            let ids: Vec<&str> = outcomes.iter().map(|o| o.id.as_str()).collect();
            let log = append_log(data, id, user, cmds, &ids, None, rotate_at)
                .inspect_err(|e| {
                    eprintln!("markban serve: cannot append to the log of board '{id}': {e}");
                })
                .err()
                .map(|_| "failed");
            Ok(RunReply { outcomes, log })
        }
        Err(error) => {
            if let RunError::PartiallyApplied { written, .. } = &error {
                let partial = vec!["partial"; cmds.len()];
                if let Err(e) = append_log(data, id, user, cmds, &partial, Some(written), rotate_at)
                {
                    eprintln!("markban serve: cannot append to the log of board '{id}': {e}");
                }
            }
            Err(ApiError::from_run(error))
        }
    }
}

#[derive(Deserialize)]
struct LogQuery {
    limit: Option<usize>,
}

/// Read size when scanning a log backwards.
const LOG_BLOCK: u64 = 64 * 1024;

/// The last `want` lines of `path` that are valid JSON, newest first, read backwards from the
/// end in blocks. A missing file has none.
fn tail_lines(path: &Path, want: usize) -> std::io::Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut out = Vec::new();
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    let mut pos = file.metadata()?.len();
    // The bytes from `pos` to the oldest line taken so far; only its first line can be torn.
    let mut buf: Vec<u8> = Vec::new();
    let keep = |line: Vec<u8>, out: &mut Vec<String>| {
        if let Ok(text) = String::from_utf8(line)
            && serde_json::from_str::<serde::de::IgnoredAny>(&text).is_ok()
        {
            out.push(text);
        }
    };
    while out.len() < want {
        while out.len() < want {
            let Some(newline) = buf.iter().rposition(|&b| b == b'\n') else {
                break;
            };
            let line = buf.split_off(newline.saturating_add(1));
            buf.truncate(newline);
            keep(line, &mut out);
        }
        if out.len() >= want {
            break;
        }
        if pos == 0 {
            keep(std::mem::take(&mut buf), &mut out);
            break;
        }
        let size = LOG_BLOCK.min(pos);
        pos = pos.saturating_sub(size);
        file.seek(SeekFrom::Start(pos))?;
        let mut block = Vec::new();
        (&mut file).take(size).read_to_end(&mut block)?;
        block.extend_from_slice(&buf);
        buf = block;
    }
    out.truncate(want);
    Ok(out)
}

/// The last `limit` valid lines of a board's log, the rotated file included, as a JSON array
/// (newest last).
fn read_log(path: &Path, limit: usize) -> Result<String, ApiError> {
    let fail = |e: std::io::Error| {
        eprintln!("markban serve: cannot read '{}': {e}", path.display());
        ApiError::internal()
    };
    let mut lines = tail_lines(path, limit).map_err(fail)?;
    let missing = limit.saturating_sub(lines.len());
    if missing > 0 {
        lines.extend(tail_lines(&access::rotated_log_path(path), missing).map_err(fail)?);
    }
    lines.reverse();
    Ok(format!("[{}]", lines.join(",")))
}

async fn log(
    State(state): State<Arc<Server>>,
    Extension(user): Extension<User>,
    path: Result<UrlPath<String>, PathRejection>,
    query: Result<Query<LogQuery>, QueryRejection>,
) -> Result<Response, ApiError> {
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
    let body = state.blocking(move || read_log(&file, limit)).await?;
    Ok(json_ok(body))
}

/// `GET /healthz`: `ok`, for container and load-balancer health checks. Unauthenticated, outside
/// the `Host` guard and the admission gate, and touches no file.
async fn healthz() -> &'static str {
    "ok"
}

async fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "no such endpoint; the API lives under /api/boards",
    )
}

/// The router: `GET /healthz` on its own, and `/api/boards…` behind the admission gate
/// (concurrency and timeout), the `Host` guard and authentication, with the body limit.
pub fn router(config: Config) -> Router {
    let limits = &config.limits;
    let state = Arc::new(Server {
        blocking: Arc::new(Semaphore::new(limits.max_blocking.max(1))),
        requests: Arc::new(Semaphore::new(limits.max_concurrent.max(1))),
        config,
        locks: Mutex::new(BTreeMap::new()),
        access: Mutex::new(None),
    });
    let api = Router::new()
        .route("/api/boards", get(list_boards))
        .route("/api/boards/{board}", get(board_view))
        .route("/api/boards/{board}/cards/{card}", get(card))
        .route("/api/boards/{board}/run", post(run))
        .route("/api/boards/{board}/log", get(log))
        .route_layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            authenticate,
        ))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            host_guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            Arc::clone(&state),
            gate,
        ))
        .with_state(state);
    Router::new().route("/healthz", get(healthz)).merge(api)
}

fn check_data(config: &Config) -> Result<(), String> {
    Access::load(&config.data).map(|_| ())
}

fn runtime(limits: &Limits) -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .max_blocking_threads(limits.max_blocking.max(1))
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

/// The accept loop: at most `max_connections` connections at once (more wait in the listen
/// queue), each HTTP/1.1 with a header read timeout, which also ends idle keep-alive
/// connections.
async fn serve_on(listener: std::net::TcpListener, config: Config) -> Result<(), String> {
    let listener =
        tokio::net::TcpListener::from_std(listener).map_err(|e| format!("cannot listen: {e}"))?;
    let header_timeout = config.limits.header_timeout;
    let connections = Arc::new(Semaphore::new(config.limits.max_connections.max(1)));
    let app = router(config);
    loop {
        let permit = Arc::clone(&connections)
            .acquire_owned()
            .await
            .map_err(|_| "server stopped: connection limit closed".to_string())?;
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                // Out of file descriptors and the like: wait instead of spinning.
                eprintln!("markban serve: cannot accept a connection: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let service = TowerToHyperService::new(app.clone());
        tokio::spawn(async move {
            let mut builder = hyper::server::conn::http1::Builder::new();
            builder
                .timer(TokioTimer::new())
                .header_read_timeout(header_timeout);
            let _ = builder
                .serve_connection(TokioIo::new(stream), service)
                .await;
            drop(permit);
        });
    }
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
    runtime(&config.limits)?.block_on(serve_on(listener, config))
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
    let runtime = runtime(&config.limits)?;
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
    fn a_configured_host_list_replaces_loopback() {
        let extra = parse_hosts("board.lan");
        for loopback in ["localhost", "localhost:7171", "127.0.0.1", "[::1]:80"] {
            assert!(!host_allowed(loopback, &extra), "{loopback}");
        }
        let extra = parse_hosts("board.lan,127.0.0.1:7171");
        assert!(host_allowed("127.0.0.1:7171", &extra));
        assert!(!host_allowed("127.0.0.1:8080", &extra));
    }

    #[test]
    #[allow(clippy::unwrap_used, clippy::indexing_slicing)]
    fn a_partial_run_is_logged_with_the_written_paths() {
        let data = tempfile::TempDir::new().unwrap();
        let cmds: Vec<Command> = serde_json::from_str(
            r#"[{"cmd":"card.add","title":"A"},{"cmd":"column.add","name":"X"}]"#,
        )
        .unwrap();
        let result = run_and_log(data.path(), "acme", "anna", &cmds, u64::MAX, || {
            Err(RunError::PartiallyApplied {
                written: vec!["cards/a.md".into()],
                failed: "board.md".into(),
                reason: "disk full".into(),
            })
        });
        let error = result.err().unwrap();
        assert_eq!(error.body.code, 4);
        let log = std::fs::read_to_string(access::log_path(data.path(), "acme")).unwrap();
        let lines: Vec<serde_json::Value> = log
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{log}");
        for line in &lines {
            assert_eq!(line["user"], "anna");
            assert_eq!(line["outcome"], "partial");
            assert_eq!(line["written"], serde_json::json!(["cards/a.md"]));
        }
        assert_eq!(lines[1]["cmd"]["cmd"], "column.add");
    }

    #[test]
    #[allow(clippy::unwrap_used)]
    fn long_text_is_truncated_in_the_log() {
        let data = tempfile::TempDir::new().unwrap();
        let long = "é".repeat(3000);
        let cmd = Command::CardEdit {
            id: "a".into(),
            title: None,
            description: Some(long.clone()),
            append: Some("short".into()),
            if_unchanged: None,
        };
        run_and_log(data.path(), "acme", "anna", &[cmd], u64::MAX, || {
            Ok(vec![markban_core::Outcome { id: "a".into() }])
        })
        .unwrap();
        let log = std::fs::read_to_string(access::log_path(data.path(), "acme")).unwrap();
        let line: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
        let description = line["cmd"]["description"].as_str().unwrap();
        assert!(
            description.len() <= LOG_TEXT_MAX + 40,
            "{}",
            description.len()
        );
        assert!(
            description.ends_with(&format!("…(truncated, {} bytes)", long.len())),
            "{description}"
        );
        assert!(long.starts_with(description.split('…').next().unwrap()));
        assert_eq!(line["cmd"]["append"], "short");
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
