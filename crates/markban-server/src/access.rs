//! The data directory and `access.toml`: users, their token hashes and the boards they may use.
//!
//! Layout of a data directory:
//!
//! ```text
//! access.toml          users (see below)
//! access.toml.lock     held while an admin command edits access.toml
//! boards/<board-id>/   one ordinary markban board each
//! log/<board-id>.jsonl append-only command log (and one rotated `.jsonl.1`)
//! removed/<id>-<time>/ boards taken out by `markban server board remove`
//! ```
//!
//! `access.toml`:
//!
//! ```toml
//! [users.alice]
//! token_sha256 = "<64 hex digits>"
//! boards = ["*"]
//! admin = true
//! ```
//!
//! Tokens are 32 random bytes, shown once and stored only as their SHA-256. The admin functions
//! here edit the file atomically (temporary file and rename, with a compare-and-swap against the
//! bytes read) under an exclusive lock on `access.toml.lock`, and keep comments and unknown keys.
//! Directories are created private (0700) and files 0600 on Unix.

use markban_core::board::BoardFile;
use markban_core::command::Command;
use markban_core::is_valid_id;
use sha2::{Digest, Sha256};
use std::fs::TryLockError;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use toml_edit::{Array, DocumentMut, Item, Table, value};

/// The file with users and their access.
pub const ACCESS_FILE: &str = "access.toml";
/// The boards directory inside the data directory.
pub const BOARDS_DIR: &str = "boards";
/// The log directory inside the data directory.
pub const LOG_DIR: &str = "log";
/// Where removed boards go inside the data directory.
pub const REMOVED_DIR: &str = "removed";
/// The lock file held while `access.toml` is edited.
pub const ACCESS_LOCK: &str = "access.toml.lock";
const LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const LOCK_POLL: Duration = Duration::from_millis(50);
/// "All boards" in a user's `boards` list.
pub const ALL_BOARDS: &str = "*";
const TOKEN_PREFIX: &str = "mb_";
/// Longest user name or board id (the same limit as card ids).
pub const MAX_ID_LEN: usize = 64;

/// One user of the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub admin: bool,
    boards: Vec<String>,
    token_sha256: [u8; 32],
}

impl User {
    /// Whether the user may see and change board `id`.
    #[must_use]
    pub fn may_use(&self, id: &str) -> bool {
        self.boards.iter().any(|b| b == ALL_BOARDS || b == id)
    }
}

/// The parsed `access.toml`.
#[derive(Debug, Clone, Default)]
pub struct Access {
    users: Vec<User>,
}

impl Access {
    /// Reads and validates `<data>/access.toml`. Any malformed entry fails the whole file, so a
    /// mistake never silently widens or narrows access.
    ///
    /// # Errors
    /// One line naming the file and what is wrong.
    pub fn load(data: &Path) -> Result<Self, String> {
        let (_, doc) = read_doc(data)?;
        parse_users(&doc).map(|users| Self { users })
    }

    /// The user whose token hashes to a stored `token_sha256`. Every user's hash is compared in
    /// constant time, and all of them are compared, so timing tells nothing about the stored
    /// hashes.
    #[must_use]
    pub fn authenticate(&self, token: &str) -> Option<&User> {
        let presented: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut found = None;
        for user in &self.users {
            let matches: bool = user.token_sha256.ct_eq(&presented).into();
            if matches && found.is_none() {
                found = Some(user);
            }
        }
        found
    }
}

/// Lower-case hex SHA-256 of a token, as stored in `access.toml`.
#[must_use]
pub fn hash_token(token: &str) -> String {
    markban_core::sha256_hex(token.as_bytes())
}

fn access_path(data: &Path) -> PathBuf {
    data.join(ACCESS_FILE)
}

/// What identifies one version of `access.toml` cheaply: size, modification time and (on Unix)
/// the inode, which every admin edit changes because it renames a new file into place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    modified: Option<std::time::SystemTime>,
    inode: u64,
}

