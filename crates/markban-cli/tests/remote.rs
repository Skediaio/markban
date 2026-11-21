#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! Remote mode (`MARKBAN_URL`) against an in-process server, the admin commands and `serve`.

use assert_cmd::Command;
use markban_server::{Config, access};
use std::io::{BufRead, BufReader};
use std::path::Path;
use tempfile::TempDir;

#[derive(Debug, PartialEq, Eq)]
struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("markban").unwrap();
    for var in [
        "MARKBAN_BOARD",
        "MARKBAN_URL",
        "MARKBAN_TOKEN",
        "MARKBAN_DATA",
        "MARKBAN_ALLOW_INSECURE_HTTP",
    ] {
        cmd.env_remove(var);
    }
    cmd
}

fn out(cmd: &mut Command, stdin: Option<&str>) -> Out {
    if let Some(input) = stdin {
        cmd.write_stdin(input);
    }
    let o = cmd.output().unwrap();
    Out {
        stdout: String::from_utf8(o.stdout).unwrap(),
        stderr: String::from_utf8(o.stderr).unwrap(),
        code: o.status.code().unwrap(),
    }
}

/// A data directory with board `acme` (served) and users `alice` (admin, `*`) and `anna`
/// (`acme` only), plus a board `other` that anna may not use.
struct Server {
    data: TempDir,
    url: String,
    admin: String,
    anna: String,
}

const COLS: [&str; 3] = ["Todo", "Doing", "Done"];

fn server() -> Server {
    let data = TempDir::new().unwrap();
    let admin = access::init(data.path(), "alice").unwrap();
    let cols: Vec<String> = COLS.map(String::from).to_vec();
    access::add_board(data.path(), "acme", "Acme", &cols).unwrap();
    access::add_board(data.path(), "other", "Other", &cols).unwrap();
    let anna = access::add_user(data.path(), "anna", &["acme".into()], false).unwrap();
    let addr =
        markban_server::spawn(Config::new(data.path().to_path_buf()), "127.0.0.1:0").unwrap();
    Server {
        data,
        url: format!("http://{addr}"),
        admin,
        anna,
    }
}

impl Server {
    fn remote(&self, token: &str, board: Option<&str>, args: &[&str], stdin: Option<&str>) -> Out {
        let mut cmd = bin();
        cmd.env("MARKBAN_URL", &self.url)
            .env("MARKBAN_TOKEN", token);
        if let Some(board) = board {
            cmd.env("MARKBAN_BOARD", board);
        }
        out(cmd.args(args), stdin)
    }

    fn board(&self, id: &str) -> std::path::PathBuf {
        self.data.path().join("boards").join(id)
    }
}

fn local(dir: &Path, args: &[&str], stdin: Option<&str>) -> Out {
    out(bin().arg("-C").arg(dir).args(args), stdin)
}

fn board_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![(
        "board.md".to_string(),
        std::fs::read(dir.join("board.md")).unwrap(),
    )];
    if let Ok(entries) = std::fs::read_dir(dir.join("cards")) {
        let mut cards: Vec<_> = entries.map(|e| e.unwrap().path()).collect();
        cards.sort();
        for card in cards {
            let name = card.file_name().unwrap().to_string_lossy().into_owned();
            files.push((format!("cards/{name}"), std::fs::read(&card).unwrap()));
        }
    }
    files
}

