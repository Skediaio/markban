//! Server side: board loading, markdown rendering, file watching and the revision counter.

use crate::api::CardDetail;
use dioxus::server::axum::extract::Request;
use dioxus::server::axum::http::StatusCode;
use dioxus::server::axum::middleware::Next;
use dioxus::server::axum::response::{IntoResponse, Response};
use markban_core::board::Board;
use markban_core::diagnostic::{Code, Diagnostic};
use markban_core::view::{BoardView, view};
use markban_server::{host_allowed, parse_hosts, request_host};
use notify_debouncer_full::notify::{self, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::watch;

const DEBOUNCE: Duration = Duration::from_millis(150);
const LONG_POLL: Duration = Duration::from_secs(25);

/// Whether `url` is relative: no scheme and not protocol-relative. Browsers ignore tabs and
/// newlines inside URLs and read `\` like `/`, so those are normalised first.
fn is_relative_url(url: &str) -> bool {
    let url: String = url
        .chars()
        .filter(|c| !matches!(c, '\t' | '\n' | '\r'))
        .collect();
    let url = url.trim_start_matches(|c: char| c.is_whitespace() || c.is_control());
    if url.starts_with("//") || url.starts_with("/\\") || url.starts_with('\\') {
        return false;
    }
    let head = url.split(['/', '?', '#']).next().unwrap_or_default();
    !head.contains(':')
}

/// Renders Markdown to HTML and strips everything active (scripts, event handlers,
/// `javascript:` URLs). Agents write this content, so it is never trusted.
///
/// Images with a remote `src` are dropped (a remote image leaks the viewer's address). Task-list
/// items become disabled checkboxes; any `<input>` is forced to a disabled checkbox.
#[must_use]
pub fn render_markdown(markdown: &str) -> String {
    let mut html = String::new();
    let parser =
        pulldown_cmark::Parser::new_ext(markdown, pulldown_cmark::Options::ENABLE_TASKLISTS);
    pulldown_cmark::html::push_html(&mut html, parser);
    let mut sanitizer = ammonia::Builder::default();
    sanitizer
        .add_tags(["input"])
        .add_tag_attributes("input", ["checked"])
        .set_tag_attribute_value("input", "type", "checkbox")
        .set_tag_attribute_value("input", "disabled", "")
        .attribute_filter(|element, attribute, value| {
            if element == "img" && attribute == "src" && !is_relative_url(value) {
                None
            } else {
                Some(value.into())
            }
        });
    sanitizer.clean(&html).to_string()
}

type BoardDebouncer = Debouncer<notify::RecommendedWatcher, RecommendedCache>;

/// Watches a board directory and bumps a revision on every change to `board.md` or `cards/*.md`.
///
/// Only the board directory and `cards/` are watched, both non-recursively: a board may live at
/// a repository root next to `.git/` and `target/`, which must not cost watches or churn.
pub struct Watcher {
    debouncer: Arc<Mutex<BoardDebouncer>>,
    watched: Arc<Mutex<Vec<PathBuf>>>,
    root: PathBuf,
    tx: Arc<watch::Sender<u64>>,
}

impl Watcher {
    /// Starts watching `dir`. The revision starts at 1.
    ///
    /// # Errors
    /// A message when the directory cannot be watched.
    pub fn start(dir: &Path) -> Result<(Self, watch::Receiver<u64>), String> {
        let (tx, rx) = watch::channel(1_u64);
        let tx = Arc::new(tx);
        let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
        let (cards_tx, cards_rx) = std::sync::mpsc::channel::<()>();
        let handler = {
            let root = root.clone();
            let tx = Arc::clone(&tx);
            move |result: DebounceEventResult| {
                let cards = root.join("cards");
                let relevant = match &result {
                    Ok(events) => {
                        if events
                            .iter()
                            .any(|e| e.kind.is_create() && e.paths.contains(&cards))
                        {
                            let _ = cards_tx.send(());
                        }
                        events.iter().any(|event| {
                            !event.kind.is_access()
                                && event.paths.iter().any(|p| is_board_path(&root, p))
                        })
                    }
                    Err(_) => true,
                };
                if relevant {
                    tx.send_modify(|rev| *rev += 1);
                }
            }
        };
        let mut debouncer = new_debouncer(DEBOUNCE, None, handler)
            .map_err(|e| format!("cannot start the file watcher: {e}"))?;
        debouncer
            .watch(&root, RecursiveMode::NonRecursive)
            .map_err(|e| format!("cannot watch '{}': {e}", root.display()))?;
        let watcher = Self {
            debouncer: Arc::new(Mutex::new(debouncer)),
            watched: Arc::new(Mutex::new(vec![root.clone()])),
            root,
            tx,
        };
        watcher.ensure_cards();
        // A `cards/` that appears after the start is watched as soon as its create event arrives.
        // The thread holds only a weak reference, so dropping the watcher stops everything.
        let debouncer = Arc::downgrade(&watcher.debouncer);
        let (watched, root, tx) = (
            Arc::clone(&watcher.watched),
            watcher.root.clone(),
            Arc::clone(&watcher.tx),
        );
        std::thread::spawn(move || {
            while cards_rx.recv().is_ok() {
                let Some(debouncer) = debouncer.upgrade() else {
                    break;
                };
                add_cards_watch(&debouncer, &watched, &root, &tx);
            }
        });
        Ok((watcher, rx))
    }

    /// Adds the `cards/` watch when it is missing and the directory exists. Cheap to call often.
    pub fn ensure_cards(&self) {
        add_cards_watch(&self.debouncer, &self.watched, &self.root, &self.tx);
    }

    /// The directories currently watched (all non-recursively).
    #[cfg(test)]
    #[must_use]
    pub fn watched(&self) -> Vec<PathBuf> {
        self.watched.lock().map(|w| w.clone()).unwrap_or_default()
    }
}

/// Watches `<root>/cards` non-recursively unless it already is, then bumps the revision: files
/// written before the watch existed would otherwise go unseen.
fn add_cards_watch(
    debouncer: &Mutex<BoardDebouncer>,
    watched: &Mutex<Vec<PathBuf>>,
    root: &Path,
    tx: &watch::Sender<u64>,
) {
    let cards = root.join("cards");
    let (Ok(mut watched), Ok(mut debouncer)) = (watched.lock(), debouncer.lock()) else {
        return;
    };
    if watched.contains(&cards) {
        return;
    }
    if debouncer.watch(&cards, RecursiveMode::NonRecursive).is_ok() {
        watched.push(cards);
        tx.send_modify(|rev| *rev += 1);
    }
}

/// Whether a changed path belongs to the board: `board.md`, `cards/` or `cards/<name>.md`.
fn is_board_path(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else {
        return false;
    };
    let mut parts = relative.components().map(|c| c.as_os_str());
    match (parts.next(), parts.next(), parts.next()) {
        (Some(first), None, _) => first == "board.md" || first == "cards",
        (Some(first), Some(name), None) => {
            first == "cards" && Path::new(name).extension().is_some_and(|e| e == "md")
        }
        _ => false,
    }
}

/// Extra allowed hosts from `MARKBAN_ALLOWED_HOSTS`, read once.
fn extra_hosts() -> &'static [String] {
    static EXTRA: OnceLock<Vec<String>> = OnceLock::new();
    EXTRA.get_or_init(|| parse_hosts(&std::env::var("MARKBAN_ALLOWED_HOSTS").unwrap_or_default()))
}

