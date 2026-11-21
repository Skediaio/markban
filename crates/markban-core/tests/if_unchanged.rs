#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! `card.edit` with `if_unchanged`: an optimistic check against the card file as last read.

mod common;

use common::{cmd, read, temp_board};
use markban_core::command::CommandError;
use markban_core::fs::{self, EXIT_CONTENDED, EXIT_INVALID, EXIT_PARTIAL, RunError};
use markban_core::view;

fn sha_of(dir: &std::path::Path, id: &str) -> String {
    let (board, _) = fs::load(dir).unwrap();
    view::card(&board.unwrap(), id).unwrap().sha256
}

#[test]
fn card_detail_carries_the_sha256_of_the_card_file() {
    let dir = temp_board();
    let bytes = std::fs::read(dir.path().join("cards/a.md")).unwrap();
    let sha = sha_of(dir.path(), "a");
    assert_eq!(sha.len(), 64);
    assert!(
        sha.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(sha, markban_core::sha256_hex(&bytes));
    // Known vector, so the hash is plain SHA-256 of the bytes.
    assert_eq!(
        markban_core::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn matching_hash_edits() {
    let dir = temp_board();
    let sha = sha_of(dir.path(), "a");
    let line = format!(r#"{{"cmd":"card.edit","id":"a","title":"New","if_unchanged":"{sha}"}}"#);
    let outcomes = fs::run(dir.path(), &[cmd(&line)]).unwrap();
    assert_eq!(outcomes[0].id, "a");
    assert!(read(dir.path(), "cards/a.md").contains("title: New"));
}

#[test]
fn uppercase_hash_is_accepted() {
    let dir = temp_board();
    let sha = sha_of(dir.path(), "a").to_ascii_uppercase();
    let line = format!(r#"{{"cmd":"card.edit","id":"a","append":"x","if_unchanged":"{sha}"}}"#);
    fs::run(dir.path(), &[cmd(&line)]).unwrap();
}

#[test]
fn stale_hash_is_contention_and_writes_nothing() {
    let dir = temp_board();
    let stale = sha_of(dir.path(), "a");
    fs::run(
        dir.path(),
        &[cmd(
            r#"{"cmd":"card.edit","id":"a","append":"someone else"}"#,
        )],
    )
    .unwrap();
    let before = read(dir.path(), "cards/a.md");
    let line = format!(r#"{{"cmd":"card.edit","id":"a","title":"Mine","if_unchanged":"{stale}"}}"#);
    let err = fs::run(dir.path(), &[cmd(&line)]).unwrap_err();
    match &err {
        RunError::Command {
            source: CommandError::CardChanged { id },
            ..
        } => assert_eq!(id, "a"),
        other => panic!("expected CardChanged, got {other:?}"),
    }
    assert_eq!(err.exit_code(), EXIT_CONTENDED);
    let message = err.to_string();
    assert!(message.contains("changed since"), "{message}");
    assert!(!message.contains('\n'));
    assert_eq!(read(dir.path(), "cards/a.md"), before);
}

#[test]
fn stale_hash_in_a_batch_leaves_the_whole_batch_unapplied() {
    let dir = temp_board();
    let batch = [
        cmd(r#"{"cmd":"card.add","title":"Fresh"}"#),
        cmd(&format!(
            r#"{{"cmd":"card.edit","id":"b","title":"X","if_unchanged":"{}"}}"#,
            "0".repeat(64)
        )),
    ];
    let err = fs::run(dir.path(), &batch).unwrap_err();
    assert_eq!(err.exit_code(), EXIT_CONTENDED);
    assert!(!dir.path().join("cards/fresh.md").exists());
}

#[test]
fn malformed_hash_is_invalid_input() {
    let dir = temp_board();
    for bad in ["", "abc", "zz", &"g".repeat(64), &"0".repeat(65)] {
        let line = format!(r#"{{"cmd":"card.edit","id":"a","title":"X","if_unchanged":"{bad}"}}"#);
        let err = fs::run(dir.path(), &[cmd(&line)]).unwrap_err();
        assert_eq!(err.exit_code(), EXIT_INVALID, "{bad}: {err}");
        assert!(err.to_string().contains("if_unchanged"), "{err}");
    }
}

#[test]
fn if_unchanged_alone_is_nothing_to_change() {
    let dir = temp_board();
    let sha = sha_of(dir.path(), "a");
    let line = format!(r#"{{"cmd":"card.edit","id":"a","if_unchanged":"{sha}"}}"#);
    let err = fs::run(dir.path(), &[cmd(&line)]).unwrap_err();
    assert_eq!(err.exit_code(), EXIT_INVALID);
}

#[test]
fn exit_codes_follow_the_spec() {
    let partial = RunError::PartiallyApplied {
        written: vec!["board.md".into()],
        failed: "cards/a.md".into(),
        reason: "x".into(),
    };
    assert_eq!(partial.exit_code(), EXIT_PARTIAL);
    let locked = RunError::Locked { path: "x".into() };
    assert_eq!(locked.exit_code(), EXIT_CONTENDED);
    let unknown = RunError::Command {
        index: 0,
        source: CommandError::UnknownCard("x".into()),
    };
    assert_eq!(unknown.exit_code(), EXIT_INVALID);
}

#[test]
fn if_unchanged_round_trips_through_json() {
    let line = r#"{"cmd":"card.edit","id":"a","title":"T","if_unchanged":"00"}"#;
    let c = cmd(line);
    assert_eq!(serde_json::to_string(&c).unwrap(), line);
    // Absent stays absent in the JSON form.
    let plain = r#"{"cmd":"card.edit","id":"a","title":"T"}"#;
    assert_eq!(serde_json::to_string(&cmd(plain)).unwrap(), plain);
}
