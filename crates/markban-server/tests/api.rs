#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! The HTTP API against a real server on an ephemeral port.

mod common;

use common::{Fixture, board_files, request};
use markban_server::access;

const ADD: &str = r#"[{"cmd":"card.add","title":"First"}]"#;

/// Every board endpoint for board `id`, as (method, path, body).
fn endpoints(id: &str) -> Vec<(&'static str, String, Option<&'static str>)> {
    vec![
        ("GET", format!("/api/boards/{id}"), None),
        ("GET", format!("/api/boards/{id}/cards/first"), None),
        ("POST", format!("/api/boards/{id}/run"), Some(ADD)),
        ("GET", format!("/api/boards/{id}/log"), None),
        ("GET", format!("/api/boards/{id}/log?limit=5"), None),
    ]
}

fn send(
    f: &Fixture,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> common::Resp {
    request(f.addr, method, path, token, body.map(str::as_bytes), None)
}

// ---- authentication ----

#[test]
fn missing_or_wrong_token_is_401_everywhere() {
    let f = Fixture::new();
    let mut all = endpoints("acme");
    all.push(("GET", "/api/boards".into(), None));
    // Same token with its last character changed; pick a character that differs from the real one.
    let last = if f.admin.ends_with('0') { '1' } else { '0' };
    let near = format!("{}{last}", &f.admin[..f.admin.len() - 1]);
    for (method, path, body) in all {
        for token in [None, Some("wrong"), Some(""), Some(near.as_str())] {
            let r = send(&f, method, &path, token, body);
            assert_eq!(r.status, 401, "{method} {path} {token:?}: {}", r.body);
            assert_eq!(r.json()["code"], 1);
            assert!(
                r.headers.contains("www-authenticate: bearer"),
                "{}",
                r.headers
            );
        }
    }
    assert!(f.log("acme").is_empty());
}

#[test]
fn malformed_authorization_headers_are_401() {
    let f = Fixture::new();
    for header in [
        format!("Basic {}", f.admin),
        format!("Bearer{}", f.admin),
        f.admin.clone(),
        format!("Bearer  {} extra", f.admin),
    ] {
        let raw = format!(
            "GET /api/boards HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: {header}\r\n\r\n",
            f.addr
        );
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(f.addr).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        assert!(out.starts_with("HTTP/1.1 401"), "{header}: {out}");
    }
    // The scheme is case-insensitive.
    let raw = format!(
        "GET /api/boards HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAuthorization: bearer {}\r\n\r\n",
        f.addr, f.admin
    );
    use std::io::{Read, Write};
    let mut s = std::net::TcpStream::connect(f.addr).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
}

#[test]
fn removed_user_is_locked_out_without_a_restart() {
    let f = Fixture::new();
    assert_eq!(f.get("/api/boards", Some(&f.anna)).status, 200);
    access::remove_user(f.data.path(), "anna").unwrap();
    assert_eq!(f.get("/api/boards", Some(&f.anna)).status, 401);
}

#[test]
fn unreadable_access_file_fails_closed() {
    let f = Fixture::new();
    // Warm any cache first: a later parse error must still refuse.
    assert_eq!(f.get("/api/boards/acme", Some(&f.admin)).status, 200);
    std::fs::write(f.data.path().join("access.toml"), "garbage [[[").unwrap();
    let r = f.get("/api/boards/acme", Some(&f.admin));
    assert_eq!(r.status, 500, "{}", r.body);
    assert!(!r.body.contains("garbage"), "{}", r.body);
}

// ---- access per board ----

#[test]
fn a_board_the_user_may_not_use_answers_exactly_like_a_missing_board() {
    let f = Fixture::new();
    // Anna has `acme` only; `other` exists, `ghost` does not.
    for ((m1, hidden, b1), (_, missing, _)) in
        endpoints("other").into_iter().zip(endpoints("ghost"))
    {
        let r1 = send(&f, m1, &hidden, Some(&f.anna), b1);
        let r2 = send(&f, m1, &missing, Some(&f.anna), b1);
        assert_eq!(r1.status, 404, "{m1} {hidden}: {}", r1.body);
        assert_eq!(
            (r1.status, &r1.body),
            (r2.status, &r2.body),
            "{m1} {hidden}"
        );
        assert!(!r1.body.contains("other"), "{}", r1.body);
    }
    assert_eq!(
        common::board_files(&f.board("other")).len(),
        1,
        "nothing written"
    );
    assert!(f.log("other").is_empty());
}

#[test]
fn board_list_shows_only_allowed_boards() {
    let f = Fixture::new();
    let anna = f.get("/api/boards", Some(&f.anna));
    assert_eq!(anna.status, 200);
    assert_eq!(anna.body, r#"[{"id":"acme","title":"Board acme"}]"#);
    let admin = f.get("/api/boards", Some(&f.admin));
    assert_eq!(
        admin.body,
        r#"[{"id":"acme","title":"Board acme"},{"id":"other","title":"Board other"}]"#
    );
}

#[test]
fn star_reaches_every_board_including_new_ones() {
    let f = Fixture::new();
    access::add_board(f.data.path(), "later", "Later", &common::cols()).unwrap();
    for (method, path, body) in endpoints("later") {
        let r = send(&f, method, &path, Some(&f.admin), body);
        assert!(
            r.status < 300 || path.contains("/cards/"),
            "{method} {path}: {}",
            r.body
        );
    }
    // anna's list does not grow.
    assert!(!f.get("/api/boards", Some(&f.anna)).body.contains("later"));
    assert_eq!(f.get("/api/boards/later", Some(&f.anna)).status, 404);
}

#[test]
fn an_access_change_of_the_same_size_takes_effect_immediately() {
    let f = Fixture::new();
    access::add_board(f.data.path(), "aaaa", "A", &common::cols()).unwrap();
    assert_eq!(f.get("/api/boards/acme", Some(&f.anna)).status, 200);
    let before = std::fs::read(f.data.path().join("access.toml")).unwrap();
    // `acme` and `aaaa` have the same length, so the file keeps its size.
    access::set_boards(f.data.path(), "anna", &["aaaa".into()]).unwrap();
    let after = std::fs::read(f.data.path().join("access.toml")).unwrap();
    assert_eq!(before.len(), after.len());
    assert_eq!(f.get("/api/boards/acme", Some(&f.anna)).status, 404);
    assert_eq!(f.get("/api/boards/aaaa", Some(&f.anna)).status, 200);
}

#[test]
fn granting_a_board_takes_effect_immediately() {
    let f = Fixture::new();
    assert_eq!(f.get("/api/boards/other", Some(&f.anna)).status, 404);
    access::set_boards(f.data.path(), "anna", &["acme".into(), "other".into()]).unwrap();
    assert_eq!(f.get("/api/boards/other", Some(&f.anna)).status, 200);
}

// ---- path safety ----

#[test]
fn hostile_board_and_card_ids_are_404() {
    let f = Fixture::new();
    // A secret file next to the boards, to make sure no path escapes.
    std::fs::write(f.data.path().join("boards/secret.md"), "secret").unwrap();
    for path in [
        "/api/boards/..",
        "/api/boards/%2e%2e",
        "/api/boards/..%2f..",
        "/api/boards/acme%2f..%2fother",
        "/api/boards/acme%2F..%2Fother",
        "/api/boards/ACME",
        "/api/boards/acme%00",
        "/api/boards/acme%20",
        "/api/boards/-acme",
        "/api/boards/secret.md",
        "/api/boards/%ff",
        "/api/boards/acme/cards/..",
        "/api/boards/acme/cards/..%2fboard",
        "/api/boards/acme/cards/%2e%2e%2f%2e%2e%2faccess",
        "/api/boards/acme/cards/..%5c..%5caccess",
        "/api/boards/acme/cards/%ff",
        "/api/boards/acme/../other",
        "/api/boards/acme/cards/a/b",
        "/api/../access.toml",
        "/access.toml",
        "/boards/acme/board.md",
    ] {
        for token in [&f.anna, &f.admin] {
            let r = f.get(path, Some(token));
            assert_eq!(r.status, 404, "{path}: {}", r.body);
            assert!(
                !r.body.contains("secret") && !r.body.contains("token_sha256"),
                "{path}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn a_symlinked_board_directory_is_not_a_board() {
    let f = Fixture::new();
    std::os::unix::fs::symlink(f.board("other"), f.board("linked")).unwrap();
    let r = f.get("/api/boards/linked", Some(&f.admin));
    assert_eq!(r.status, 404, "{}", r.body);
    assert!(!f.get("/api/boards", Some(&f.admin)).body.contains("linked"));
}

#[test]
fn unknown_host_header_is_refused() {
    let f = Fixture::new();
    let r = request(
        f.addr,
        "GET",
        "/api/boards",
        Some(&f.admin),
        None,
        Some("evil.example"),
    );
    assert_eq!(r.status, 403, "{}", r.body);
    let r = request(
        f.addr,
        "GET",
        "/api/boards",
        Some(&f.admin),
        None,
        Some("localhost:1"),
    );
    assert_eq!(r.status, 200, "{}", r.body);
}

#[test]
fn allowed_hosts_are_configurable() {
    let data = tempfile::TempDir::new().unwrap();
    let token = access::init(data.path(), "alice").unwrap();
    let mut config = markban_server::Config::new(data.path().to_path_buf());
    config.allowed_hosts = markban_server::parse_hosts("board.example");
    let addr = markban_server::spawn(config, "127.0.0.1:0").unwrap();
    let r = request(
        addr,
        "GET",
        "/api/boards",
        Some(&token),
        None,
        Some("board.example"),
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let r = request(
        addr,
        "GET",
        "/api/boards",
        Some(&token),
        None,
        Some("evil.example"),
    );
    assert_eq!(r.status, 403);
    // A configured list replaces loopback: only the listed names are served.
    for loopback in ["localhost", "127.0.0.1", "[::1]:7171"] {
        let r = request(
            addr,
            "GET",
            "/api/boards",
            Some(&token),
            None,
            Some(loopback),
        );
        assert_eq!(r.status, 403, "{loopback}: {}", r.body);
    }
}

// ---- reading ----

#[test]
fn view_and_card_match_the_core() {
    let f = Fixture::new();
    f.post(
        "/api/boards/acme/run",
        Some(&f.anna),
        r#"[{"cmd":"card.add","title":"First","description":"Body"}]"#,
    );
    let r = f.get("/api/boards/acme", Some(&f.anna));
    assert_eq!(r.status, 200);
    assert!(
        r.headers.contains("content-type: application/json"),
        "{}",
        r.headers
    );
    let (board, diagnostics) = markban_core::fs::load(&f.board("acme")).unwrap();
    let board = board.unwrap();
    let mut expected = markban_core::view(&board);
    expected.diagnostics = diagnostics;
    assert_eq!(r.body, serde_json::to_string(&expected).unwrap());

    let r = f.get("/api/boards/acme/cards/first", Some(&f.anna));
    assert_eq!(r.status, 200);
    let card = markban_core::view::card(&board, "first").unwrap();
    assert_eq!(r.body, serde_json::to_string(&card).unwrap());

    let r = f.get("/api/boards/acme/cards/nope", Some(&f.anna));
    assert_eq!(r.status, 404);
    assert_eq!(
        r.json()["error"],
        "no card 'nope'; card ids are the file names in cards/ without .md"
    );
}

// ---- running commands ----

#[test]
fn run_applies_and_logs_one_line_per_command() {
    let f = Fixture::new();
    let r = f.post(
        "/api/boards/acme/run",
        Some(&f.anna),
        r#"[{"cmd":"card.add","title":"First"},{"cmd":"card.move","id":"first","column":"Done"}]"#,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, r#"{"outcomes":[{"id":"first"},{"id":"first"}]}"#);
    let log = f.log("acme");
    let lines: Vec<serde_json::Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2, "{log}");
    assert_eq!(lines[0]["user"], "anna");
    assert_eq!(lines[0]["cmd"]["cmd"], "card.add");
    assert_eq!(lines[0]["outcome"], "first");
    assert_eq!(lines[1]["cmd"]["column"], "Done");
    let ts = lines[0]["ts"].as_str().unwrap();
    assert_eq!(ts.len(), 20, "{ts}");
    assert!(ts.ends_with('Z') && ts.as_bytes()[10] == b'T', "{ts}");
    // Key order is fixed: ts, user, cmd, outcome.
    let first = log.lines().next().unwrap();
    assert!(first.starts_with(r#"{"ts":""#), "{first}");
    assert!(
        first.contains(r#"","user":"anna","cmd":{"cmd":"card.add""#),
        "{first}"
    );

    // The log endpoint returns the same entries, newest last, limited.
    let all = f.get("/api/boards/acme/log", Some(&f.anna));
    assert_eq!(all.status, 200);
    assert_eq!(
        all.body,
        format!("[{}]", log.lines().collect::<Vec<_>>().join(","))
    );
    let last = f.get("/api/boards/acme/log?limit=1", Some(&f.anna));
    assert_eq!(last.body, format!("[{}]", log.lines().nth(1).unwrap()));
    assert_eq!(
        f.get("/api/boards/acme/log?limit=0", Some(&f.anna)).body,
        "[]"
    );
    for bad in ["?limit=x", "?limit=-1", "?limit=99999999999999999999999"] {
        let r = f.get(&format!("/api/boards/acme/log{bad}"), Some(&f.anna));
        assert_eq!(r.status, 400, "{bad}: {}", r.body);
    }
}

#[test]
fn a_failing_batch_writes_nothing_and_logs_nothing() {
    let f = Fixture::new();
    let before = board_files(&f.board("acme"));
    let r = f.post(
        "/api/boards/acme/run",
        Some(&f.anna),
        r#"[{"cmd":"card.add","title":"First"},{"cmd":"card.move","id":"ghost","column":"Done"}]"#,
    );
    assert_eq!(r.status, 400, "{}", r.body);
    let v = r.json();
    assert_eq!(v["code"], 1);
    assert_eq!(v["index"], 1);
    assert_eq!(
        v["error"],
        "no card 'ghost'; card ids are the file names in cards/ without .md"
    );
    assert_eq!(board_files(&f.board("acme")), before);
    assert!(f.log("acme").is_empty());
}

#[test]
fn stale_if_unchanged_is_409_with_code_3() {
    let f = Fixture::new();
    f.post("/api/boards/acme/run", Some(&f.anna), ADD);
    let card = f.get("/api/boards/acme/cards/first", Some(&f.anna)).json();
    let sha = card["sha256"].as_str().unwrap().to_string();
    f.post(
        "/api/boards/acme/run",
        Some(&f.admin),
        r#"[{"cmd":"card.edit","id":"first","append":"admin was here"}]"#,
    );
    let body =
        format!(r#"[{{"cmd":"card.edit","id":"first","title":"Mine","if_unchanged":"{sha}"}}]"#);
    let r = f.post("/api/boards/acme/run", Some(&f.anna), &body);
    assert_eq!(r.status, 409, "{}", r.body);
    assert_eq!(r.json()["code"], 3);
    assert_eq!(f.log("acme").lines().count(), 2);
}

#[test]
fn board_init_through_the_api_cannot_replace_a_board() {
    let f = Fixture::new();
    let before = board_files(&f.board("acme"));
    let r = f.post(
        "/api/boards/acme/run",
        Some(&f.anna),
        r#"[{"cmd":"board.init","title":"Mine","columns":["X"]}]"#,
    );
    assert_eq!(r.status, 400, "{}", r.body);
    assert_eq!(board_files(&f.board("acme")), before);
}

#[test]
fn concurrent_requests_serialise_without_lost_updates() {
    let f = Fixture::new();
    f.post("/api/boards/acme/run", Some(&f.anna), ADD);
    let per_thread = 15;
    std::thread::scope(|scope| {
        for (who, token) in [("a", &f.anna), ("m", &f.admin)] {
            let f = &f;
            scope.spawn(move || {
                for i in 0..per_thread {
                    let body = format!(
                        r#"[{{"cmd":"card.edit","id":"first","append":"{who}{i}"}},{{"cmd":"card.add","title":"{who} {i}"}}]"#
                    );
                    let r = f.post("/api/boards/acme/run", Some(token), &body);
                    assert_eq!(r.status, 200, "{}", r.body);
                }
            });
        }
    });
    let card = std::fs::read_to_string(f.board("acme").join("cards/first.md")).unwrap();
    for who in ["a", "m"] {
        for i in 0..per_thread {
            assert!(
                card.lines().any(|l| l == format!("{who}{i}")),
                "lost {who}{i}:\n{card}"
            );
            assert!(
                f.board("acme")
                    .join(format!("cards/{who}-{i}.md"))
                    .is_file()
            );
        }
    }
    assert_eq!(f.log("acme").lines().count(), 1 + 2 * 2 * per_thread);
    assert!(
        markban_core::check(&markban_core::fs::load(&f.board("acme")).unwrap().0.unwrap())
            .is_empty()
    );
}

// ---- limits and garbage ----

#[test]
fn oversized_body_is_413() {
    let f = Fixture::new();
    let big = format!(
        r#"[{{"cmd":"card.add","title":"x","description":"{}"}}]"#,
        "a".repeat(2 * 1024 * 1024)
    );
    let r = f.post("/api/boards/acme/run", Some(&f.anna), &big);
    assert_eq!(r.status, 413, "{}", &r.body[..r.body.len().min(200)]);
    assert_eq!(r.json()["code"], 1);
    assert!(f.log("acme").is_empty());
}

#[test]
fn too_many_commands_is_400() {
    let f = Fixture::new();
    let one = r#"{"cmd":"card.remove","id":"x"}"#;
    let body = format!(
        "[{}]",
        vec![one; markban_server::MAX_COMMANDS + 1].join(",")
    );
    let r = f.post("/api/boards/acme/run", Some(&f.anna), &body);
    assert_eq!(r.status, 400, "{}", r.body);
    assert!(
        r.json()["error"].as_str().unwrap().contains("at most"),
        "{}",
        r.body
    );
}

#[test]
fn garbage_bodies_get_one_line_json_errors() {
    let f = Fixture::new();
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    let bodies = [
        "",
        "null",
        "{}",
        r#"{"cmd":"card.add","title":"x"}"#,
        "[1,2,3]",
        r#"[{"cmd":"card.fly"}]"#,
        r#"[{"cmd":"card.add","title":"x","colour":"red"}]"#,
        "[{\"cmd\":\"card.add\",\"title\":\"\u{0}\"}]",
        "\u{feff}[]",
        "[",
        &deep,
        "[\"\\ud800\"]",
    ];
    for body in bodies {
        let r = f.post("/api/boards/acme/run", Some(&f.anna), body);
        assert!(
            matches!(r.status, 200 | 400),
            "{body:.40}: {} {}",
            r.status,
            r.body
        );
        if r.status == 400 {
            let v = r.json();
            assert_eq!(v["code"], 1);
            let msg = v["error"].as_str().unwrap();
            assert!(!msg.is_empty() && !msg.contains('\n'), "{msg}");
        }
    }
    // Invalid UTF-8.
    let r = request(
        f.addr,
        "POST",
        "/api/boards/acme/run",
        Some(&f.anna),
        Some(&[0xff, 0xfe]),
        None,
    );
    assert_eq!(r.status, 400);
    // The server is still alive.
    assert_eq!(f.get("/api/boards", Some(&f.anna)).status, 200);
}

#[test]
fn adversarial_commands_never_kill_the_server() {
    let f = Fixture::new();
    let words = [
        "", " ", "..", "../x", "a/b", "x\ny", "日本", "\u{202e}", "-", "Done", "first",
    ];
    let mut sent = 0;
    for w in words {
        let w = serde_json::to_string(w).unwrap();
        for cmd in [
            format!(r#"{{"cmd":"card.add","title":{w},"id":{w}}}"#),
            format!(r#"{{"cmd":"card.add","title":"t","column":{w},"before":{w}}}"#),
            format!(r#"{{"cmd":"card.move","id":{w},"column":{w}}}"#),
            format!(r#"{{"cmd":"card.edit","id":{w},"title":{w},"if_unchanged":{w}}}"#),
            format!(r#"{{"cmd":"card.remove","id":{w}}}"#),
            format!(r#"{{"cmd":"column.add","name":{w},"id":{w}}}"#),
            format!(r#"{{"cmd":"column.rename","column":{w},"name":{w}}}"#),
            format!(r#"{{"cmd":"column.remove","column":{w},"move_to":{w}}}"#),
            format!(r#"{{"cmd":"board.init","title":{w},"columns":[{w}]}}"#),
        ] {
            let r = f.post("/api/boards/acme/run", Some(&f.anna), &format!("[{cmd}]"));
            assert!(
                matches!(r.status, 200 | 400 | 409),
                "{cmd}: {} {}",
                r.status,
                r.body
            );
            sent += 1;
        }
    }
    assert!(sent > 50);
    let (board, _) = markban_core::fs::load(&f.board("acme")).unwrap();
    assert!(board.is_some());
    // Nothing escaped the board directory.
    let mut top: Vec<String> = std::fs::read_dir(f.data.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    top.sort();
    assert_eq!(top, ["access.toml", "access.toml.lock", "boards", "log"]);
}

#[test]
fn unknown_endpoints_are_json_404() {
    let f = Fixture::new();
    for path in ["/", "/api", "/api/nope", "/api/boards/acme/cards"] {
        let r = f.get(path, Some(&f.admin));
        assert_eq!(r.status, 404, "{path}");
        assert_eq!(r.json()["code"], 1);
    }
}
