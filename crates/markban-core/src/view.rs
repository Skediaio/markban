//! `view`: the read-only projection of a board that a viewer renders.

use crate::board::{Board, CardFile};
use crate::check::check;
use crate::diagnostic::Diagnostic;
use crate::order::Decimal;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardSummary {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColumnView {
    pub id: String,
    pub name: String,
    pub cards: Vec<CardSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardView {
    pub title: String,
    pub description: String,
    pub columns: Vec<ColumnView>,
    /// Cards whose column is missing or not a column of the board, sorted by id.
    pub unsorted: Vec<CardSummary>,
    pub diagnostics: Vec<Diagnostic>,
}

/// One card in full: what `markban show` prints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CardDetail {
    pub id: String,
    pub title: String,
    pub column: Option<String>,
    /// The text after the frontmatter, without the leading blank line and trailing newlines.
    pub description: String,
}

/// The card `id` in full, `None` when the board has no such card.
#[must_use]
pub fn card(board: &Board, id: &str) -> Option<CardDetail> {
    let card = board.cards.get(id)?;
    let body = card
        .body
        .strip_prefix("\r\n")
        .or_else(|| card.body.strip_prefix('\n'))
        .unwrap_or(&card.body);
    Some(CardDetail {
        id: id.to_string(),
        title: text(card, "title").unwrap_or_else(|| id.to_string()),
        column: text(card, "column"),
        description: body.trim_end_matches(['\n', '\r']).to_string(),
    })
}

struct Entry {
    order: Option<Decimal>,
    summary: CardSummary,
}

fn compare(a: &Entry, b: &Entry) -> Ordering {
    match (&a.order, &b.order) {
        (Some(x), Some(y)) => x.cmp(y).then_with(|| a.summary.id.cmp(&b.summary.id)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.summary.id.cmp(&b.summary.id),
    }
}

fn text(card: &CardFile, key: &str) -> Option<String> {
    card.get(key)?.ok().filter(|v| !v.is_empty())
}

/// Builds the view. Sorting follows spec section 4: cards with an `order` ascending (exact
/// decimal comparison), ties by id; then cards without a valid `order`, by id.
#[must_use]
pub fn view(board: &Board) -> BoardView {
    let file = &board.board;
    let mut columns: Vec<(String, String)> = Vec::new();
    for column in file.columns() {
        if let Some(id) = column
            .id
            .filter(|id| !columns.iter().any(|(seen, _)| seen == id))
        {
            let name = column
                .name
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| id.clone());
            columns.push((id, name));
        }
    }

    let mut by_column: BTreeMap<&str, Vec<Entry>> = BTreeMap::new();
    let mut unsorted = Vec::new();
    for (id, card) in &board.cards {
        let summary = CardSummary {
            id: id.clone(),
            title: text(card, "title").unwrap_or_else(|| id.clone()),
        };
        let order = text(card, "order").and_then(|o| Decimal::parse(&o));
        let known = text(card, "column").and_then(|c| columns.iter().find(|(cid, _)| *cid == c));
        match known {
            Some((cid, _)) => by_column
                .entry(cid.as_str())
                .or_default()
                .push(Entry { order, summary }),
            None => unsorted.push(summary),
        }
    }

    let columns = columns
        .iter()
        .map(|(id, name)| {
            let mut entries = by_column.remove(id.as_str()).unwrap_or_default();
            entries.sort_by(compare);
            ColumnView {
                id: id.clone(),
                name: name.clone(),
                cards: entries.into_iter().map(|e| e.summary).collect(),
            }
        })
        .collect();

    BoardView {
        title: file
            .frontmatter
            .get("title")
            .and_then(Result::ok)
            .unwrap_or_default(),
        description: file.description.trim().to_string(),
        columns,
        unsorted,
        diagnostics: check(board),
    }
}
