#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::board::Board;
use markban_core::order::Decimal;
use markban_core::{slug, view};

fn board_with(cards: &[(&str, &str)]) -> Board {
    let board = b"---\nmarkban: 0.1\ntitle: My board\ncolumns:\n  - id: a\n    name: Alpha\n  - id: b\n    name: Beta\n---\n\nSome *text*.\n";
    let cards = cards
        .iter()
        .map(|(id, fm)| (format!("{id}.md"), format!("---\n{fm}\n---\n").into_bytes()))
        .collect();
    Board::parse(board, cards, vec![]).0.unwrap()
}

fn ids(v: &[markban_core::view::CardSummary]) -> Vec<&str> {
    v.iter().map(|c| c.id.as_str()).collect()
}

#[test]
fn orders_compare_as_decimals() {
    let b = board_with(&[
        ("c1", "title: C1\ncolumn: a\norder: 10"),
        ("c2", "title: C2\ncolumn: a\norder: 9"),
        ("c3", "title: C3\ncolumn: a\norder: -5"),
        ("c4", "title: C4\ncolumn: a\norder: 9.5"),
        ("c5", "title: C5\ncolumn: a\norder: -5.25"),
        ("c6", "title: C6\ncolumn: a\norder: 1500.25"),
        ("c7", "title: C7\ncolumn: a\norder: 0010"),
    ]);
    let v = view(&b);
    let col = v.columns.first().unwrap();
    assert_eq!(col.id, "a");
    assert_eq!(col.name, "Alpha");
    assert_eq!(ids(&col.cards), ["c5", "c3", "c2", "c4", "c1", "c7", "c6"]);
}

#[test]
fn ties_break_by_id_then_unordered_cards_by_id() {
    let b = board_with(&[
        ("zz", "title: Z\ncolumn: a\norder: 1"),
        ("aa", "title: A\ncolumn: a\norder: 1.0"),
        ("n2", "title: N2\ncolumn: a"),
        ("n1", "title: N1\ncolumn: a\norder: bogus"),
        ("mm", "title: M\ncolumn: a\norder: 2"),
    ]);
    let v = view(&b);
    assert_eq!(
        ids(&v.columns.first().unwrap().cards),
        ["aa", "zz", "mm", "n1", "n2"]
    );
}

#[test]
fn unknown_or_missing_column_goes_to_unsorted() {
    let b = board_with(&[
        ("x", "title: X\ncolumn: nope"),
        ("y", "title: Y"),
        ("z", "title: Z\ncolumn: b"),
        ("w", "title: \"bad"),
    ]);
    let v = view(&b);
    assert_eq!(ids(&v.unsorted), ["w", "x", "y"]);
    assert_eq!(ids(&v.columns.get(1).unwrap().cards), ["z"]);
    assert!(v.columns.first().unwrap().cards.is_empty());
    assert!(!v.diagnostics.is_empty());
}

#[test]
fn archived_cards_leave_columns_and_unsorted_and_list_by_id() {
    let b = board_with(&[
        ("z", "title: Z\ncolumn: a\narchived: true"),
        ("m", "title: M\ncolumn: a"),
        ("a", "title: A\ncolumn: nope\narchived: true"),
    ]);
    let v = view(&b);
    assert_eq!(ids(&v.archived), ["a", "z"]);
    assert_eq!(ids(&v.columns.first().unwrap().cards), ["m"]);
    assert!(v.unsorted.is_empty());
    // an archived card still needs a valid column (E007 unchanged)
    assert!(
        v.diagnostics
            .iter()
            .any(|d| d.code.as_str() == "E007" && d.file == "cards/a.md"),
        "{:?}",
        v.diagnostics
    );
}

#[test]
fn view_has_title_description_and_serializes() {
    let b = board_with(&[("x", "title: X\ncolumn: a")]);
    let v = view(&b);
    assert_eq!(v.title, "My board");
    assert_eq!(v.description.trim(), "Some *text*.");
    let json = serde_json::to_string(&v).unwrap();
    assert!(json.contains("\"title\":\"My board\""));
    assert!(json.contains("\"unsorted\":[]"));
}

