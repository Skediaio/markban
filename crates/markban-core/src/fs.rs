//! Loading a board directory from disk, and applying changes to it safely.

use crate::board::Board;
use crate::check::check;
use crate::command::{Command, CommandError, Outcome, execute_batch};
use crate::diagnostic::{Code, Diagnostic};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::TryLockError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
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

/// What the `cards/` directory held.
#[derive(Debug, Default)]
struct CardsRead {
    /// `(file name, bytes)` of every regular `*.md` file.
    cards: Vec<(String, Vec<u8>)>,
    /// Other names (directories, non-`.md` files).
    ignored: Vec<String>,
    /// Symbolic links, which are never followed.
    symlinks: Vec<String>,
}

/// Reads the `(name, path)` entries of `cards/`. Entries that vanish while reading are skipped.
fn read_cards(mut entries: Vec<(String, PathBuf)>) -> Result<CardsRead, LoadError> {
    entries.sort();
    let mut read = CardsRead::default();
    for (name, path) in entries {
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(&path)(e)),
        };
        if meta.file_type().is_symlink() {
            read.symlinks.push(name);
        } else if name.ends_with(".md") && meta.is_file() {
            match std::fs::read(&path) {
                Ok(bytes) => read.cards.push((name, bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(&path)(e)),
            }
        } else {
            read.ignored.push(name);
        }
    }
    Ok(read)
}

fn symlink_diagnostic(file: impl Into<String>) -> Diagnostic {
    Diagnostic::new(
        Code::W001,
        file,
        None,
        "symbolic link ignored; markban never follows links, replace it with a regular file",
    )
}

/// Reads `board.md` and `cards/*.md` of `dir`, in sorted order, and checks the result.
///
/// Returns the board (`None` when `board.md` is missing or unusable) and all diagnostics from
/// parsing and [`check`]. One bad card never stops the rest from loading. Symbolic links are
/// never followed: one in `cards/` is reported as W001 and `board.md` being one counts as missing.
/// The tool-owned `.markban.lock` file is not part of the board and is never reported.
///
/// # Errors
/// [`LoadError`] when `dir` is not a directory or a file cannot be read.
pub fn load(dir: &Path) -> Result<(Option<Board>, Vec<Diagnostic>), LoadError> {
    if !dir.is_dir() {
        return Err(LoadError::NotADirectory(dir.to_path_buf()));
    }
    let board_path = dir.join("board.md");
    let board_meta = std::fs::symlink_metadata(&board_path).ok();
    if !board_meta.as_ref().is_some_and(std::fs::Metadata::is_file) {
        let mut diagnostics = Vec::new();
        let message = if board_meta
            .as_ref()
            .is_some_and(|m| m.file_type().is_symlink())
        {
            diagnostics.push(symlink_diagnostic("board.md"));
            "board.md is a symbolic link and is not read; replace it with a regular file"
        } else {
            "board.md is missing; run in the board directory or create board.md"
        };
        diagnostics.push(Diagnostic::new(Code::E001, "board.md", None, message));
        return Ok((None, diagnostics));
    }
    let board_md = std::fs::read(&board_path).map_err(io(&board_path))?;

    let mut read = CardsRead::default();
    let cards_dir = dir.join("cards");
    if cards_dir.is_dir() {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&cards_dir).map_err(io(&cards_dir))? {
            let entry = entry.map_err(io(&cards_dir))?;
            entries.push((
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            ));
        }
        read = read_cards(entries)?;
    }

    let (board, mut diagnostics) = Board::parse(&board_md, read.cards, read.ignored);
    diagnostics.extend(
        read.symlinks
            .iter()
            .map(|name| symlink_diagnostic(format!("cards/{name}"))),
    );
    let board = board.map(|mut board| {
        board.taken.extend(read.symlinks);
        board
    });
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
    #[error(
        "'{path}' already exists on disk but is not a card the board could read; remove or rename it"
    )]
    Exists { path: String },
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
    #[error("board is locked by another markban process ({path}); try again shortly")]
    Locked { path: String },
    #[error(
        "applied to {}; failed at {failed}: {reason}; run `markban check` and inspect",
        .written.join(", ")
    )]
    PartiallyApplied {
        written: Vec<String>,
        failed: String,
        reason: String,
    },
}