/// The [`Stamp`] of `<data>/access.toml`.
///
/// # Errors
/// When the file cannot be inspected.
pub fn access_stamp(data: &Path) -> Result<Stamp, String> {
    let meta = std::fs::metadata(access_path(data))
        .map_err(|e| format!("cannot read {ACCESS_FILE}: {e}"))?;
    #[cfg(unix)]
    let inode = std::os::unix::fs::MetadataExt::ino(&meta);
    #[cfg(not(unix))]
    let inode = 0;
    Ok(Stamp {
        len: meta.len(),
        modified: meta.modified().ok(),
        inode,
    })
}

/// Creates `path` and missing parents, private to the owner (0700 on Unix).
///
/// # Errors
/// The I/O failure.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

/// Options that create a file private to the owner (0600 on Unix).
#[must_use]
#[cfg_attr(not(unix), allow(unused_mut))]
pub fn private_file() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

/// Takes the exclusive lock on `<data>/access.toml.lock`, polling for up to 5 seconds. Held
/// until the returned file is dropped. A filesystem without locks is not locked.
fn lock_access(data: &Path) -> Result<std::fs::File, String> {
    let path = data.join(ACCESS_LOCK);
    let file = private_file()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| format!("cannot open {ACCESS_LOCK}: {e}"))?;
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::Error(e)) if e.kind() == std::io::ErrorKind::Unsupported => {
                return Ok(file);
            }
            Err(TryLockError::Error(e)) => return Err(format!("cannot lock {ACCESS_LOCK}: {e}")),
            Err(TryLockError::WouldBlock) if start.elapsed() >= LOCK_TIMEOUT => {
                return Err(format!(
                    "{ACCESS_FILE} is being edited by another markban admin command; try again shortly"
                ));
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(LOCK_POLL),
        }
    }
}

fn read_doc(data: &Path) -> Result<(Vec<u8>, DocumentMut), String> {
    let path = access_path(data);
    let bytes = std::fs::read(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            format!(
                "no {ACCESS_FILE} in '{}'; create the data directory with `markban server init <dir>`",
                data.display()
            )
        } else {
            format!("cannot read {ACCESS_FILE}: {e}")
        }
    })?;
    let text = String::from_utf8(bytes.clone())
        .map_err(|_| format!("{ACCESS_FILE} is not valid UTF-8"))?;
    let doc = text.parse::<DocumentMut>().map_err(|e| {
        format!(
            "{ACCESS_FILE} is not valid TOML: {}",
            one_line(&e.to_string())
        )
    })?;
    parse_users(&doc)?;
    Ok((bytes, doc))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_hash(hex: &str) -> Option<[u8; 32]> {
    let bytes = hex.as_bytes();
    if bytes.len() != 64 {
        return None;
    }
    let nibble = |b: u8| {
        char::from(b)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
    };
    let mut out = [0_u8; 32];
    for (slot, [high, low]) in out.iter_mut().zip(bytes.as_chunks::<2>().0) {
        *slot = (nibble(*high)? << 4) | nibble(*low)?;
    }
    Some(out)
}

