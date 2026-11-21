#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::board::BoardFile;

const BOARD: &str = "---\nmarkban: 0.1\ntitle: T\ncolumns:\n  # note\n  - id: a\n    name: A\n    wip: 2\n\n  - id: b\n    name: B\n  - id: c\n    name: C\n---\n\nDesc\n";

fn parse(text: &str) -> BoardFile {
    BoardFile::parse(text).unwrap()
}

fn ids(b: &BoardFile) -> Vec<String> {
    b.columns().into_iter().filter_map(|c| c.id).collect()
}

#[test]
fn parses_columns_with_unknown_keys_and_extra_lines() {
    let b = parse(BOARD);
    assert_eq!(ids(&b), ["a", "b", "c"]);
    let cols = b.columns();
    assert_eq!(
        cols.first().and_then(|c| c.name.clone()).as_deref(),
        Some("A")
    );
    assert_eq!(b.to_string(), BOARD);
    assert_eq!(b.description, "\nDesc\n");
}

#[test]
fn rename_changes_only_the_name_line() {
    let mut b = parse(BOARD);
    b.rename_column("a", "Alpha: one").unwrap();
    assert_eq!(
        b.to_string(),
        BOARD.replace("    name: A\n", "    name: \"Alpha: one\"\n")
    );
}

#[test]
fn rename_unknown_column_errors() {
    let mut b = parse(BOARD);
    assert!(b.rename_column("zzz", "X").is_err());
}

#[test]
fn rename_adds_name_when_missing() {
    let mut b = parse("---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: a\n---\n");
    b.rename_column("a", "A").unwrap();
    assert_eq!(
        b.to_string(),
        "---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: a\n    name: A\n---\n"
    );
}

#[test]
fn add_column_at_end_and_before() {
    let mut b = parse(BOARD);
    b.add_column("d", "D", None).unwrap();
    assert_eq!(ids(&b), ["a", "b", "c", "d"]);
    assert!(
        b.to_string()
            .contains("    name: C\n  - id: d\n    name: D\n---\n")
    );

    let mut b = parse(BOARD);
    b.add_column("x", "X", Some("b")).unwrap();
    assert_eq!(ids(&b), ["a", "x", "b", "c"]);
    assert_eq!(
        b.to_string(),
        BOARD.replace("  - id: b\n", "  - id: x\n    name: X\n  - id: b\n")
    );
}

#[test]
fn add_column_rejects_duplicates_and_unknown_anchor() {
    let mut b = parse(BOARD);
    assert!(b.add_column("a", "A2", None).is_err());
    assert!(b.add_column("n", "N", Some("zzz")).is_err());
    assert_eq!(b.to_string(), BOARD);
}

#[test]
fn add_column_with_crlf_uses_crlf() {
    let crlf = "---\r\nmarkban: 0.1\r\ntitle: T\r\ncolumns:\r\n  - id: a\r\n    name: A\r\n---\r\n";
    let mut b = parse(crlf);
    b.add_column("b", "B", None).unwrap();
    assert_eq!(
        b.to_string(),
        "---\r\nmarkban: 0.1\r\ntitle: T\r\ncolumns:\r\n  - id: a\r\n    name: A\r\n  - id: b\r\n    name: B\r\n---\r\n"
    );
}

#[test]
fn move_column_moves_whole_item() {
    let mut b = parse(BOARD);
    b.move_column("c", Some("a")).unwrap();
    assert_eq!(ids(&b), ["c", "a", "b"]);
    let mut b = parse(BOARD);
    b.move_column("a", None).unwrap();
    assert_eq!(ids(&b), ["b", "c", "a"]);
    assert!(
        b.to_string()
            .contains("  - id: a\n    name: A\n    wip: 2\n")
    );
    // a moved item keeps all its lines: line count is unchanged
    assert_eq!(b.to_string().lines().count(), BOARD.lines().count());
}

#[test]
fn remove_column_removes_only_its_lines() {
    let mut b = parse(BOARD);
    b.remove_column("b").unwrap();
    assert_eq!(ids(&b), ["a", "c"]);
    assert_eq!(b.to_string(), BOARD.replace("  - id: b\n    name: B\n", ""));
    assert!(b.remove_column("b").is_err());
}
