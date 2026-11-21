#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{cmd, exists, read, temp_board};
use markban_core::board::Board;
use markban_core::command::{Command, execute_batch};
use markban_core::diagnostic::{Code, Severity};
use markban_core::fs::{self, ApplyError, RunError};
use std::io;
use std::path::Path;

fn loaded(dir: &Path) -> Board {
    fs::load(dir).unwrap().0.unwrap()
}

fn cmds(jsons: &[&str]) -> Vec<Command> {
    jsons.iter().map(|j| cmd(j)).collect()
}

fn error_count(dir: &Path, code: Code) -> usize {
    fs::load(dir)
        .unwrap()
        .1
        .iter()
        .filter(|d| d.severity == Severity::Error && d.code == code)
        .count()
}

/// Applies `jsons` once with a failure injected before write number `fail_at` (`None`: no
/// failure); returns the number of writes attempted and the result.
fn apply_failing_at(
    dir: &Path,
    jsons: &[&str],
    fail_at: Option<usize>,
) -> (usize, Result<Vec<String>, ApplyError>) {
    let before = loaded(dir);
    let mut after = before.clone();
    execute_batch(&mut after, &cmds(jsons)).unwrap();
    let mut calls = 0;
    let result = fs::apply_with_hook(dir, &before, &after, &mut |_, _| {
        calls += 1;
        if fail_at == Some(calls - 1) {
            Err(io::Error::other("injected"))
        } else {
            Ok(())
        }
    });
    (calls, result)
}

/// At every possible interruption point the board on disk has no E007.
fn valid_at_every_step(jsons: &[&str]) {
    let probe = temp_board();
    let (writes, result) = apply_failing_at(probe.path(), jsons, None);
    result.unwrap();
    assert!(writes >= 2, "scenario should need several writes");
    assert_eq!(error_count(probe.path(), Code::E007), 0);
    for n in 0..writes {
        let dir = temp_board();
        let (_, result) = apply_failing_at(dir.path(), jsons, Some(n));
        assert!(result.is_err());
        assert_eq!(
            error_count(dir.path(), Code::E007),
            0,
            "E007 on disk after failing before write {n}"
        );
    }
}

