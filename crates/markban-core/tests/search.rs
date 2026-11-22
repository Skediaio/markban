#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use markban_core::board::Board;
use markban_core::search::{CardHit, ColumnHit, SearchResults, Snippet, search as try_search};
use proptest::prelude::*;

/// `search` that must succeed.
fn search(board: &Board, terms: &[String]) -> SearchResults {
    try_search(board, terms).unwrap()
}

/// The fixed, mixed board every test searches: two columns, active cards (one with a
/// description, one without), an archived card, an unsorted card and an id-only match.
fn board() -> Board {
    let board_md = b"---\nmarkban: 0.1\ntitle: Acme board\ncolumns:\n  - id: todo\n    name: Todo\n  - id: doing\n    name: Doing\n---\n\nBoard notes about acme.\n";
    let long = format!("{}needle{}\n", "a".repeat(40), "b".repeat(100));
    let cards = vec![
        (
            "a.md".to_string(),
            b"---\ntitle: Alpha task\ncolumn: todo\norder: 1000\n---\n\nIntro.\n\nLine two has the roadmap word.\n".to_vec(),
        ),
        (
            "b.md".to_string(),
            b"---\ntitle: Beta chore\ncolumn: doing\n---\n".to_vec(),
        ),
        (
            "gone.md".to_string(),
            b"---\ntitle: Old roadmap\ncolumn: todo\narchived: true\n---\n\nArchived body.\nSecond roadmap line.\n".to_vec(),
        ),
        (
            "lost.md".to_string(),
            b"---\ntitle: Lost thing\ncolumn: nowhere\n---\n".to_vec(),
        ),
        (
            "spec.md".to_string(),
            b"---\ntitle: Unrelated\ncolumn: doing\n---\n".to_vec(),
        ),
        (
            "long.md".to_string(),
            format!("---\ntitle: Long\ncolumn: doing\n---\n\n{long}").into_bytes(),
        ),
    ];
    Board::parse(board_md, cards, vec![]).0.unwrap()
}

fn terms(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_string()).collect()
}

fn card_ids(hits: &[CardHit]) -> Vec<&str> {
    hits.iter().map(|c| c.id.as_str()).collect()
}

#[test]
fn empty_terms_match_nothing() {
    let results = search(&board(), &[]);
    assert_eq!(results.cards, Vec::new());
    assert_eq!(results.columns, Vec::new());
    assert!(!results.board);
}

#[test]
fn every_term_is_required() {
    let b = board();
    // "roadmap" alone hits the active card and the archived one.
    assert_eq!(
        card_ids(&search(&b, &terms(&["roadmap"])).cards),
        ["a", "gone"]
    );
    // Both terms are still within each card's union of fields.
    assert_eq!(
        card_ids(&search(&b, &terms(&["roadmap", "line"])).cards),
        ["a", "gone"]
    );
    // One term nowhere: no hit.
    let none = search(&b, &terms(&["roadmap", "zzz"]));
    assert_eq!(card_ids(&none.cards), Vec::<&str>::new());
    assert!(none.columns.is_empty());
    assert!(!none.board);
}

#[test]
fn matching_is_case_insensitive_on_both_sides() {
    let b = board();
    // Mixed-case terms match lower-case board text.
    assert_eq!(
        card_ids(&search(&b, &terms(&["ROADMAP"])).cards),
        ["a", "gone"]
    );
    assert_eq!(
        card_ids(&search(&b, &terms(&["RoadMap"])).cards),
        ["a", "gone"]
    );
    // Mixed-case board text matches a lower-case term.
    assert_eq!(card_ids(&search(&b, &terms(&["alpha"])).cards), ["a"]);
    assert_eq!(
        card_ids(&search(&b, &terms(&["lost thing"])).cards),
        ["lost"]
    );
}

#[test]
fn a_hit_in_the_id_alone_stands() {
    let results = search(&board(), &terms(&["spec"]));
    assert_eq!(card_ids(&results.cards), ["spec"]);
    let hit = results.cards.first().unwrap();
    assert_eq!(hit.title, "Unrelated");
    assert_eq!(hit.column.as_deref(), Some("doing"));
    assert_eq!(hit.snippet, None);
}

#[test]
fn snippets_are_first_matching_line_trimmed_and_capped() {
    let results = search(&board(), &terms(&["roadmap"]));
    let a = results.cards.first().unwrap();
    // "Intro." is line 1, the blank line 2, the match line 3 of the description.
    assert_eq!(
        a.snippet,
        Some(Snippet {
            line: 3,
            text: "Line two has the roadmap word.".to_string(),
        })
    );
    let gone = results.cards.get(1).unwrap();
    assert_eq!(gone.id, "gone");
    // "Old roadmap" has the term in its title: no snippet.
    assert_eq!(gone.snippet, None);
    let second = search(&board(), &terms(&["second"]));
    assert_eq!(
        second.cards.first().unwrap().snippet,
        Some(Snippet {
            line: 2,
            text: "Second roadmap line.".to_string(),
        })
    );

    // A matching line of more than 120 characters is cut with a trailing ellipsis.
    let long = search(&board(), &terms(&["needle"]));
    assert_eq!(card_ids(&long.cards), ["long"]);
    let text = &long.cards.first().unwrap().snippet.as_ref().unwrap().text;
    assert_eq!(text.chars().count(), 121, "{text}");
    assert!(text.ends_with('…') && text.starts_with("aaaa"), "{text}");

    // A term that only matches the id or title carries no snippet.
    let b = search(&board(), &terms(&["beta"]));
    assert_eq!(b.cards.first().unwrap().snippet, None);
}