/// Middleware: answers 403 unless the request's `Host` is allowed (see [`host_allowed`]).
/// `/healthz` is exempt: it reveals nothing, and container health checks reach it under
/// whatever host name the container itself uses.
pub async fn host_guard(request: Request, next: Next) -> Response {
    if request.uri().path() == "/healthz"
        || request_host(&request).is_some_and(|h| host_allowed(&h, extra_hosts()))
    {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "forbidden: unknown Host header; set MARKBAN_ALLOWED_HOSTS to allow it\n",
        )
            .into_response()
    }
}

/// Returns the value in `cell`, creating it with `make` first when it is empty. A failure is
/// returned but not stored, so the next call tries again.
fn init_or_retry<T>(
    cell: &OnceLock<T>,
    make: impl FnOnce() -> Result<T, String>,
) -> Result<&T, String> {
    if let Some(value) = cell.get() {
        return Ok(value);
    }
    // Losing a race just drops this value; the winner's stays.
    let _ = cell.set(make()?);
    cell.get().ok_or_else(|| "state is unavailable".to_string())
}

pub struct State {
    dir: PathBuf,
    rx: watch::Receiver<u64>,
    watcher: Watcher,
}

static STATE: OnceLock<State> = OnceLock::new();

/// The board directory inside the git clone, when the board comes from `MARKBAN_GIT_URL`.
static GIT_BOARD: OnceLock<PathBuf> = OnceLock::new();