fn parse_users(doc: &DocumentMut) -> Result<Vec<User>, String> {
    let Some(users) = doc.get("users") else {
        return Ok(Vec::new());
    };
    let users = users
        .as_table_like()
        .ok_or_else(|| format!("{ACCESS_FILE}: `users` must be a table of [users.<name>]"))?;
    let mut out = Vec::new();
    for (name, entry) in users.iter() {
        let bad = |what: &str| format!("{ACCESS_FILE}: user '{name}': {what}");
        if !valid_name(name) {
            return Err(bad(
                "invalid name; use lowercase letters, digits and single '-'",
            ));
        }
        let table = entry
            .as_table_like()
            .ok_or_else(|| bad("must be a table"))?;
        let token_sha256 = table
            .get("token_sha256")
            .and_then(Item::as_str)
            .and_then(decode_hash)
            .ok_or_else(|| bad("`token_sha256` must be 64 hex digits"))?;
        let boards = table
            .get("boards")
            .and_then(Item::as_array)
            .ok_or_else(|| bad("`boards` must be a list such as [\"*\"] or [\"board-id\"]"))?
            .iter()
            .map(|b| {
                b.as_str()
                    .filter(|b| *b == ALL_BOARDS || valid_name(b))
                    .map(str::to_string)
                    .ok_or_else(|| bad("`boards` holds a value that is not \"*\" or a board id"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let admin = match table.get("admin") {
            None => false,
            Some(item) => item
                .as_bool()
                .ok_or_else(|| bad("`admin` must be true or false"))?,
        };
        out.push(User {
            name: name.to_string(),
            admin,
            boards,
            token_sha256,
        });
    }
    Ok(out)
}

/// A user name or board id: `[a-z0-9]+(-[a-z0-9]+)*`, at most 64 characters.
#[must_use]
pub fn valid_name(name: &str) -> bool {
    name.len() <= MAX_ID_LEN && is_valid_id(name)
}

/// The directory of board `id`, when `id` is a valid board id and `<data>/boards/<id>` is a real
/// directory (a symbolic link is not a board).
#[must_use]
pub fn board_dir(data: &Path, id: &str) -> Option<PathBuf> {
    if !valid_name(id) {
        return None;
    }
    let dir = data.join(BOARDS_DIR).join(id);
    std::fs::symlink_metadata(&dir)
        .ok()
        .filter(std::fs::Metadata::is_dir)
        .map(|_| dir)
}

/// The log file of board `id` (which must be valid).
#[must_use]
pub fn log_path(data: &Path, id: &str) -> PathBuf {
    data.join(LOG_DIR).join(format!("{id}.jsonl"))
}

/// The rotated (previous) log file next to `log`: `<id>.jsonl.1`.
#[must_use]
pub fn rotated_log_path(log: &Path) -> PathBuf {
    let mut name = log.as_os_str().to_os_string();
    name.push(".1");
    PathBuf::from(name)
}

fn new_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| format!("cannot get random bytes: {e}"))?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("{TOKEN_PREFIX}{hex}"))
}

/// Writes `bytes` as `<data>/access.toml`: a private temporary file, synced, renamed into place,
/// unless the file no longer holds `expected` (another admin command ran meanwhile).
fn write_access(data: &Path, expected: Option<&[u8]>, bytes: &[u8]) -> Result<(), String> {
    let path = access_path(data);
    let temp = data.join(format!(".{ACCESS_FILE}.tmp-{}", std::process::id()));
    let written = private_file()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("cannot write {ACCESS_FILE}: {e}"));
    }
    let current = std::fs::read(&path).ok();
    if current.as_deref() != expected {
        let _ = std::fs::remove_file(&temp);
        return Err(format!(
            "{ACCESS_FILE} changed while it was being edited; run the command again"
        ));
    }
    std::fs::rename(&temp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        format!("cannot write {ACCESS_FILE}: {e}")
    })?;
    if let Ok(dir) = std::fs::File::open(data) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Reads the file, applies `edit` to the document and writes it back atomically, under the
/// admin lock.
fn edit_doc(
    data: &Path,
    edit: impl FnOnce(&mut DocumentMut) -> Result<(), String>,
) -> Result<(), String> {
    let _lock = lock_access(data)?;
    edit_doc_locked(data, edit)
}

/// [`edit_doc`] for a caller that already holds the admin lock.
fn edit_doc_locked(
    data: &Path,
    edit: impl FnOnce(&mut DocumentMut) -> Result<(), String>,
) -> Result<(), String> {
    let (bytes, mut doc) = read_doc(data)?;
    edit(&mut doc)?;
    parse_users(&doc)?;
    write_access(data, Some(&bytes), doc.to_string().as_bytes())
}

