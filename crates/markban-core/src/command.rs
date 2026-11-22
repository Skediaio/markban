//! Commands: every change to a board (spec section 7), executed in memory on the lossless model.
//!
//! A command changes only the lines it must. After each command the result is checked; a
//! command that would introduce an error diagnostic is refused and the board is left untouched.

use crate::board::{Board, BoardError, BoardFile, CardFile};
use crate::check::{SUPPORTED_VERSION, check};
use crate::diagnostic::{Diagnostic, Severity};
use crate::id::{MAX_ID_LEN, is_valid_id, slug};
use crate::order::{self, Decimal, OrderError};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

fn is_false(value: &bool) -> bool {
    !value
}

/// One command. JSON form: `{"cmd":"card.move","id":"x","top":true}`.
///
/// Positions are `top`, `before` or `after` (a card id); at most one may be given, the default
/// is the end of the column. Column parameters accept an id or a name, case-insensitively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", deny_unknown_fields)]
pub enum Command {
    /// Creates a board. Needs a directory without `board.md`: see [`crate::fs::run`] and
    /// [`Board::init`]; [`execute`] refuses it because a board already exists.
    #[serde(rename = "board.init")]
    BoardInit { title: String, columns: Vec<String> },
    #[serde(rename = "card.add")]
    CardAdd {
        title: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        top: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
    },
    #[serde(rename = "card.move")]
    CardMove {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        column: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        top: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        after: Option<String>,
    },
    #[serde(rename = "card.edit")]
    CardEdit {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        append: Option<String>,
        /// Optimistic check: the SHA-256 (hex) of the card file as last read (`show --json`
        /// gives it). A mismatch refuses the edit with [`CommandError::CardChanged`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        if_unchanged: Option<String>,
    },
    #[serde(rename = "card.remove")]
    CardRemove { id: String },
    #[serde(rename = "card.archive")]
    CardArchive { id: String },
    #[serde(rename = "card.unarchive")]
    CardUnarchive { id: String },
    #[serde(rename = "column.add")]
    ColumnAdd {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    #[serde(rename = "column.rename")]
    ColumnRename { column: String, name: String },
    #[serde(rename = "column.move")]
    ColumnMove {
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    #[serde(rename = "column.remove")]
    ColumnRemove {
        column: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        move_to: Option<String>,
    },
}

impl Command {
    /// Parses one JSON command.
    ///
    /// # Errors
    /// [`CommandError::Parse`] for bad JSON, an unknown `cmd`, an unknown or missing field.
    pub fn parse_line(line: &str) -> Result<Self, CommandError> {
        serde_json::from_str(line)
            .map_err(|e| CommandError::Parse(e.to_string().replace('\n', " ")))
    }
}

/// What a command touched: the card or column id, or the board title for `board.init`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub id: String,
}

/// A command was refused; the board is unchanged. Display is one actionable line.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandError {
    #[error("invalid command: {0}")]
    Parse(String),
    #[error("{0}")]
    Invalid(String),
    #[error("unknown column '{name}'; valid columns: {valid}")]
    UnknownColumn { name: String, valid: String },
    #[error("no card '{0}'; card ids are the file names in cards/ without .md")]
    UnknownCard(String),
    #[error("card '{0}' already exists; leave out `id` to get a free one")]
    CardExists(String),
    #[error("give only one of `top`, `before` and `after`")]
    ConflictingPosition,
    #[error("cannot position relative to '{0}' itself")]
    AnchorIsSelf(String),
    #[error(
        "anchor card '{anchor}' is in column '{column}', not '{wanted}'; pass `column` or pick an anchor in '{wanted}'"
    )]
    AnchorElsewhere {
        anchor: String,
        column: String,
        wanted: String,
    },
    #[error(
        "anchor card '{0}' has no valid `order`; anchor on a card that has one, or add to the end"
    )]
    AnchorUnordered(String),
    #[error("card '{0}' is archived; run card.unarchive first")]
    CardArchived(String),
    #[error("anchor card '{0}' is archived; anchor on an active card or run card.unarchive first")]
    AnchorArchived(String),
    #[error(
        "column '{column}' still has {count} card(s){}; move or remove them first, or pass `move_to` with another column",
        archived_part(.archived)
    )]
    ColumnInUse {
        column: String,
        count: usize,
        archived: usize,
    },
    #[error(
        "card '{id}' changed since it was read (`if_unchanged` does not match); show it again and retry"
    )]
    CardChanged { id: String },
    #[error("card '{id}' has no valid frontmatter (E006); fix the file by hand first")]
    CardBroken { id: String },
    #[error("board.init needs a directory without board.md; this board already exists")]
    AlreadyInitialised,
    #[error(transparent)]
    Order(#[from] OrderError),
    #[error(transparent)]
    Board(#[from] BoardError),
    #[error("change refused, it would make the board invalid: {}", summarize(.0))]
    WouldInvalidate(Vec<Diagnostic>),
}

/// A user-supplied value made safe for a one-line message: control characters become spaces and
/// long values are cut.
fn flat(text: &str) -> String {
    const MAX: usize = 80;
    let mut out: String = text
        .chars()
        .take(MAX)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if text.chars().count() > MAX {
        out.push('…');
    }
    out
}

/// The archived share of a [`CommandError::ColumnInUse`] message: `" (N archived)"` when the
/// column has archived cards, empty otherwise.
fn archived_part(archived: &usize) -> String {
    if *archived > 0 {
        format!(" ({archived} archived)")
    } else {
        String::new()
    }
}

fn summarize(diagnostics: &[Diagnostic]) -> String {
    let shown: Vec<String> = diagnostics
        .iter()
        .take(3)
        .map(ToString::to_string)
        .collect();
    let more = diagnostics.len().saturating_sub(3);
    if more == 0 {
        shown.join("; ")
    } else {
        format!("{}; and {more} more", shown.join("; "))
    }
}

/// Where a card goes in its column.
enum Position {
    End,
    Top,
    Before(String),
    After(String),
}

fn position(
    top: bool,
    before: Option<&String>,
    after: Option<&String>,
) -> Result<Option<Position>, CommandError> {
    match (top, before, after) {
        (false, None, None) => Ok(None),
        (true, None, None) => Ok(Some(Position::Top)),
        (false, Some(b), None) => Ok(Some(Position::Before(b.clone()))),
        (false, None, Some(a)) => Ok(Some(Position::After(a.clone()))),
        _ => Err(CommandError::ConflictingPosition),
    }
}

/// Runs `cmd` on `board`. All or nothing: on `Err` the board is unchanged.
///
/// `board.init` is refused here; use [`Board::init`] (and [`crate::fs::run`], which does it when
/// the directory has no `board.md`).
///
/// # Errors
/// A [`CommandError`] saying what is wrong and what is valid. A command whose result has an
/// error diagnostic that the board did not have before is refused with
/// [`CommandError::WouldInvalidate`].
pub fn execute(board: &mut Board, cmd: &Command) -> Result<Outcome, CommandError> {
    let mut known = error_counts(&check(board));
    let mut work = board.clone();
    let outcome = apply_command(&mut work, cmd)?;
    let fresh: Vec<Diagnostic> = check(&work)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| match known.get_mut(&diagnostic_key(d)) {
            Some(left) if *left > 0 => {
                *left -= 1;
                false
            }
            _ => true,
        })
        .collect();
    if !fresh.is_empty() {
        return Err(CommandError::WouldInvalidate(fresh));
    }
    *board = work;
    Ok(outcome)
}