/// With `MARKBAN_GIT_URL` set: clones (or updates) the repository once, then keeps it in sync
/// in the background. A failed first sync is logged and retried; the server still starts.
///
/// # Errors
/// A message when the git settings are invalid.
pub fn start_git() -> Result<(), String> {
    let Some(source) = crate::git::GitSource::from_env()? else {
        return Ok(());
    };
    let _ = GIT_BOARD.set(source.board_dir());
    if let Err(e) = source.sync() {
        eprintln!("markban: git sync failed, retrying: {e}");
    }
    crate::git::spawn(source);
    Ok(())
}

/// Probes `GET /healthz` on this host's `PORT` (default 8080). True when it answers `ok`.
#[must_use]
pub fn healthcheck() -> bool {
    use std::io::{Read, Write};
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let timeout = Some(Duration::from_secs(3));
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(3)) else {
        return false;
    };
    let _ = stream.set_read_timeout(timeout);
    let _ = stream.set_write_timeout(timeout);
    if stream
        .write_all(b"GET /healthz HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut answer = String::new();
    let _ = stream.take(4096).read_to_string(&mut answer);
    answer.split_whitespace().nth(1) == Some("200") && answer.ends_with("ok")
}

/// The shared state; created on first use from the git clone (`MARKBAN_GIT_URL`) or else
/// `MARKBAN_BOARD` (default: current directory).
/// A failed start is not cached: the next request tries again.
///
/// # Errors
/// A message when the watcher cannot start.
pub fn state() -> Result<&'static State, String> {
    let state = init_or_retry(&STATE, || {
        let dir = GIT_BOARD.get().cloned().unwrap_or_else(|| {
            std::env::var_os("MARKBAN_BOARD").map_or_else(|| PathBuf::from("."), PathBuf::from)
        });
        let (watcher, rx) = Watcher::start(&dir)?;
        Ok(State { dir, rx, watcher })
    })?;
    // A `cards/` that was missing at the start is picked up on the next request.
    state.watcher.ensure_cards();
    Ok(state)
}

fn failed_view(message: String) -> BoardView {
    BoardView {
        title: String::new(),
        description: String::new(),
        columns: Vec::new(),
        unsorted: Vec::new(),
        diagnostics: vec![Diagnostic::new(Code::E001, ".", None, message)],
    }
}

/// Loads the board in `dir` as a view. Problems become diagnostics; this never fails.
#[must_use]
pub fn load_view(dir: &Path) -> BoardView {
    match markban_core::fs::load(dir) {
        Ok((Some(board), _)) => view(&board),
        Ok((None, diagnostics)) => BoardView {
            diagnostics,
            ..failed_view(String::new())
        },
        Err(e) => failed_view(e.to_string()),
    }
}

/// The name of the column that shows card `id`, or "Unsorted".
fn column_name(board: &Board, id: &str) -> String {
    view(board)
        .columns
        .into_iter()
        .find(|c| c.cards.iter().any(|card| card.id == id))
        .map_or_else(|| "Unsorted".to_string(), |c| c.name)
}

/// Loads one card of the board in `dir` with its description rendered to sanitised HTML.
///
/// # Errors
/// A message when the board or the card cannot be read.
pub fn load_card(dir: &Path, id: &str) -> Result<CardDetail, String> {
    let (board, _) = markban_core::fs::load(dir).map_err(|e| e.to_string())?;
    let board = board.ok_or_else(|| "board.md is missing or unusable".to_string())?;
    let card = board
        .cards
        .get(id)
        .ok_or_else(|| format!("no card with id '{id}'"))?;
    let text = |key: &str| card.get(key).and_then(Result::ok).unwrap_or_default();
    let column = column_name(&board, id);
    let title = Some(text("title"))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| id.to_string());
    Ok(CardDetail {
        id: id.to_string(),
        title,
        column,
        html: render_markdown(&card.body),
    })
}

impl State {
    #[must_use]
    pub fn board_view(&self) -> BoardView {
        load_view(&self.dir)
    }

    /// # Errors
    /// A message when the board or the card cannot be read.
    pub fn card(&self, id: &str) -> Result<CardDetail, String> {
        load_card(&self.dir, id)
    }