fn users_table(doc: &mut DocumentMut) -> Result<&mut Table, String> {
    if doc.get("users").is_none() {
        let mut table = Table::new();
        table.set_implicit(true);
        doc.insert("users", Item::Table(table));
    }
    doc.get_mut("users")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| format!("{ACCESS_FILE}: `users` must be a table of [users.<name>]"))
}

fn check_name(name: &str) -> Result<(), String> {
    if valid_name(name) {
        Ok(())
    } else {
        Err(format!(
            "'{}' is not a valid user name; use lowercase letters, digits and single '-', at most {MAX_ID_LEN} characters",
            name.escape_debug()
        ))
    }
}

/// Validates a board list: `*` or ids of boards that exist.
fn check_boards(data: &Path, boards: &[String]) -> Result<(), String> {
    if boards.is_empty() {
        return Err("give at least one board id, or * for all boards".into());
    }
    for board in boards {
        if board != ALL_BOARDS && board_dir(data, board).is_none() {
            return Err(format!(
                "no board '{}' in {BOARDS_DIR}/; create it with `markban server board add` or use * for all boards",
                board.escape_debug()
            ));
        }
    }
    Ok(())
}

fn boards_array(boards: &[String]) -> Item {
    value(boards.iter().map(String::as_str).collect::<Array>())
}

fn user_table(hash: &str, boards: &[String], admin: bool) -> Table {
    let mut table = Table::new();
    table.insert("token_sha256", value(hash));
    table.insert("boards", boards_array(boards));
    if admin {
        table.insert("admin", value(true));
    }
    table
}

/// Creates a data directory: `boards/`, `log/` and `access.toml` with one admin user that may use
/// every board. Returns that user's token, which is not stored anywhere.
///
/// # Errors
/// An invalid name, an existing `access.toml`, or an I/O failure.
pub fn init(data: &Path, admin: &str) -> Result<String, String> {
    check_name(admin)?;
    if access_path(data).exists() {
        return Err(format!(
            "'{}' already has an {ACCESS_FILE}; add users with `markban server user add`",
            data.display()
        ));
    }
    for sub in [BOARDS_DIR, LOG_DIR] {
        create_private_dir(&data.join(sub))
            .map_err(|e| format!("cannot create '{}': {e}", data.join(sub).display()))?;
    }
    let _lock = lock_access(data)?;
    if access_path(data).exists() {
        return Err(format!(
            "'{}' already has an {ACCESS_FILE}; add users with `markban server user add`",
            data.display()
        ));
    }
    let token = new_token()?;
    let mut doc: DocumentMut = "# markban server: users, their token hashes and boards.\n# Edit with `markban server user ...`; tokens are stored only as SHA-256.\n"
        .parse()
        .map_err(|e| format!("cannot build {ACCESS_FILE}: {e}"))?;
    users_table(&mut doc)?.insert(
        admin,
        Item::Table(user_table(
            &hash_token(&token),
            &[ALL_BOARDS.to_string()],
            true,
        )),
    );
    write_access(data, None, doc.to_string().as_bytes())?;
    Ok(token)
}

/// Adds a user with access to `boards` (`*` for all). Returns the new token, shown once.
///
/// # Errors
/// An invalid or existing name, an unknown board, or an I/O failure.
pub fn add_user(data: &Path, name: &str, boards: &[String], admin: bool) -> Result<String, String> {
    check_name(name)?;
    check_boards(data, boards)?;
    let token = new_token()?;
    let hash = hash_token(&token);
    edit_doc(data, |doc| {
        let users = users_table(doc)?;
        if users.contains_key(name) {
            return Err(format!(
                "user '{name}' already exists; remove it first or change its boards with `markban server user boards`"
            ));
        }
        users.insert(name, Item::Table(user_table(&hash, boards, admin)));
        Ok(())
    })?;
    Ok(token)
}