/// Attempts `run` makes before giving up on concurrent writers.
const MAX_ATTEMPTS: usize = 3;

type Files = BTreeMap<String, Vec<u8>>;

type Hook<'a> = &'a mut dyn FnMut(usize, &str) -> std::io::Result<()>;

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

/// Flushes the directory entry of `path`'s parent. Best effort: not every platform can do it.
fn sync_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::File::open(parent).and_then(|dir| dir.sync_all());
    }
}

fn create_file(dir: &Path, rel: &str, bytes: &[u8]) -> Result<(), ApplyError> {
    let target = dir.join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(rel))?;
    }
    let temp = write_temp(&target, rel, bytes)?;
    // A hard link fails when the target exists, so this never overwrites.
    let result = match std::fs::hard_link(&temp, &target) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(conflict(rel)),
        // Filesystem without hard links: create-new write in place, synced; a partial file
        // is removed again.
        Err(_) => match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(conflict(rel)),
            Err(e) => Err(io_err(rel)(e)),
            Ok(mut file) => file
                .write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| {
                    let _ = std::fs::remove_file(&target);
                    io_err(rel)(e)
                }),
        },
    };
    let _ = std::fs::remove_file(&temp);
    if result.is_ok() {
        sync_parent(&target);
    }
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
    } else {
        sync_parent(&target);
    }
    result
}

fn delete_file(dir: &Path, rel: &str, expected: &[u8]) -> Result<(), ApplyError> {
    let target = dir.join(rel);
    expect_unchanged(&target, rel, expected)?;
    std::fs::remove_file(&target).map_err(io_err(rel))?;
    sync_parent(&target);
    Ok(())
}

fn file_map(board: &Board) -> Files {
    board.files().into_iter().collect()
}

/// One write of an apply, in the order it is performed.
enum Step<'a> {
    Create {
        path: &'a str,
        bytes: &'a [u8],
    },
    Replace {
        path: &'a str,
        expected: &'a [u8],
        bytes: &'a [u8],
        /// Whether `expected` is what was read (and so can be checked up front); false for the
        /// second write of a file that is written twice.
        precheck: bool,
    },
    Delete {
        path: &'a str,
        expected: &'a [u8],
    },
}

impl Step<'_> {
    fn path(&self) -> &str {
        match self {
            Step::Create { path, .. } | Step::Replace { path, .. } | Step::Delete { path, .. } => {
                path
            }
        }
    }
}

/// Where `board.md` goes among the writes.
enum BoardAt {
    /// Before the cards: the board only gains columns, or keeps the same ones.
    First,
    /// After the cards: the board loses columns, so its cards must have moved away first.
    Last,
    /// Gains and loses columns: first this union of old and new columns, then the cards, then
    /// the final `board.md`.
    Split(Vec<u8>),
}

fn column_ids(board: &Board) -> BTreeSet<String> {
    board
        .board
        .columns()
        .into_iter()
        .filter_map(|c| c.id)
        .collect()
}

fn board_placement(before: &Board, after: &Board) -> BoardAt {
    let (old, new) = (column_ids(before), column_ids(after));
    let removes = old.difference(&new).next().is_some();
    let added: Vec<_> = after
        .board
        .columns()
        .into_iter()
        .filter(|c| c.id.as_ref().is_some_and(|id| !old.contains(id)))
        .collect();
    match (added.is_empty(), removes) {
        (_, false) => BoardAt::First,
        (true, true) => BoardAt::Last,
        (false, true) => {
            let mut union = before.board.clone();
            for column in added {
                let Some(id) = column.id else { continue };
                let name = column.name.unwrap_or_else(|| id.clone());
                if union.add_column(&id, &name, None).is_err() {
                    return BoardAt::Last;
                }
            }
            BoardAt::Split(union.to_bytes())
        }
    }
}

