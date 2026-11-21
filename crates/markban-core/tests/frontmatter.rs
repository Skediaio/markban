#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::frontmatter::{Frontmatter, FrontmatterError, ValueError};

fn fm(text: &str) -> Frontmatter {
    Frontmatter::split(text).unwrap().0
}

const CARD: &str = "---\ntitle: T\ncolumn: doing\norder: 2000\n---\nbody\n";

fn changed_lines(before: &str, after: &str) -> usize {
    let b: Vec<_> = before.split_inclusive('\n').collect();
    let a: Vec<_> = after.split_inclusive('\n').collect();
    assert_eq!(a.len(), b.len(), "line count differs");
    a.iter().zip(&b).filter(|(x, y)| x != y).count()
}

#[test]
fn split_returns_body_and_renders_back() {
    let (f, body) = Frontmatter::split(CARD).unwrap();
    assert_eq!(body, "body\n");
    assert_eq!(f.to_string() + body, CARD);
}

#[test]
fn split_errors() {
    assert_eq!(
        Frontmatter::split("").unwrap_err(),
        FrontmatterError::Missing
    );
    assert_eq!(
        Frontmatter::split("title: x\n").unwrap_err(),
        FrontmatterError::Missing
    );
    assert_eq!(
        Frontmatter::split("---\ntitle: x\n").unwrap_err(),
        FrontmatterError::Unterminated
    );
}

#[test]
fn get_plain_and_quoted() {
    let f = fm("---\na: plain value  \nb: \"q: \\\"x\\\" \\\\ #\"\nc:\nd: \"bad\n---\n");
    assert_eq!(f.get("a"), Some(Ok("plain value".to_string())));
    assert_eq!(f.get("b"), Some(Ok("q: \"x\" \\ #".to_string())));
    assert_eq!(f.get("c"), Some(Ok(String::new())));
    assert_eq!(f.get("d"), Some(Err(ValueError::Unterminated)));
    assert_eq!(f.get("zzz"), None);
}

#[test]
fn get_malformed_quotes() {
    let f = fm("---\na: \"x\" tail\nb: \"a\\nb\"\n---\n");
    assert_eq!(f.get("a"), Some(Err(ValueError::TrailingText)));
    assert_eq!(f.get("b"), Some(Err(ValueError::BadEscape('n'))));
}

#[test]
fn line_of_is_one_based_file_line() {
    let f = fm(CARD);
    assert_eq!(f.line_of("title"), Some(2));
    assert_eq!(f.line_of("order"), Some(4));
    assert_eq!(f.line_of("nope"), None);
}

#[test]
fn set_column_changes_exactly_one_line() {
    let (mut f, body) = Frontmatter::split(CARD).unwrap();
    f.set("column", "done").unwrap();
    let out = f.to_string() + body;
    assert_eq!(changed_lines(CARD, &out), 1);
    assert_eq!(out, "---\ntitle: T\ncolumn: done\norder: 2000\n---\nbody\n");
}

#[test]
fn set_keeps_key_spacing_and_crlf() {
    let text = "---\r\ntitle: T\r\ncolumn:    doing  \r\n---\r\n";
    let mut f = fm(text);
    f.set("column", "done").unwrap();
    assert_eq!(
        f.to_string(),
        "---\r\ntitle: T\r\ncolumn:    done\r\n---\r\n"
    );
}

#[test]
fn set_quoting_rule() {
    let cases = [
        ("plain", "k: plain"),
        ("", "k: \"\""),
        (" lead", "k: \" lead\""),
        ("trail ", "k: \"trail \""),
        ("a: b", "k: \"a: b\""),
        ("a #b", "k: \"a #b\""),
        ("a#b", "k: a#b"),
        ("a:b", "k: a:b"),
        ("say \"hi\"", "k: say \"hi\""),
        ("\"quoted\"", "k: \"\\\"quoted\\\"\""),
        ("back\\slash", "k: back\\slash"),
        ("- dash", "k: \"- dash\""),
        ("[x]", "k: \"[x]\""),
        ("#x", "k: \"#x\""),
        ("@x", "k: \"@x\""),
        ("a-b", "k: a-b"),
        ("-72", "k: -72"),
        ("-1500.25", "k: -1500.25"),
        ("-x", "k: \"-x\""),
        ("-", "k: \"-\""),
    ];
    for (value, expected) in cases {
        let mut f = fm("---\nk: old\n---\n");
        f.set("k", value).unwrap();
        assert_eq!(
            f.to_string(),
            format!("---\n{expected}\n---\n"),
            "value {value:?}"
        );
        // and it reads back
        assert_eq!(
            f.get("k"),
            Some(Ok(value.to_string())),
            "readback {value:?}"
        );
    }
}

#[test]
fn set_quoted_backslash_roundtrips() {
    let mut f = fm("---\nk: old\n---\n");
    f.set("k", "\"a\\b\"").unwrap();
    assert_eq!(f.get("k"), Some(Ok("\"a\\b\"".to_string())));
}

#[test]
fn set_rejects_newlines() {
    let mut f = fm("---\nk: old\n---\n");
    assert_eq!(f.set("k", "a\nb"), Err(ValueError::Newline));
}

#[test]
fn set_absent_key_inserts_after_last_key() {
    let text = "---\ntitle: T\ncolumn: todo\n---\nbody\n";
    let mut f = fm(text);
    f.set("order", "1500").unwrap();
    assert_eq!(
        f.to_string(),
        "---\ntitle: T\ncolumn: todo\norder: 1500\n---\n"
    );
}

#[test]
fn set_absent_key_uses_dominant_line_ending() {
    let mut f = fm("---\r\ntitle: T\r\ncolumn: todo\r\n---\r\n");
    f.set("order", "1").unwrap();
    assert_eq!(
        f.to_string(),
        "---\r\ntitle: T\r\ncolumn: todo\r\norder: 1\r\n---\r\n"
    );
}

#[test]
fn set_absent_key_goes_after_block_continuation() {
    let mut f = fm("---\ntitle: T\ncolumns:\n  - id: a\n    name: A\n---\n");
    f.set("extra", "1").unwrap();
    assert_eq!(
        f.to_string(),
        "---\ntitle: T\ncolumns:\n  - id: a\n    name: A\nextra: 1\n---\n"
    );
}

#[test]
fn set_in_empty_block() {
    let mut f = fm("---\n---\n");
    f.set("a", "1").unwrap();
    assert_eq!(f.to_string(), "---\na: 1\n---\n");
}

#[test]
fn unknown_lines_survive_set_and_remove() {
    let text = "---\nodd line\ntitle: T\n  indented: x\nlabels: [a]\n---\n";
    let mut f = fm(text);
    f.set("title", "U").unwrap();
    assert_eq!(
        f.to_string(),
        "---\nodd line\ntitle: U\n  indented: x\nlabels: [a]\n---\n"
    );
    assert!(f.remove("title"));
    assert!(!f.remove("title"));
    assert_eq!(
        f.to_string(),
        "---\nodd line\n  indented: x\nlabels: [a]\n---\n"
    );
}
