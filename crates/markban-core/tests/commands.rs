#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{changed_lines, cmd, column_ids, exists, read, temp_board};
use markban_core::command::{Command, CommandError, execute, execute_batch};
use markban_core::fs;
use markban_core::order::{self, Decimal};
use tempfile::TempDir;

fn run_one(dir: &std::path::Path, json: &str) -> String {
    fs::run(dir, &[cmd(json)]).unwrap().remove(0).id
}

fn run_err(dir: &std::path::Path, json: &str) -> String {
    match fs::run(dir, &[cmd(json)]) {
        Err(e) => e.to_string(),
        Ok(o) => panic!("expected an error, got {o:?}"),
    }
}

fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

// ---- JSON model ----

#[test]
fn every_command_parses_from_json() {
    let lines = [
        r#"{"cmd":"board.init","title":"T","columns":["A","B"]}"#,
        r#"{"cmd":"card.add","title":"T","column":"A","description":"d","id":"x","before":"y"}"#,
        r#"{"cmd":"card.move","id":"x","column":"A","top":true}"#,
        r#"{"cmd":"card.edit","id":"x","title":"T","description":"d","append":"a"}"#,
        r#"{"cmd":"card.remove","id":"x"}"#,
        r#"{"cmd":"column.add","name":"N","id":"n","before":"A"}"#,
        r#"{"cmd":"column.rename","column":"A","name":"N"}"#,
        r#"{"cmd":"column.move","column":"A","before":"B"}"#,
        r#"{"cmd":"column.remove","column":"A","move_to":"B"}"#,
    ];
    for line in lines {
        let c = Command::parse_line(line).unwrap();
        let again = Command::parse_line(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(c, again, "{line}");
    }
}

#[test]
fn unknown_fields_and_commands_are_rejected_with_one_line() {
    for line in [
        r#"{"cmd":"card.add","title":"T","colour":"red"}"#,
        r#"{"cmd":"card.fly","id":"x"}"#,
        r#"{"title":"T"}"#,
        r#"{"cmd":"card.remove"}"#,
        "not json",
    ] {
        let e = Command::parse_line(line).unwrap_err().to_string();
        assert!(!e.contains('\n') && !e.is_empty(), "{line}: {e}");
    }
}

// ---- card.add ----

#[test]
fn add_to_first_column_end_with_description() {
    let dir = temp_board();
    let id = run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"New: thing","description":"Hello\nworld"}"#,
    );
    assert_eq!(id, "new-thing");
    let expected_order = order::end("new-thing", Some(&dec("3000"))).unwrap();
    assert_eq!(
        read(dir.path(), "cards/new-thing.md"),
        format!(
            "---\ntitle: \"New: thing\"\ncolumn: todo\norder: {expected_order}\n---\n\nHello\nworld\n"
        )
    );
    assert_eq!(column_ids(dir.path(), "todo"), ["a", "b", "c", "new-thing"]);
}

#[test]
fn add_to_empty_column_gets_order_1000_and_no_body() {
    let dir = temp_board();
    run_one(dir.path(), r#"{"cmd":"column.add","name":"Review"}"#);
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"R","column":"review"}"#,
    );
    assert_eq!(
        read(dir.path(), "cards/r.md"),
        "---\ntitle: R\ncolumn: review\norder: 1000\n---\n"
    );
}

#[test]
fn add_id_collision_gets_numeric_suffix_and_leaves_existing_alone() {
    let dir = temp_board();
    let before = read(dir.path(), "cards/a.md");
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"card.add","title":"A"}"#),
        "a-2"
    );
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"card.add","title":"A"}"#),
        "a-3"
    );
    assert_eq!(read(dir.path(), "cards/a.md"), before);
}

#[test]
fn add_with_explicit_id_validates_and_refuses_collisions() {
    let dir = temp_board();
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"card.add","title":"X","id":"my-id"}"#),
        "my-id"
    );
    let e = run_err(dir.path(), r#"{"cmd":"card.add","title":"X","id":"a"}"#);
    assert!(e.contains("'a'") && e.contains("exists"), "{e}");
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.add","title":"X","id":"Bad_Id"}"#,
    );
    assert!(e.contains("valid id"), "{e}");
}

