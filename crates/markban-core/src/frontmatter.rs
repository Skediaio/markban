//! Lossless frontmatter: the markban subset of YAML (spec section 2).
//!
//! The block keeps its opening line, every inner line with its own line ending, and
//! its closing line. Lines the subset does not understand are never touched.

use std::fmt;
use thiserror::Error;

/// The frontmatter block is absent or broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum FrontmatterError {
    #[error("frontmatter is missing; the file must start with a line `---`")]
    Missing,
    #[error("frontmatter is not terminated; add a closing line `---`")]
    Unterminated,
}

/// A value cannot be read or written.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValueError {
    #[error("quoted value is not closed; add a closing `\"`")]
    Unterminated,
    #[error("quoted value has text after the closing quote; escape inner quotes as `\\\"`")]
    TrailingText,
    #[error("unknown escape `\\{0}` in quoted value; only `\\\"` and `\\\\` are allowed")]
    BadEscape(char),
    #[error("a value cannot contain a line break")]
    Newline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    open: String,
    lines: Vec<String>,
    close: String,
}

/// A line without its line ending.
pub(crate) fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// The line ending of a line (`\n`, `\r\n`, or empty).
pub(crate) fn eol_of(line: &str) -> &str {
    line.get(strip_eol(line).len()..).unwrap_or("")
}

/// Splits `key: rest` where the key matches `[a-z][a-z0-9_]*`.
pub(crate) fn key_line(text: &str) -> Option<(&str, &str)> {
    let (key, rest) = text.split_once(':')?;
    let mut chars = key.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
    let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    (first_ok && rest_ok).then_some((key, rest))
}

fn trim_blanks(s: &str) -> &str {
    s.trim_matches([' ', '\t'])
}

/// Reads a plain or double-quoted value (the text after `key:`).
pub(crate) fn parse_value(raw: &str) -> Result<String, ValueError> {
    let text = trim_blanks(raw);
    let Some(inner) = text.strip_prefix('"') else {
        return Ok(text.to_string());
    };
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                return if chars.next().is_none() {
                    Ok(out)
                } else {
                    Err(ValueError::TrailingText)
                };
            }
            '\\' => match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => return Err(ValueError::BadEscape(other)),
                None => return Err(ValueError::Unterminated),
            },
            other => out.push(other),
        }
    }
    Err(ValueError::Unterminated)
}

