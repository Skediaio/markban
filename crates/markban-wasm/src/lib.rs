//! WebAssembly bindings for markban-core. A board is passed in as in-memory files
//! (`{ path, content }`), never read from disk. The pure functions at the top are
//! testable natively; the `wasm_bindgen` exports at the bottom only convert to and from JSON.

use markban_core::board::Board;
use markban_core::diagnostic::{Code, Diagnostic};
use markban_core::search::{SearchResults, search as core_search};
use markban_core::view::{self, BoardView, CardDetail};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// One file of a board: `board.md` or `cards/<id>.md`, with its UTF-8 text.
#[derive(Debug, Deserialize)]
pub struct InFile {
    pub path: String,
    pub content: String,
}

/// `(file name, bytes)` of the card files.
type Cards = Vec<(String, Vec<u8>)>;

/// What `split` makes of the files.
struct Split {
    board_md: Option<Vec<u8>>,
    cards: Cards,
    /// Names in `cards/` that are not card files: other extensions, subdirectories.
    ignored: Vec<String>,
}

/// Splits the files into `board.md`, card files and ignored names. Paths use `/` (or `\\`) and
/// are relative to the board directory. Anything outside `cards/` other than `board.md` is
/// dropped. Below `cards/`, a path with a further `/` (a subdirectory, or `cards/dir/` for an
/// empty one) is reported by its first segment, as the core does for a directory on disk.
fn split(files: Vec<InFile>) -> Split {
    let mut out = Split {
        board_md: None,
        cards: Vec::new(),
        ignored: Vec::new(),
    };
    for file in files {
        let path = file.path.replace('\\', "/");
        let path = path.strip_prefix("./").unwrap_or(&path);
        if path == "board.md" {
            out.board_md = Some(file.content.into_bytes());
        } else if let Some(rest) = path.strip_prefix("cards/") {
            match rest.split_once('/') {
                Some((dir, _)) if !dir.is_empty() => out.ignored.push(dir.to_string()),
                Some(_) => {}
                None if !rest.is_empty() => out
                    .cards
                    .push((rest.to_string(), file.content.into_bytes())),
                None => {}
            }
        }
    }
    out
}

fn load(files: Vec<InFile>) -> (Option<Board>, Vec<Diagnostic>) {
    let Split {
        board_md,
        cards,
        ignored,
    } = split(files);
    match board_md {
        Some(bytes) => Board::parse(&bytes, cards, ignored),
        None => (
            None,
            vec![Diagnostic::new(
                Code::E001,
                "board.md",
                None,
                "board.md is missing; add a file named board.md with the board frontmatter",
            )],
        ),
    }
}

fn sort(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by(|a, b| {
        (&a.file, a.line, a.code, &a.message).cmp(&(&b.file, b.line, b.code, &b.message))
    });
}

/// The board view, with the parse diagnostics next to the `check` ones (as `markban ls --json`).
/// A board without a usable `board.md` gives an empty view that carries the diagnostics.
#[must_use]
pub fn parse_board(files: Vec<InFile>) -> BoardView {
    let (board, parse_diagnostics) = load(files);
    match board {
        Some(board) => {
            let mut board_view = view::view(&board);
            let mut diagnostics = parse_diagnostics;
            diagnostics.append(&mut board_view.diagnostics);
            sort(&mut diagnostics);
            board_view.diagnostics = diagnostics;
            board_view
        }
        None => BoardView {
            title: String::new(),
            description: String::new(),
            columns: Vec::new(),
            unsorted: Vec::new(),
            archived: Vec::new(),
            diagnostics: parse_diagnostics,
        },
    }
}

/// The card `id` in full, `None` when there is no usable board or no such card.
#[must_use]
pub fn card_detail(files: Vec<InFile>, id: &str) -> Option<CardDetail> {
    let (board, _) = load(files);
    view::card(&board?, id)
}

/// All diagnostics of the board, sorted.
#[must_use]
pub fn check_board(files: Vec<InFile>) -> Vec<Diagnostic> {
    parse_board(files).diagnostics
}

/// The hits of a full-board search (see `markban search`). A board without a usable
/// `board.md` gives empty results: search is a pure query and carries no diagnostics.
///
/// # Errors
/// More than 32 distinct terms, or a term over 256 bytes.
pub fn search(files: Vec<InFile>, terms: Vec<String>) -> Result<SearchResults, String> {
    match load(files).0 {
        Some(board) => core_search(&board, &terms).map_err(|e| e.to_string()),
        None => markban_core::search::check_terms(&terms)
            .map(|()| SearchResults {
                cards: Vec::new(),
                columns: Vec::new(),
                board: false,
            })
            .map_err(|e| e.to_string()),
    }
}

fn files_from(value: JsValue) -> Result<Vec<InFile>, JsError> {
    serde_wasm_bindgen::from_value(value).map_err(|e| {
        JsError::new(&format!(
            "files must be an array of {{ path, content }} strings: {e}"
        ))
    })
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| JsError::new(&format!("cannot encode result: {e}")))
}

/// `parseBoard(files)`: the `BoardView`.
///
/// # Errors
/// `files` is not an array of `{ path, content }` strings.
#[wasm_bindgen(js_name = parseBoard)]
pub fn parse_board_js(files: JsValue) -> Result<JsValue, JsError> {
    to_js(&parse_board(files_from(files)?))
}

/// `card(files, id)`: the `CardDetail`, or `null`.
///
/// # Errors
/// `files` is not an array of `{ path, content }` strings.
#[wasm_bindgen(js_name = card)]
pub fn card_js(files: JsValue, id: &str) -> Result<JsValue, JsError> {
    to_js(&card_detail(files_from(files)?, id))
}

/// `check(files)`: the diagnostics.
///
/// # Errors
/// `files` is not an array of `{ path, content }` strings.
#[wasm_bindgen(js_name = check)]
pub fn check_js(files: JsValue) -> Result<JsValue, JsError> {
    to_js(&check_board(files_from(files)?))
}

/// `search(files, terms)`: the hits.
///
/// # Errors
/// `files` is not an array of `{ path, content }` strings, `terms` is not an array of
/// strings, or the terms are over the limits (32 distinct, 256 bytes each).
#[wasm_bindgen(js_name = search)]
pub fn search_js(files: JsValue, terms: JsValue) -> Result<JsValue, JsError> {
    let terms: Vec<String> = serde_wasm_bindgen::from_value(terms)
        .map_err(|e| JsError::new(&format!("terms must be an array of strings: {e}")))?;
    let hits = search(files_from(files)?, terms).map_err(|e| JsError::new(&e))?;
    to_js(&hits)
}