fn unknown_user(name: &str) -> String {
    format!(
        "no user '{}'; see the [users.<name>] tables in {ACCESS_FILE}",
        name.escape_debug()
    )
}

/// Removes a user; its token stops working at once.
///
/// # Errors
/// An unknown user or an I/O failure.
pub fn remove_user(data: &Path, name: &str) -> Result<(), String> {
    edit_doc(data, |doc| {
        users_table(doc)?
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| unknown_user(name))
    })
}

/// Replaces the boards a user may use.
///
/// # Errors
/// An unknown user or board, or an I/O failure.
pub fn set_boards(data: &Path, name: &str, boards: &[String]) -> Result<(), String> {
    check_boards(data, boards)?;
    edit_doc(data, |doc| {
        let user = users_table(doc)?
            .get_mut(name)
            .and_then(Item::as_table_like_mut)
            .ok_or_else(|| unknown_user(name))?;
        user.insert("boards", boards_array(boards));
        Ok(())
    })
}

/// Writes `token` and a newline to a new file at `path`, private to the owner (0600 on Unix).
/// An existing file is never overwritten.
///
/// # Errors
/// An existing file or an I/O failure; the message never contains the token.
pub fn write_token_file(path: &Path, token: &str) -> Result<(), String> {
    let mut file = private_file()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!(
                    "'{}' already exists; give a new file for the token",
                    path.display()
                )
            } else {
                format!("cannot create '{}': {e}", path.display())
            }
        })?;
    file.write_all(format!("{token}\n").as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| format!("cannot write '{}': {e}", path.display()))
}

/// Creates board `id` with `board.init`. Returns the board id.
///
/// # Errors
/// An invalid or existing id, an invalid title or columns, or an I/O failure.
pub fn add_board(data: &Path, id: &str, title: &str, columns: &[String]) -> Result<String, String> {
    if !valid_name(id) {
        return Err(format!(
            "'{}' is not a valid board id; use lowercase letters, digits and single '-', at most {MAX_ID_LEN} characters",
            id.escape_debug()
        ));
    }
    let boards = data.join(BOARDS_DIR);
    if !boards.is_dir() {
        return Err(format!(
            "'{}' is not a markban data directory (no {BOARDS_DIR}/); run `markban server init` first",
            data.display()
        ));
    }
    let dir = boards.join(id);
    if std::fs::symlink_metadata(&dir).is_ok() {
        return Err(format!("board '{id}' already exists"));
    }
    let init = Command::BoardInit {
        title: title.to_string(),
        columns: columns.to_vec(),
    };
    markban_core::fs::run(&dir, &[init]).map_err(|e| match e {
        markban_core::fs::RunError::Command { source, .. } => source.to_string(),
        other => other.to_string(),
    })?;
    Ok(id.to_string())
}

