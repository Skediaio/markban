#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{cmd, read, temp_board};
use markban_core::command::execute;
use markban_core::fs::{self, ApplyError, RunError};

fn loaded(dir: &std::path::Path) -> markban_core::board::Board {
    fs::load(dir).unwrap().0.unwrap()
}

fn after_with(before: &markban_core::board::Board, json: &str) -> markban_core::board::Board {
    let mut after = before.clone();
    execute(&mut after, &cmd(json)).unwrap();
    after
}

fn no_temp_files(dir: &std::path::Path) {
    for sub in [dir.to_path_buf(), dir.join("cards")] {
        for entry in std::fs::read_dir(sub).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            assert!(!name.contains(".tmp"), "left over temp file {name}");
        }
    }
}

#[test]
fn apply_writes_only_changed_files_and_reports_them() {
    let dir = temp_board();
    let before = loaded(dir.path());
    let after = after_with(&before, r#"{"cmd":"card.move","id":"a","column":"done"}"#);
    let changed = fs::apply(dir.path(), &before, &after).unwrap();
    assert_eq!(changed, ["cards/a.md"]);
    assert!(read(dir.path(), "cards/a.md").contains("column: done"));
    no_temp_files(dir.path());
}

#[test]
fn archive_and_unarchive_through_the_fs_layer_write_only_the_card_file() {
    let dir = temp_board();
    let original = read(dir.path(), "cards/a.md");
    let before = loaded(dir.path());
    let after = after_with(&before, r#"{"cmd":"card.archive","id":"a"}"#);
    assert_eq!(
        fs::apply(dir.path(), &before, &after).unwrap(),
        ["cards/a.md"]
    );
    assert!(read(dir.path(), "cards/a.md").contains("archived: true"));
    let out = fs::run(dir.path(), &[cmd(r#"{"cmd":"card.unarchive","id":"a"}"#)]).unwrap();
    assert_eq!(out.first().map(|o| o.id.as_str()), Some("a"));
    assert_eq!(read(dir.path(), "cards/a.md"), original);
    no_temp_files(dir.path());
}

#[test]
fn apply_orders_creations_then_modifications_then_deletions() {
    let dir = temp_board();
    let before = loaded(dir.path());
    let mut after = before.clone();
    for json in [
        r#"{"cmd":"card.remove","id":"e"}"#,
        r#"{"cmd":"card.edit","id":"a","title":"A2"}"#,
        r#"{"cmd":"card.add","title":"Z"}"#,
    ] {
        execute(&mut after, &cmd(json)).unwrap();
    }
    let changed = fs::apply(dir.path(), &before, &after).unwrap();
    assert_eq!(changed, ["cards/z.md", "cards/a.md", "cards/e.md"]);
}

#[test]
fn modified_file_on_disk_is_a_conflict_and_is_not_overwritten() {
    let dir = temp_board();
    let before = loaded(dir.path());
    let after = after_with(&before, r#"{"cmd":"card.edit","id":"a","title":"Mine"}"#);
    std::fs::write(
        dir.path().join("cards/a.md"),
        "---\ntitle: Theirs\ncolumn: todo\n---\n",
    )
    .unwrap();
    let err = fs::apply(dir.path(), &before, &after).unwrap_err();
    assert!(
        matches!(&err, ApplyError::Conflict { path } if path == "cards/a.md"),
        "{err:?}"
    );
    assert_eq!(
        read(dir.path(), "cards/a.md"),
        "---\ntitle: Theirs\ncolumn: todo\n---\n"
    );
    no_temp_files(dir.path());
}

#[test]
fn conflict_writes_nothing_at_all() {
    let dir = temp_board();
    let before = loaded(dir.path());
    let mut after = before.clone();
    execute(&mut after, &cmd(r#"{"cmd":"card.add","title":"New"}"#)).unwrap();
    execute(
        &mut after,
        &cmd(r#"{"cmd":"card.edit","id":"b","title":"B2"}"#),
    )
    .unwrap();
    execute(
        &mut after,
        &cmd(r#"{"cmd":"card.edit","id":"c","title":"C2"}"#),
    )
    .unwrap();
    std::fs::write(dir.path().join("cards/c.md"), "changed\n").unwrap();
    assert!(fs::apply(dir.path(), &before, &after).is_err());
    assert!(!dir.path().join("cards/new.md").exists());
    assert!(read(dir.path(), "cards/b.md").contains("title: Beta"));
    no_temp_files(dir.path());
}

#[test]
fn deleting_a_file_that_changed_or_vanished_is_a_conflict() {
    let dir = temp_board();
    let before = loaded(dir.path());
    let after = after_with(&before, r#"{"cmd":"card.remove","id":"a"}"#);
    std::fs::write(dir.path().join("cards/a.md"), "edited\n").unwrap();
    assert!(matches!(
        fs::apply(dir.path(), &before, &after),
        Err(ApplyError::Conflict { .. })
    ));
    assert!(dir.path().join("cards/a.md").exists());
    std::fs::remove_file(dir.path().join("cards/a.md")).unwrap();
    assert!(matches!(
        fs::apply(dir.path(), &before, &after),
        Err(ApplyError::Conflict { .. })
    ));
}

#[test]
fn creating_over_an_existing_file_is_a_conflict() {
    let dir = temp_board();
    let before = loaded(dir.path());
    let after = after_with(&before, r#"{"cmd":"card.add","title":"Race"}"#);
    std::fs::write(dir.path().join("cards/race.md"), "someone else\n").unwrap();
    let err = fs::apply(dir.path(), &before, &after).unwrap_err();
    assert!(
        matches!(&err, ApplyError::Conflict { path } if path == "cards/race.md"),
        "{err:?}"
    );
    assert_eq!(read(dir.path(), "cards/race.md"), "someone else\n");
    assert!(err.to_string().contains("cards/race.md"));
}

#[test]
fn run_retries_after_a_conflict_and_then_succeeds() {
    let dir = temp_board();
    let mut attempts = Vec::new();
    let out = fs::run_with_hook(
        dir.path(),
        &[cmd(r#"{"cmd":"card.move","id":"b","top":true}"#)],
        &mut |attempt| {
            attempts.push(attempt);
            if attempt == 0 {
                let p = dir.path().join("cards/b.md");
                let mut text = std::fs::read_to_string(&p).unwrap();
                text.push_str("\nConcurrent note.\n");
                std::fs::write(p, text).unwrap();
            }
        },
    )
    .unwrap();
    assert_eq!(out[0].id, "b");
    assert_eq!(attempts, [0, 1]);
    let b = read(dir.path(), "cards/b.md");
    assert!(
        b.contains("Concurrent note.") && !b.contains("order: 2000"),
        "{b}"
    );
}

#[test]
fn run_gives_up_after_three_conflicts_with_one_line() {
    let dir = temp_board();
    let mut calls = 0;
    let err = fs::run_with_hook(
        dir.path(),
        &[cmd(r#"{"cmd":"card.edit","id":"b","title":"X"}"#)],
        &mut |_| {
            calls += 1;
            let p = dir.path().join("cards/b.md");
            let mut text = std::fs::read_to_string(&p).unwrap();
            text.push('x');
            std::fs::write(p, text).unwrap();
        },
    )
    .unwrap_err();
    assert_eq!(calls, 3);
    assert!(matches!(err, RunError::Contended { .. }), "{err:?}");
    assert!(!err.to_string().contains('\n'));
}

// ---- run_guarded ----

fn snapshot(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![(
        "board.md".to_string(),
        std::fs::read(dir.join("board.md")).unwrap(),
    )];
    let mut cards: Vec<_> = std::fs::read_dir(dir.join("cards"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    cards.sort();
    for card in cards {
        let name = card.file_name().unwrap().to_string_lossy().into_owned();
        files.push((name, std::fs::read(&card).unwrap()));
    }
    files
}

#[test]
fn run_guarded_sees_before_and_after_and_a_refusal_writes_nothing() {
    let dir = temp_board();
    let before = snapshot(dir.path());
    let seen = std::cell::Cell::new(None);
    let guard = |old: Option<&markban_core::board::Board>, new: &markban_core::board::Board| {
        seen.set(Some((old.map(|b| b.cards.len()), new.cards.len())));
        Err::<(), String>("too many cards on this server".to_string())
    };
    let err = fs::run_guarded(
        dir.path(),
        &[cmd(r#"{"cmd":"card.add","title":"New one"}"#)],
        &guard,
    )
    .unwrap_err();
    assert!(
        matches!(&err, RunError::Refused { reason } if reason == "too many cards on this server"),
        "{err:?}"
    );
    assert_eq!(err.exit_code(), fs::EXIT_INVALID);
    assert_eq!(err.to_string(), "too many cards on this server");
    let (old, new) = seen.get().unwrap();
    assert_eq!(new, old.unwrap() + 1);
    assert_eq!(snapshot(dir.path()), before, "a refused run writes nothing");
    no_temp_files(dir.path());
}

#[test]
fn run_guarded_applies_when_the_guard_agrees() {
    let dir = temp_board();
    let ok = |_: Option<&markban_core::board::Board>, _: &markban_core::board::Board| Ok(());
    let outcomes = fs::run_guarded(
        dir.path(),
        &[cmd(r#"{"cmd":"card.add","title":"New one"}"#)],
        &ok,
    )
    .unwrap();
    assert_eq!(outcomes[0].id, "new-one");
    assert!(dir.path().join("cards/new-one.md").is_file());
}
