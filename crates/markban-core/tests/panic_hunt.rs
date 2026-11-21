#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! Panic hunt: adversarial commands against several board states. No command may panic; every
//! accepted command leaves a board without new errors that still round-trips.

mod common;

use markban_core::board::Board;
use markban_core::command::{Command, execute};
use markban_core::diagnostic::Severity;
use markban_core::fs::load;
use proptest::prelude::*;
use std::path::Path;
use std::sync::OnceLock;

fn boards() -> &'static Vec<Board> {
    static BOARDS: OnceLock<Vec<Board>> = OnceLock::new();
    BOARDS.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut dirs = vec![
            root.join("../../examples/demo-board"),
            common::fixture("commands"),
        ];
        let mut invalid: Vec<_> = std::fs::read_dir(common::fixture("invalid"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        invalid.sort();
        dirs.extend(invalid);
        let mut boards: Vec<Board> = dirs.iter().filter_map(|d| load(d).unwrap().0).collect();
        let empty = "---\nmarkban: 0.1\ntitle: Empty\ncolumns:\n---\n";
        boards.push(Board::parse(empty.as_bytes(), vec![], vec![]).0.unwrap());
        boards
    })
}

fn pool() -> Vec<String> {
    let mut words: Vec<String> = [
        "", " ", "a", "Done", "TODO", "-", "---", "a: b", "#", "\"", "\\", "日本", "x\ny", "\r",
        "1e3",
    ]
    .iter()
    .map(|s| (*s).to_string())
    .collect();
    words.push("z".repeat(5000));
    words.push("-".repeat(300));
    for board in boards() {
        words.extend(board.cards.keys().cloned());
        for c in board.board.columns() {
            words.extend(c.id);
            words.extend(c.name);
        }
    }
    words.sort();
    words.dedup();
    words
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => proptest::sample::select(pool()),
        2 => any::<String>(),
        1 => "\\PC{0,24}",
    ]
}

fn opt() -> impl Strategy<Value = Option<String>> {
    prop_oneof![2 => Just(None), 3 => text().prop_map(Some)]
}

fn command() -> impl Strategy<Value = Command> {
    prop_oneof![
        (text(), proptest::collection::vec(text(), 0..4))
            .prop_map(|(title, columns)| Command::BoardInit { title, columns }),
        (text(), opt(), opt(), opt(), any::<bool>(), opt(), opt()).prop_map(
            |(title, column, description, id, top, before, after)| Command::CardAdd {
                title,
                column,
                description,
                id,
                top,
                before,
                after
            }
        ),
        (text(), opt(), any::<bool>(), opt(), opt()).prop_map(
            |(id, column, top, before, after)| Command::CardMove {
                id,
                column,
                top,
                before,
                after
            }
        ),
        (text(), opt(), opt(), opt()).prop_map(|(id, title, description, append)| {
            Command::CardEdit {
                id,
                title,
                description,
                append,
            }
        }),
        text().prop_map(|id| Command::CardRemove { id }),
        (text(), opt(), opt()).prop_map(|(name, id, before)| Command::ColumnAdd {
            name,
            id,
            before
        }),
        (text(), text()).prop_map(|(column, name)| Command::ColumnRename { column, name }),
        (text(), opt()).prop_map(|(column, before)| Command::ColumnMove { column, before }),
        (text(), opt()).prop_map(|(column, move_to)| Command::ColumnRemove { column, move_to }),
    ]
}

fn error_keys(board: &Board) -> Vec<(String, String, String)> {
    markban_core::check(board)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| (d.code.to_string(), d.file, d.message))
        .collect()
}

fn reparse(board: &Board) -> Board {
    let files = board.files();
    let (board_md, cards) = files.split_first().unwrap();
    let cards = cards
        .iter()
        .map(|(path, bytes)| {
            (
                path.strip_prefix("cards/").unwrap().to_string(),
                bytes.clone(),
            )
        })
        .collect();
    Board::parse(&board_md.1, cards, board.ignored.clone())
        .0
        .unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn commands_never_panic_and_keep_boards_valid(
        start in 0..boards().len(),
        cmds in proptest::collection::vec(command(), 1..5),
    ) {
        let mut board = boards()[start].clone();
        for cmd in &cmds {
            let known = error_keys(&board);
            let snapshot = board.clone();
            match execute(&mut board, cmd) {
                Ok(_) => {
                    for key in error_keys(&board) {
                        prop_assert!(known.contains(&key), "new error {key:?} after {cmd:?}");
                    }
                    prop_assert_eq!(reparse(&board).files(), board.files());
                }
                Err(e) => {
                    let line = e.to_string();
                    prop_assert!(!line.is_empty() && !line.contains('\n'), "{line:?}");
                    prop_assert_eq!(&board, &snapshot);
                }
            }
        }
    }

    #[test]
    fn json_lines_never_panic(line in "\\PC{0,200}") {
        let _ = Command::parse_line(&line);
    }
}

#[test]
fn added_titles_and_names_read_back_unchanged() {
    for title in [
        "a: b",
        " lead",
        "trail ",
        "\"q\"",
        "#x",
        "- dash",
        "back\\slash",
        "日本語",
        "[x]",
    ] {
        let mut board = boards()[1].clone();
        let id = execute(
            &mut board,
            &Command::CardAdd {
                title: title.into(),
                column: None,
                description: None,
                id: None,
                top: false,
                before: None,
                after: None,
            },
        )
        .unwrap()
        .id;
        assert_eq!(board.cards[&id].get("title").unwrap().unwrap(), title);
        assert_eq!(
            reparse(&board).cards[&id].get("title").unwrap().unwrap(),
            title
        );
    }
}
