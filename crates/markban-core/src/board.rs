//! The lossless board model: `board.md`, card files, and the board built from them.

use crate::diagnostic::{Code, Diagnostic};
use crate::frontmatter::{
    Frontmatter, FrontmatterError, ValueError, key_line, line_number, parse_value, rewrite_line,
    strip_eol,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use thiserror::Error;

const COLUMN_ITEM_PREFIX: &str = "  - ";
const ITEM_KEY_INDENT: usize = 4;

/// A column mutation was refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BoardError {
    #[error("board.md has no `columns:` block; add a line `columns:` to its frontmatter")]
    NoColumnsBlock,
    #[error("'{0}' is not a column of this board")]
    UnknownColumn(String),
    #[error("column '{0}' already exists")]
    DuplicateColumn(String),
    #[error(transparent)]
    Value(#[from] ValueError),
}

/// One item of the `columns:` block, as found in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Column {
    pub id: Option<String>,
    pub name: Option<String>,
    /// Value errors (key, error) found in `id` or `name`.
    pub errors: Vec<(&'static str, ValueError)>,
    /// Index of the item's first line in the frontmatter lines.
    pub start: usize,
    /// Index after the item's last line.
    pub end: usize,
}

impl Column {
    /// 1-based file line of the item.
    #[must_use]
    pub fn line(&self) -> u32 {
        line_number(self.start)
    }
}

/// `board.md`: frontmatter plus the description (everything after the closing `---`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoardFile {
    pub frontmatter: Frontmatter,
    pub description: String,
}

impl BoardFile {
    /// # Errors
    /// The frontmatter is missing or unterminated.
    pub fn parse(text: &str) -> Result<Self, FrontmatterError> {
        let (frontmatter, body) = Frontmatter::split(text)?;
        Ok(Self {
            frontmatter,
            description: body.to_string(),
        })
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }

    /// `(header index, block end)`: the `columns:` line and the index after the block's last
    /// non-blank line.
    fn block(&self) -> Option<(usize, usize)> {
        let lines = self.frontmatter.lines();
        let header = lines.iter().position(
            |l| matches!(key_line(strip_eol(l)), Some(("columns", rest)) if rest.trim().is_empty()),
        )?;
        let mut end = header + 1;
        let mut index = end;
        while let Some(line) = lines.get(index) {
            let text = strip_eol(line);
            if text.trim().is_empty() {
                index += 1;
            } else if text.starts_with([' ', '\t']) {
                index += 1;
                end = index;
            } else {
                break;
            }
        }
        Some((header, end))
    }

    /// Whether `board.md` has a `columns:` block.
    #[must_use]
    pub fn has_columns_block(&self) -> bool {
        self.block().is_some()
    }

    /// The column items in file order.
    #[must_use]
    pub fn columns(&self) -> Vec<Column> {
        let Some((header, end)) = self.block() else {
            return Vec::new();
        };
        let lines = self.frontmatter.lines();
        let starts: Vec<usize> = (header + 1..end)
            .filter(|&i| {
                lines
                    .get(i)
                    .is_some_and(|l| strip_eol(l).starts_with(COLUMN_ITEM_PREFIX))
            })
            .collect();
        starts
            .iter()
            .enumerate()
            .map(|(n, &start)| {
                let item_end = starts.get(n + 1).copied().unwrap_or(end);
                parse_item(lines, start, item_end)
            })
            .collect()
    }

    fn find_column(&self, id: &str) -> Result<Column, BoardError> {
        self.columns()
            .into_iter()
            .find(|c| c.id.as_deref() == Some(id))
            .ok_or_else(|| BoardError::UnknownColumn(id.to_string()))
    }

    /// Line index where an item goes: before column `before`, or at the end of the block.
    fn insertion_point(&self, before: Option<&str>) -> Result<usize, BoardError> {
        let (_, end) = self.block().ok_or(BoardError::NoColumnsBlock)?;
        match before {
            Some(id) => Ok(self.find_column(id)?.start),
            None => Ok(end),
        }
    }

    /// Changes the `name` line of a column (adds one when the item has none).
    ///
    /// # Errors
    /// Unknown column, or a name with a line break.
    pub fn rename_column(&mut self, id: &str, name: &str) -> Result<(), BoardError> {
        if name.contains(['\n', '\r']) {
            return Err(ValueError::Newline.into());
        }
        let column = self.find_column(id)?;
        let eol = self.frontmatter.dominant_eol();
        let lines = self.frontmatter.lines_mut();
        let existing = (column.start..column.end).find(|&i| {
            lines.get(i).is_some_and(|l| {
                matches!(item_key(strip_eol(l), i == column.start), Some(("name", _)))
            })
        });
        match existing {
            Some(i) => {
                if let Some(slot) = lines.get_mut(i) {
                    *slot = rewrite_line(slot, ITEM_KEY_INDENT, "name", name);
                }
            }
            None => {
                let last = (column.start..column.end)
                    .rev()
                    .find(|&i| {
                        lines
                            .get(i)
                            .is_some_and(|l| !strip_eol(l).trim().is_empty())
                    })
                    .unwrap_or(column.start);
                let line = rewrite_line(&format!("    name:{eol}"), ITEM_KEY_INDENT, "name", name);
                lines.insert((last + 1).min(lines.len()), line);
            }
        }
        Ok(())
    }