/// Runs `cmds` in order, all or nothing: on `Err((index, error))` the board is unchanged.
///
/// # Errors
/// The index of the first failing command and its error.
pub fn execute_batch(
    board: &mut Board,
    cmds: &[Command],
) -> Result<Vec<Outcome>, (usize, CommandError)> {
    let mut work = board.clone();
    let mut outcomes = Vec::with_capacity(cmds.len());
    for (index, cmd) in cmds.iter().enumerate() {
        outcomes.push(execute(&mut work, cmd).map_err(|e| (index, e))?);
    }
    *board = work;
    Ok(outcomes)
}

/// Identity of a diagnostic for the "no new errors" guard: code and file, without the message
/// (which can embed lists that change, such as the columns in E007) or the line.
fn diagnostic_key(d: &Diagnostic) -> (String, String) {
    (d.code.to_string(), d.file.clone())
}

/// How many error diagnostics each (code, file) has.
fn error_counts(diagnostics: &[Diagnostic]) -> BTreeMap<(String, String), usize> {
    let mut counts = BTreeMap::new();
    for d in diagnostics.iter().filter(|d| d.severity == Severity::Error) {
        *counts.entry(diagnostic_key(d)).or_insert(0) += 1;
    }
    counts
}

fn apply_command(board: &mut Board, cmd: &Command) -> Result<Outcome, CommandError> {
    let id = match cmd {
        Command::BoardInit { .. } => return Err(CommandError::AlreadyInitialised),
        Command::CardAdd {
            title,
            column,
            description,
            id,
            top,
            before,
            after,
        } => {
            let pos = position(*top, before.as_ref(), after.as_ref())?;
            card_add(
                board,
                title,
                column.as_deref(),
                description.as_deref(),
                id.as_deref(),
                pos,
            )?
        }
        Command::CardMove {
            id,
            column,
            top,
            before,
            after,
        } => {
            let pos = position(*top, before.as_ref(), after.as_ref())?;
            card_move(board, id, column.as_deref(), pos)?;
            id.clone()
        }
        Command::CardEdit {
            id,
            title,
            description,
            append,
            if_unchanged,
        } => {
            card_edit(
                board,
                id,
                title.as_deref(),
                description.as_deref(),
                append.as_deref(),
                if_unchanged.as_deref(),
            )?;
            id.clone()
        }
        Command::CardRemove { id } => {
            board
                .cards
                .remove(id)
                .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
            id.clone()
        }
        Command::CardArchive { id } => {
            card_archive(board, id)?;
            id.clone()
        }
        Command::CardUnarchive { id } => {
            card_unarchive(board, id)?;
            id.clone()
        }
        Command::ColumnAdd { name, id, before } => {
            column_add(board, name, id.as_deref(), before.as_deref())?
        }
        Command::ColumnRename { column, name } => {
            one_line("column name", name)?;
            let column = resolve_column(board, column)?;
            board.board.rename_column(&column, name)?;
            column
        }
        Command::ColumnMove { column, before } => {
            let column = resolve_column(board, column)?;
            let before = before
                .as_deref()
                .map(|b| resolve_column(board, b))
                .transpose()?;
            board.board.move_column(&column, before.as_deref())?;
            column
        }
        Command::ColumnRemove { column, move_to } => {
            column_remove(board, column, move_to.as_deref())?
        }
    };
    Ok(Outcome { id })
}