    /// Waits until the revision differs from `since` (at most 25 s), then returns it with the board.
    pub async fn wait_for_change(&self, since: u64) -> (u64, BoardView) {
        let mut rx = self.rx.clone();
        let _ = tokio::time::timeout(LONG_POLL, rx.wait_for(|rev| *rev != since)).await;
        let rev = *rx.borrow();
        (rev, load_view(&self.dir))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // tests may expect
mod tests {
    use super::*;

    async fn guarded_status(path: &str, host: &str) -> StatusCode {
        use dioxus::server::axum::Router;
        use dioxus::server::axum::body::Body;
        use dioxus::server::axum::routing::get;
        use tower::ServiceExt;
        let app = Router::new()
            .route("/", get(|| async { "the board" }))
            .route("/healthz", get(|| async { "ok" }))
            .layer(dioxus::server::axum::middleware::from_fn(host_guard));
        let request = Request::builder()
            .uri(path)
            .header("host", host)
            .body(Body::empty())
            .expect("request");
        app.oneshot(request).await.expect("infallible").status()
    }

    #[tokio::test]
    async fn healthz_ignores_the_host_guard_but_pages_do_not() {
        assert_eq!(
            guarded_status("/healthz", "evil.example").await,
            StatusCode::OK
        );
        assert_eq!(
            guarded_status("/", "evil.example").await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(guarded_status("/", "127.0.0.1:8080").await, StatusCode::OK);
    }

    #[test]
    fn strips_script_tags() {
        let html = render_markdown("hello <script>alert(1)</script> world");
        assert!(!html.contains("script"), "{html}");
        assert!(!html.contains("alert"), "{html}");
        assert!(html.contains("hello"));
    }

    #[test]
    fn strips_javascript_links() {
        let html =
            render_markdown("[x](javascript:alert(1)) and <a href=\"javascript:alert(2)\">y</a>");
        assert!(!html.contains("javascript"), "{html}");
    }

    #[test]
    fn strips_event_handlers() {
        let html = render_markdown("<img src=x onerror=\"alert(1)\">");
        assert!(!html.contains("onerror"), "{html}");
    }

    #[test]
    fn keeps_normal_markdown() {
        let html = render_markdown("# Title\n\n- a\n- [b](https://example.com)");
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(html.contains("<li>a</li>"), "{html}");
        assert!(html.contains("href=\"https://example.com\""), "{html}");
    }

    #[test]
    fn remote_images_are_dropped() {
        for md in [
            "![x](https://example.com/p.png)",
            "<img src=\"//evil.example/p.png\">",
            "<img src=\"/\\evil.example/p.png\">",
            "<img src=\"data:image/png;base64,AAAA\">",
            "<img src=\"HTTP://evil.example/p.png\">",
        ] {
            let html = render_markdown(md);
            assert!(!html.contains("evil.example"), "{md}: {html}");
            assert!(!html.contains("example.com"), "{md}: {html}");
            assert!(!html.contains("data:"), "{md}: {html}");
        }
        let html = render_markdown("![x](pics/p.png)");
        assert!(html.contains("src=\"pics/p.png\""), "{html}");
    }

    #[test]
    fn task_lists_render_as_disabled_checkboxes() {
        let html = render_markdown("- [x] done\n- [ ] open\n");
        assert_eq!(html.matches("<input").count(), 2, "{html}");
        assert_eq!(html.matches("type=\"checkbox\"").count(), 2, "{html}");
        assert_eq!(html.matches("disabled").count(), 2, "{html}");
        assert_eq!(html.matches("checked").count(), 1, "{html}");
    }

    #[test]
    fn raw_inputs_become_disabled_checkboxes_only() {
        let html = render_markdown("<input type=\"text\" name=\"q\" value=\"v\" onfocus=\"x()\">");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("disabled"), "{html}");
        assert!(!html.contains("text"), "{html}");
        assert!(!html.contains("name="), "{html}");
        assert!(!html.contains("onfocus"), "{html}");
    }

    #[test]
    fn failed_setup_is_retried_not_cached() {
        let cell = OnceLock::new();
        assert!(init_or_retry(&cell, || Err::<u32, _>("boom".to_string())).is_err());
        assert_eq!(init_or_retry(&cell, || Ok(7)).copied(), Ok(7));
        // Once set, the value stays and `make` is not called again.
        assert_eq!(
            init_or_retry(&cell, || Err("must not run".to_string())).copied(),
            Ok(7)
        );
    }

    #[test]
    fn only_board_files_count_as_board_paths() {
        let root = Path::new("/b");
        for yes in ["/b/board.md", "/b/cards", "/b/cards/a.md"] {
            assert!(is_board_path(root, Path::new(yes)), "{yes}");
        }
        for no in [
            "/b",
            "/b/notes.md",
            "/b/cards/a.txt",
            "/b/cards/sub/a.md",
            "/b/.git/HEAD",
            "/b/target/x.md",
            "/elsewhere/board.md",
        ] {
            assert!(!is_board_path(root, Path::new(no)), "{no}");
        }
    }

    #[test]
    fn card_detail_shows_the_column_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("board.md"),
            "---\nmarkban: 0.1\ncolumns:\n  - id: todo\n    name: To do\n---\n",
        )
        .expect("board");
        std::fs::create_dir(dir.path().join("cards")).expect("cards");
        std::fs::write(
            dir.path().join("cards/a.md"),
            "---\ntitle: A\ncolumn: todo\norder: 1000\n---\n",
        )
        .expect("a");
        std::fs::write(
            dir.path().join("cards/lost.md"),
            "---\ntitle: L\ncolumn: gone\n---\n",
        )
        .expect("lost");
        assert_eq!(load_card(dir.path(), "a").expect("a").column, "To do");
        assert_eq!(load_card(dir.path(), "lost").expect("l").column, "Unsorted");
    }