fn plan<'a>(before: &'a Files, after: &'a Files, board_at: &'a BoardAt) -> Vec<Step<'a>> {
    const BOARD: &str = "board.md";
    let mut cards = Vec::new();
    for (path, new) in after {
        if path != BOARD && !before.contains_key(path) {
            cards.push(Step::Create { path, bytes: new });
        }
    }
    for (path, old) in before {
        if let Some(new) = after.get(path).filter(|new| *new != old)
            && path != BOARD
        {
            cards.push(Step::Replace {
                path,
                expected: old,
                bytes: new,
                precheck: true,
            });
        }
    }
    for (path, old) in before {
        if path != BOARD && !after.contains_key(path) {
            cards.push(Step::Delete {
                path,
                expected: old,
            });
        }
    }
    let board_step = |expected: &'a [u8], bytes: &'a [u8], precheck| Step::Replace {
        path: BOARD,
        expected,
        bytes,
        precheck,
    };
    let create_board = |bytes: &'a [u8]| Step::Create { path: BOARD, bytes };
    let (old, new) = (before.get(BOARD), after.get(BOARD));
    let mut steps = Vec::new();
    let board_steps = match (old, new) {
        (Some(old), Some(new)) if old != new => Some((old, new)),
        (None, Some(new)) => {
            steps.push(create_board(new));
            None
        }
        _ => None,
    };
    if let (Some((old, new)), BoardAt::Split(union)) = (board_steps, board_at) {
        steps.push(board_step(old, union, true));
        steps.extend(cards);
        steps.push(board_step(union, new, false));
        return steps;
    }
    match (board_steps, board_at) {
        (Some((old, new)), BoardAt::Last) => {
            steps.extend(cards);
            steps.push(board_step(old, new, true));
        }
        (Some((old, new)), _) => {
            steps.push(board_step(old, new, true));
            steps.extend(cards);
        }
        (None, _) => steps.extend(cards),
    }
    steps
}

/// A failed apply and the files written before it failed.
struct Failure {
    written: Vec<String>,
    error: ApplyError,
}

impl ApplyError {
    fn path(&self) -> &str {
        match self {
            Self::Conflict { path } | Self::Io { path, .. } | Self::Exists { path } => path,
        }
    }

    fn reason(&self) -> String {
        match self {
            Self::Conflict { .. } => "changed on disk since it was read".into(),
            Self::Io { source, .. } => source.to_string(),
            Self::Exists { .. } => "already exists".into(),
        }
    }
}

