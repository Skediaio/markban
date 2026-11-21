//! Loading a board directory from disk, and applying changes to it safely.

use crate::board::Board;
use crate::check::check;
use crate::command::{Command, CommandError, Outcome, execute_batch};
use crate::diagnostic::{Code, Diagnostic};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Applying a change to disk failed.
#[derive(Debug, Error)]
pub enum ApplyError {
    #[error(
        "'{path}' changed on disk since it was read (concurrent edit); reload the board and retry"
    )]
    Conflict { path: String },
    #[error("cannot write '{path}': {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
}

/// Running commands against a directory failed.
#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    Load(#[from] LoadError),
    #[error("no usable board in '{dir}': {reason}")]
    NoBoard { dir: PathBuf, reason: String },
    #[error("command {}: {source}", index + 1)]
    Command { index: usize, source: CommandError },
    #[error(
        "'{path}' kept changing on disk during {attempts} attempts; try again when other writers are done"
    )]
    Contended { attempts: usize, path: String },
    #[error(transparent)]
    Apply(#[from] ApplyError),
}

/// Attempts `run` makes before giving up on concurrent writers.
const MAX_ATTEMPTS: usize = 3;

type Files = BTreeMap<String, Vec<u8>>;

fn io_err(path: &str) -> impl FnOnce(std::io::Error) -> ApplyError + '_ {
    move |source| ApplyError::Io {
        path: path.to_string(),
        source,
    }
}

fn conflict(path: &str) -> ApplyError {
    ApplyError::Conflict {
        path: path.to_string(),
    }
}

/// Bytes on disk, `None` when the file does not exist.
fn read_current(target: &Path, rel: &str) -> Result<Option<Vec<u8>>, ApplyError> {
    match std::fs::read(target) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err(rel)(e)),
    }
}

/// Compare-and-swap check: the file on disk must still be exactly `expected`.
fn expect_unchanged(target: &Path, rel: &str, expected: &[u8]) -> Result<(), ApplyError> {
    match read_current(target, rel)? {
        Some(bytes) if bytes == expected => Ok(()),
        _ => Err(conflict(rel)),
    }
}

/// Writes `bytes` to a fresh temporary file next to `target` and syncs it.
fn write_temp(target: &Path, rel: &str, bytes: &[u8]) -> Result<PathBuf, ApplyError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(
        ".{name}.tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    match written {
        Ok(()) => Ok(temp),
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            Err(io_err(rel)(e))
        }
    }
}

fn create_file(dir: &Path, rel: &str, bytes: &[u8]) -> Result<(), ApplyError> {
    let target = dir.join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(rel))?;
    }
    let temp = write_temp(&target, rel, bytes)?;
    // A hard link fails when the target exists, so this never overwrites.
    let linked = std::fs::hard_link(&temp, &target);
    let result = match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(conflict(rel)),
        // Filesystem without hard links: create-new write in place.
        Err(_) => std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .and_then(|mut f| f.write_all(bytes))
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    conflict(rel)
                } else {
                    io_err(rel)(e)
                }
            }),
    };
    let _ = std::fs::remove_file(&temp);
    result
}

