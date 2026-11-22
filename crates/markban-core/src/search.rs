//! `search`: a linear scan over the parsed board — card ids, titles and descriptions, column
//! ids and names, and the board title and description (docs/decisions.md: no index is read or
//! written). Terms are literal, case-insensitive substrings, all of them required (AND).

use crate::board::{Board, CardFile};
use crate::view::view;
use serde::{Deserialize, Serialize};

/// At most this many distinct terms per search.
pub const MAX_TERMS: usize = 32;

/// A term is at most this many bytes.
pub const MAX_TERM_BYTES: usize = 256;

/// A search refused for its terms; one actionable line.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct SearchError(String);

/// A matching description line holds at most this many characters; longer ones are cut with a
/// trailing `…`.
const SNIPPET_MAX: usize = 120;

/// One matching description line of a card hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// 1-based line within the description.
    pub line: u32,
    /// The line, trimmed and capped at [`SNIPPET_MAX`] characters.
    pub text: String,
}

/// A card whose id, title or description contains every term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardHit {
    pub id: String,
    pub title: String,
    /// The card's column id; `None` when its `column` key is absent, empty or not a column of
    /// the board (`unsorted` in the CLI).
    pub column: Option<String>,
    pub archived: bool,
    /// The first description line containing a term that matched only the description (not the
    /// id or title).
    pub snippet: Option<Snippet>,
}

/// A column whose id and name contain every term.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnHit {
    pub id: String,
    pub name: String,
}

/// The hits of one search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    /// Matching cards: active ones first in `ls` order, then archived ones by id.
    pub cards: Vec<CardHit>,
    /// Matching columns, in board order.
    pub columns: Vec<ColumnHit>,
    /// Whether the board title and description contain every term.
    pub board: bool,
}

/// Value of `key`: present, readable and non-empty.
fn text(card: &CardFile, key: &str) -> Option<String> {
    card.get(key)?.ok().filter(|v| !v.is_empty())
}

/// The description as `view::card` shows it: the body without the leading blank line and the
/// trailing newlines.
fn description(card: &CardFile) -> &str {
    let body = card
        .body
        .strip_prefix("\r\n")
        .or_else(|| card.body.strip_prefix('\n'))
        .unwrap_or(&card.body);
    body.trim_end_matches(['\n', '\r'])
}

/// Whether `haystack` (lower case) contains every term (lower case). A `\n` joins the fields,
/// so a term never matches across two of them.
fn matches_all(haystack: &str, terms: &[String]) -> bool {
    terms.iter().all(|t| haystack.contains(t.as_str()))
}

/// The lower-cased terms without duplicates (first occurrence wins), after the limits are
/// checked: at most [`MAX_TERMS`] distinct terms of at most [`MAX_TERM_BYTES`] bytes each.
fn prepare(terms: &[String]) -> Result<Vec<String>, SearchError> {
    if let Some(term) = terms.iter().find(|t| t.len() > MAX_TERM_BYTES) {
        return Err(SearchError(format!(
            "a search term is {} bytes; the limit is {MAX_TERM_BYTES} bytes per term",
            term.len()
        )));
    }
    let mut unique: Vec<String> = Vec::new();
    for term in terms {
        let lower = term.to_lowercase();
        if !unique.contains(&lower) {
            unique.push(lower);
        }
    }
    if unique.len() > MAX_TERMS {
        return Err(SearchError(format!(
            "{} distinct search terms; the limit is {MAX_TERMS}, use fewer words",
            unique.len()
        )));
    }
    Ok(unique)
}

/// Checks the limits without searching, for callers that may have no board to search.
///
/// # Errors
/// More than [`MAX_TERMS`] distinct terms, or a term over [`MAX_TERM_BYTES`] bytes.
pub fn check_terms(terms: &[String]) -> Result<(), SearchError> {
    prepare(terms).map(|_| ())
}

/// The first line of `text` containing any term, trimmed and capped with a trailing `…`.
fn snippet(text: &str, terms: &[String]) -> Option<Snippet> {
    for (index, line) in text.lines().enumerate() {
        let lower = line.to_lowercase();
        if !terms.iter().any(|t| lower.contains(t.as_str())) {
            continue;
        }
        let trimmed = line.trim();
        let mut cut: String = trimmed.chars().take(SNIPPET_MAX).collect();
        if cut.chars().count() < trimmed.chars().count() {
            cut.push('…');
        }
        return Some(Snippet {
            line: u32::try_from(index + 1).unwrap_or(u32::MAX),
            text: cut,
        });
    }
    None
}

/// The terms that match neither the id nor the title: only those earn a snippet.
fn only_in_description(id: &str, title: &str, terms: &[String]) -> Vec<String> {
    let head = format!("{id}\n{title}").to_lowercase();
    terms
        .iter()
        .filter(|t| !head.contains(t.as_str()))
        .cloned()
        .collect()
}

/// Searches the whole board: a hit must contain every term (case-insensitive, literal
/// substring) in the union of its fields. A pure scan over `board`; no IO, no index. Duplicate
/// terms count once.
///
/// # Errors
/// More than [`MAX_TERMS`] distinct terms, or a term over [`MAX_TERM_BYTES`] bytes.
pub fn search(board: &Board, terms: &[String]) -> Result<SearchResults, SearchError> {
    let terms = prepare(terms)?;
    if terms.is_empty() {
        return Ok(SearchResults {
            cards: Vec::new(),
            columns: Vec::new(),
            board: false,
        });
    }

    // The view gives the `ls` order of the active cards, the unsorted ones and the archived
    // ones (already by id).
    let board_view = view(board);
    let valid_columns: Vec<&str> = board_view.columns.iter().map(|c| c.id.as_str()).collect();

    let mut cards = Vec::new();
    let order = board_view
        .columns
        .iter()
        .flat_map(|c| c.cards.iter().map(|s| s.id.as_str()))
        .chain(board_view.unsorted.iter().map(|s| s.id.as_str()))
        .chain(board_view.archived.iter().map(|s| s.id.as_str()));
    for id in order {
        let Some(card) = board.cards.get(id) else {
            continue;
        };
        let title = text(card, "title").unwrap_or_else(|| id.to_string());
        let description = description(card);
        let haystack = format!("{id}\n{title}\n{description}").to_lowercase();
        if !matches_all(&haystack, &terms) {
            continue;
        }
        let snippet = snippet(description, &only_in_description(id, &title, &terms));
        cards.push(CardHit {
            id: id.to_string(),
            title,
            column: text(card, "column").filter(|c| valid_columns.contains(&c.as_str())),
            archived: card.is_archived(),
            snippet,
        });
    }

    let columns = board_view
        .columns
        .iter()
        .filter(|c| matches_all(&format!("{}\n{}", c.id, c.name).to_lowercase(), &terms))
        .map(|c| ColumnHit {
            id: c.id.clone(),
            name: c.name.clone(),
        })
        .collect();

    let board_hit = matches_all(
        &format!("{}\n{}", board_view.title, board_view.description).to_lowercase(),
        &terms,
    );

    Ok(SearchResults {
        cards,
        columns,
        board: board_hit,
    })
}