#[test]
fn remote_and_local_give_identical_output_exit_codes_and_files() {
    let s = server();
    let local_dir = TempDir::new().unwrap();
    let mut init = vec!["init", "Acme"];
    init.extend(COLS);
    assert_eq!(local(local_dir.path(), &init, None).code, 0);
    assert_eq!(board_files(local_dir.path()), board_files(&s.board("acme")));

    let script = "# a script\n{\"cmd\":\"card.add\",\"title\":\"Scripted\",\"column\":\"Doing\"}\n\n{\"cmd\":\"card.move\",\"id\":\"scripted\",\"top\":true}\n";
    let bad_script = "{\"cmd\":\"card.add\",\"title\":\"Ok\"}\n{\"cmd\":\"card.move\",\"id\":\"ghost\",\"column\":\"Done\"}\n";
    let steps: Vec<(Vec<&str>, Option<&str>)> = vec![
        (vec!["ls"], None),
        (vec!["add", "First"], None),
        (
            vec!["add", "Second", "-c", "Doing", "-d", "Body text\nline two"],
            None,
        ),
        (vec!["add", "Third", "--top"], None),
        (vec!["add", "Piped", "-d", "-"], Some("from stdin\n")),
        (vec!["add", "Dup", "--id", "first"], None),
        (vec!["mv", "first", "done"], None),
        (vec!["mv", "second", "--top"], None),
        (vec!["mv", "third", "--after", "piped"], None),
        (vec!["mv", "ghost", "done"], None),
        (vec!["edit", "first", "--title", "First renamed"], None),
        (vec!["edit", "first", "--append", "a note"], None),
        (vec!["edit", "second", "-d", "replaced"], None),
        (vec!["edit", "second"], None),
        (vec!["show", "first"], None),
        (vec!["show", "first", "--json"], None),
        (vec!["show", "nope"], None),
        (vec!["ls", "--json"], None),
        (vec!["ls", "doing"], None),
        (vec!["ls", "unsorted"], None),
        (vec!["ls", "nope"], None),
        (vec!["col", "add", "Review", "--before", "done"], None),
        (vec!["col", "rename", "review", "Reviewing"], None),
        (vec!["col", "mv", "reviewing"], None),
        (vec!["col", "rm", "doing"], None),
        (vec!["col", "rm", "reviewing"], None),
        (vec!["col", "rm", "todo", "--move-to", "done"], None),
        (vec!["rm", "third"], None),
        (vec!["rm", "third"], None),
        (vec!["run"], Some(script)),
        (vec!["run"], Some(bad_script)),
        (vec!["run"], Some("not json\n")),
        (vec!["init", "Again", "A"], None),
        (
            vec![
                "edit",
                "first",
                "--title",
                "X",
                "--if-unchanged",
                "0000000000000000000000000000000000000000000000000000000000000000",
            ],
            None,
        ),
        (
            vec!["edit", "first", "--title", "X", "--if-unchanged", "bad"],
            None,
        ),
        (vec!["check"], None),
        (vec!["ls"], None),
    ];
    let mut codes = Vec::new();
    for (args, stdin) in &steps {
        let l = local(local_dir.path(), args, *stdin);
        let r = s.remote(&s.anna, Some("acme"), args, *stdin);
        assert_eq!(l, r, "{args:?}");
        codes.push(l.code);
    }
    // The sequence covers success, invalid input and contention.
    assert!(
        codes.contains(&0) && codes.contains(&1) && codes.contains(&3),
        "{codes:?}"
    );
    assert_eq!(board_files(local_dir.path()), board_files(&s.board("acme")));

    // A matching hash edits, through both.
    let sha = |o: Out| -> String {
        let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
        v["sha256"].as_str().unwrap().to_string()
    };
    let local_sha = sha(local(local_dir.path(), &["show", "first", "--json"], None));
    let remote_sha = sha(s.remote(&s.anna, Some("acme"), &["show", "first", "--json"], None));
    assert_eq!(local_sha, remote_sha);
    let args = [
        "edit",
        "first",
        "--title",
        "Checked",
        "--if-unchanged",
        &local_sha,
    ];
    assert_eq!(
        local(local_dir.path(), &args, None),
        s.remote(&s.anna, Some("acme"), &args, None)
    );
    assert_eq!(board_files(local_dir.path()), board_files(&s.board("acme")));
}