fn replace_file(dir: &Path, rel: &str, expected: &[u8], bytes: &[u8]) -> Result<(), ApplyError> {
    let target = dir.join(rel);
    let temp = write_temp(&target, rel, bytes)?;
    if let Ok(meta) = std::fs::metadata(&target) {
        let _ = std::fs::set_permissions(&temp, meta.permissions());
    }
    let result = expect_unchanged(&target, rel, expected)
        .and_then(|()| std::fs::rename(&temp, &target).map_err(io_err(rel)));
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn delete_file(dir: &Path, rel: &str, expected: &[u8]) -> Result<(), ApplyError> {
    let target = dir.join(rel);
    expect_unchanged(&target, rel, expected)?;
    std::fs::remove_file(&target).map_err(io_err(rel))
}

fn file_map(board: &Board) -> Files {
    board.files().into_iter().collect()
}

fn apply_files(dir: &Path, before: &Files, after: &Files) -> Result<Vec<String>, ApplyError> {
    let created: Vec<&String> = after.keys().filter(|p| !before.contains_key(*p)).collect();
    let modified: Vec<(&String, &Vec<u8>, &Vec<u8>)> = before
        .iter()
        .filter_map(|(p, old)| {
            after
                .get(p)
                .filter(|new| *new != old)
                .map(|new| (p, old, new))
        })
        .collect();
    let deleted: Vec<(&String, &Vec<u8>)> = before
        .iter()
        .filter(|(p, _)| !after.contains_key(*p))
        .collect();

    // Check everything first, so a conflict normally writes nothing at all.
    for path in &created {
        if dir.join(path).symlink_metadata().is_ok() {
            return Err(conflict(path));
        }
    }
    for (path, old, _) in &modified {
        expect_unchanged(&dir.join(path), path, old)?;
    }
    for (path, old) in &deleted {
        expect_unchanged(&dir.join(path), path, old)?;
    }

    let mut changed = Vec::new();
    for path in created {
        let bytes = after.get(path).map_or(&[][..], Vec::as_slice);
        create_file(dir, path, bytes)?;
        changed.push(path.clone());
    }
    for (path, old, new) in modified {
        replace_file(dir, path, old, new)?;
        changed.push(path.clone());
    }
    for (path, old) in deleted {
        delete_file(dir, path, old)?;
        changed.push(path.clone());
    }
    Ok(changed)
}

/// Writes the difference between `before` (what was read) and `after` to `dir`, file by file,
/// and returns the changed paths in the order they were written.
///
/// - Every file is written to a temporary file in its own directory and renamed into place, so
///   a reader never sees a half-written file.
/// - Compare-and-swap: before a file is replaced or deleted, its bytes on disk are compared with
///   `before`; any difference (or a vanished file) is [`ApplyError::Conflict`] and that file is
///   not written. A new card file is created with create-new semantics and conflicts when the
///   path exists. All files are checked before the first write, so a conflict normally leaves
///   the directory untouched; only a conflict arising between that check and a later write can
///   leave earlier files applied.
/// - Order: creations, then modifications, then deletions. A crash in between leaves at worst an
///   orphan card (a card nothing else needs) or a stale-but-valid board, never a card pointing at
///   a missing column, because columns live in `board.md` and are removed only after their cards
///   moved. A leftover `.<name>.tmp-*` file (crash during write) shows up as warning W001.
///
/// # Errors
/// [`ApplyError::Conflict`] or [`ApplyError::Io`].
pub fn apply(dir: &Path, before: &Board, after: &Board) -> Result<Vec<String>, ApplyError> {
    apply_files(dir, &file_map(before), &file_map(after))
}

/// Writes a board that does not exist on disk yet (all files are created, none may exist).
/// Creates `dir` when needed.
///
/// # Errors
/// [`ApplyError::Conflict`] when a file exists, or [`ApplyError::Io`].
pub fn apply_init(dir: &Path, after: &Board) -> Result<Vec<String>, ApplyError> {
    std::fs::create_dir_all(dir).map_err(io_err("."))?;
    apply_files(dir, &Files::new(), &file_map(after))
}

/// Loads `dir`, executes `cmds` all or nothing and applies the result; on [`ApplyError::Conflict`]
/// the whole load, execute, apply cycle is repeated, at most 3 times in total. When `dir` has no
/// `board.md` and the first command is `board.init`, the board is created.
///
/// A board that already has errors is allowed; the commands may not add new ones.
///
/// # Errors
/// [`RunError`].
pub fn run(dir: &Path, cmds: &[Command]) -> Result<Vec<Outcome>, RunError> {
    run_with_hook(dir, cmds, &mut |_| {})
}

/// [`run`] with a hook called after execute and before apply with the attempt number (from 0).
/// For tests that simulate a concurrent writer.
#[doc(hidden)]
pub fn run_with_hook(
    dir: &Path,
    cmds: &[Command],
    between: &mut dyn FnMut(usize),
) -> Result<Vec<Outcome>, RunError> {
    if cmds.is_empty() {
        return Ok(Vec::new());
    }
    let mut last = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        match attempt_run(dir, cmds, &mut |_| between(attempt)) {
            Err(RunError::Apply(ApplyError::Conflict { path })) => last = path,
            other => return other,
        }
    }
    Err(RunError::Contended {
        attempts: MAX_ATTEMPTS,
        path: last,
    })
}

fn attempt_run(
    dir: &Path,
    cmds: &[Command],
    between: &mut dyn FnMut(()),
) -> Result<Vec<Outcome>, RunError> {
    let no_board = |reason: String| RunError::NoBoard {
        dir: dir.to_path_buf(),
        reason,
    };
    let command_error = |offset: usize| {
        move |(index, source)| RunError::Command {
            index: index + offset,
            source,
        }
    };
    if !dir.join("board.md").exists() {
        let Some(Command::BoardInit { title, columns }) = cmds.first() else {
            return Err(no_board(
                "board.md is missing; start with a board.init command".into(),
            ));
        };
        let mut board =
            Board::init(title, columns).map_err(|source| RunError::Command { index: 0, source })?;
        let mut outcomes = vec![Outcome { id: title.clone() }];
        outcomes.extend(
            execute_batch(&mut board, cmds.get(1..).unwrap_or(&[])).map_err(command_error(1))?,
        );
        between(());
        apply_init(dir, &board)?;
        return Ok(outcomes);
    }
    let (board, diagnostics) = load(dir)?;
    let Some(before) = board else {
        let reason = diagnostics.first().map_or_else(
            || "board.md cannot be read".to_string(),
            ToString::to_string,
        );
        return Err(no_board(reason));
    };
    let mut after = before.clone();
    let outcomes = execute_batch(&mut after, cmds).map_err(command_error(0))?;
    between(());
    apply(dir, &before, &after)?;
    Ok(outcomes)
}
