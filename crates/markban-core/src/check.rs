//! `check`: validates a board against spec section 6.

use crate::board::{Board, CardFile};
use crate::diagnostic::{Code, Diagnostic};
use crate::frontmatter::ValueError;
use crate::id::{MAX_ID_LEN, is_valid_id};
use crate::order::Decimal;
use std::collections::{BTreeMap, BTreeSet};

/// The only format version this tool supports.
pub const SUPPORTED_VERSION: &str = "0.1";

const BOARD_FILE: &str = "board.md";

/// Reads a key as `Ok(Some(non-empty value))`, `Ok(None)` when absent or empty, or the
/// malformed-value error.
fn value(card: &CardFile, key: &str) -> Result<Option<String>, ValueError> {
    match card.get(key) {
        None => Ok(None),
        Some(Err(e)) => Err(e),
        Some(Ok(v)) if v.is_empty() => Ok(None),
        Some(Ok(v)) => Ok(Some(v)),
    }
}

/// Validates `board`; the result is sorted by file, line and code.
#[must_use]
pub fn check(board: &Board) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let column_ids = check_board_file(board, &mut out);
    let mut orders: BTreeMap<(String, Decimal), Vec<&str>> = BTreeMap::new();
    for (id, card) in &board.cards {
        check_card(id, card, &column_ids, &mut out, &mut orders);
    }
    for ((column, _), ids) in &orders {
        for later in ids.iter().skip(1) {
            let first = ids.first().copied().unwrap_or_default();
            out.push(Diagnostic::new(
                Code::W002,
                format!("cards/{later}.md"),
                card_line(board, later, "order"),
                format!("card has the same order as '{first}' in column '{column}'; ties sort by id, give it another order"),
            ));
        }
    }
    for name in &board.ignored {
        out.push(Diagnostic::new(
            Code::W001,
            format!("cards/{name}"),
            None,
            "not a card file; cards are `<id>.md`, remove or move this entry",
        ));
    }
    out.sort_by(|a, b| {
        (&a.file, a.line, a.code, &a.message).cmp(&(&b.file, b.line, b.code, &b.message))
    });
    out
}

fn card_line(board: &Board, id: &str, key: &str) -> Option<u32> {
    board.cards.get(id)?.frontmatter.as_ref()?.line_of(key)
}

/// Checks `board.md`; returns the valid, first-seen column ids in order.
fn check_board_file(board: &Board, out: &mut Vec<Diagnostic>) -> Vec<String> {
    let file = &board.board;
    let fm = &file.frontmatter;
    let mut push = |code, line, message: String| {
        out.push(Diagnostic::new(code, BOARD_FILE, line, message));
    };

    match fm.get("markban") {
        Some(Err(e)) => push(Code::E009, fm.line_of("markban"), e.to_string()),
        Some(Ok(v)) if !v.is_empty() => {
            if v != SUPPORTED_VERSION {
                push(
                    Code::E002,
                    fm.line_of("markban"),
                    format!("unsupported version '{v}'; this tool supports {SUPPORTED_VERSION}"),
                );
            }
        }
        _ => push(
            Code::E002,
            Some(1),
            format!("`markban` is missing; add `markban: {SUPPORTED_VERSION}`"),
        ),
    }
    match fm.get("title") {
        Some(Err(e)) => push(Code::E009, fm.line_of("title"), e.to_string()),
        Some(Ok(v)) if !v.is_empty() => {}
        _ => push(
            Code::E003,
            Some(1),
            "board `title` is missing; add `title: <name>`".to_string(),
        ),
    }

    if !file.has_columns_block() {
        push(
            Code::E004,
            Some(1),
            "`columns:` block is missing; add `columns:` followed by items `  - id: <id>` and `    name: <name>`"
                .to_string(),
        );
    }
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for column in file.columns() {
        for (key, e) in &column.errors {
            push(
                Code::E009,
                Some(column.line()),
                format!("column {key}: {e}"),
            );
        }
        match column.id.as_deref() {
            None | Some("") => {
                if !column.errors.iter().any(|(k, _)| *k == "id") {
                    push(
                        Code::E004,
                        Some(column.line()),
                        "column item has no `id`; add `  - id: <id>`".to_string(),
                    );
                }
            }
            Some(id) if !is_valid_id(id) => push(
                Code::E004,
                Some(column.line()),
                format!(
                    "column id '{id}' is malformed; use lowercase letters, digits and single '-'"
                ),
            ),
            Some(id) if !seen.insert(id.to_string()) => push(
                Code::E004,
                Some(column.line()),
                format!("column id '{id}' is used twice; ids must be unique"),
            ),
            Some(id) => ids.push(id.to_string()),
        }
        if column.name.as_deref().is_none_or(str::is_empty)
            && !column.errors.iter().any(|(k, _)| *k == "name")
        {
            push(
                Code::E004,
                Some(column.line()),
                format!(
                    "column '{}' has no `name`; add `    name: <name>`",
                    column.id.as_deref().unwrap_or("?")
                ),
            );
        }
    }
    ids
}

fn check_card<'a>(
    id: &str,
    card: &'a CardFile,
    column_ids: &[String],
    out: &mut Vec<Diagnostic>,
    orders: &mut BTreeMap<(String, Decimal), Vec<&'a str>>,
) {
    let file = format!("cards/{}", card.file_name());
    let mut push =
        |code, line, message: String| out.push(Diagnostic::new(code, file.clone(), line, message));

    if !is_valid_id(id) || id.len() > MAX_ID_LEN {
        push(
            Code::E005,
            None,
            format!(
                "card file name '{id}.md' is not a valid id; use lowercase letters, digits and single '-', at most {MAX_ID_LEN} characters, e.g. 'fix-login'"
            ),
        );
    }
    let Some(fm) = &card.frontmatter else {
        let why = card.error.map(|e| e.to_string()).unwrap_or_default();
        push(Code::E006, Some(1), why);
        return;
    };

    match value(card, "title") {
        Err(e) => push(Code::E009, fm.line_of("title"), e.to_string()),
        Ok(None) => push(
            Code::E003,
            Some(1),
            "card `title` is missing; add `title: <text>`".to_string(),
        ),
        Ok(Some(_)) => {}
    }

    let known = if column_ids.is_empty() {
        "(none)".to_string()
    } else {
        column_ids.join(", ")
    };
    let column = match value(card, "column") {
        Err(e) => {
            push(Code::E009, fm.line_of("column"), e.to_string());
            None
        }
        Ok(None) => {
            push(
                Code::E007,
                Some(1),
                format!("card has no `column`; columns: {known}"),
            );
            None
        }
        Ok(Some(c)) if !column_ids.contains(&c) => {
            push(
                Code::E007,
                fm.line_of("column"),
                format!("card column '{c}' is not a column; columns: {known}"),
            );
            None
        }
        Ok(Some(c)) => Some(c),
    };

    match card.get("order") {
        None => {}
        Some(Err(e)) => push(Code::E009, fm.line_of("order"), e.to_string()),
        Some(Ok(text)) => match Decimal::parse(&text) {
            None => push(
                Code::E008,
                fm.line_of("order"),
                format!("card order '{text}' is not a decimal number; use e.g. 1500 or 1500.25"),
            ),
            Some(order) => {
                if let Some(column) = column {
                    orders.entry((column, order)).or_default().push(&card.id);
                }
            }
        },
    }
}
