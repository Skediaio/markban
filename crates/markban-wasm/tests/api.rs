//! The in-memory API must agree with loading the same board from disk.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use markban_core::fs::load;
use markban_core::view;
use markban_wasm::{InFile, card_detail, check_board, parse_board};
use std::path::{Path, PathBuf};

fn demo_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/demo-board")
}

fn read_files(dir: &Path) -> Vec<InFile> {
    let mut files = vec![InFile {
        path: "board.md".into(),
        content: std::fs::read_to_string(dir.join("board.md")).unwrap(),
    }];
    let mut names: Vec<_> = std::fs::read_dir(dir.join("cards"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        files.push(InFile {
            path: format!("cards/{name}"),
            content: std::fs::read_to_string(dir.join("cards").join(&name)).unwrap(),
        });
    }
    files
}

fn file(path: &str, content: &str) -> InFile {
    InFile {
        path: path.into(),
        content: content.into(),
    }
}

#[test]
fn demo_board_matches_the_disk_view() {
    let (board, diagnostics) = load(&demo_dir()).unwrap();
    let mut expected = view::view(&board.unwrap());
    expected.diagnostics = diagnostics;
    assert_eq!(parse_board(read_files(&demo_dir())), expected);
    assert_eq!(expected.columns.len(), 3);
}

#[test]
fn card_returns_detail_or_none() {
    let card = card_detail(read_files(&demo_dir()), "0001-spec-draft").unwrap();
    assert_eq!(card.title, "Write spec v0.1");
    assert!(card.description.starts_with("The format in"));
    assert!(card_detail(read_files(&demo_dir()), "nope").is_none());
}

#[test]
fn missing_board_md_is_an_e001_diagnostic_not_a_panic() {
    let view = parse_board(vec![file("cards/a.md", "---\ntitle: A\n---\n")]);
    assert!(view.columns.is_empty());
    assert_eq!(view.diagnostics.len(), 1);
    assert_eq!(view.diagnostics[0].code.as_str(), "E001");
    assert!(card_detail(vec![], "a").is_none());
}

#[test]
fn problems_in_a_card_are_reported_by_check() {
    let files = vec![
        file(
            "board.md",
            "---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: todo\n    name: Todo\n---\n",
        ),
        file("cards/a.md", "no frontmatter\n"),
        file("notes.txt", "ignored"),
        file("cards/sub/deep.md", "ignored"),
    ];
    let diagnostics = check_board(files);
    assert!(diagnostics.iter().any(|d| d.file == "cards/a.md"));
    assert!(diagnostics.iter().all(|d| !d.file.contains("deep")));
    let w001: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "W001")
        .collect();
    assert_eq!(w001.len(), 1, "{w001:?}");
    assert!(w001[0].file.ends_with("sub"), "{w001:?}");
}

#[test]
fn windows_separators_and_dot_prefix_are_accepted() {
    let files = vec![
        file(
            ".\\board.md",
            "---\ntitle: T\ncolumns:\n  - id: a\n    name: A\n---\n",
        ),
        file("cards\\x.md", "---\ntitle: X\ncolumn: a\n---\n"),
    ];
    let view = parse_board(files);
    assert_eq!(view.title, "T");
    assert_eq!(view.columns.first().map(|c| c.cards.len()), Some(1));
}

#[test]
fn search_finds_cards_columns_and_the_board() {
    let files = read_files(&demo_dir());
    let hits = markban_wasm::search(files, vec!["experiments".to_string()]).unwrap();
    assert_eq!(hits.cards.len(), 2, "{hits:?}");
    // The active card comes first, the archived one after, flagged.
    assert_eq!(hits.cards[0].id, "0001-spec-draft");
    assert!(!hits.cards[0].archived);
    assert!(hits.cards[0].snippet.is_some());
    assert_eq!(hits.cards[1].id, "0004-research");
    assert!(hits.cards[1].archived);
    // Every term is required.
    let hits = markban_wasm::search(
        read_files(&demo_dir()),
        vec!["roadmap".to_string(), "ZZZ".to_string()],
    )
    .unwrap();
    assert!(hits.cards.is_empty() && hits.columns.is_empty() && !hits.board);
}

#[test]
fn search_without_a_usable_board_gives_empty_results() {
    let hits = markban_wasm::search(vec![], vec!["anything".to_string()]).unwrap();
    assert_eq!(hits.cards, Vec::new());
    assert_eq!(hits.columns, Vec::new());
    assert!(!hits.board);
}

#[test]
fn search_refuses_too_many_terms_and_overlong_terms() {
    let many: Vec<String> = (0..33).map(|i| format!("t{i}")).collect();
    assert!(markban_wasm::search(vec![], many).is_err());
    assert!(markban_wasm::search(vec![], vec!["a".repeat(257)]).is_err());
}
