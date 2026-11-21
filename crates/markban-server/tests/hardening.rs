#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! Resource limits, the log policy and slow or hostile clients.

mod common;

use common::{Fixture, board_files};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

/// Opens a connection and sends `head` (without the blank line when `complete` is false).
fn open(addr: SocketAddr, head: &str) -> TcpStream {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(head.as_bytes()).unwrap();
    stream
}

/// Everything the server sends until it closes, or `None` when it stays silent for `wait`.
fn answer(stream: &mut TcpStream, wait: Duration) -> Option<String> {
    stream.set_read_timeout(Some(wait)).unwrap();
    let mut raw = Vec::new();
    match stream.read_to_end(&mut raw) {
        Ok(_) => Some(String::from_utf8_lossy(&raw).into_owned()),
        Err(_) if !raw.is_empty() => Some(String::from_utf8_lossy(&raw).into_owned()),
        Err(_) => None,
    }
}

fn post_head(f: &Fixture, token: Option<&str>, length: usize) -> String {
    let mut head = format!(
        "POST /api/boards/kuuno/run HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {length}\r\n",
        f.addr
    );
    if let Some(token) = token {
        head.push_str(&format!("Authorization: Bearer {token}\r\n"));
    }
    head.push_str("\r\n");
    head
}

// ---- 1. the log ----