fn apply_steps(
    dir: &Path,
    taken: &BTreeSet<String>,
    steps: &[Step<'_>],
    hook: Hook<'_>,
) -> Result<Vec<String>, Failure> {
    let fail = |written: &[String], error| Failure {
        written: written.to_vec(),
        error,
    };
    // Check everything first, so a conflict normally writes nothing at all.
    for step in steps {
        let checked = match step {
            Step::Create { path, .. } => {
                let name = path.strip_prefix("cards/").unwrap_or(path);
                if taken.contains(name) {
                    Err(ApplyError::Exists {
                        path: (*path).to_string(),
                    })
                } else if dir.join(path).symlink_metadata().is_ok() {
                    Err(conflict(path))
                } else {
                    Ok(())
                }
            }
            Step::Replace {
                path,
                expected,
                precheck: true,
                ..
            }
            | Step::Delete { path, expected } => expect_unchanged(&dir.join(path), path, expected),
            Step::Replace { .. } => Ok(()),
        };
        checked.map_err(|e| fail(&[], e))?;
    }

    let mut written: Vec<String> = Vec::new();
    for (count, step) in steps.iter().enumerate() {
        let path = step.path();
        hook(count, path)
            .map_err(io_err(path))
            .map_err(|e| fail(&written, e))?;
        match step {
            Step::Create { bytes, .. } => create_file(dir, path, bytes),
            Step::Replace {
                expected, bytes, ..
            } => replace_file(dir, path, expected, bytes),
            Step::Delete { expected, .. } => delete_file(dir, path, expected),
        }
        .map_err(|e| fail(&written, e))?;
        if !written.iter().any(|w| w == path) {
            written.push(path.to_string());
        }
    }
    Ok(written)
}

fn apply_boards(
    dir: &Path,
    before: &Board,
    after: &Board,
    hook: Hook<'_>,
) -> Result<Vec<String>, Failure> {
    let (old, new) = (file_map(before), file_map(after));
    let board_at = board_placement(before, after);
    let steps = plan(&old, &new, &board_at);
    apply_steps(dir, &before.taken, &steps, hook)
}

/// Writes the difference between `before` (what was read) and `after` to `dir`, file by file,
/// and returns the changed paths in the order they were written.
///
/// - Every file is written to a temporary file in its own directory and renamed into place, so
///   a reader never sees a half-written file; the directory is synced after each change.
/// - Compare-and-swap: before a file is replaced or deleted, its bytes on disk are compared with
///   `before`; any difference (or a vanished file) is [`ApplyError::Conflict`] and that file is
///   not written. A new card file is created with create-new semantics and conflicts when the
///   path exists; when the path was already occupied at load time (an entry the board could
///   not read, see [`Board::taken`]) it is [`ApplyError::Exists`]. All files are checked before
///   the first write, so a conflict normally leaves the directory untouched; only a conflict
///   arising between that check and a later write can leave earlier files applied.
/// - Order, so that the board on disk never has a card whose column is missing (E007) at any
///   point: if `board.md` only gains columns (or keeps them) it is written first; if it only
///   loses columns it is written last, after the cards moved away; if it does both, a
///   `board.md` with the union of old and new columns is written first and the final one last.
///   The cards in between are written as creations, then modifications, then deletions. A
///   leftover `.<name>.tmp-*` file (crash during write) shows up as warning W001 in `cards/`.
///
/// # Errors
/// [`ApplyError::Conflict`], [`ApplyError::Exists`] or [`ApplyError::Io`]. The error does not
/// say which files were already written; [`run`] reports that as [`RunError::PartiallyApplied`].
pub fn apply(dir: &Path, before: &Board, after: &Board) -> Result<Vec<String>, ApplyError> {
    apply_with_hook(dir, before, after, &mut |_, _| Ok(()))
}

/// [`apply`] with a fault-injection hook called before each write with the number of writes
/// done so far and the path about to be written; an `Err` aborts as an I/O failure at that
/// path. For tests.
///
/// # Errors
/// See [`apply`].
#[doc(hidden)]
pub fn apply_with_hook(
    dir: &Path,
    before: &Board,
    after: &Board,
    hook: Hook<'_>,
) -> Result<Vec<String>, ApplyError> {
    apply_boards(dir, before, after, hook).map_err(|f| f.error)
}

/// Writes a board that does not exist on disk yet (all files are created, none may exist).
/// Creates `dir` when needed. An existing `cards/` directory must be empty.
///
/// # Errors
/// [`ApplyError::Conflict`] when a file exists, [`ApplyError::Exists`] when `cards/` is not
/// empty, or [`ApplyError::Io`].
pub fn apply_init(dir: &Path, after: &Board) -> Result<Vec<String>, ApplyError> {
    apply_init_inner(dir, after, &mut |_, _| Ok(())).map_err(|f| f.error)
}

fn apply_init_inner(dir: &Path, after: &Board, hook: Hook<'_>) -> Result<Vec<String>, Failure> {
    let fail = |error| Failure {
        written: Vec::new(),
        error,
    };
    std::fs::create_dir_all(dir)
        .map_err(io_err("."))
        .map_err(fail)?;
    let cards = dir.join("cards");
    if let Ok(mut entries) = std::fs::read_dir(&cards)
        && entries.next().is_some()
    {
        return Err(fail(ApplyError::Exists {
            path: "cards".into(),
        }));
    }
    let new = file_map(after);
    let none = Files::new();
    let steps = plan(&none, &new, &BoardAt::First);
    apply_steps(dir, &BTreeSet::new(), &steps, hook)
}

/// The tool-owned lock file in a board directory. Never part of the board.
const LOCK_FILE: &str = ".markban.lock";
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(50);

/// Takes the exclusive advisory lock of the board in `dir`, polling for up to [`LOCK_TIMEOUT`].
/// `None` when `dir` is no directory (the run will report why) or the filesystem has no locks.
fn lock_board(dir: &Path, cmds: &[Command]) -> Result<Option<std::fs::File>, RunError> {
    if !dir.exists() && matches!(cmds.first(), Some(Command::BoardInit { .. })) {
        std::fs::create_dir_all(dir).map_err(|source| {
            RunError::Apply(ApplyError::Io {
                path: ".".into(),
                source,
            })
        })?;
    }
    if !dir.is_dir() {
        return Ok(None);
    }
    let path = dir.join(LOCK_FILE);
    let io_error = |source| {
        RunError::Apply(ApplyError::Io {
            path: LOCK_FILE.into(),
            source,
        })
    };
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(io_error)?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(TryLockError::Error(e)) if e.kind() == std::io::ErrorKind::Unsupported => {
                return Ok(None);
            }
            Err(TryLockError::Error(e)) => return Err(io_error(e)),
            Err(TryLockError::WouldBlock) if start.elapsed() >= LOCK_TIMEOUT => {
                return Err(RunError::Locked {
                    path: path.display().to_string(),
                });
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
        }
    }
}