    /// Adds a column item before column `before`, or at the end.
    ///
    /// # Errors
    /// No columns block, duplicate id, unknown anchor, or a value with a line break.
    pub fn add_column(
        &mut self,
        id: &str,
        name: &str,
        before: Option<&str>,
    ) -> Result<(), BoardError> {
        if id.contains(['\n', '\r']) || name.contains(['\n', '\r']) {
            return Err(ValueError::Newline.into());
        }
        if self.columns().iter().any(|c| c.id.as_deref() == Some(id)) {
            return Err(BoardError::DuplicateColumn(id.to_string()));
        }
        let at = self.insertion_point(before)?;
        let eol = self.frontmatter.dominant_eol();
        let id_line = rewrite_line(&format!("  - id:{eol}"), ITEM_KEY_INDENT, "id", id);
        let name_line = rewrite_line(&format!("    name:{eol}"), ITEM_KEY_INDENT, "name", name);
        let lines = self.frontmatter.lines_mut();
        let at = at.min(lines.len());
        lines.splice(at..at, [id_line, name_line]);
        Ok(())
    }

    /// Moves a column item (all its lines) before column `before`, or to the end.
    ///
    /// # Errors
    /// Unknown column or anchor.
    pub fn move_column(&mut self, id: &str, before: Option<&str>) -> Result<(), BoardError> {
        let column = self.find_column(id)?;
        if before == Some(id) {
            return Ok(());
        }
        // Validate the anchor before touching anything.
        self.insertion_point(before)?;
        let lines = self.frontmatter.lines_mut();
        let end = column.end.min(lines.len());
        let item: Vec<String> = lines.drain(column.start.min(end)..end).collect();
        let at = self.insertion_point(before)?;
        let lines = self.frontmatter.lines_mut();
        let at = at.min(lines.len());
        lines.splice(at..at, item);
        Ok(())
    }

    /// Removes a column item (all its lines).
    ///
    /// # Errors
    /// Unknown column.
    pub fn remove_column(&mut self, id: &str) -> Result<(), BoardError> {
        let column = self.find_column(id)?;
        let lines = self.frontmatter.lines_mut();
        let end = column.end.min(lines.len());
        lines.drain(column.start.min(end)..end);
        Ok(())
    }
}

impl fmt::Display for BoardFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.frontmatter, self.description)
    }
}

/// `key: rest` of an item line: the first line is `  - key: ...`, later ones `    key: ...`.
fn item_key(text: &str, first: bool) -> Option<(&str, &str)> {
    let rest = if first {
        text.strip_prefix(COLUMN_ITEM_PREFIX)?
    } else {
        let rest = text.strip_prefix("    ")?;
        if rest.starts_with(' ') {
            return None;
        }
        rest
    };
    key_line(rest)
}

fn parse_item(lines: &[String], start: usize, end: usize) -> Column {
    let mut column = Column {
        id: None,
        name: None,
        errors: Vec::new(),
        start,
        end,
    };
    for i in start..end {
        let Some(line) = lines.get(i) else { break };
        match item_key(strip_eol(line), i == start) {
            Some(("id", rest)) if column.id.is_none() => match parse_value(rest) {
                Ok(v) => column.id = Some(v),
                Err(e) => column.errors.push(("id", e)),
            },
            Some(("name", rest)) if column.name.is_none() => match parse_value(rest) {
                Ok(v) => column.name = Some(v),
                Err(e) => column.errors.push(("name", e)),
            },
            _ => {}
        }
    }
    column
}

/// A card file. A file whose frontmatter is missing or broken keeps the whole text as `body`
/// and records why in `error`, so it still round-trips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardFile {
    pub id: String,
    pub frontmatter: Option<Frontmatter>,
    pub error: Option<FrontmatterError>,
    pub body: String,
}

impl CardFile {
    #[must_use]
    pub fn parse(id: &str, text: &str) -> Self {
        match Frontmatter::split(text) {
            Ok((frontmatter, body)) => Self {
                id: id.to_string(),
                frontmatter: Some(frontmatter),
                error: None,
                body: body.to_string(),
            },
            Err(error) => Self {
                id: id.to_string(),
                frontmatter: None,
                error: Some(error),
                body: text.to_string(),
            },
        }
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_string().into_bytes()
    }