#[test]
fn add_rejects_empty_or_multiline_titles_and_unknown_columns() {
    let dir = temp_board();
    assert!(run_err(dir.path(), r#"{"cmd":"card.add","title":"  "}"#).contains("title"));
    assert!(run_err(dir.path(), r#"{"cmd":"card.add","title":"a\nb"}"#).contains("title"));
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.add","title":"x","column":"nope"}"#,
    );
    assert!(
        e.contains("'nope'") && e.contains("todo") && e.contains("done"),
        "{e}"
    );
    assert!(!exists(dir.path(), "cards/x.md"));
}

#[test]
fn add_positions_top_before_after() {
    let dir = temp_board();
    run_one(dir.path(), r#"{"cmd":"card.add","title":"Top","top":true}"#);
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Bef","before":"b"}"#,
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Aft","after":"b"}"#,
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Last","after":"c"}"#,
    );
    assert_eq!(
        column_ids(dir.path(), "todo"),
        ["top", "a", "bef", "b", "aft", "c", "last"]
    );
}

#[test]
fn conflicting_positions_are_refused() {
    let dir = temp_board();
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.add","title":"x","top":true,"before":"a"}"#,
    );
    assert!(e.contains("only one") && e.contains("top"), "{e}");
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.move","id":"a","before":"b","after":"c"}"#,
    );
    assert!(e.contains("only one"), "{e}");
}

#[test]
fn two_writers_adding_into_the_same_gap_get_different_orders() {
    let dir = temp_board();
    let other = temp_board();
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"One","after":"a"}"#,
    );
    run_one(
        other.path(),
        r#"{"cmd":"card.add","title":"Two","after":"a"}"#,
    );
    let o1 = read(dir.path(), "cards/one.md");
    let o2 = read(other.path(), "cards/two.md");
    let line = |t: &str| {
        t.lines()
            .find(|l| l.starts_with("order:"))
            .unwrap()
            .to_string()
    };
    assert_ne!(line(&o1), line(&o2));
}

#[test]
fn add_anchor_errors() {
    let dir = temp_board();
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.add","title":"x","before":"zzz"}"#,
    );
    assert!(e.contains("'zzz'"), "{e}");
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.add","title":"x","column":"todo","before":"d"}"#,
    );
    assert!(e.contains("'d'") && e.contains("doing"), "{e}");
    let e = run_err(
        dir.path(),
        r#"{"cmd":"card.add","title":"x","column":"done","before":"e"}"#,
    );
    assert!(e.contains("'e'") && e.contains("order"), "{e}");
}

// ---- card.move ----

#[test]
fn move_to_column_rewrites_only_column_and_order() {
    let dir = temp_board();
    let before = read(dir.path(), "cards/b.md");
    assert_eq!(
        run_one(
            dir.path(),
            r#"{"cmd":"card.move","id":"b","column":"Doing"}"#
        ),
        "b"
    );
    let after = read(dir.path(), "cards/b.md");
    let changed = changed_lines(&before, &after);
    assert_eq!(changed.len(), 2, "{changed:?}");
    assert_eq!(changed[0], ("column: todo".into(), "column: doing".into()));
    let expected = order::end("b", Some(&dec("1000"))).unwrap();
    assert_eq!(
        changed[1],
        ("order: 2000".into(), format!("order: {expected}"))
    );
}