#[test]
fn decimal_compare_is_exact() {
    let d = |s| Decimal::parse(s).unwrap();
    assert!(d("0.1") < d("0.10000000000000000000001"));
    assert!(d("9007199254740993") > d("9007199254740992"));
    assert_eq!(d("1.50"), d("1.5"));
    assert_eq!(d("-0"), d("0"));
    assert!(d("-1") < d("-0.5"));
    assert!(Decimal::parse("").is_none());
    for bad in [
        "1.", ".5", "+1", "1e3", "NaN", "inf", "-", "1.2.3", " 1", "--1", "١",
    ] {
        assert!(Decimal::parse(bad).is_none(), "{bad}");
    }
}

#[test]
fn slug_rules() {
    assert_eq!(slug("Fix login timeout"), "fix-login-timeout");
    assert_eq!(slug("  --Hello,   World!!  "), "hello-world");
    assert_eq!(slug("!!!"), "card");
    assert_eq!(slug(""), "card");
    assert_eq!(slug("A_B"), "a-b");
    let long = slug(&"abcd ".repeat(40));
    assert!(long.len() <= 64 && !long.ends_with('-') && !long.starts_with('-'));
    assert_eq!(slug(&format!("{} x", "a".repeat(63))), "a".repeat(63));
    assert!(markban_core::is_valid_id(&slug("Anything goes ?!")));
}

#[test]
fn card_detail_has_title_column_and_description() {
    let mut b = board_with(&[("c1", "title: C1\ncolumn: a")]);
    b.cards.get_mut("c1").unwrap().body = "\nFirst line.\n\nSecond *line*.\n".to_string();
    let detail = markban_core::view::card(&b, "c1").unwrap();
    assert_eq!(detail.id, "c1");
    assert_eq!(detail.title, "C1");
    assert_eq!(detail.column.as_deref(), Some("a"));
    assert_eq!(detail.description, "First line.\n\nSecond *line*.");
}

#[test]
fn card_detail_tolerates_missing_keys_and_unknown_cards() {
    let b = board_with(&[("c1", "order: 1")]);
    let detail = markban_core::view::card(&b, "c1").unwrap();
    assert_eq!(detail.title, "c1");
    assert_eq!(detail.column, None);
    assert_eq!(detail.description, "");
    assert!(markban_core::view::card(&b, "nope").is_none());
}

#[test]
fn card_detail_reports_archived_only_when_true() {
    let b = board_with(&[
        ("live", "title: Live\ncolumn: a"),
        ("gone", "title: Gone\ncolumn: a\narchived: true"),
    ]);
    let active = markban_core::view::card(&b, "live").unwrap();
    assert!(!active.archived);
    // the key is absent from the JSON, so an active card serializes as before
    let json = serde_json::to_string(&active).unwrap();
    assert!(!json.contains("archived"), "{json}");
    let archived = markban_core::view::card(&b, "gone").unwrap();
    assert!(archived.archived);
    let json = serde_json::to_string(&archived).unwrap();
    assert!(json.contains("\"archived\":true"), "{json}");
}

#[test]
fn card_description_strips_a_leading_crlf() {
    let board = b"---\nmarkban: 0.1\ntitle: T\ncolumns:\n  - id: a\n    name: A\n---\n";
    let cards = vec![(
        "c.md".to_string(),
        b"---\r\ntitle: C\r\ncolumn: a\r\n---\r\n\r\nText\r\n".to_vec(),
    )];
    let b = Board::parse(board, cards, vec![]).0.unwrap();
    assert_eq!(
        markban_core::view::card(&b, "c").unwrap().description,
        "Text"
    );
}

#[test]
fn view_with_archived_keeps_archived_cards_in_their_column_position() {
    let b = board_with(&[
        ("a1", "title: A1\ncolumn: a\norder: 1"),
        ("a2", "title: A2\ncolumn: a\norder: 2\narchived: true"),
        ("a3", "title: A3\ncolumn: a\norder: 3"),
        ("u", "title: U\ncolumn: nope\narchived: true"),
    ]);
    let v = markban_core::view::view_with_archived(&b);
    let col = v.columns.first().unwrap();
    assert_eq!(ids(&col.cards), ["a1", "a2", "a3"]);
    let flags: Vec<bool> = col.cards.iter().map(|c| c.archived).collect();
    assert_eq!(flags, [false, true, false]);
    assert_eq!(ids(&v.unsorted), ["u"]);
    assert!(v.archived.is_empty());
    // the plain view is unchanged and serializes no `archived` on summaries
    let plain = view(&b);
    assert_eq!(ids(&plain.columns.first().unwrap().cards), ["a1", "a3"]);
    assert!(
        !serde_json::to_string(&plain.columns)
            .unwrap()
            .contains("archived")
    );
}