#[test]
fn board_flag_overrides_the_environment() {
    let s = server();
    let o = s.remote(
        &s.admin,
        Some("acme"),
        &["-b", "other", "add", "Here"],
        None,
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(s.board("other").join("cards/here.md").is_file());
    assert!(!s.board("acme").join("cards/here.md").exists());
}

#[test]
fn remote_without_a_board_says_how_to_pick_one() {
    let s = server();
    let o = s.remote(&s.anna, None, &["ls"], None);
    assert_eq!(o.code, 1);
    assert!(
        o.stderr.contains("MARKBAN_BOARD") && o.stderr.contains("-b"),
        "{}",
        o.stderr
    );
    for bad in ["../other", "Acme", "a/b"] {
        let o = s.remote(&s.anna, Some(bad), &["ls"], None);
        assert_eq!(o.code, 1, "{bad}");
        assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
    }
}

#[test]
fn hidden_board_reads_like_a_missing_one() {
    let s = server();
    for args in [
        &["ls"][..],
        &["show", "x"],
        &["add", "X"],
        &["log"],
        &["check"],
    ] {
        let hidden = s.remote(&s.anna, Some("other"), args, None);
        let missing = s.remote(&s.anna, Some("ghost"), args, None);
        assert_eq!(hidden, missing, "{args:?}");
        assert_eq!(hidden.code, 1);
        assert!(!hidden.stderr.contains("other"), "{}", hidden.stderr);
    }
    assert_eq!(board_files(&s.board("other")).len(), 1);
}

#[test]
fn bad_token_is_one_line_and_never_echoed() {
    let s = server();
    let token = "mb_this-is-not-a-valid-token-value";
    let o = s.remote(token, Some("acme"), &["ls"], None);
    assert_eq!(o.code, 1);
    assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
    assert!(o.stderr.contains("token"), "{}", o.stderr);
    assert!(!o.stderr.contains(token) && !o.stdout.contains(token));
    // Passed as a flag, it is not echoed either.
    let mut cmd = bin();
    cmd.env("MARKBAN_URL", &s.url)
        .args(["--token", token, "-b", "acme", "ls"]);
    let o = out(&mut cmd, None);
    assert_eq!(o.code, 1);
    assert!(!o.stderr.contains(token));
}

#[test]
fn missing_token_is_an_actionable_error() {
    let s = server();
    let mut cmd = bin();
    cmd.env("MARKBAN_URL", &s.url).args(["-b", "acme", "ls"]);
    let o = out(&mut cmd, None);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("MARKBAN_TOKEN"), "{}", o.stderr);
}

#[test]
fn unreachable_server_is_one_line_exit_1() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let mut cmd = bin();
    cmd.env("MARKBAN_URL", &url)
        .env("MARKBAN_TOKEN", "mb_secret-token")
        .args(["-b", "acme", "ls"]);
    let o = out(&mut cmd, None);
    assert_eq!(o.code, 1);
    assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
    assert!(o.stderr.starts_with("error: cannot reach"), "{}", o.stderr);
    assert!(!o.stderr.contains("secret-token"));
}

#[test]
fn dir_flag_is_refused_in_remote_mode() {
    let s = server();
    let o = s.remote(&s.anna, Some("acme"), &["-C", ".", "ls"], None);
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("-C"), "{}", o.stderr);
}

#[test]
fn boards_lists_what_the_token_may_use() {
    let s = server();
    let o = s.remote(&s.anna, None, &["boards"], None);
    assert_eq!(
        (o.code, o.stdout.as_str()),
        (0, "acme  Acme\n"),
        "{}",
        o.stderr
    );
    let o = s.remote(&s.admin, None, &["boards"], None);
    assert_eq!(o.stdout, "acme  Acme\nother  Other\n");
    let o = s.remote(&s.anna, None, &["boards", "--json"], None);
    assert_eq!(o.stdout, "[{\"id\":\"acme\",\"title\":\"Acme\"}]\n");
}

#[test]
fn log_prints_the_last_entries_as_json_lines() {
    let s = server();
    for title in ["One", "Two", "Three"] {
        assert_eq!(
            s.remote(&s.anna, Some("acme"), &["add", title], None).code,
            0
        );
    }
    let o = s.remote(&s.anna, Some("acme"), &["log", "-n", "2"], None);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{}", o.stdout);
    let file = std::fs::read_to_string(s.data.path().join("log/acme.jsonl")).unwrap();
    let stored: Vec<&str> = file.lines().collect();
    assert_eq!(lines, stored[1..]);
    assert!(lines[1].contains("\"user\":\"anna\"") && lines[1].contains("\"outcome\":\"three\""));
}

#[test]
fn boards_and_log_need_a_server() {
    let dir = TempDir::new().unwrap();
    for args in [&["boards"][..], &["log"]] {
        let o = out(bin().current_dir(dir.path()).args(args), None);
        assert_eq!(o.code, 1, "{args:?}");
        assert!(o.stderr.contains("MARKBAN_URL"), "{}", o.stderr);
    }
}

// ---- admin commands ----

fn admin(data: &Path, args: &[&str]) -> Out {
    let mut cmd = bin();
    cmd.arg("server").arg("--data").arg(data).args(args);
    out(&mut cmd, None)
}