#[test]
fn column_remove_with_move_to_is_valid_at_every_step() {
    valid_at_every_step(&[r#"{"cmd":"column.remove","column":"todo","move_to":"doing"}"#]);
}

#[test]
fn column_add_then_card_add_is_valid_at_every_step() {
    valid_at_every_step(&[
        r#"{"cmd":"column.add","name":"Review"}"#,
        r#"{"cmd":"card.add","title":"New","column":"review"}"#,
    ]);
}

#[test]
fn mixed_add_and_remove_column_batch_is_valid_at_every_step() {
    valid_at_every_step(&[
        r#"{"cmd":"column.add","name":"Review"}"#,
        r#"{"cmd":"card.add","title":"New","column":"review"}"#,
        r#"{"cmd":"column.remove","column":"todo","move_to":"review"}"#,
    ]);
}

#[test]
fn mixed_batch_final_state_is_the_expected_board() {
    let dir = temp_board();
    let (_, result) = apply_failing_at(
        dir.path(),
        &[
            r#"{"cmd":"column.add","name":"Review"}"#,
            r#"{"cmd":"column.remove","column":"todo","move_to":"review"}"#,
        ],
        None,
    );
    result.unwrap();
    let board = read(dir.path(), "board.md");
    assert!(board.contains("id: review") && !board.contains("id: todo"));
    assert!(read(dir.path(), "cards/a.md").contains("column: review"));
}

// ---- partial application ----

#[test]
fn conflict_after_a_write_is_partially_applied_and_not_retried() {
    let dir = temp_board();
    let mut attempts = 0;
    let mut calls = 0;
    let err = fs::run_with_hooks(
        dir.path(),
        &cmds(&[
            r#"{"cmd":"card.add","title":"Fix login"}"#,
            r#"{"cmd":"card.edit","id":"a","title":"A2"}"#,
        ]),
        &mut |_| attempts += 1,
        &mut |_, path| {
            calls += 1;
            if path == "cards/a.md" {
                std::fs::write(dir.path().join(path), "someone else\n").unwrap();
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(attempts, 1, "no retry once a file was written");
    match &err {
        RunError::PartiallyApplied {
            written, failed, ..
        } => {
            assert_eq!(written, &["cards/fix-login.md"]);
            assert_eq!(failed, "cards/a.md");
        }
        other => panic!("{other:?}"),
    }
    assert!(!err.to_string().contains('\n'), "{err}");
    assert!(err.to_string().contains("markban check"), "{err}");
    assert!(exists(dir.path(), "cards/fix-login.md"));
    assert!(!exists(dir.path(), "cards/fix-login-2.md"));
    assert_eq!(calls, 2);
}

#[test]
fn io_error_after_a_write_is_partially_applied() {
    let dir = temp_board();
    let err = fs::run_with_hooks(
        dir.path(),
        &cmds(&[
            r#"{"cmd":"card.add","title":"Fix login"}"#,
            r#"{"cmd":"card.edit","id":"a","title":"A2"}"#,
        ]),
        &mut |_| {},
        &mut |_, path| {
            if path == "cards/a.md" {
                Err(io::Error::other("disk full"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    match &err {
        RunError::PartiallyApplied {
            written,
            failed,
            reason,
        } => {
            assert_eq!(written, &["cards/fix-login.md"]);
            assert_eq!(failed, "cards/a.md");
            assert!(reason.contains("disk full"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn failure_before_any_write_stays_a_plain_error() {
    let dir = temp_board();
    let err = fs::run_with_hooks(
        dir.path(),
        &cmds(&[r#"{"cmd":"card.edit","id":"a","title":"A2"}"#]),
        &mut |_| {},
        &mut |_, _| Err(io::Error::other("nope")),
    )
    .unwrap_err();
    assert!(
        matches!(err, RunError::Apply(ApplyError::Io { .. })),
        "{err:?}"
    );
}

// ---- locking ----

#[test]
fn run_waits_for_the_board_lock() {
    let dir = temp_board();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.path().join(".markban.lock"))
        .unwrap();
    lock.lock().unwrap();
    let path = dir.path().to_path_buf();
    let handle = std::thread::spawn(move || {
        fs::run(&path, &cmds(&[r#"{"cmd":"card.remove","id":"a"}"#])).unwrap();
    });
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(exists(dir.path(), "cards/a.md"), "run ignored the lock");
    drop(lock);
    handle.join().unwrap();
    assert!(!exists(dir.path(), "cards/a.md"));
}

#[test]
fn run_gives_up_with_a_contention_error_when_the_lock_stays_held() {
    let dir = temp_board();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.path().join(".markban.lock"))
        .unwrap();
    lock.lock().unwrap();
    let err = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.remove","id":"a"}"#])).unwrap_err();
    assert!(matches!(err, RunError::Locked { .. }), "{err:?}");
    assert!(!err.to_string().contains('\n'));
    assert!(exists(dir.path(), "cards/a.md"));
}

#[test]
fn concurrent_runs_are_serialised_without_lost_updates() {
    let dir = temp_board();
    let handles: Vec<_> = (0..6)
        .map(|n| {
            let path = dir.path().to_path_buf();
            std::thread::spawn(move || {
                let column = ["doing", "done"][n % 2];
                fs::run(
                    &path,
                    &cmds(&[
                        &format!(r#"{{"cmd":"card.add","title":"Same","description":"w{n}"}}"#),
                        &format!(r#"{{"cmd":"card.move","id":"a","column":"{column}"}}"#),
                    ]),
                )
                .map(|o| o[0].id.clone())
            })
        })
        .collect();
    let mut ids: Vec<String> = handles
        .into_iter()
        .map(|h| h.join().unwrap().unwrap())
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        ["same", "same-2", "same-3", "same-4", "same-5", "same-6"]
    );
    assert_eq!(
        fs::load(dir.path())
            .unwrap()
            .1
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count(),
        0
    );
}

#[test]
fn lock_file_is_not_reported_by_load() {
    let dir = temp_board();
    fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.remove","id":"a"}"#])).unwrap();
    assert!(exists(dir.path(), ".markban.lock"));
    assert!(fs::load(dir.path()).unwrap().1.is_empty());
}

// ---- entries the loader could not read ----

#[test]
fn card_add_avoids_a_name_taken_by_a_non_utf8_file() {
    let dir = temp_board();
    std::fs::write(dir.path().join("cards/fix.md"), [0xff, 0xfe, b'\n']).unwrap();
    let out = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.add","title":"Fix"}"#])).unwrap();
    assert_eq!(out[0].id, "fix-2");
    assert_eq!(
        std::fs::read(dir.path().join("cards/fix.md")).unwrap(),
        [0xff, 0xfe, b'\n']
    );
}

#[test]
fn card_add_avoids_a_name_taken_by_a_directory() {
    let dir = temp_board();
    std::fs::create_dir(dir.path().join("cards/fix.md")).unwrap();
    let out = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.add","title":"Fix"}"#])).unwrap();
    assert_eq!(out[0].id, "fix-2");
}

#[test]
fn id_taken_covers_loaded_cards_and_unreadable_entries() {
    let dir = temp_board();
    std::fs::write(dir.path().join("cards/bad.md"), [0xff]).unwrap();
    let board = loaded(dir.path());
    assert!(board.id_taken("a"));
    assert!(board.id_taken("bad"));
    assert!(!board.id_taken("free"));
}

#[test]
fn explicit_id_of_an_unreadable_entry_is_refused_without_contention() {
    let dir = temp_board();
    std::fs::write(dir.path().join("cards/bad.md"), [0xff]).unwrap();
    let err = fs::run(
        dir.path(),
        &cmds(&[r#"{"cmd":"card.add","title":"X","id":"bad"}"#]),
    )
    .unwrap_err();
    assert!(!matches!(err, RunError::Contended { .. }), "{err:?}");
}

#[test]
fn creating_over_an_entry_that_existed_at_load_is_a_plain_error() {
    let dir = temp_board();
    std::fs::write(dir.path().join("cards/bad.md"), [0xff]).unwrap();
    let before = loaded(dir.path());
    let mut after = before.clone();
    let card = markban_core::board::CardFile::parse("bad", "---\ntitle: T\ncolumn: todo\n---\n");
    after.cards.insert("bad".into(), card);
    let err = fs::apply(dir.path(), &before, &after).unwrap_err();
    assert!(matches!(err, ApplyError::Exists { .. }), "{err:?}");
    assert!(!err.to_string().contains('\n'));
}

#[test]
fn init_refuses_an_existing_non_empty_cards_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("cards")).unwrap();
    std::fs::write(dir.path().join("cards/old.md"), "x\n").unwrap();
    let err = fs::run(
        dir.path(),
        &cmds(&[r#"{"cmd":"board.init","title":"T","columns":["A"]}"#]),
    )
    .unwrap_err();
    assert!(
        matches!(err, RunError::Apply(ApplyError::Exists { .. })),
        "{err:?}"
    );
    assert!(!exists(dir.path(), "board.md"));
}

#[test]
fn init_accepts_an_existing_empty_cards_dir() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join("cards")).unwrap();
    fs::run(
        dir.path(),
        &cmds(&[r#"{"cmd":"board.init","title":"T","columns":["A"]}"#]),
    )
    .unwrap();
    assert!(exists(dir.path(), "board.md"));
}

// ---- vanishing entries and symlinks ----

#[cfg(unix)]
#[test]
fn symlinked_card_is_not_read_and_reported_as_w001() {
    let dir = temp_board();
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::write(
        outside.path().join("x.md"),
        "---\ntitle: X\ncolumn: todo\n---\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(outside.path().join("x.md"), dir.path().join("cards/x.md")).unwrap();
    let (board, diags) = fs::load(dir.path()).unwrap();
    let board = board.unwrap();
    assert!(!board.cards.contains_key("x"));
    assert!(board.id_taken("x"));
    let w: Vec<_> = diags
        .iter()
        .filter(|d| d.code == Code::W001 && d.file == "cards/x.md")
        .collect();
    assert_eq!(w.len(), 1, "{diags:?}");
    assert!(
        w[0].message.contains("symbolic link ignored"),
        "{}",
        w[0].message
    );
}

#[cfg(unix)]
#[test]
fn symlinked_board_md_is_not_read() {
    let dir = temp_board();
    let real = dir.path().join("real-board.md");
    std::fs::rename(dir.path().join("board.md"), &real).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join("board.md")).unwrap();
    let (board, diags) = fs::load(dir.path()).unwrap();
    assert!(board.is_none());
    assert!(
        diags.iter().any(|d| d.code == Code::W001
            && d.file == "board.md"
            && d.message.contains("symbolic link ignored")),
        "{diags:?}"
    );
}

// ---- a symlinked cards/ directory ----

#[cfg(unix)]
#[test]
fn symlinked_cards_dir_is_not_read_and_reported_as_w001() {
    let dir = temp_board();
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::write(
        outside.path().join("secret.md"),
        "---\ntitle: S\ncolumn: todo\n---\n",
    )
    .unwrap();
    std::fs::remove_dir_all(dir.path().join("cards")).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("cards")).unwrap();
    let (board, diags) = fs::load(dir.path()).unwrap();
    assert!(board.unwrap().cards.is_empty());
    assert!(
        diags.iter().any(|d| d.code == Code::W001
            && d.file == "cards"
            && d.message.contains("symbolic link")),
        "{diags:?}"
    );
}

#[cfg(unix)]
#[test]
fn writes_into_a_symlinked_cards_dir_are_refused() {
    let dir = temp_board();
    let outside = tempfile::TempDir::new().unwrap();
    std::fs::remove_dir_all(dir.path().join("cards")).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("cards")).unwrap();
    let err = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.add","title":"Leak"}"#])).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn init_into_a_symlinked_cards_dir_is_refused() {
    let dir = tempfile::TempDir::new().unwrap();
    let outside = tempfile::TempDir::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("cards")).unwrap();
    let err = fs::run(
        dir.path(),
        &cmds(&[r#"{"cmd":"board.init","title":"T","columns":["A"]}"#]),
    )
    .unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_symlinked_lock_file_is_refused() {
    let dir = temp_board();
    let outside = tempfile::TempDir::new().unwrap();
    let target = outside.path().join("victim");
    std::os::unix::fs::symlink(&target, dir.path().join(".markban.lock")).unwrap();
    let err = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.remove","id":"a"}"#])).unwrap_err();
    assert!(err.to_string().contains("symbolic link"), "{err}");
    assert!(!target.exists());
}

// ---- unreadable cards ----

#[cfg(unix)]
#[test]
fn one_unreadable_card_does_not_fail_the_load() {
    use std::os::unix::fs::PermissionsExt;
    let dir = temp_board();
    let path = dir.path().join("cards/a.md");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        return; // running as root: permissions do not apply
    }
    let (board, diags) = fs::load(dir.path()).unwrap();
    let board = board.unwrap();
    assert!(!board.cards.contains_key("a") && board.cards.contains_key("b"));
    assert!(board.id_taken("a"));
    assert!(
        diags.iter().any(|d| d.code == Code::E009
            && d.file == "cards/a.md"
            && d.message.starts_with("cannot read file: ")),
        "{diags:?}"
    );
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
}

// ---- the lock file only belongs in boards ----

#[test]
fn run_in_a_directory_without_a_board_leaves_no_lock() {
    let dir = tempfile::TempDir::new().unwrap();
    let err = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.remove","id":"a"}"#])).unwrap_err();
    assert!(matches!(err, RunError::NoBoard { .. }), "{err:?}");
    assert!(!exists(dir.path(), ".markban.lock"));
}

#[test]
fn failed_init_leaves_nothing_behind() {
    let dir = tempfile::TempDir::new().unwrap();
    let missing = dir.path().join("new-board");
    let bad = cmds(&[r#"{"cmd":"board.init","title":"T","columns":[]}"#]);
    assert!(fs::run(&missing, &bad).is_err());
    assert!(!missing.exists());
    assert!(fs::run(dir.path(), &bad).is_err());
    assert!(!exists(dir.path(), ".markban.lock"));
    // Init is fine but a later command fails: still nothing behind.
    let later = cmds(&[
        r#"{"cmd":"board.init","title":"T","columns":["A"]}"#,
        r#"{"cmd":"card.remove","id":"nope"}"#,
    ]);
    assert!(fs::run(&missing, &later).is_err());
    assert!(!missing.exists());
    assert!(fs::run(dir.path(), &later).is_err());
    assert!(!exists(dir.path(), ".markban.lock") && !exists(dir.path(), "board.md"));
}

#[test]
fn successful_init_still_works_and_locks() {
    let dir = tempfile::TempDir::new().unwrap();
    let board = dir.path().join("new-board");
    fs::run(
        &board,
        &cmds(&[r#"{"cmd":"board.init","title":"T","columns":["A"]}"#]),
    )
    .unwrap();
    assert!(exists(&board, "board.md"));
}

// ---- names that differ only in case ----

#[test]
fn card_add_avoids_a_name_that_differs_only_in_case() {
    let dir = temp_board();
    std::fs::write(dir.path().join("cards/FOO.md"), [0xff, 0xfe, b'\n']).unwrap();
    let out = fs::run(dir.path(), &cmds(&[r#"{"cmd":"card.add","title":"Foo"}"#])).unwrap();
    assert_eq!(out[0].id, "foo-2");
    let err = fs::run(
        dir.path(),
        &cmds(&[r#"{"cmd":"card.add","title":"x","id":"foo"}"#]),
    )
    .unwrap_err();
    assert!(matches!(err, RunError::Command { .. }), "{err:?}");
}

#[test]
fn creating_a_case_variant_of_an_existing_name_is_exists_not_conflict() {
    let dir = temp_board();
    let clean = loaded(dir.path());
    let mut after = clean.clone();
    execute_batch(
        &mut after,
        &cmds(&[r#"{"cmd":"card.add","title":"x","id":"foo"}"#]),
    )
    .unwrap();
    std::fs::write(dir.path().join("cards/FOO.md"), [0xff, 0xfe, b'\n']).unwrap();
    let before = loaded(dir.path());
    let err = fs::apply(dir.path(), &before, &after).unwrap_err();
    assert!(matches!(err, ApplyError::Exists { .. }), "{err:?}");
    assert!(!exists(dir.path(), "cards/foo.md"));
}