    #[tokio::test]
    async fn cards_dir_created_after_start_is_watched() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("board.md"), "---\nmarkban: 0.1\n---\n").expect("board");
        let (_watcher, mut rx) = Watcher::start(dir.path()).expect("watcher");
        let before = *rx.borrow();
        std::fs::create_dir(dir.path().join("cards")).expect("cards");
        let card = dir.path().join("cards/0001-a.md");
        std::fs::write(&card, "---\ntitle: A\ncolumn: todo\n---\n").expect("card");
        tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|rev| *rev != before))
            .await
            .expect("bumped after cards/ and a card appeared")
            .expect("sender alive");
        // Let the first burst settle, then edit: a watcher on the new directory must see it.
        tokio::time::sleep(Duration::from_millis(600)).await;
        let settled = *rx.borrow();
        std::fs::write(&card, "---\ntitle: B\ncolumn: todo\n---\n").expect("edit");
        tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|rev| *rev != settled))
            .await
            .expect("edit in the new cards/ seen")
            .expect("sender alive");
    }

    #[tokio::test]
    async fn nested_directories_are_not_watched() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("board.md"), "---\nmarkban: 0.1\n---\n").expect("board");
        std::fs::create_dir(dir.path().join("cards")).expect("cards");
        std::fs::create_dir_all(dir.path().join("deep/x")).expect("deep");
        let (watcher, rx) = Watcher::start(dir.path()).expect("watcher");
        let root = dir.path().canonicalize().expect("root");
        let mut watched = watcher.watched();
        watched.sort();
        assert_eq!(watched, vec![root.clone(), root.join("cards")]);
        let before = *rx.borrow();
        std::fs::write(dir.path().join("deep/x/y.md"), "x").expect("nested");
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(*rx.borrow(), before);
    }

    #[tokio::test]
    async fn unrelated_files_do_not_bump_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("board.md"), "---\nmarkban: 0.1\n---\n").expect("board");
        let (_watcher, rx) = Watcher::start(dir.path()).expect("watcher");
        let before = *rx.borrow();
        std::fs::write(dir.path().join("scratch.txt"), "x").expect("scratch");
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert_eq!(*rx.borrow(), before);
    }

    #[tokio::test]
    async fn card_change_bumps_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("board.md"), "---\nmarkban: 0.1\n---\n").expect("board");
        std::fs::create_dir(dir.path().join("cards")).expect("cards");
        let card = dir.path().join("cards/0001-a.md");
        std::fs::write(&card, "---\ntitle: A\ncolumn: todo\n---\n").expect("card");

        let (_watcher, mut rx) = Watcher::start(dir.path()).expect("watcher");
        let before = *rx.borrow();
        std::fs::write(&card, "---\ntitle: B\ncolumn: todo\n---\n").expect("edit");
        tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|rev| *rev != before))
            .await
            .expect("revision bumped in time")
            .expect("sender alive");
        assert!(*rx.borrow() > before);
    }

    #[tokio::test]
    async fn board_change_bumps_revision() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("board.md"), "---\nmarkban: 0.1\n---\n").expect("board");
        let (_watcher, mut rx) = Watcher::start(dir.path()).expect("watcher");
        let before = *rx.borrow();
        std::fs::write(
            dir.path().join("board.md"),
            "---\nmarkban: 0.1\ntitle: T\n---\n",
        )
        .expect("edit");
        tokio::time::timeout(Duration::from_secs(5), rx.wait_for(|rev| *rev != before))
            .await
            .expect("revision bumped in time")
            .expect("sender alive");
    }
}