// ---- board.init ----

impl Board {
    /// A new board in memory: `board.md` with `title` and one column per name, no cards.
    /// Column ids are the slugs of the names, made unique with `-2`, `-3`, ….
    ///
    /// # Errors
    /// An empty or multi-line title or column name, or no columns at all.
    pub fn init(title: &str, columns: &[String]) -> Result<Self, CommandError> {
        one_line("board title", title)?;
        if columns.is_empty() {
            return Err(CommandError::Invalid(
                "board.init needs at least one column name in `columns`, e.g. [\"Todo\",\"Doing\",\"Done\"]".into(),
            ));
        }
        let skeleton = format!("---\nmarkban: {SUPPORTED_VERSION}\ntitle:\ncolumns:\n---\n");
        let mut file =
            BoardFile::parse(&skeleton).map_err(|e| CommandError::Invalid(e.to_string()))?;
        file.frontmatter
            .set("title", title)
            .map_err(|e| CommandError::Invalid(e.to_string()))?;
        let mut taken = BTreeSet::new();
        for name in columns {
            one_line("column name", name)?;
            let id = unique_id(&slug(name), None, |c| taken.contains(c))?;
            file.add_column(&id, name, None)?;
            taken.insert(id);
        }
        Ok(Self {
            board: file,
            cards: std::collections::BTreeMap::new(),
            ignored: Vec::new(),
            taken: BTreeSet::new(),
        })
    }
}