/// Removes board `id`: drops it from every user's `boards` and moves its directory (and log) to
/// `<data>/removed/<id>-<timestamp>/`. Nothing is deleted. Returns where the board went.
///
/// # Errors
/// An unknown board or an I/O failure.
pub fn remove_board(data: &Path, id: &str) -> Result<PathBuf, String> {
    if !valid_name(id) {
        return Err(format!(
            "'{}' is not a valid board id; list the boards with `markban server board list`",
            id.escape_debug()
        ));
    }
    let _lock = lock_access(data)?;
    let Some(dir) = board_dir(data, id) else {
        return Err(format!(
            "no board '{id}' in {BOARDS_DIR}/; list the boards with `markban server board list`"
        ));
    };
    // Grants go first: should the move fail, the board stays but nobody new reaches it by id.
    edit_doc_locked(data, |doc| {
        for (_, user) in users_table(doc)?.iter_mut() {
            let Some(user) = user.as_table_like_mut() else {
                continue;
            };
            let Some(boards) = user.get("boards").and_then(Item::as_array) else {
                continue;
            };
            let kept: Vec<String> = boards
                .iter()
                .filter_map(|b| b.as_str())
                .filter(|b| *b != id)
                .map(str::to_string)
                .collect();
            if kept.len() != boards.len() {
                user.insert("boards", boards_array(&kept));
            }
        }
        Ok(())
    })?;
    let removed = data.join(REMOVED_DIR);
    create_private_dir(&removed)
        .map_err(|e| format!("cannot create '{}': {e}", removed.display()))?;
    let target = free_removed_path(&removed, id)?;
    std::fs::rename(&dir, &target)
        .map_err(|e| format!("cannot move board '{id}' to {REMOVED_DIR}/: {e}"))?;
    let log = log_path(data, id);
    for (from, to) in [
        (log.clone(), target.join("log.jsonl")),
        (rotated_log_path(&log), target.join("log.jsonl.1")),
    ] {
        match std::fs::rename(&from, &to) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "moved board '{id}' but cannot move its log: {e}; move {} by hand",
                    from.display()
                ));
            }
            _ => {}
        }
    }
    Ok(target)
}

/// `<removed>/<id>-<UTC time>`, with `-2`, `-3`... when that is taken.
fn free_removed_path(removed: &Path, id: &str) -> Result<PathBuf, String> {
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    for n in 1..1000_u32 {
        let name = if n == 1 {
            format!("{id}-{stamp}")
        } else {
            format!("{id}-{stamp}-{n}")
        };
        let path = removed.join(name);
        if std::fs::symlink_metadata(&path).is_err() {
            return Ok(path);
        }
    }
    Err(format!(
        "too many removed copies of '{id}' this second in {REMOVED_DIR}/; try again"
    ))
}

/// The title in `board.md` of a board directory; empty when it cannot be read.
#[must_use]
pub fn board_title(dir: &Path) -> String {
    let path = dir.join("board.md");
    let regular = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file());
    if !regular {
        return String::new();
    }
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| BoardFile::parse(&String::from_utf8_lossy(&bytes)).ok())
        .and_then(|file| file.frontmatter.get("title").and_then(Result::ok))
        .unwrap_or_default()
}

/// Every board whose id `keep` accepts, as `(id, title)`, sorted by id. `board.md` is only read
/// for kept ids. Entries that are not valid ids or not real directories are skipped.
///
/// # Errors
/// When `boards/` cannot be read.
pub fn list_boards_where(
    data: &Path,
    keep: impl Fn(&str) -> bool,
) -> Result<Vec<(String, String)>, String> {
    let boards = data.join(BOARDS_DIR);
    let entries = std::fs::read_dir(&boards).map_err(|e| {
        format!(
            "cannot read '{}': {e}; is this a markban data directory?",
            boards.display()
        )
    })?;
    let mut out: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let id = entry.file_name().to_str()?.to_string();
            if !keep(&id) {
                return None;
            }
            let dir = board_dir(data, &id)?;
            Some((id, board_title(&dir)))
        })
        .collect();
    out.sort();
    Ok(out)
}

/// Every board as `(id, title)`, sorted by id. Entries that are not valid ids or not real
/// directories are skipped.
///
/// # Errors
/// When `boards/` cannot be read.
pub fn list_boards(data: &Path) -> Result<Vec<(String, String)>, String> {
    list_boards_where(data, |_| true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hash_decoding_accepts_only_64_hex_digits() {
        assert_eq!(decode_hash(&"ab".repeat(32)), Some([0xab; 32]));
        assert_eq!(decode_hash(&"AB".repeat(32)), Some([0xab; 32]));
        for bad in [
            "",
            "ab",
            &"ab".repeat(33),
            &"zz".repeat(32),
            &"é".repeat(32),
            &"+a".repeat(32),
        ] {
            assert_eq!(decode_hash(bad), None, "{bad}");
        }
    }
}