    /// Value of `key`; `None` when absent or the card has no frontmatter.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Result<String, ValueError>> {
        self.frontmatter.as_ref()?.get(key)
    }

    /// Whether the card is archived: an `archived` key whose value is exactly `true` (plain or
    /// double-quoted; an empty value counts as missing). Any other value counts as active
    /// (warning W003).
    #[must_use]
    pub fn is_archived(&self) -> bool {
        matches!(self.get("archived"), Some(Ok(v)) if v == "true")
    }

    /// The card's file name.
    #[must_use]
    pub fn file_name(&self) -> String {
        format!("{}.md", self.id)
    }
}

impl fmt::Display for CardFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(frontmatter) = &self.frontmatter {
            write!(f, "{frontmatter}")?;
        }
        f.write_str(&self.body)
    }
}

/// A whole board held in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub board: BoardFile,
    /// Cards by id (the file name without `.md`), sorted.
    pub cards: BTreeMap<String, CardFile>,
    /// Names in `cards/` that are not `<id>.md` (reported as W001).
    pub ignored: Vec<String>,
    /// Names in `cards/` that no card in `cards` accounts for (ignored entries, files that are
    /// not UTF-8, symbolic links): an id whose `<id>.md` is among them is not free.
    pub taken: BTreeSet<String>,
}

fn utf8_diagnostic(file: &str, bytes: &[u8]) -> Option<Diagnostic> {
    let error = std::str::from_utf8(bytes).err()?;
    let valid = bytes.get(..error.valid_up_to()).unwrap_or(&[]);
    let line = valid.iter().filter(|&&b| b == b'\n').count() + 1;
    Some(Diagnostic::new(
        Code::E009,
        file,
        u32::try_from(line).ok(),
        format!(
            "file is not valid UTF-8 (first bad byte at offset {}); re-save it as UTF-8",
            error.valid_up_to()
        ),
    ))
}

impl Board {
    /// Whether a card with this id exists, or its file name is occupied on disk by something
    /// the board could not load. Compared ignoring ASCII case, so ids stay distinct on
    /// case-insensitive filesystems.
    #[must_use]
    pub fn id_taken(&self, id: &str) -> bool {
        let file = format!("{id}.md");
        self.cards.keys().any(|c| c.eq_ignore_ascii_case(id))
            || self.taken.iter().any(|t| t.eq_ignore_ascii_case(&file))
    }

    /// Builds a board from in-memory files. Tolerant: whatever parses is kept, problems come
    /// back as diagnostics (E001 for `board.md`, E009 for files that are not UTF-8).
    ///
    /// `cards` are `(file name, bytes)`; names ending in `.md` become cards, others are
    /// ignored (W001). `ignored` lists extra names the caller already skipped, such as
    /// directories. Semantic problems are found by [`crate::check`].
    #[must_use]
    pub fn parse(
        board_md: &[u8],
        cards: Vec<(String, Vec<u8>)>,
        ignored: Vec<String>,
    ) -> (Option<Self>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let mut card_files = BTreeMap::new();
        let mut ignored = ignored;
        let mut taken = BTreeSet::new();
        for (name, bytes) in cards {
            let Some(id) = name.strip_suffix(".md") else {
                ignored.push(name);
                continue;
            };
            let file = format!("cards/{name}");
            match std::str::from_utf8(&bytes) {
                Ok(text) => {
                    card_files.insert(id.to_string(), CardFile::parse(id, text));
                }
                Err(_) => {
                    taken.insert(name.clone());
                    diagnostics.extend(utf8_diagnostic(&file, &bytes));
                }
            }
        }
        ignored.sort();
        ignored.dedup();
        taken.extend(ignored.iter().cloned());

        let board = match std::str::from_utf8(board_md) {
            Err(_) => {
                diagnostics.extend(utf8_diagnostic("board.md", board_md));
                None
            }
            Ok(text) => match BoardFile::parse(text) {
                Ok(board) => Some(board),
                Err(error) => {
                    diagnostics.push(Diagnostic::new(
                        Code::E001,
                        "board.md",
                        Some(1),
                        error.to_string(),
                    ));
                    None
                }
            },
        };
        let board = board.map(|board| Self {
            board,
            cards: card_files,
            ignored,
            taken,
        });
        (board, diagnostics)
    }

    /// Every file of the board as `(relative path, bytes)`, `board.md` first, cards sorted by id.
    /// Each is byte-identical to the input it was parsed from.
    #[must_use]
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        let mut files = vec![("board.md".to_string(), self.board.to_bytes())];
        files.extend(
            self.cards
                .values()
                .map(|c| (format!("cards/{}", c.file_name()), c.to_bytes())),
        );
        files
    }
}