// ---- helpers ----

fn one_line(label: &str, value: &str) -> Result<(), CommandError> {
    if value.trim().is_empty() {
        return Err(CommandError::Invalid(format!("{label} must not be empty")));
    }
    if value.contains(['\n', '\r']) {
        return Err(CommandError::Invalid(format!("{label} must be one line")));
    }
    Ok(())
}

/// `base`, or `base-2`, `base-3`, … (kept within `max_len`) until `taken` is false.
fn unique_id(
    base: &str,
    max_len: Option<usize>,
    taken: impl Fn(&str) -> bool,
) -> Result<String, CommandError> {
    if !taken(base) {
        return Ok(base.to_string());
    }
    (2_usize..)
        .map(|n| {
            let suffix = format!("-{n}");
            let keep = max_len.map_or(base.len(), |m| m.saturating_sub(suffix.len()));
            let stem = base.get(..keep.min(base.len())).unwrap_or(base);
            format!("{}{suffix}", stem.trim_end_matches('-'))
        })
        .find(|candidate| !taken(candidate))
        .ok_or_else(|| CommandError::Invalid("no free id".into()))
}

/// `(id, name)` of every usable column, in board order.
fn columns(board: &Board) -> Vec<(String, String)> {
    board
        .board
        .columns()
        .into_iter()
        .filter_map(|c| {
            let id = c.id.filter(|id| is_valid_id(id))?;
            Some((id, c.name.unwrap_or_default()))
        })
        .collect()
}