/// Loads `dir`, executes `cmds` all or nothing and applies the result. The whole cycle runs under
/// an exclusive advisory lock (`.markban.lock` in `dir`) so two markban processes never
/// interleave; waiting for it times out after about 5 seconds ([`RunError::Locked`]). Writers
/// that ignore the lock can still cause an [`ApplyError::Conflict`]: when nothing was written yet
/// the cycle is repeated, at most 3 times in total; once a file was written, a failure is
/// [`RunError::PartiallyApplied`] and never retried. When `dir` has no `board.md` and the first
/// command is `board.init`, the board is created.
///
/// A board that already has errors is allowed; the commands may not add new ones.
///
/// # Errors
/// [`RunError`].
pub fn run(dir: &Path, cmds: &[Command]) -> Result<Vec<Outcome>, RunError> {
    run_with_hook(dir, cmds, &mut |_| {})
}

/// [`run_with_hook`] plus a write hook, see [`apply_with_hook`]. For tests.
///
/// # Errors
/// [`RunError`].
#[doc(hidden)]
pub fn run_with_hooks(
    dir: &Path,
    cmds: &[Command],
    between: &mut dyn FnMut(usize),
    write: Hook<'_>,
) -> Result<Vec<Outcome>, RunError> {
    if cmds.is_empty() {
        return Ok(Vec::new());
    }
    let _lock = lock_board(dir, cmds)?;
    let mut last = String::new();
    for attempt in 0..MAX_ATTEMPTS {
        match attempt_run(dir, cmds, &mut |()| between(attempt), &mut *write) {
            Err(RunError::Apply(ApplyError::Conflict { path })) => last = path,
            other => return other,
        }
    }
    Err(RunError::Contended {
        attempts: MAX_ATTEMPTS,
        path: last,
    })
}

/// [`run`] with a hook called after execute and before apply with the attempt number (from 0).
/// For tests that simulate a concurrent writer.
#[doc(hidden)]
pub fn run_with_hook(
    dir: &Path,
    cmds: &[Command],
    between: &mut dyn FnMut(usize),
) -> Result<Vec<Outcome>, RunError> {
    run_with_hooks(dir, cmds, between, &mut |_, _| Ok(()))
}

/// A failure with nothing written is a plain apply error (and may be retried); one after
/// some files were written is [`RunError::PartiallyApplied`].
fn finish(applied: Result<Vec<String>, Failure>) -> Result<(), RunError> {
    match applied {
        Ok(_) => Ok(()),
        Err(Failure { written, error }) if written.is_empty() => Err(error.into()),
        Err(Failure { written, error }) => Err(RunError::PartiallyApplied {
            failed: error.path().to_string(),
            reason: error.reason(),
            written,
        }),
    }
}

fn attempt_run(
    dir: &Path,
    cmds: &[Command],
    between: &mut dyn FnMut(()),
    write: Hook<'_>,
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
        finish(apply_init_inner(dir, &board, write))?;
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
    finish(apply_boards(dir, &before, &after, write))?;
    Ok(outcomes)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn entries_that_vanished_after_the_listing_are_skipped() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("kept.md"), "x\n").unwrap();
        let entries = vec![
            ("gone.md".to_string(), dir.path().join("gone.md")),
            ("kept.md".to_string(), dir.path().join("kept.md")),
        ];
        let read = read_cards(entries).unwrap();
        assert_eq!(read.cards, [("kept.md".to_string(), b"x\n".to_vec())]);
        assert!(read.ignored.is_empty() && read.symlinks.is_empty());
    }
}