#[test]
fn columns_match_on_id_and_name_in_board_order() {
    let b = board();
    let by_id = search(&b, &terms(&["todo"]));
    assert_eq!(by_id.cards, Vec::new());
    assert_eq!(
        by_id.columns,
        vec![ColumnHit {
            id: "todo".to_string(),
            name: "Todo".to_string(),
        }]
    );
    let by_name = search(&b, &terms(&["doing"]));
    assert_eq!(by_name.columns.len(), 1);
    assert_eq!(by_name.columns.first().unwrap().id, "doing");
    // A term in the id and a term in the name: still one hit.
    let both = search(&b, &terms(&["todo", "todo"]));
    assert_eq!(both.columns.len(), 1);
    // Board order, several hits.
    let wide = search(&b, &terms(&["o"]));
    let ids: Vec<&str> = wide.columns.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["todo", "doing"]);
    // The board itself did not match.
    assert!(!by_id.board);
}

#[test]
fn the_board_matches_on_title_and_description() {
    let b = board();
    let by_title = search(&b, &terms(&["acme board"]));
    assert!(by_title.cards.is_empty() && by_title.columns.is_empty());
    assert!(by_title.board);
    let by_description = search(&b, &terms(&["notes"]));
    assert!(by_description.board);
    // One term in the title, one in the description.
    let split = search(&b, &terms(&["acme", "notes"]));
    assert!(split.board);
    let none = search(&b, &terms(&["acme", "zzz"]));
    assert!(!none.board);
}

#[test]
fn archived_hits_come_after_active_ones_and_are_flagged() {
    let results = search(&board(), &terms(&["roadmap"]));
    assert_eq!(card_ids(&results.cards), ["a", "gone"]);
    assert!(!results.cards.first().unwrap().archived);
    let gone = results.cards.get(1).unwrap();
    assert!(gone.archived);
    assert_eq!(gone.column.as_deref(), Some("todo"));
}

#[test]
fn a_card_without_a_usable_column_is_unsorted() {
    let results = search(&board(), &terms(&["lost"]));
    assert_eq!(card_ids(&results.cards), ["lost"]);
    let hit = results.cards.first().unwrap();
    assert_eq!(hit.column, None);
    // The raw key is not searched either: no card hit for its column name.
    let raw = search(&board(), &terms(&["nowhere"]));
    assert!(raw.cards.is_empty() && raw.columns.is_empty() && !raw.board);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn search_never_panics_and_is_deterministic(
        term in any::<String>(),
        extra in proptest::collection::vec(any::<String>(), 0..2),
    ) {
        let b = board();
        let mut all = vec![term];
        all.extend(extra);
        let Ok(first) = try_search(&b, &all) else { return Ok(()); };
        prop_assert_eq!(first.clone(), search(&b, &all));
        // Internally consistent: no duplicate ids, at most one hit per card.
        prop_assert!(first.cards.windows(2).all(|w| w[0].id != w[1].id));
        prop_assert!(first.cards.len() <= b.cards.len());
    }
}

#[test]
fn duplicate_terms_count_once_and_the_limits_are_one_line_errors() {
    let b = board();
    let single = search(&b, &terms(&["roadmap"]));
    assert_eq!(
        search(&b, &terms(&["roadmap", "ROADMAP", "roadmap"])),
        single
    );

    // 32 distinct terms are fine (they just match nothing), 33 are refused
    let ok: Vec<String> = (0..32).map(|i| format!("t{i}")).collect();
    assert!(try_search(&b, &ok).is_ok());
    let many: Vec<String> = (0..33).map(|i| format!("t{i}")).collect();
    let e = try_search(&b, &many).unwrap_err().to_string();
    assert!(
        e.contains("33") && e.contains("32") && !e.contains('\n'),
        "{e}"
    );
    // 33 words that dedupe to fewer are accepted
    let dup: Vec<String> = (0..33).map(|_| "x".to_string()).collect();
    assert!(try_search(&b, &dup).is_ok());

    // a term of 256 bytes is fine, 257 is refused
    assert!(try_search(&b, &["a".repeat(256)]).is_ok());
    let e = try_search(&b, &["a".repeat(257)]).unwrap_err().to_string();
    assert!(
        e.contains("257") && e.contains("256") && !e.contains('\n'),
        "{e}"
    );
}

#[test]
fn a_snippet_only_shows_a_term_that_matched_just_the_description() {
    let board_md = b"---\nmarkban: 0.1\ncolumns:\n  - id: todo\n    name: Todo\n---\n";
    let cards = vec![
        (
            "both.md".to_string(),
            b"---\ntitle: Parser rewrite\ncolumn: todo\n---\n\nThe parser is slow.\n".to_vec(),
        ),
        (
            "mixed.md".to_string(),
            b"---\ntitle: Parser rewrite\ncolumn: todo\n---\n\nThe parser is slow.\nCache misses hurt.\n".to_vec(),
        ),
    ];
    let b = Board::parse(board_md, cards, vec![]).0.unwrap();
    // the term matches title and description: no snippet
    let r = search(&b, &terms(&["parser"]));
    assert_eq!(r.cards.len(), 2);
    assert!(r.cards.iter().all(|c| c.snippet.is_none()), "{r:?}");
    // a second term only in the description gets its snippet
    let r = search(&b, &terms(&["parser", "cache"]));
    assert_eq!(card_ids(&r.cards), ["mixed"]);
    let snippet = r.cards[0].snippet.as_ref().unwrap();
    assert_eq!(
        (snippet.line, snippet.text.as_str()),
        (2, "Cache misses hurt.")
    );
}
