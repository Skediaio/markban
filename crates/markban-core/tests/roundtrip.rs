#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::board::{Board, BoardFile, CardFile};
use markban_core::fs;
use proptest::prelude::*;
use std::path::{Path, PathBuf};

fn fixtures(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(sub)
}

fn boards_in(sub: &str) -> Vec<PathBuf> {
    let mut dirs: Vec<_> = std::fs::read_dir(fixtures(sub))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn fixtures_round_trip_byte_for_byte() {
    let dirs = boards_in("roundtrip");
    assert!(dirs.len() >= 5);
    for dir in dirs {
        let (board, _diags) = fs::load(&dir).unwrap();
        let board = board.unwrap_or_else(|| panic!("{} did not load", dir.display()));
        let files = board.files();
        assert!(!files.is_empty());
        for (path, bytes) in files {
            let on_disk = std::fs::read(dir.join(&path)).unwrap();
            assert_eq!(bytes, on_disk, "{} / {path} changed", dir.display());
        }
    }
}

#[test]
fn crlf_fixture_really_has_crlf() {
    let bytes = std::fs::read(fixtures("roundtrip/crlf/board.md")).unwrap();
    assert!(bytes.windows(2).any(|w| w == b"\r\n"));
}

#[test]
fn files_are_sorted_and_deterministic() {
    let (board, _) = fs::load(&fixtures("roundtrip/no-final-newline")).unwrap();
    let paths: Vec<_> = board.unwrap().files().into_iter().map(|f| f.0).collect();
    assert_eq!(paths, ["board.md", "cards/body.md", "cards/last.md"]);
}

fn line() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("---".to_string()),
        Just("markban: 0.1".to_string()),
        Just("title: Hello: \"x\" #y".to_string()),
        Just("columns:".to_string()),
        Just("  - id: todo".to_string()),
        Just("    name: Todo".to_string()),
        Just("column: todo".to_string()),
        Just("order: 12.50".to_string()),
        Just(String::new()),
        Just("  ".to_string()),
        "[ -~]{0,12}",
        any::<String>(),
    ]
}

fn text() -> impl Strategy<Value = String> {
    (
        proptest::collection::vec(
            (line(), prop_oneof![Just("\n"), Just("\r\n"), Just("")]),
            0..14,
        ),
        any::<bool>(),
    )
        .prop_map(|(lines, crlf_tail)| {
            let mut s = String::new();
            for (l, e) in lines {
                s.push_str(&l);
                s.push_str(e);
            }
            if crlf_tail {
                s.push_str("\r\n");
            }
            s
        })
}

proptest! {
    #[test]
    fn arbitrary_bytes_never_panic(
        board in proptest::collection::vec(any::<u8>(), 0..300),
        card in proptest::collection::vec(any::<u8>(), 0..300),
        name in "[ -~]{0,10}",
    ) {
        let (b, diags) = Board::parse(&board, vec![(name, card)], vec![]);
        if let Some(b) = &b {
            let _ = markban_core::check(b);
            let _ = markban_core::view(b);
            let _ = b.files();
        }
        let _ = diags.iter().map(ToString::to_string).count();
    }

    #[test]
    fn valid_utf8_round_trips(board in text(), card in text()) {
        if let Ok(f) = BoardFile::parse(&board) {
            prop_assert_eq!(f.to_string(), board.clone());
        }
        let c = CardFile::parse("x", &card);
        prop_assert_eq!(c.to_string(), card.clone());

        let (b, _) = Board::parse(board.as_bytes(), vec![("x.md".to_string(), card.clone().into_bytes())], vec![]);
        if let Some(b) = b {
            let files = b.files();
            prop_assert_eq!(files.first().map(|f| f.1.clone()), Some(board.clone().into_bytes()));
            prop_assert_eq!(files.get(1).map(|f| f.1.clone()), Some(card.into_bytes()));
        }
    }

    #[test]
    fn mutations_never_panic(board in text(), id in "[a-z]{0,3}", name in any::<String>()) {
        if let Ok(mut f) = BoardFile::parse(&board) {
            let _ = f.rename_column(&id, &name);
            let _ = f.add_column(&id, &name, Some(&id));
            let _ = f.move_column(&id, None);
            let _ = f.remove_column(&id);
            let _ = f.columns();
        }
    }
}