#[test]
fn move_within_column_changes_only_the_order_line() {
    let dir = temp_board();
    let before = read(dir.path(), "cards/c.md");
    run_one(dir.path(), r#"{"cmd":"card.move","id":"c","top":true}"#);
    let after = read(dir.path(), "cards/c.md");
    let changed = changed_lines(&before, &after);
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(changed[0].1.starts_with("order: "));
    assert!(after.contains("labels: x") && after.ends_with("no final newline."));
    assert_eq!(column_ids(dir.path(), "todo"), ["c", "a", "b"]);
}

#[test]
fn move_before_and_after_anchors() {
    let dir = temp_board();
    run_one(dir.path(), r#"{"cmd":"card.move","id":"c","before":"b"}"#);
    assert_eq!(column_ids(dir.path(), "todo"), ["a", "c", "b"]);
    run_one(dir.path(), r#"{"cmd":"card.move","id":"a","after":"b"}"#);
    assert_eq!(column_ids(dir.path(), "todo"), ["c", "b", "a"]);
}

#[test]
fn move_adds_order_to_a_card_without_one() {
    let dir = temp_board();
    run_one(dir.path(), r#"{"cmd":"card.move","id":"e","top":true}"#);
    assert_eq!(
        read(dir.path(), "cards/e.md"),
        "---\ntitle: Epsilon\ncolumn: done\norder: 1000\n---\n"
    );
}

#[test]
fn move_errors() {
    let dir = temp_board();
    assert!(run_err(dir.path(), r#"{"cmd":"card.move","id":"zzz","top":true}"#).contains("'zzz'"));
    assert!(run_err(dir.path(), r#"{"cmd":"card.move","id":"a"}"#).contains("column"));
    assert!(run_err(dir.path(), r#"{"cmd":"card.move","id":"a","before":"a"}"#).contains("itself"));
    assert!(
        run_err(dir.path(), r#"{"cmd":"card.move","id":"a","column":"x"}"#)
            .contains("valid columns")
    );
}

// ---- card.edit / card.remove ----

#[test]
fn edit_title_changes_only_the_title_line_never_the_id() {
    let dir = temp_board();
    let before = read(dir.path(), "cards/a.md");
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"a","title":"Renamed"}"#,
    );
    let changed = changed_lines(&before, &read(dir.path(), "cards/a.md"));
    assert_eq!(
        changed,
        [("title: Alpha".to_string(), "title: Renamed".to_string())]
    );
    assert!(!exists(dir.path(), "cards/renamed.md"));
}

#[test]
fn edit_description_replaces_the_body() {
    let dir = temp_board();
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"a","description":"New body"}"#,
    );
    assert_eq!(
        read(dir.path(), "cards/a.md"),
        "---\ntitle: Alpha\ncolumn: todo\norder: 1000\n---\n\nNew body\n"
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"a","description":""}"#,
    );
    assert_eq!(
        read(dir.path(), "cards/a.md"),
        "---\ntitle: Alpha\ncolumn: todo\norder: 1000\n---\n"
    );
}

#[test]
fn edit_append_adds_a_line_with_newline_separation() {
    let dir = temp_board();
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"a","append":"- more"}"#,
    );
    assert!(read(dir.path(), "cards/a.md").ends_with("---\n\nAlpha body.\n- more\n"));
    // c has no final newline
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"c","append":"tail"}"#,
    );
    assert!(read(dir.path(), "cards/c.md").ends_with("no final newline.\ntail\n"));
    // b has no body at all
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"b","append":"first"}"#,
    );
    assert_eq!(
        read(dir.path(), "cards/b.md"),
        "---\ntitle: Beta\ncolumn: todo\norder: 2000\n---\n\nfirst\n"
    );
}