fn resolve_column(board: &Board, wanted: &str) -> Result<String, CommandError> {
    let columns = columns(board);
    let lower = wanted.to_lowercase();
    let by_id = columns.iter().find(|(id, _)| id.to_lowercase() == lower);
    let found = by_id.or_else(|| {
        columns
            .iter()
            .find(|(_, name)| name.to_lowercase() == lower)
    });
    match found {
        Some((id, _)) => Ok(id.clone()),
        None => Err(CommandError::UnknownColumn {
            name: flat(wanted),
            valid: if columns.is_empty() {
                "(none; add one with column.add)".to_string()
            } else {
                columns
                    .iter()
                    .map(|(id, name)| format!("{id} ({name})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        }),
    }
}

fn card_column(card: &CardFile) -> Option<String> {
    card.get("column")?.ok().filter(|v| !v.is_empty())
}

fn card_order(card: &CardFile) -> Option<Decimal> {
    Decimal::parse(&card.get("order")?.ok()?)
}

/// Active cards of `column` except `exclude`, in spec sort order, with their valid order.
/// Archived cards are invisible: they are not neighbours, not top or end anchors.
fn column_cards(
    board: &Board,
    column: &str,
    exclude: Option<&str>,
) -> Vec<(String, Option<Decimal>)> {
    let mut cards: Vec<(String, Option<Decimal>)> = board
        .cards
        .iter()
        .filter(|(id, card)| {
            Some(id.as_str()) != exclude
                && card_column(card).as_deref() == Some(column)
                && !card.is_archived()
        })
        .map(|(id, card)| (id.clone(), card_order(card)))
        .collect();
    cards.sort_by(|a, b| match (&a.1, &b.1) {
        (Some(x), Some(y)) => x.cmp(y).then_with(|| a.0.cmp(&b.0)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.0.cmp(&b.0),
    });
    cards
}

/// The order for `subject` at `pos` in `column`, `None` for "no `order` key": the end of a
/// column that has cards without a valid `order`, which sort after every ordered card.
/// Never touches other cards.
fn allocate(
    board: &Board,
    column: &str,
    subject: &str,
    pos: &Position,
) -> Result<Option<Decimal>, CommandError> {
    let cards = column_cards(board, column, Some(subject));
    let mut ordered = cards.iter().filter_map(|c| c.1.as_ref());
    let anchor = match pos {
        Position::End if cards.iter().any(|c| c.1.is_none()) => return Ok(None),
        Position::End => {
            return Ok(Some(order::end(subject, ordered.next_back())?));
        }
        Position::Top => return Ok(Some(order::top(subject, ordered.next())?)),
        Position::Before(a) | Position::After(a) => a,
    };
    if anchor == subject {
        return Err(CommandError::AnchorIsSelf(anchor.clone()));
    }
    let Some(at) = cards.iter().position(|c| &c.0 == anchor) else {
        return Err(match board.cards.get(anchor) {
            None => CommandError::UnknownCard(flat(anchor)),
            Some(card) if card.is_archived() => CommandError::AnchorArchived(anchor.clone()),
            Some(card) => CommandError::AnchorElsewhere {
                anchor: anchor.clone(),
                column: card_column(card).unwrap_or_default(),
                wanted: column.to_string(),
            },
        });
    };
    let anchor_order = cards
        .get(at)
        .and_then(|c| c.1.as_ref())
        .ok_or_else(|| CommandError::AnchorUnordered(anchor.clone()))?;
    let neighbour =
        |index: Option<usize>| index.and_then(|i| cards.get(i)).and_then(|c| c.1.as_ref());
    Ok(Some(match pos {
        Position::Before(_) => match neighbour(at.checked_sub(1)) {
            Some(prev) => order::between(subject, prev, anchor_order)?,
            None => order::top(subject, Some(anchor_order))?,
        },
        _ => match neighbour(at.checked_add(1)) {
            Some(next) => order::between(subject, anchor_order, next)?,
            None => order::end(subject, Some(anchor_order))?,
        },
    }))
}

fn broken(card: &mut CardFile) -> Result<&mut crate::frontmatter::Frontmatter, CommandError> {
    let id = card.id.clone();
    card.frontmatter
        .as_mut()
        .ok_or(CommandError::CardBroken { id })
}

fn set(card: &mut CardFile, key: &str, value: &str) -> Result<(), CommandError> {
    broken(card)?
        .set(key, value)
        .map_err(|e| CommandError::Invalid(e.to_string()))
}

/// Writes `order`, or removes the key when there is none.
fn set_order(card: &mut CardFile, order: Option<&Decimal>) -> Result<(), CommandError> {
    match order {
        Some(order) => set(card, "order", &order.to_string()),
        None => {
            broken(card)?.remove("order");
            Ok(())
        }
    }
}

/// `text` with every line break turned into `eol`, without trailing line breaks.
fn with_eol(text: &str, eol: &str) -> String {
    text.trim_end_matches(['\n', '\r'])
        .replace("\r\n", "\n")
        .replace('\n', eol)
}

fn body_of(text: &str, eol: &str) -> String {
    let text = with_eol(text, eol);
    if text.is_empty() {
        String::new()
    } else {
        format!("{eol}{text}{eol}")
    }
}

// ---- cards ----

fn card_add(
    board: &mut Board,
    title: &str,
    column: Option<&str>,
    description: Option<&str>,
    id: Option<&str>,
    pos: Option<Position>,
) -> Result<String, CommandError> {
    one_line("title", title)?;
    let column = match column {
        Some(c) => resolve_column(board, c)?,
        None => columns(board)
            .into_iter()
            .next()
            .map(|(id, _)| id)
            .ok_or_else(|| {
                CommandError::Invalid("board has no columns; add one with column.add".into())
            })?,
    };
    let id = match id {
        Some(id) if !is_valid_id(id) || id.len() > MAX_ID_LEN => {
            return Err(CommandError::Invalid(format!(
                "'{}' is not a valid id; use lowercase letters, digits and single '-', at most {MAX_ID_LEN} characters",
                flat(id)
            )));
        }
        Some(id) if board.id_taken(id) => {
            return Err(CommandError::CardExists(id.to_string()));
        }
        Some(id) => id.to_string(),
        None => unique_id(&slug(title), Some(MAX_ID_LEN), |c| board.id_taken(c))?,
    };
    let order = allocate(board, &column, &id, &pos.unwrap_or(Position::End))?;
    let mut card = CardFile::parse(&id, "---\n---\n");
    set(&mut card, "title", title)?;
    set(&mut card, "column", &column)?;
    set_order(&mut card, order.as_ref())?;
    card.body = body_of(description.unwrap_or(""), "\n");
    board.cards.insert(id.clone(), card);
    Ok(id)
}

fn card_move(
    board: &mut Board,
    id: &str,
    column: Option<&str>,
    pos: Option<Position>,
) -> Result<(), CommandError> {
    let card = board
        .cards
        .get(id)
        .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
    if card.frontmatter.is_none() {
        return Err(CommandError::CardBroken { id: id.to_string() });
    }
    if card.is_archived() {
        return Err(CommandError::CardArchived(flat(id)));
    }
    let current = card_column(card);
    if column.is_none() && pos.is_none() {
        return Err(CommandError::Invalid(
            "nothing to do: give `column` and/or one of `top`, `before`, `after`".into(),
        ));
    }
    let target = match column {
        Some(c) => resolve_column(board, c)?,
        None => current.clone().ok_or_else(|| {
            CommandError::Invalid(format!("card '{id}' has no valid column; pass `column`"))
        })?,
    };
    let order = allocate(board, &target, id, &pos.unwrap_or(Position::End))?;
    let card = board
        .cards
        .get_mut(id)
        .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
    if current.as_deref() != Some(target.as_str()) {
        set(card, "column", &target)?;
    }
    set_order(card, order.as_ref())
}

/// `card.archive`: sets `archived: true`; the card keeps its column, order and everything else.
fn card_archive(board: &mut Board, id: &str) -> Result<(), CommandError> {
    let card = board
        .cards
        .get(id)
        .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
    if card.frontmatter.is_none() {
        return Err(CommandError::CardBroken { id: id.to_string() });
    }
    if card.is_archived() {
        return Err(CommandError::Invalid(format!(
            "card '{id}' is already archived"
        )));
    }
    let card = board
        .cards
        .get_mut(id)
        .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
    set(card, "archived", "true")
}

/// `card.unarchive`: removes the `archived` key line. Column and order are untouched, so the
/// card returns exactly to its old place.
fn card_unarchive(board: &mut Board, id: &str) -> Result<(), CommandError> {
    let card = board
        .cards
        .get_mut(id)
        .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
    if card.frontmatter.is_none() {
        return Err(CommandError::CardBroken { id: id.to_string() });
    }
    if !card.is_archived() {
        return Err(CommandError::Invalid(format!(
            "card '{id}' is not archived"
        )));
    }
    broken(card)?.remove("archived");
    Ok(())
}

fn card_edit(
    board: &mut Board,
    id: &str,
    title: Option<&str>,
    description: Option<&str>,
    append: Option<&str>,
    if_unchanged: Option<&str>,
) -> Result<(), CommandError> {
    if title.is_none() && description.is_none() && append.is_none() {
        return Err(CommandError::Invalid(
            "nothing to change: give `title`, `description` and/or `append`".into(),
        ));
    }
    if let Some(title) = title {
        one_line("title", title)?;
    }
    if let Some(expected) = if_unchanged
        && (expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(CommandError::Invalid(
            "`if_unchanged` must be the 64 hex digit sha256 of the card, as `show --json` prints it"
                .into(),
        ));
    }
    let card = board
        .cards
        .get_mut(id)
        .ok_or_else(|| CommandError::UnknownCard(flat(id)))?;
    if let Some(expected) = if_unchanged
        && !crate::sha256_hex(&card.to_bytes()).eq_ignore_ascii_case(expected)
    {
        return Err(CommandError::CardChanged { id: id.to_string() });
    }
    let eol = broken(card)?.dominant_eol();
    if let Some(title) = title {
        set(card, "title", title)?;
    }
    if let Some(description) = description {
        card.body = body_of(description, eol);
    }
    if let Some(line) = append {
        let line = with_eol(line, eol);
        if card.body.trim().is_empty() {
            card.body = format!("{eol}{line}{eol}");
        } else {
            if !card.body.ends_with('\n') {
                card.body.push_str(eol);
            }
            card.body.push_str(&line);
            card.body.push_str(eol);
        }
    }
    Ok(())
}

// ---- columns ----

fn column_add(
    board: &mut Board,
    name: &str,
    id: Option<&str>,
    before: Option<&str>,
) -> Result<String, CommandError> {
    one_line("column name", name)?;
    let before = before.map(|b| resolve_column(board, b)).transpose()?;
    let existing: BTreeSet<String> = board
        .board
        .columns()
        .into_iter()
        .filter_map(|c| c.id)
        .collect();
    let id = match id {
        Some(id) if !is_valid_id(id) => {
            return Err(CommandError::Invalid(format!(
                "'{}' is not a valid column id; use lowercase letters, digits and single '-'",
                flat(id)
            )));
        }
        Some(id) => id.to_string(),
        None => unique_id(&slug(name), None, |c| existing.contains(c))?,
    };
    board.board.add_column(&id, name, before.as_deref())?;
    Ok(id)
}

fn column_remove(
    board: &mut Board,
    column: &str,
    move_to: Option<&str>,
) -> Result<String, CommandError> {
    let column = resolve_column(board, column)?;
    // Every card of the column, archived included (they count for the refusal and move along).
    // Active cards come first in their sort order (so they keep their relative order at the
    // target), then the archived ones by id.
    let mut members: Vec<String> = column_cards(board, &column, None)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    members.extend(
        board
            .cards
            .iter()
            .filter(|(_, card)| {
                card.is_archived() && card_column(card).as_deref() == Some(column.as_str())
            })
            .map(|(id, _)| id.clone()),
    );
    if !members.is_empty() {
        let archived = members
            .iter()
            .filter(|id| board.cards.get(*id).is_some_and(CardFile::is_archived))
            .count();
        let Some(target) = move_to else {
            return Err(CommandError::ColumnInUse {
                column,
                count: members.len(),
                archived,
            });
        };
        let target = resolve_column(board, target)?;
        if target == column {
            return Err(CommandError::Invalid(format!(
                "`move_to` cannot be the column being removed itself ('{column}'); pick another column"
            )));
        }
        for id in members {
            let is_archived = board.cards.get(&id).is_some_and(CardFile::is_archived);
            if is_archived {
                // `card_move` refuses archived cards; they only change their `column` key and
                // keep their order, so unarchiving puts them back in place.
                let card = board
                    .cards
                    .get_mut(&id)
                    .ok_or_else(|| CommandError::UnknownCard(flat(&id)))?;
                set(card, "column", &target)?;
            } else {
                card_move(board, &id, Some(&target), Some(Position::End))?;
            }
        }
    } else if let Some(target) = move_to {
        resolve_column(board, target)?;
    }
    board.board.remove_column(&column)?;
    Ok(column)
}