/// Formats a value, quoting it when the spec rule says so.
pub(crate) fn format_value(value: &str) -> String {
    const SPECIAL: &str = "\"'[]{}&*!|>%@`#,?:-";
    let needs_quote = value.is_empty()
        || value.starts_with([' ', '\t'])
        || value.ends_with([' ', '\t'])
        || value.starts_with(|c| SPECIAL.contains(c))
        || value.contains(": ")
        || value.contains(" #");
    if needs_quote {
        let mut out = String::from('"');
        for c in value.chars() {
            if c == '"' || c == '\\' {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    } else {
        value.to_string()
    }
}

/// Rewrites one `key: value` line whose key starts after `indent` bytes: the indent, key and
/// spacing after the colon are kept, only the value (and trailing blanks) change.
pub(crate) fn rewrite_line(line: &str, indent: usize, key: &str, value: &str) -> String {
    let text = strip_eol(line);
    let head = text.get(..indent).unwrap_or("");
    let after = text
        .get(indent..)
        .and_then(|rest| rest.strip_prefix(key))
        .and_then(|rest| rest.strip_prefix(':'))
        .unwrap_or("");
    let spacing_len = after.len() - after.trim_start_matches([' ', '\t']).len();
    let spacing = match after.get(..spacing_len) {
        Some(s) if !s.is_empty() => s,
        _ => " ",
    };
    format!(
        "{head}{key}:{spacing}{}{}",
        format_value(value),
        eol_of(line)
    )
}

impl Frontmatter {
    /// Splits a file into its frontmatter and the rest (the body).
    ///
    /// # Errors
    /// [`FrontmatterError::Missing`] when the first line is not `---`,
    /// [`FrontmatterError::Unterminated`] when there is no closing `---`.
    pub fn split(text: &str) -> Result<(Self, &str), FrontmatterError> {
        let mut iter = text.split_inclusive('\n');
        let open = iter.next().ok_or(FrontmatterError::Missing)?;
        if strip_eol(open) != "---" {
            return Err(FrontmatterError::Missing);
        }
        let mut consumed = open.len();
        let mut lines = Vec::new();
        for line in iter {
            consumed += line.len();
            if strip_eol(line) == "---" {
                let body = text.get(consumed..).unwrap_or("");
                return Ok((
                    Self {
                        open: open.to_string(),
                        lines,
                        close: line.to_string(),
                    },
                    body,
                ));
            }
            lines.push(line.to_string());
        }
        Err(FrontmatterError::Unterminated)
    }

    /// The inner lines, each with its line ending.
    #[must_use]
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub(crate) fn lines_mut(&mut self) -> &mut Vec<String> {
        &mut self.lines
    }

    fn find(&self, key: &str) -> Option<usize> {
        self.lines
            .iter()
            .position(|l| matches!(key_line(strip_eol(l)), Some((k, _)) if k == key))
    }

    /// The value of the first top-level `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Result<String, ValueError>> {
        let line = self.lines.get(self.find(key)?)?;
        let (_, rest) = key_line(strip_eol(line))?;
        Some(parse_value(rest))
    }

    /// The 1-based file line of `key` (the opening `---` is line 1).
    #[must_use]
    pub fn line_of(&self, key: &str) -> Option<u32> {
        self.find(key).map(line_number)
    }

    /// The line ending most lines use; `\n` on a tie.
    pub(crate) fn dominant_eol(&self) -> &'static str {
        let all = std::iter::once(&self.open)
            .chain(&self.lines)
            .chain(std::iter::once(&self.close));
        let (mut crlf, mut lf) = (0usize, 0usize);
        for line in all {
            match eol_of(line) {
                "\r\n" => crlf += 1,
                "\n" => lf += 1,
                _ => {}
            }
        }
        if crlf > lf { "\r\n" } else { "\n" }
    }

    /// Index after the last top-level key line and its indented continuation lines.
    fn insert_index(&self) -> usize {
        let Some(last) = self
            .lines
            .iter()
            .rposition(|l| key_line(strip_eol(l)).is_some())
        else {
            return self.lines.len();
        };
        let mut index = last + 1;
        while self
            .lines
            .get(index)
            .is_some_and(|l| strip_eol(l).starts_with([' ', '\t']))
        {
            index += 1;
        }
        index
    }

    /// Sets `key`. An existing line gets only its value rewritten; an absent key is inserted
    /// after the last key, with the block's dominant line ending.
    ///
    /// # Errors
    /// [`ValueError::Newline`] when `value` contains a line break.
    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ValueError> {
        if value.contains(['\n', '\r']) {
            return Err(ValueError::Newline);
        }
        if let Some(index) = self.find(key) {
            if let Some(slot) = self.lines.get_mut(index) {
                *slot = rewrite_line(slot, 0, key, value);
            }
        } else {
            let line = format!("{key}: {}{}", format_value(value), self.dominant_eol());
            let index = self.insert_index().min(self.lines.len());
            self.lines.insert(index, line);
        }
        Ok(())
    }

    /// Removes the line of `key`. Returns whether it existed.
    pub fn remove(&mut self, key: &str) -> bool {
        match self.find(key) {
            Some(index) => {
                self.lines.remove(index);
                true
            }
            None => false,
        }
    }
}

/// File line number (1-based) of inner line `index`.
pub(crate) fn line_number(index: usize) -> u32 {
    u32::try_from(index.saturating_add(2)).unwrap_or(u32::MAX)
}

impl fmt::Display for Frontmatter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.open)?;
        for line in &self.lines {
            f.write_str(line)?;
        }
        f.write_str(&self.close)
    }
}
