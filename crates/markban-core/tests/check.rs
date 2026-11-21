#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::board::Board;
use markban_core::diagnostic::{Code, Severity};
use markban_core::fs;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn fixtures(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(sub)
}

#[test]
fn demo_board_has_zero_diagnostics() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo-board");
    let (board, diags) = fs::load(&dir).unwrap();
    assert!(board.is_some());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn every_code_has_exactly_one_fixture_yielding_only_that_code() {
    let all = [
        "E001", "E002", "E003", "E004", "E005", "E006", "E007", "E008", "E009", "W001", "W002",
    ];
    for code in all {
        let dir = fixtures(&format!("invalid/{code}"));
        let (_board, diags) = fs::load(&dir).unwrap();
        let got: BTreeSet<&str> = diags.iter().map(|d| d.code.as_str()).collect();
        assert_eq!(got, BTreeSet::from([code]), "{code}: {diags:?}");
    }
}

#[test]
fn warnings_have_warning_severity() {
    let (_, diags) = fs::load(&fixtures("invalid/W001")).unwrap();
    assert!(diags.iter().all(|d| d.severity == Severity::Warning));
    let (_, diags) = fs::load(&fixtures("invalid/E007")).unwrap();
    assert!(diags.iter().all(|d| d.severity == Severity::Error));
    assert_eq!(Code::E007.as_str(), "E007");
}

#[test]
fn e007_display_is_one_actionable_line() {
    let (_, diags) = fs::load(&fixtures("invalid/E007")).unwrap();
    assert_eq!(diags.len(), 1);
    let d = diags.first().unwrap();
    assert_eq!(d.file, "cards/x.md");
    assert_eq!(d.line, Some(3));
    assert_eq!(
        d.to_string(),
        "cards/x.md:3: E007 card column 'dong' is not a column; columns: todo, done"
    );
}

#[test]
fn display_without_line() {
    let (_, diags) = fs::load(&fixtures("invalid/W001")).unwrap();
    let text = diags.first().unwrap().to_string();
    assert!(text.starts_with("cards/notes.txt: W001 "), "{text}");
    assert!(!text.contains('\n'));
}

#[test]
fn missing_board_md_is_e001_and_missing_cards_dir_is_fine() {
    let tmp = std::env::temp_dir().join(format!("markban-core-test-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let (board, diags) = fs::load(&tmp).unwrap();
    assert!(board.is_none());
    assert_eq!(diags.len(), 1);
    assert_eq!(diags.first().unwrap().code, Code::E001);
    assert_eq!(diags.first().unwrap().file, "board.md");
    std::fs::write(
        tmp.join("board.md"),
        "---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: a\n    name: A\n---\n",
    )
    .unwrap();
    let (board, diags) = fs::load(&tmp).unwrap();
    assert!(board.is_some() && diags.is_empty());
    std::fs::remove_dir_all(&tmp).unwrap();
}

#[test]
fn load_of_missing_directory_is_an_error() {
    assert!(fs::load(Path::new("/definitely/not/here")).is_err());
}

#[test]
fn one_bad_card_does_not_stop_the_rest() {
    let board = b"---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: a\n    name: A\n---\n";
    let cards = vec![
        ("bad.md".to_string(), vec![0xff, 0xfe]),
        (
            "ok.md".to_string(),
            b"---\ntitle: Ok\ncolumn: a\n---\n".to_vec(),
        ),
        ("nofm.md".to_string(), b"hello".to_vec()),
    ];
    let (b, parse_diags) = Board::parse(board, cards, vec![]);
    let b = b.unwrap();
    assert_eq!(parse_diags.len(), 1);
    assert_eq!(parse_diags.first().unwrap().code, Code::E009);
    assert!(b.cards.contains_key("ok"));
    let codes: Vec<_> = markban_core::check(&b).iter().map(|d| d.code).collect();
    assert_eq!(codes, [Code::E006]);
}

#[test]
fn other_rules() {
    let board = b"---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: a\n    name: A\n  - id: B_d\n  - name: Z\n---\n";
    let cards = vec![
        (
            "c.md".to_string(),
            b"---\ntitle: \"x\" y\ncolumn: a\norder: 1.\n---\n".to_vec(),
        ),
        ("d.md".to_string(), b"---\ntitle: D\n---\n".to_vec()),
    ];
    let (b, _) = Board::parse(board, cards, vec![]);
    let diags = markban_core::check(&b.unwrap());
    let codes: Vec<_> = diags.iter().map(|d| d.code.as_str()).collect();
    // B_d malformed id, B_d missing name, item without id, c: E009 + E008, d: E007
    assert_eq!(
        codes.iter().filter(|c| **c == "E004").count(),
        3,
        "{diags:?}"
    );
    assert!(codes.contains(&"E009") && codes.contains(&"E008") && codes.contains(&"E007"));
}
