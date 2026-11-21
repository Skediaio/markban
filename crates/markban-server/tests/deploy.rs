#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! Deployment support: the health endpoint.

mod common;

use common::{Fixture, raw, request};
use std::io::Write;
use std::net::TcpStream;
use std::time::Duration;

// ---- /healthz ----

#[test]
fn healthz_answers_ok_without_a_token_and_says_nothing_else() {
    let f = Fixture::new();
    let r = raw(f.addr, "GET /healthz HTTP/1.1\r\n");
    assert_eq!(r.status, 200, "{}", r.body);
    assert_eq!(r.body, "ok");
    assert!(!r.headers.contains("server:"), "{}", r.headers);
    // Only GET: everything else under /healthz is the ordinary JSON 404 or 405.
    let r = raw(f.addr, "POST /healthz HTTP/1.1\r\nContent-Length: 0\r\n");
    assert_ne!(r.status, 200);
}

#[test]
fn healthz_ignores_the_host_list_and_the_concurrency_limit() {
    let f = Fixture::with(|c| {
        c.allowed_hosts = markban_server::parse_hosts("board.example");
        c.limits.max_concurrent = 1;
        c.limits.request_timeout = Duration::from_secs(3);
    });
    // The API refuses this Host header, health does not care.
    assert_eq!(
        request(f.addr, "GET", "/api/boards", Some(&f.admin), None, None).status,
        403
    );
    // Hold the only request slot with an authenticated request waiting for its body.
    let mut held = TcpStream::connect(f.addr).unwrap();
    held.write_all(
        format!(
            "POST /api/boards/acme/run HTTP/1.1\r\nHost: board.example\r\nAuthorization: Bearer {}\r\nContent-Length: 50\r\n\r\n",
            f.anna
        )
        .as_bytes(),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let busy = request(
        f.addr,
        "GET",
        "/api/boards",
        Some(&f.admin),
        None,
        Some("board.example"),
    );
    assert_eq!(busy.status, 503, "{}", busy.body);
    let r = raw(f.addr, "GET /healthz HTTP/1.1\r\n");
    assert_eq!((r.status, r.body.as_str()), (200, "ok"));
    drop(held);
}
