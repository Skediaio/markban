//! Loading a board directory from disk (read only).

use crate::board::Board;
use crate::check::check;
use crate::diagnostic::{Code, Diagnostic};
use std::path::{Path, PathBuf};
use thiserror::Error;

/// The directory cannot be read at all (a broken board is not an error: it yields diagnostics).
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("'{0}' is not a directory; pass the board directory that contains board.md")]
    NotADirectory(PathBuf),
    #[error("cannot read '{path}': {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> LoadError + '_ {
    move |source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Reads `board.md` and `cards/*.md` of `dir`, in sorted order, and checks the result.
///
/// Returns the board (`None` when `board.md` is missing or unusable) and all diagnostics from
/// parsing and [`check`]. One bad card never stops the rest from loading.
///
/// # Errors
/// [`LoadError`] when `dir` is not a directory or a file cannot be read.
pub fn load(dir: &Path) -> Result<(Option<Board>, Vec<Diagnostic>), LoadError> {
    if !dir.is_dir() {
        return Err(LoadError::NotADirectory(dir.to_path_buf()));
    }
    let board_path = dir.join("board.md");
    if !board_path.is_file() {
        let missing = Diagnostic::new(
            Code::E001,
            "board.md",
            None,
            "board.md is missing; run in the board directory or create board.md",
        );
        return Ok((None, vec![missing]));
    }
    let board_md = std::fs::read(&board_path).map_err(io(&board_path))?;

    let mut cards = Vec::new();
    let mut ignored = Vec::new();
    let cards_dir = dir.join("cards");
    if cards_dir.is_dir() {
        let mut names = Vec::new();
        for entry in std::fs::read_dir(&cards_dir).map_err(io(&cards_dir))? {
            let entry = entry.map_err(io(&cards_dir))?;
            names.push((
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            ));
        }
        names.sort();
        for (name, path) in names {
            if name.ends_with(".md") && path.is_file() {
                cards.push((name, std::fs::read(&path).map_err(io(&path))?));
            } else {
                ignored.push(name);
            }
        }
    }

    let (board, mut diagnostics) = Board::parse(&board_md, cards, ignored);
    if let Some(board) = &board {
        diagnostics.extend(check(board));
    }
    diagnostics.sort_by(|a, b| {
        (&a.file, a.line, a.code, &a.message).cmp(&(&b.file, b.line, b.code, &b.message))
    });
    Ok((board, diagnostics))
}