#[test]
fn edit_needs_something_to_change_and_a_known_card() {
    let dir = temp_board();
    assert!(run_err(dir.path(), r#"{"cmd":"card.edit","id":"a"}"#).contains("nothing"));
    assert!(run_err(dir.path(), r#"{"cmd":"card.edit","id":"q","title":"x"}"#).contains("'q'"));
    assert!(run_err(dir.path(), r#"{"cmd":"card.edit","id":"a","title":""}"#).contains("title"));
}

#[test]
fn remove_deletes_only_that_file() {
    let dir = temp_board();
    let b = read(dir.path(), "cards/b.md");
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"card.remove","id":"a"}"#),
        "a"
    );
    assert!(!exists(dir.path(), "cards/a.md"));
    assert_eq!(read(dir.path(), "cards/b.md"), b);
    assert!(run_err(dir.path(), r#"{"cmd":"card.remove","id":"a"}"#).contains("'a'"));
}

// ---- columns ----

#[test]
fn column_add_end_and_before_with_slug_ids() {
    let dir = temp_board();
    let board = read(dir.path(), "board.md");
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"column.add","name":"In Review!"}"#),
        "in-review"
    );
    assert_eq!(
        read(dir.path(), "board.md"),
        board.replace(
            "    name: Done\n",
            "    name: Done\n  - id: in-review\n    name: In Review!\n"
        )
    );
    assert_eq!(
        run_one(
            dir.path(),
            r#"{"cmd":"column.add","name":"Doing","before":"done"}"#
        ),
        "doing-2"
    );
    assert!(
        read(dir.path(), "board.md").contains("  - id: doing-2\n    name: Doing\n  - id: done\n")
    );
    assert_eq!(
        run_one(
            dir.path(),
            r#"{"cmd":"column.add","name":"X","id":"first","before":"TODO"}"#
        ),
        "first"
    );
    assert!(
        run_err(
            dir.path(),
            r#"{"cmd":"column.add","name":"X","id":"first"}"#
        )
        .contains("first")
    );
}

#[test]
fn column_rename_changes_only_the_name_line() {
    let dir = temp_board();
    let before = read(dir.path(), "board.md");
    assert_eq!(
        run_one(
            dir.path(),
            r#"{"cmd":"column.rename","column":"to do","name":"Backlog"}"#
        ),
        "todo"
    );
    let changed = changed_lines(&before, &read(dir.path(), "board.md"));
    assert_eq!(
        changed,
        [(
            "    name: To do".to_string(),
            "    name: Backlog".to_string()
        )]
    );
    assert_eq!(
        read(dir.path(), "cards/a.md"),
        read(temp_board().path(), "cards/a.md")
    );
}

#[test]
fn column_move_before_and_to_end() {
    let dir = temp_board();
    run_one(
        dir.path(),
        r#"{"cmd":"column.move","column":"done","before":"todo"}"#,
    );
    assert!(read(dir.path(), "board.md").contains(
        "columns:\n  - id: done\n    name: Done\n  - id: todo\n    name: To do\n  - id: doing\n"
    ));
    run_one(dir.path(), r#"{"cmd":"column.move","column":"done"}"#);
    assert!(read(dir.path(), "board.md").ends_with(
        "columns:\n  - id: todo\n    name: To do\n  - id: doing\n    name: Doing\n  - id: done\n    name: Done\n---\n\nFixture description.\n"
    ));
}

#[test]
fn column_remove_refuses_while_cards_refer_to_it() {
    let dir = temp_board();
    let e = run_err(dir.path(), r#"{"cmd":"column.remove","column":"todo"}"#);
    assert!(e.contains("3 card") && e.contains("move_to"), "{e}");
    assert!(read(dir.path(), "board.md").contains("id: todo"));
}

#[test]
fn column_remove_with_move_to_appends_cards_in_relative_order() {
    let dir = temp_board();
    assert_eq!(
        run_one(
            dir.path(),
            r#"{"cmd":"column.remove","column":"todo","move_to":"Doing"}"#
        ),
        "todo"
    );
    assert!(!read(dir.path(), "board.md").contains("id: todo"));
    assert_eq!(column_ids(dir.path(), "doing"), ["d", "a", "b", "c"]);
    assert!(read(dir.path(), "cards/a.md").contains("column: doing"));
}

#[test]
fn column_remove_of_an_empty_column_and_self_move_to() {
    let dir = temp_board();
    run_one(dir.path(), r#"{"cmd":"card.remove","id":"d"}"#);
    run_one(dir.path(), r#"{"cmd":"column.remove","column":"doing"}"#);
    assert!(!read(dir.path(), "board.md").contains("id: doing"));
    let e = run_err(
        dir.path(),
        r#"{"cmd":"column.remove","column":"todo","move_to":"todo"}"#,
    );
    assert!(e.contains("itself") || e.contains("same"), "{e}");
}

#[test]
fn columns_resolve_by_id_or_name_case_insensitively_and_errors_list_valid_ones() {
    let dir = temp_board();
    run_one(
        dir.path(),
        r#"{"cmd":"card.move","id":"a","column":"DONE"}"#,
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.move","id":"a","column":"to do"}"#,
    );
    let e = run_err(dir.path(), r#"{"cmd":"card.move","id":"a","column":"to"}"#);
    assert!(
        e.contains("'to'") && e.contains("todo") && e.contains("To do") && e.contains("Done"),
        "{e}"
    );
    assert!(!e.contains('\n'));
}

// ---- board.init ----

#[test]
fn init_creates_board_md_with_deduplicated_slug_ids() {
    let parent = TempDir::new().unwrap();
    let dir = parent.path().join("new-board");
    let id = run_one(
        &dir,
        r#"{"cmd":"board.init","title":"My: board","columns":["To do","Done","done","日本"]}"#,
    );
    assert_eq!(id, "My: board");
    assert_eq!(
        read(&dir, "board.md"),
        "---\nmarkban: 0.1\ntitle: \"My: board\"\ncolumns:\n  - id: to-do\n    name: To do\n  - id: done\n    name: Done\n  - id: done-2\n    name: done\n  - id: card\n    name: 日本\n---\n"
    );
    let (board, diags) = fs::load(&dir).unwrap();
    assert!(board.is_some() && diags.is_empty(), "{diags:?}");
}

#[test]
fn init_followed_by_commands_in_one_batch_and_refusals() {
    let parent = TempDir::new().unwrap();
    let dir = parent.path().join("b");
    let out = fs::run(
        &dir,
        &[
            cmd(r#"{"cmd":"board.init","title":"B","columns":["Todo"]}"#),
            cmd(r#"{"cmd":"card.add","title":"First"}"#),
        ],
    )
    .unwrap();
    assert_eq!(out[1].id, "first");
    assert_eq!(
        read(&dir, "cards/first.md"),
        "---\ntitle: First\ncolumn: todo\norder: 1000\n---\n"
    );
    // second init on an existing board
    let e = run_err(&dir, r#"{"cmd":"board.init","title":"B","columns":["x"]}"#);
    assert!(e.contains("already"), "{e}");
    // no board and no init
    let none = parent.path().join("none");
    std::fs::create_dir_all(&none).unwrap();
    let e = run_err(&none, r#"{"cmd":"card.add","title":"x"}"#);
    assert!(e.contains("board.init"), "{e}");
    // bad init params
    let e = run_err(
        &parent.path().join("c"),
        r#"{"cmd":"board.init","title":"","columns":["x"]}"#,
    );
    assert!(e.contains("title"), "{e}");
    let e = run_err(
        &parent.path().join("d"),
        r#"{"cmd":"board.init","title":"T","columns":[]}"#,
    );
    assert!(e.contains("column"), "{e}");
}

// ---- batch ----

#[test]
fn batch_is_all_or_nothing_in_memory_and_on_disk() {
    let dir = temp_board();
    let (board, _) = fs::load(dir.path()).unwrap();
    let mut board = board.unwrap();
    let original = board.clone();
    let cmds = [
        cmd(r#"{"cmd":"card.add","title":"Ok"}"#),
        cmd(r#"{"cmd":"card.remove","id":"nope"}"#),
    ];
    let (index, err) = execute_batch(&mut board, &cmds).unwrap_err();
    assert_eq!(index, 1);
    assert!(matches!(err, CommandError::UnknownCard(_)), "{err:?}");
    assert_eq!(board, original);

    let err = fs::run(dir.path(), &cmds).unwrap_err().to_string();
    assert!(
        err.contains("command 2") || err.contains("#2") || err.contains("2:"),
        "{err}"
    );
    assert!(!exists(dir.path(), "cards/ok.md"));
}

#[test]
fn batch_applies_commands_in_order_and_reports_outcomes() {
    let dir = temp_board();
    let out = fs::run(
        dir.path(),
        &[
            cmd(r#"{"cmd":"card.add","title":"One"}"#),
            cmd(r#"{"cmd":"card.add","title":"One"}"#),
            cmd(r#"{"cmd":"card.move","id":"one-2","top":true}"#),
        ],
    )
    .unwrap();
    let ids: Vec<_> = out.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(ids, ["one", "one-2", "one-2"]);
    assert_eq!(
        column_ids(dir.path(), "todo"),
        ["one-2", "a", "b", "c", "one"]
    );
}

#[test]
fn execute_is_deterministic() {
    let run = || {
        let dir = temp_board();
        let (board, _) = fs::load(dir.path()).unwrap();
        let mut board = board.unwrap();
        execute(
            &mut board,
            &cmd(r#"{"cmd":"card.add","title":"Same","after":"a"}"#),
        )
        .unwrap();
        board.files()
    };
    assert_eq!(run(), run());
}

#[test]
fn commands_on_a_board_with_existing_errors_do_not_add_new_ones() {
    // card x has an unknown column (E007 already present); adding a card still works.
    let dir = temp_board();
    std::fs::write(
        dir.path().join("cards/x.md"),
        "---\ntitle: X\ncolumn: gone\n---\n",
    )
    .unwrap();
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"card.add","title":"Fine"}"#),
        "fine"
    );
}

// ---- review fixes ----

#[test]
fn edit_refuses_a_card_without_valid_frontmatter_and_leaves_it_alone() {
    let dir = temp_board();
    let raw = "# Not a card\nno frontmatter here\n";
    std::fs::write(dir.path().join("cards/x.md"), raw).unwrap();
    for json in [
        r#"{"cmd":"card.edit","id":"x","description":"new"}"#,
        r#"{"cmd":"card.edit","id":"x","append":"more"}"#,
        r#"{"cmd":"card.edit","id":"x","title":"T"}"#,
    ] {
        let err = run_err(dir.path(), json);
        assert!(
            err.contains("card 'x' has no valid frontmatter (E006); fix the file by hand first"),
            "{json}: {err}"
        );
        assert_eq!(read(dir.path(), "cards/x.md"), raw, "{json}");
    }
}

#[test]
fn column_add_repairs_an_existing_unknown_column_error() {
    let dir = temp_board();
    std::fs::write(
        dir.path().join("cards/x.md"),
        "---\ntitle: X\ncolumn: gone\n---\n",
    )
    .unwrap();
    // Unrelated column commands keep working while the E007 exists ...
    assert_eq!(
        run_one(dir.path(), r#"{"cmd":"column.add","name":"Review"}"#),
        "review"
    );
    run_one(dir.path(), r#"{"cmd":"column.move","column":"review"}"#);
    run_one(dir.path(), r#"{"cmd":"column.remove","column":"review"}"#);
    // ... and the repair itself succeeds.
    assert_eq!(
        run_one(
            dir.path(),
            r#"{"cmd":"column.add","name":"Gone","id":"gone"}"#
        ),
        "gone"
    );
    let (board, diags) = fs::load(dir.path()).unwrap();
    assert!(board.is_some());
    assert!(diags.is_empty(), "{diags:?}");
}

#[test]
fn end_with_high_precision_orders_in_the_column() {
    let dir = temp_board();
    std::fs::write(
        dir.path().join("cards/hp.md"),
        "---\ntitle: HP\ncolumn: doing\norder: 1500.1234567\n---\n",
    )
    .unwrap();
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"End","column":"doing"}"#,
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Top","column":"doing","top":true}"#,
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Mid","column":"doing","after":"d"}"#,
    );
    assert_eq!(
        column_ids(dir.path(), "doing"),
        ["top", "d", "mid", "hp", "end"]
    );
}

#[test]
fn between_high_precision_neighbours_is_strictly_between() {
    let dir = temp_board();
    std::fs::write(
        dir.path().join("cards/hp.md"),
        "---\ntitle: HP\ncolumn: doing\norder: 1500.1234567\n---\n",
    )
    .unwrap();
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Mid","column":"doing","before":"hp"}"#,
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Mid2","column":"doing","after":"mid"}"#,
    );
    assert_eq!(column_ids(dir.path(), "doing"), ["d", "mid", "mid2", "hp"]);
}

#[test]
fn end_in_a_column_with_unordered_cards_writes_no_order() {
    // column `done` holds e, which has no order and sorts after every ordered card.
    let dir = temp_board();
    run_one(
        dir.path(),
        r#"{"cmd":"card.add","title":"Zeta","column":"done"}"#,
    );
    assert!(!read(dir.path(), "cards/zeta.md").contains("order"));
    assert_eq!(column_ids(dir.path(), "done"), ["e", "zeta"]);
    // Moving an ordered card to the end of that column drops its order too.
    run_one(
        dir.path(),
        r#"{"cmd":"card.move","id":"d","column":"done"}"#,
    );
    assert!(!read(dir.path(), "cards/d.md").contains("order"));
    assert_eq!(column_ids(dir.path(), "done"), ["d", "e", "zeta"]);
}

#[test]
fn crlf_cards_keep_crlf_when_edited() {
    let dir = temp_board();
    std::fs::write(
        dir.path().join("cards/w.md"),
        "---\r\ntitle: W\r\ncolumn: todo\r\norder: 9000\r\n---\r\n\r\nBody\r\n",
    )
    .unwrap();
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"w","append":"more"}"#,
    );
    assert_eq!(
        read(dir.path(), "cards/w.md"),
        "---\r\ntitle: W\r\ncolumn: todo\r\norder: 9000\r\n---\r\n\r\nBody\r\nmore\r\n"
    );
    run_one(
        dir.path(),
        r#"{"cmd":"card.edit","id":"w","description":"one\ntwo"}"#,
    );
    assert_eq!(
        read(dir.path(), "cards/w.md"),
        "---\r\ntitle: W\r\ncolumn: todo\r\norder: 9000\r\n---\r\n\r\none\r\ntwo\r\n"
    );
}