#[test]
fn admin_commands_manage_users_and_boards_by_file_access() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let o = out(bin().args(["server", "init"]).arg(&data), None);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let admin_token = o.stdout.trim().to_string();
    assert!(admin_token.starts_with("mb_") && o.stdout.lines().count() == 1);
    assert!(o.stderr.contains("once"), "{}", o.stderr);

    let o = admin(&data, &["board", "add", "acme", "Acme", "Todo", "Done"]);
    assert_eq!((o.code, o.stdout.as_str()), (0, "acme\n"), "{}", o.stderr);
    let o = admin(&data, &["board", "add", "site", "Site", "Todo"]);
    assert_eq!(o.code, 0);
    let o = admin(&data, &["board", "list"]);
    assert_eq!(o.stdout, "acme  Acme\nsite  Site\n");

    let o = admin(&data, &["user", "add", "anna", "--boards", "acme"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let anna = o.stdout.trim().to_string();
    let o = admin(
        &data,
        &["user", "add", "bot", "--boards", "acme,site", "--admin"],
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    let text = std::fs::read_to_string(data.join("access.toml")).unwrap();
    assert!(!text.contains(&anna) && !text.contains(&admin_token));
    assert!(
        text.contains("[users.bot]") && text.contains(r#"boards = ["acme", "site"]"#),
        "{text}"
    );

    let parsed = access::Access::load(&data).unwrap();
    assert!(parsed.authenticate(&admin_token).unwrap().admin);
    let user = parsed.authenticate(&anna).unwrap();
    assert!(user.may_use("acme") && !user.may_use("site") && !user.admin);

    let o = admin(&data, &["user", "boards", "anna", "site"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let parsed = access::Access::load(&data).unwrap();
    assert!(parsed.authenticate(&anna).unwrap().may_use("site"));

    for bad in [
        &["user", "add", "anna", "--boards", "acme"][..],
        &["user", "add", "Bad", "--boards", "acme"],
        &["user", "add", "eve", "--boards", "ghost"],
        &["user", "remove", "ghost"],
        &["board", "add", "acme", "Again", "Todo"],
        &["board", "add", "../x", "X", "Todo"],
    ] {
        let o = admin(&data, bad);
        assert_eq!(o.code, 1, "{bad:?}");
        assert_eq!(o.stderr.lines().count(), 1, "{bad:?}: {}", o.stderr);
    }
    let o = out(bin().args(["server", "init"]).arg(&data), None);
    assert_eq!(o.code, 1, "init twice");

    let o = admin(&data, &["user", "remove", "anna"]);
    assert_eq!(o.code, 0);
    assert!(
        access::Access::load(&data)
            .unwrap()
            .authenticate(&anna)
            .is_none()
    );
}

#[test]
fn admin_commands_never_go_over_http() {
    // Remote settings in the environment do not turn admin commands into requests.
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    assert_eq!(out(bin().args(["server", "init"]).arg(&data), None).code, 0);
    let mut cmd = bin();
    cmd.env("MARKBAN_URL", "http://127.0.0.1:9")
        .env("MARKBAN_TOKEN", "mb_x")
        .env("MARKBAN_DATA", &data)
        .args(["server", "board", "add", "acme", "Acme", "Todo"]);
    let o = out(&mut cmd, None);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(data.join("boards/acme/board.md").is_file());
}

#[test]
fn there_is_no_admin_endpoint_over_http() {
    let s = server();
    // Even the admin token cannot create boards or users through the API.
    for (method, path) in [
        ("POST", "/api/boards"),
        ("POST", "/api/users"),
        ("PUT", "/api/boards/new"),
    ] {
        let r = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .new_agent()
            .run(
                ureq::http::Request::builder()
                    .method(method)
                    .uri(format!("{}{path}", s.url))
                    .header("Authorization", format!("Bearer {}", s.admin))
                    .body(String::new())
                    .unwrap(),
            )
            .unwrap();
        assert!(
            matches!(r.status().as_u16(), 404 | 405),
            "{method} {path}: {}",
            r.status()
        );
    }
    assert!(!s.board("new").exists());
}

#[test]
fn serve_runs_the_api_from_the_command_line() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("data");
    let token = out(bin().args(["server", "init"]).arg(&data), None)
        .stdout
        .trim()
        .to_string();
    assert_eq!(
        admin(&data, &["board", "add", "acme", "Acme", "Todo"]).code,
        0
    );
    let bin_path = assert_cmd::cargo::cargo_bin("markban");
    let mut child = std::process::Command::new(bin_path)
        .args(["serve", "--listen", "127.0.0.1:0", "--data"])
        .arg(&data)
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stderr.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line
        .split_whitespace()
        .find(|w| w.starts_with("http://"))
        .unwrap_or_else(|| panic!("no url in {line:?}"))
        .to_string();
    let mut cmd = bin();
    cmd.env("MARKBAN_URL", &url)
        .env("MARKBAN_TOKEN", &token)
        .args(["-b", "acme", "add", "Over the wire"]);
    let o = out(&mut cmd, None);
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(
        (o.code, o.stdout.as_str()),
        (0, "over-the-wire\n"),
        "{}",
        o.stderr
    );
}

#[test]
fn serve_without_a_data_directory_fails_fast() {
    let dir = TempDir::new().unwrap();
    let o = out(
        bin()
            .args(["serve", "--listen", "127.0.0.1:0", "--data"])
            .arg(dir.path()),
        None,
    );
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("markban server init"), "{}", o.stderr);
}

// ---- transport safety ----

#[test]
fn plain_http_to_a_remote_host_is_refused_before_sending_the_token() {
    // `.invalid` never resolves: had a request been tried, the error would be "cannot reach".
    for url in [
        "http://board.invalid:7171".to_string(),
        "http://192.0.2.1:9".to_string(),
        "http://[::2]:9".to_string(),
        "http://localhost.evil.invalid".to_string(),
    ] {
        let mut cmd = bin();
        cmd.env("MARKBAN_URL", &url)
            .env("MARKBAN_TOKEN", "mb_secret-token")
            .args(["-b", "acme", "ls"]);
        let o = out(&mut cmd, None);
        assert_eq!(o.code, 1, "{url}");
        assert_eq!(o.stderr.lines().count(), 1, "{url}: {}", o.stderr);
        assert!(o.stderr.contains("https://"), "{url}: {}", o.stderr);
        assert!(
            o.stderr.contains("--allow-insecure-http"),
            "{url}: {}",
            o.stderr
        );
        assert!(!o.stderr.contains("secret-token"));
    }
}

#[test]
fn plain_http_is_fine_on_loopback_and_with_the_escape_hatch() {
    let s = server();
    let port = s.url.rsplit(':').next().unwrap().to_string();
    for url in [
        s.url.clone(),
        format!("http://localhost:{port}"),
        format!("http://LocalHost:{port}/"),
    ] {
        let mut cmd = bin();
        cmd.env("MARKBAN_URL", &url)
            .env("MARKBAN_TOKEN", &s.anna)
            .args(["boards"]);
        let o = out(&mut cmd, None);
        assert_eq!(o.code, 0, "{url}: {}", o.stderr);
    }
    // The escape hatch only lifts the transport check: an unreachable host still fails there.
    for (flag, env) in [(true, false), (false, true)] {
        let mut cmd = bin();
        cmd.env("MARKBAN_URL", "http://unreachable.invalid:9")
            .env("MARKBAN_TOKEN", "mb_x");
        if flag {
            cmd.arg("--allow-insecure-http");
        }
        if env {
            cmd.env("MARKBAN_ALLOW_INSECURE_HTTP", "1");
        }
        let o = out(cmd.args(["-b", "acme", "ls"]), None);
        assert_eq!(o.code, 1);
        assert!(o.stderr.contains("cannot reach"), "{}", o.stderr);
    }
}

#[test]
fn a_proxy_in_the_environment_is_not_used_for_loopback_http() {
    let s = server();
    // A dead proxy: any request through it fails.
    let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let mut cmd = bin();
    for var in [
        "ALL_PROXY",
        "all_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "HTTPS_PROXY",
        "https_proxy",
    ] {
        cmd.env(var, &proxy);
    }
    cmd.env_remove("NO_PROXY").env_remove("no_proxy");
    cmd.env("MARKBAN_URL", &s.url)
        .env("MARKBAN_TOKEN", &s.anna)
        .args(["boards"]);
    let o = out(&mut cmd, None);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "acme  Acme\n");
}

#[test]
fn a_url_with_credentials_is_refused_without_echoing_them() {
    let s = server();
    let url = s.url.replace("http://", "http://anna:hunter2@");
    let mut cmd = bin();
    cmd.env("MARKBAN_URL", &url)
        .env("MARKBAN_TOKEN", &s.anna)
        .args(["boards"]);
    let o = out(&mut cmd, None);
    assert_eq!(o.code, 1);
    assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
    assert!(o.stderr.contains("user name or password"), "{}", o.stderr);
    assert!(!o.stderr.contains("hunter2") && !o.stderr.contains("anna:"));
}

// ---- server board remove ----

#[test]
fn board_remove_moves_the_board_away_and_drops_its_grants() {
    let s = server();
    let data = s.data.path();
    assert_eq!(
        admin(data, &["user", "boards", "anna", "acme,other"]).code,
        0
    );
    let o = admin(data, &["board", "remove", "other"]);
    assert_eq!((o.code, o.stdout.as_str()), (0, "other\n"), "{}", o.stderr);
    assert!(!s.board("other").exists());
    let removed: Vec<_> = std::fs::read_dir(data.join("removed"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(removed.len(), 1);
    assert!(removed[0].join("board.md").is_file());
    let text = std::fs::read_to_string(data.join("access.toml")).unwrap();
    assert!(text.contains(r#"boards = ["acme"]"#), "{text}");
    assert_eq!(admin(data, &["board", "list"]).stdout, "acme  Acme\n");
    let o = admin(data, &["board", "remove", "other"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.stderr.lines().count(), 1, "{}", o.stderr);
}

#[test]
fn serve_takes_limit_flags() {
    let dir = TempDir::new().unwrap();
    let o = out(
        bin()
            .args([
                "serve",
                "--listen",
                "127.0.0.1:0",
                "--max-cards",
                "10",
                "--max-card-bytes",
                "4096",
                "--max-requests",
                "8",
                "--data",
            ])
            .arg(dir.path()),
        None,
    );
    // The flags parse; the empty data directory is what stops it.
    assert_eq!(o.code, 1, "{}", o.stderr);
    assert!(o.stderr.contains("markban server init"), "{}", o.stderr);
}

// ---- tokens to a file ----

#[test]
fn init_and_user_add_can_write_the_token_to_a_private_file() {
    let parent = TempDir::new().unwrap();
    let data = parent.path().join("data");
    let file = parent.path().join("admin.token");
    let o = out(
        bin()
            .args(["server", "init"])
            .arg(&data)
            .arg("--token-file")
            .arg(&file),
        None,
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "", "the token must not reach stdout");
    let token = std::fs::read_to_string(&file).unwrap();
    assert!(token.starts_with("mb_") && token.ends_with('\n'));
    assert!(!o.stderr.contains(token.trim()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    assert!(
        access::Access::load(&data)
            .unwrap()
            .authenticate(token.trim())
            .is_some()
    );

    // user add: an existing file is refused before anything changes.
    let o = admin(
        &data,
        &["user", "add", "anna", "--boards", "*", "--token-file"],
    );
    assert_eq!(o.code, 2, "--token-file needs a path");
    let o = out(
        bin()
            .arg("server")
            .arg("--data")
            .arg(&data)
            .args(["user", "add", "anna", "--boards", "*", "--token-file"])
            .arg(&file),
        None,
    );
    assert_eq!(o.code, 1);
    assert!(o.stderr.contains("already exists"), "{}", o.stderr);
    assert!(
        !std::fs::read_to_string(data.join("access.toml"))
            .unwrap()
            .contains("anna")
    );
    let anna_file = parent.path().join("anna.token");
    let o = out(
        bin()
            .arg("server")
            .arg("--data")
            .arg(&data)
            .args(["user", "add", "anna", "--boards", "*", "--token-file"])
            .arg(&anna_file),
        None,
    );
    assert_eq!((o.code, o.stdout.as_str()), (0, ""), "{}", o.stderr);
    let anna = std::fs::read_to_string(&anna_file).unwrap();
    assert!(
        access::Access::load(&data)
            .unwrap()
            .authenticate(anna.trim())
            .is_some()
    );

    // init into an existing token file is refused before the data directory is created.
    let other = parent.path().join("other");
    let o = out(
        bin()
            .args(["server", "init"])
            .arg(&other)
            .arg("--token-file")
            .arg(&file),
        None,
    );
    assert_eq!(o.code, 1);
    assert!(!other.join("access.toml").exists());
}
