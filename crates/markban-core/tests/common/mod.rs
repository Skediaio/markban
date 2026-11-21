#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use markban_core::command::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A temp copy of `tests/fixtures/commands`.
pub fn temp_board() -> TempDir {
    let dir = TempDir::new().unwrap();
    copy_dir(&fixture("commands"), dir.path());
    dir
}

pub fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

pub fn exists(dir: &Path, rel: &str) -> bool {
    dir.join(rel).exists()
}

pub fn cmd(json: &str) -> Command {
    Command::parse_line(json).unwrap()
}

/// Lines that differ between two texts of the same line count, as `(old, new)`.
pub fn changed_lines(old: &str, new: &str) -> Vec<(String, String)> {
    let (o, n): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    assert_eq!(o.len(), n.len(), "line count differs:\n{old}\n----\n{new}");
    o.iter()
        .zip(&n)
        .filter(|(a, b)| a != b)
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

/// Card ids of a column in spec sort order, read from disk.
pub fn column_ids(dir: &Path, column: &str) -> Vec<String> {
    let (board, _) = markban_core::fs::load(dir).unwrap();
    let view = markban_core::view(&board.unwrap());
    let col = view
        .columns
        .iter()
        .find(|c| c.id == column)
        .unwrap_or_else(|| panic!("no column {column}"));
    col.cards.iter().map(|c| c.id.clone()).collect()
}