fn log_lines(f: &Fixture, id: &str) -> Vec<serde_json::Value> {
    f.log(id)
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn the_log_rotates_at_its_size_cap_and_keeps_one_old_file() {
    let f = Fixture::with(|c| c.limits.log_rotate_bytes = 2048);
    for i in 0..60 {
        let body = format!(r#"[{{"cmd":"card.add","title":"Card number {i}"}}]"#);
        let r = f.post("/api/boards/kuuno/run", Some(&f.anna), &body);
        assert_eq!(r.status, 200, "{}", r.body);
    }
    let log = f.data.path().join("log");
    let current = std::fs::metadata(log.join("kuuno.jsonl")).unwrap().len();
    let old = std::fs::metadata(log.join("kuuno.jsonl.1")).unwrap().len();
    assert!(current <= 2048 + 200, "{current}");
    assert!((2048..2048 + 200).contains(&old), "{old}");
    assert!(!log.join("kuuno.jsonl.2").exists());
    // The endpoint reads across the rotation: the newest entries, in order, no gap.
    let r = f.get("/api/boards/kuuno/log?limit=20", Some(&f.anna));
    let entries: Vec<serde_json::Value> = serde_json::from_str(&r.body).unwrap();
    assert_eq!(entries.len(), 20);
    let ids: Vec<&str> = entries
        .iter()
        .map(|e| e["outcome"].as_str().unwrap())
        .collect();
    let expected: Vec<String> = (40..60).map(|i| format!("card-number-{i}")).collect();
    assert_eq!(ids, expected);
}

#[test]
fn the_log_endpoint_reads_the_tail_of_a_large_log() {
    let f = Fixture::new();
    let path = f.data.path().join("log/kuuno.jsonl");
    let mut text = String::new();
    // A long valid line, garbage and a torn last line around ordinary entries.
    text.push_str(&format!(
        "{{\"outcome\":\"long\",\"pad\":\"{}\"}}\n",
        "x".repeat(300_000)
    ));
    for i in 0..3000 {
        text.push_str(&format!("{{\"outcome\":\"e{i}\"}}\n"));
        if i % 100 == 0 {
            text.push_str("not json\n");
        }
    }
    text.push_str("{\"outcome\":\"torn");
    std::fs::write(&path, &text).unwrap();
    let get = |q: &str| {
        let r = f.get(&format!("/api/boards/kuuno/log{q}"), Some(&f.anna));
        assert_eq!(r.status, 200, "{}", r.body);
        serde_json::from_str::<Vec<serde_json::Value>>(&r.body).unwrap()
    };
    let last = get("?limit=3");
    assert_eq!(
        last.iter()
            .map(|e| e["outcome"].clone())
            .collect::<Vec<_>>(),
        ["e2997", "e2998", "e2999"]
    );
    // More than the cap gives the cap.
    let many = get("?limit=100000");
    assert_eq!(many.len(), markban_server::LOG_MAX);
    assert_eq!(many[0]["outcome"], "e2000");
    // A small log with a very long line is returned whole.
    std::fs::write(
        &path,
        format!(
            "{{\"outcome\":\"long\",\"pad\":\"{}\"}}\n{{\"outcome\":\"z\"}}\n",
            "x".repeat(300_000)
        ),
    )
    .unwrap();
    let both = get("");
    assert_eq!(both.len(), 2);
    assert_eq!(both[0]["pad"].as_str().unwrap().len(), 300_000);
}

#[test]
fn large_text_is_truncated_in_the_log_but_not_on_the_board() {
    let f = Fixture::new();
    let long = "d".repeat(10_000);
    let body = format!(r#"[{{"cmd":"card.add","title":"Big","description":"{long}"}}]"#);
    let r = f.post("/api/boards/kuuno/run", Some(&f.anna), &body);
    assert_eq!(r.status, 200, "{}", r.body);
    let line = &log_lines(&f, "kuuno")[0];
    let logged = line["cmd"]["description"].as_str().unwrap();
    assert!(logged.len() < 2200, "{}", logged.len());
    assert!(logged.ends_with("…(truncated, 10000 bytes)"), "{logged}");
    let card = std::fs::read_to_string(f.board("kuuno").join("cards/big.md")).unwrap();
    assert!(card.contains(&long));
}

#[test]
fn a_failed_log_write_still_answers_success_and_says_so() {
    let f = Fixture::new();
    // A directory where the log file should be: no one can append to it.
    std::fs::create_dir_all(f.data.path().join("log/kuuno.jsonl")).unwrap();
    let r = f.post(
        "/api/boards/kuuno/run",
        Some(&f.anna),
        r#"[{"cmd":"card.add","title":"First"}]"#,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, r#"{"outcomes":[{"id":"first"}],"log":"failed"}"#);
    assert!(f.board("kuuno").join("cards/first.md").is_file());
}

#[cfg(unix)]
#[test]
fn log_files_and_a_recreated_log_directory_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    std::fs::remove_dir(f.data.path().join("log")).unwrap();
    let r = f.post(
        "/api/boards/kuuno/run",
        Some(&f.anna),
        r#"[{"cmd":"card.add","title":"First"}]"#,
    );
    assert_eq!(r.status, 200, "{}", r.body);
    let mode = |p: &str| {
        std::fs::metadata(f.data.path().join(p))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("log"), 0o700);
    assert_eq!(mode("log/kuuno.jsonl"), 0o600);
}

// ---- 2. authentication before the body, concurrency and timeouts ----

#[test]
fn an_unauthenticated_request_is_refused_before_its_body_arrives() {
    let f = Fixture::new();
    for token in [None, Some("wrong")] {
        // Announce a large body and never send it.
        let mut s = open(f.addr, &post_head(&f, token, 900_000));
        let reply = answer(&mut s, Duration::from_secs(3))
            .unwrap_or_else(|| panic!("{token:?}: no answer before the body"));
        assert!(reply.starts_with("HTTP/1.1 401"), "{reply}");
    }
}

#[test]
fn a_request_that_never_finishes_its_body_times_out() {
    let f = Fixture::with(|c| c.limits.request_timeout = Duration::from_millis(800));
    let mut s = open(f.addr, &post_head(&f, Some(&f.anna), 100));
    s.write_all(b"[{\"cmd\"").unwrap();
    let start = Instant::now();
    let reply = answer(&mut s, Duration::from_secs(5)).expect("the server must give up");
    assert!(start.elapsed() < Duration::from_secs(4));
    assert!(reply.starts_with("HTTP/1.1 503"), "{reply}");
    assert!(reply.contains(r#""code":4"#), "{reply}");
    assert!(f.log("kuuno").is_empty());
}

#[test]
fn requests_over_the_concurrency_limit_are_503_at_once() {
    let f = Fixture::with(|c| {
        c.limits.max_concurrent = 2;
        c.limits.request_timeout = Duration::from_secs(3);
    });
    // Two authenticated requests that hold their slot while waiting for a body.
    let mut held: Vec<TcpStream> = (0..2)
        .map(|_| open(f.addr, &post_head(&f, Some(&f.anna), 100)))
        .collect();
    std::thread::sleep(Duration::from_millis(300));
    let start = Instant::now();
    let r = f.get("/api/boards", Some(&f.anna));
    assert_eq!(r.status, 503, "{}", r.body);
    assert_eq!(r.json()["code"], 3);
    assert!(start.elapsed() < Duration::from_secs(1));
    for s in &mut held {
        s.write_all(&[b' '; 100]).unwrap();
    }
    drop(held);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(f.get("/api/boards", Some(&f.anna)).status, 200);
}

// ---- 3. one busy board does not stall the others ----

#[test]
fn many_runs_on_one_board_do_not_stall_another_board() {
    let f = Fixture::with(|c| c.limits.max_blocking = 4);
    // Hold board kuuno's lock, as another markban process would, so every run on it waits.
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(f.board("kuuno").join(".markban.lock"))
        .unwrap();
    lock.lock().unwrap();
    std::thread::scope(|scope| {
        for i in 0..24 {
            let f = &f;
            scope.spawn(move || {
                let body = format!(r#"[{{"cmd":"card.add","title":"Card {i}"}}]"#);
                let r = f.post("/api/boards/kuuno/run", Some(&f.anna), &body);
                assert!(matches!(r.status, 200 | 409 | 503), "{}", r.body);
            });
        }
        std::thread::sleep(Duration::from_millis(400));
        let start = Instant::now();
        let r = f.get("/api/boards/other", Some(&f.admin));
        let took = start.elapsed();
        assert_eq!(r.status, 200, "{}", r.body);
        assert!(
            took < Duration::from_millis(1000),
            "board other took {took:?}"
        );
        lock.unlock().unwrap();
    });
}

// ---- 5. size limits ----

#[test]
fn a_run_past_the_card_limit_is_refused_and_writes_nothing() {
    let f = Fixture::with(|c| c.limits.max_cards = 3);
    let add = |t: &str| {
        f.post(
            "/api/boards/kuuno/run",
            Some(&f.anna),
            &format!(r#"[{{"cmd":"card.add","title":"{t}"}}]"#),
        )
    };
    for t in ["One", "Two", "Three"] {
        assert_eq!(add(t).status, 200);
    }
    let before = board_files(&f.board("kuuno"));
    let r = add("Four");
    assert_eq!(r.status, 400, "{}", r.body);
    let v = r.json();
    assert_eq!(v["code"], 1);
    let msg = v["error"].as_str().unwrap();
    assert!(msg.contains("at most 3 cards"), "{msg}");
    assert!(!msg.contains('\n'));
    assert_eq!(board_files(&f.board("kuuno")), before);
    assert_eq!(f.log("kuuno").lines().count(), 3);
    // Shrinking is always allowed.
    let r = f.post(
        "/api/boards/kuuno/run",
        Some(&f.anna),
        r#"[{"cmd":"card.remove","id":"one"}]"#,
    );
    assert_eq!(r.status, 200, "{}", r.body);
}

#[test]
fn a_card_file_past_the_size_limit_is_refused() {
    let f = Fixture::with(|c| c.limits.max_card_bytes = 1024);
    let r = f.post(
        "/api/boards/kuuno/run",
        Some(&f.anna),
        &format!(
            r#"[{{"cmd":"card.add","title":"Big","description":"{}"}}]"#,
            "x".repeat(2000)
        ),
    );
    assert_eq!(r.status, 400, "{}", r.body);
    assert_eq!(r.json()["code"], 1);
    assert!(
        r.json()["error"]
            .as_str()
            .unwrap()
            .contains("at most 1024 bytes"),
        "{}",
        r.body
    );
    assert!(!f.board("kuuno").join("cards/big.md").exists());
    // Growing an existing card past the limit by appending is refused too.
    let r = f.post(
        "/api/boards/kuuno/run",
        Some(&f.anna),
        r#"[{"cmd":"card.add","title":"Small"}]"#,
    );
    assert_eq!(r.status, 200);
    let before = board_files(&f.board("kuuno"));
    let r = f.post(
        "/api/boards/kuuno/run",
        Some(&f.anna),
        &format!(
            r#"[{{"cmd":"card.edit","id":"small","append":"{}"}}]"#,
            "y".repeat(1500)
        ),
    );
    assert_eq!(r.status, 400, "{}", r.body);
    assert_eq!(board_files(&f.board("kuuno")), before);
}

// ---- 11. the board list ----

#[test]
fn the_board_list_skips_boards_the_user_may_not_use() {
    let f = Fixture::new();
    let ids: Vec<String> = markban_server::access::list_boards_where(f.data.path(), |id| {
        assert_ne!(id, "other-never-read");
        id == "kuuno"
    })
    .unwrap()
    .into_iter()
    .map(|(id, _)| id)
    .collect();
    assert_eq!(ids, ["kuuno"]);
}

// ---- 12. connections ----

#[test]
fn a_client_that_never_finishes_its_headers_is_dropped() {
    let f = Fixture::with(|c| c.limits.header_timeout = Duration::from_millis(500));
    let mut s = open(f.addr, "GET /api/boards HTTP/1.1\r\nHost: 127.0.0.1\r\n");
    let start = Instant::now();
    let reply = answer(&mut s, Duration::from_secs(4));
    assert!(
        reply.is_some(),
        "the connection must be closed by the server"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn idle_connections_count_against_the_connection_limit() {
    let f = Fixture::with(|c| {
        c.limits.max_connections = 1;
        c.limits.header_timeout = Duration::from_millis(700);
    });
    let idle = TcpStream::connect(f.addr).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let start = Instant::now();
    // Served only once the idle connection was dropped by the header timeout.
    let r = f.get("/api/boards", Some(&f.anna));
    assert_eq!(r.status, 200, "{}", r.body);
    assert!(
        start.elapsed() >= Duration::from_millis(400),
        "{:?}",
        start.elapsed()
    );
    drop(idle);
}
