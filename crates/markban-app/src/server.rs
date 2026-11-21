//! Server side: board loading, markdown rendering, file watching and the revision counter.

use crate::api::CardDetail;
use markban_core::diagnostic::{Code, Diagnostic};
use markban_core::view::{BoardView, view};
use notify_debouncer_full::notify::{self, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::sync::watch;

const DEBOUNCE: Duration = Duration::from_millis(150);
const LONG_POLL: Duration = Duration::from_secs(25);

/// Renders Markdown to HTML and strips everything active (scripts, event handlers,
/// `javascript:` URLs). Agents write this content, so it is never trusted.
#[must_use]
pub fn render_markdown(markdown: &str) -> String {
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(markdown));
    ammonia::clean(&html)
}

/// Watches `board.md` and `cards/` of a board directory and bumps a revision on every change.
pub struct Watcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl Watcher {
    /// Starts watching `dir`. The revision starts at 1.
    ///
    /// # Errors
    /// A message when the directory cannot be watched.
    pub fn start(dir: &Path) -> Result<(Self, watch::Receiver<u64>), String> {
        let (tx, rx) = watch::channel(1_u64);
        let handler = move |result: DebounceEventResult| {
            if matches!(&result, Ok(events) if events.is_empty()) {
                return;
            }
            tx.send_modify(|rev| *rev += 1);
        };
        let mut debouncer = new_debouncer(DEBOUNCE, None, handler)
            .map_err(|e| format!("cannot start the file watcher: {e}"))?;
        // The directory itself catches board.md and the creation of cards/; cards/ catches edits.
        debouncer
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| format!("cannot watch '{}': {e}", dir.display()))?;
        let cards = dir.join("cards");
        if cards.is_dir() {
            debouncer
                .watch(&cards, RecursiveMode::NonRecursive)
                .map_err(|e| format!("cannot watch '{}': {e}", cards.display()))?;
        }
        Ok((
            Self {
                _debouncer: debouncer,
            },
            rx,
        ))
    }
}

pub struct State {
    dir: PathBuf,
    rx: watch::Receiver<u64>,
    _watcher: Watcher,
}

static STATE: OnceLock<Result<State, String>> = OnceLock::new();

/// The shared state; created on first use from `MARKBAN_BOARD` (default: current directory).
///
/// # Errors
/// A message when the watcher cannot start.
pub fn state() -> Result<&'static State, String> {
    STATE
        .get_or_init(|| {
            let dir =
                std::env::var_os("MARKBAN_BOARD").map_or_else(|| PathBuf::from("."), PathBuf::from);
            let (watcher, rx) = Watcher::start(&dir)?;
            Ok(State {
                dir,
                rx,
                _watcher: watcher,
            })
        })
        .as_ref()
        .map_err(Clone::clone)
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

impl State {
    #[must_use]
    pub fn board_view(&self) -> BoardView {
        load_view(&self.dir)
    }

    /// # Errors
    /// A message when the board or the card cannot be read.
    pub fn card(&self, id: &str) -> Result<CardDetail, String> {
        let (board, _) = markban_core::fs::load(&self.dir).map_err(|e| e.to_string())?;
        let board = board.ok_or_else(|| "board.md is missing or unusable".to_string())?;
        let card = board
            .cards
            .get(id)
            .ok_or_else(|| format!("no card with id '{id}'"))?;
        let text = |key: &str| card.get(key).and_then(Result::ok).unwrap_or_default();
        let title = Some(text("title"))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.to_string());
        Ok(CardDetail {
            id: id.to_string(),
            title,
            column: text("column"),
            html: render_markdown(&card.body),
        })
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
