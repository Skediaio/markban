#![allow(clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

fn bin() -> Command {
    let mut cmd = Command::cargo_bin("markban").unwrap();
    cmd.env_remove("MARKBAN_BOARD")
        .env_remove("MARKBAN_URL")
        .env_remove("MARKBAN_TOKEN")
        .env_remove("MARKBAN_DATA");
    cmd
}

fn out(o: &std::process::Output) -> Out {
    Out {
        stdout: String::from_utf8(o.stdout.clone()).unwrap(),
        stderr: String::from_utf8(o.stderr.clone()).unwrap(),
        code: o.status.code().unwrap(),
    }
}

/// Runs `markban -C dir args...`.
fn mb(dir: &Path, args: &[&str]) -> Out {
    out(&bin().arg("-C").arg(dir).args(args).output().unwrap())
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let o = mb(dir, args);
    assert_eq!(o.code, 0, "args {args:?}: stderr {}", o.stderr);
    assert_eq!(o.stderr, "");
    o.stdout
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(dir.join(rel)).unwrap()
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn demo() -> PathBuf {
    repo_root().join("examples/demo-board")
}

fn tree(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, acc: &mut Vec<(String, Vec<u8>)>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(base, &path, acc);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                acc.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut acc = Vec::new();
    walk(dir, dir, &mut acc);
    acc
}

fn init_board() -> TempDir {
    let dir = TempDir::new().unwrap();
    assert_eq!(
        ok(dir.path(), &["init", "Demo", "Todo", "Doing", "Done"]),
        "Demo\n"
    );
    dir
}

#[test]
fn init_creates_board_with_exact_bytes() {
    let dir = init_board();
    assert_eq!(
        read(dir.path(), "board.md"),
        "---\nmarkban: 0.1\ntitle: Demo\ncolumns:\n  - id: todo\n    name: Todo\n  - id: doing\n    name: Doing\n  - id: done\n    name: Done\n---\n"
    );
    let again = mb(dir.path(), &["init", "Other", "A"]);
    assert_eq!(again.code, 1);
    assert_eq!(again.stdout, "");
    assert!(again.stderr.starts_with("error: "), "{}", again.stderr);
    assert_eq!(again.stderr.lines().count(), 1);
}

#[test]
fn init_defaults_to_current_directory() {
    let dir = TempDir::new().unwrap();
    let o = out(&bin()
        .current_dir(dir.path())
        .args(["init", "Here", "Todo"])
        .output()
        .unwrap());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(dir.path().join("board.md").is_file());
}

#[test]
fn add_prints_id_and_writes_exact_card() {
    let dir = init_board();
    assert_eq!(
        ok(dir.path(), &["add", "Write the docs", "-d", "Some text."]),
        "write-the-docs\n"
    );
    assert_eq!(
        read(dir.path(), "cards/write-the-docs.md"),
        "---\ntitle: Write the docs\ncolumn: todo\norder: 1000\n---\n\nSome text.\n"
    );
    assert_eq!(
        ok(dir.path(), &["add", "Second", "-c", "Doing", "--id", "two"]),
        "two\n"
    );
    assert!(read(dir.path(), "cards/two.md").contains("column: doing\n"));
}

#[test]
fn add_reads_description_from_stdin() {
    let dir = init_board();
    let o = out(&bin()
        .arg("-C")
        .arg(dir.path())
        .args(["add", "From stdin", "-d", "-"])
        .write_stdin("line one\nline two\n")
        .output()
        .unwrap());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "from-stdin\n");
    assert!(read(dir.path(), "cards/from-stdin.md").ends_with("---\n\nline one\nline two\n"));
}

#[test]
fn add_positions() {
    let dir = init_board();
    ok(dir.path(), &["add", "A", "--id", "a"]);
    ok(dir.path(), &["add", "B", "--id", "b"]);
    ok(dir.path(), &["add", "Top", "--id", "t", "--top"]);
    ok(dir.path(), &["add", "Mid", "--id", "m", "--after", "t"]);
    ok(dir.path(), &["add", "Pre", "--id", "p", "--before", "b"]);
    let ls = ok(dir.path(), &["ls"]);
    let ids: Vec<&str> = ls
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split("  ").next().unwrap())
        .collect();
    assert_eq!(ids, ["t", "m", "a", "p", "b"]);
}

#[test]
fn mv_edit_rm_flow() {
    let dir = init_board();
    ok(dir.path(), &["add", "Task", "--id", "task"]);
    assert_eq!(ok(dir.path(), &["mv", "task", "Doing"]), "task\n");
    let card = read(dir.path(), "cards/task.md");
    assert!(
        card.starts_with("---\ntitle: Task\ncolumn: doing\norder: "),
        "{card}"
    );
    ok(
        dir.path(),
        &["add", "Other", "--id", "other", "-c", "doing"],
    );
    assert_eq!(ok(dir.path(), &["mv", "other", "--top"]), "other\n");
    let ls = ok(dir.path(), &["ls", "doing"]);
    assert_eq!(ls, "# Demo\n## Doing (doing) 2\nother  Other\ntask  Task\n");

    assert_eq!(
        ok(
            dir.path(),
            &["edit", "task", "--title", "Renamed", "-d", "Body"]
        ),
        "task\n"
    );
    assert_eq!(
        ok(dir.path(), &["edit", "task", "--append", "More"]),
        "task\n"
    );
    let show = ok(dir.path(), &["show", "task"]);
    assert_eq!(show, "task  Renamed\ncolumn: doing\n\nBody\nMore\n");

    assert_eq!(ok(dir.path(), &["rm", "task"]), "task\n");
    assert!(!dir.path().join("cards/task.md").exists());
}

#[test]
fn col_operations() {
    let dir = init_board();
    ok(dir.path(), &["add", "X", "--id", "x", "-c", "done"]);
    assert_eq!(
        ok(dir.path(), &["col", "add", "Review", "--before", "Done"]),
        "review\n"
    );
    assert_eq!(
        ok(dir.path(), &["col", "add", "Later", "--id", "someday"]),
        "someday\n"
    );
    assert_eq!(
        ok(dir.path(), &["col", "rename", "someday", "Someday"]),
        "someday\n"
    );
    assert_eq!(
        ok(dir.path(), &["col", "mv", "someday", "--before", "todo"]),
        "someday\n"
    );
    assert_eq!(
        ok(dir.path(), &["ls"]),
        "# Demo\n## Someday (someday) 0\n## Todo (todo) 0\n## Doing (doing) 0\n## Review (review) 0\n## Done (done) 1\nx  X\n"
    );
    let refused = mb(dir.path(), &["col", "rm", "done"]);
    assert_eq!(refused.code, 1);
    assert!(
        refused
            .stderr
            .starts_with("error: column 'done' still has 1 card(s)"),
        "{}",
        refused.stderr
    );
    assert_eq!(
        ok(dir.path(), &["col", "rm", "done", "--move-to", "review"]),
        "done\n"
    );
    assert!(read(dir.path(), "cards/x.md").contains("column: review\n"));
}

#[test]
fn errors_are_one_line_on_stderr_with_exit_1() {
    let dir = init_board();
    let o = mb(dir.path(), &["mv", "ghost", "Doing"]);
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert_eq!(
        o.stderr,
        "error: no card 'ghost'; card ids are the file names in cards/ without .md\n"
    );
    let o = mb(dir.path(), &["add", "T", "-c", "Nope"]);
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stderr,
        "error: unknown column 'Nope'; valid columns: todo (Todo), doing (Doing), done (Done)\n"
    );
}

#[test]
fn usage_errors_exit_2() {
    let dir = init_board();
    assert_eq!(mb(dir.path(), &["frobnicate"]).code, 2);
    assert_eq!(mb(dir.path(), &["add"]).code, 2);
    assert_eq!(
        mb(dir.path(), &["add", "T", "--top", "--before", "x"]).code,
        2
    );
}

#[test]
fn ls_on_demo_board_is_exact() {
    let o = mb(&demo(), &["ls"]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        o.stdout,
        "# Markban roadmap\n## Backlog (backlog) 0\n## Doing (doing) 0\n## Done (done) 3\n0001-spec-draft  Write spec v0.1\n0002-parser  Core library and CLI\n0003-board-ui  Read-only board viewer\n"
    );
}

#[test]
fn ls_json_is_the_board_view() {
    let o = mb(&demo(), &["ls", "--json"]);
    assert_eq!(o.code, 0);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["title"], "Markban roadmap");
    assert_eq!(v["columns"][0]["id"], "backlog");
    assert_eq!(v["columns"][2]["id"], "done");
    assert_eq!(v["columns"][2]["cards"][0]["id"], "0001-spec-draft");
}

#[test]
fn ls_shows_unsorted_and_diagnostics() {
    let dir = init_board();
    ok(dir.path(), &["add", "A", "--id", "a"]);
    std::fs::write(
        dir.path().join("cards/lost.md"),
        "---\ntitle: Lost\ncolumn: gone\n---\n",
    )
    .unwrap();
    let o = mb(dir.path(), &["ls"]);
    assert_eq!(o.code, 0);
    assert!(
        o.stdout.contains("## Unsorted 1\nlost  Lost\n"),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout.ends_with("\n! 1 problem(s); run markban check\n"),
        "{}",
        o.stdout
    );
    assert!(!o.stdout.contains("E007"), "{}", o.stdout);
    let json = mb(dir.path(), &["ls", "--json"]);
    assert!(json.stdout.contains("E007"), "{}", json.stdout);
}

#[test]
fn append_reads_from_stdin() {
    let dir = init_board();
    ok(dir.path(), &["add", "Task", "--id", "task", "-d", "Body"]);
    let o = out(&bin()
        .arg("-C")
        .arg(dir.path())
        .args(["edit", "task", "--append", "-"])
        .write_stdin("From stdin\n")
        .output()
        .unwrap());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        ok(dir.path(), &["show", "task"]),
        "task  Task\ncolumn: todo\n\nBody\nFrom stdin\n"
    );
}

#[test]
fn terminal_escapes_in_board_content_are_neutralised_in_text_mode() {
    let dir = init_board();
    std::fs::create_dir_all(dir.path().join("cards")).unwrap();
    std::fs::write(
        dir.path().join("cards/evil.md"),
        "---\ntitle: Evil \u{1b}[31mred\u{9b}\ncolumn: todo\norder: 1000\n---\n\nbody \u{1b}]0;pwned\u{7}\tkept\n",
    )
    .unwrap();
    for args in [&["ls"][..], &["show", "evil"], &["check"]] {
        let o = mb(dir.path(), args);
        assert!(
            !o.stdout
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
            "{args:?}: {:?}",
            o.stdout
        );
    }
    let show = mb(dir.path(), &["show", "evil"]);
    assert!(
        show.stdout.contains("Evil \u{fffd}[31mred\u{fffd}"),
        "{:?}",
        show.stdout
    );
    assert!(show.stdout.contains("\tkept"), "{:?}", show.stdout);
    let json = mb(dir.path(), &["show", "evil", "--json"]);
    assert!(json.stdout.contains("\\u001b[31mred"), "{:?}", json.stdout);
}

#[test]
fn show_prints_card_and_json() {
    let o = mb(&demo(), &["show", "0001-spec-draft"]);
    assert_eq!(o.code, 0);
    assert_eq!(
        o.stdout,
        "0001-spec-draft  Write spec v0.1\ncolumn: done\n\nThe format in `spec/SPEC.md`, chosen after the experiments in `research/format-experiments`.\n"
    );
    let o = mb(&demo(), &["show", "0001-spec-draft", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&o.stdout).unwrap();
    assert_eq!(v["title"], "Write spec v0.1");
    assert_eq!(v["column"], "done");
    let o = mb(&demo(), &["show", "nope"]);
    assert_eq!(o.code, 1);
    assert!(o.stderr.starts_with("error: no card 'nope'"));
}

#[test]
fn check_demo_board_passes() {
    let o = out(&bin().arg("check").arg(demo()).output().unwrap());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "");
}

#[test]
fn check_invalid_fixture_fails() {
    let fixture = repo_root().join("crates/markban-core/tests/fixtures/invalid/E007");
    let o = out(&bin().arg("check").arg(&fixture).output().unwrap());
    assert_eq!(o.code, 1);
    assert_eq!(
        o.stdout,
        "cards/x.md:3: E007 card column 'dong' is not a column; columns: todo, done\n"
    );
}

#[test]
fn check_warnings_exit_0() {
    let dir = init_board();
    std::fs::create_dir_all(dir.path().join("cards")).unwrap();
    std::fs::write(dir.path().join("cards/notes.txt"), "x").unwrap();
    let o = mb(dir.path(), &["check"]);
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("W001"), "{}", o.stdout);
}

#[test]
fn run_applies_all_commands_and_prints_ids() {
    let dir = init_board();
    let script = "# a comment\n\n{\"cmd\":\"card.add\",\"title\":\"One\"}\n{\"cmd\":\"card.add\",\"title\":\"Two\",\"column\":\"doing\"}\n";
    let o = out(&bin()
        .arg("-C")
        .arg(dir.path())
        .args(["run", "-"])
        .write_stdin(script)
        .output()
        .unwrap());
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "one\ntwo\n");
    assert!(dir.path().join("cards/two.md").is_file());
}

#[test]
fn run_reads_a_file_and_can_init_a_board() {
    let dir = TempDir::new().unwrap();
    let script = dir.path().join("script.jsonl");
    std::fs::write(
        &script,
        "{\"cmd\":\"board.init\",\"title\":\"New\",\"columns\":[\"A\",\"B\"]}\n{\"cmd\":\"card.add\",\"title\":\"Hi\"}\n",
    )
    .unwrap();
    let board = dir.path().join("board");
    std::fs::create_dir(&board).unwrap();
    let o = mb(&board, &["run", script.to_str().unwrap()]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(o.stdout, "New\nhi\n");
}

#[test]
fn run_is_all_or_nothing_and_names_the_line() {
    let dir = init_board();
    let before = tree(dir.path());
    let script = "{\"cmd\":\"card.add\",\"title\":\"One\"}\n# skip\n{\"cmd\":\"card.move\",\"id\":\"ghost\"}\n";
    let o = out(&bin()
        .arg("-C")
        .arg(dir.path())
        .args(["run", "-"])
        .write_stdin(script)
        .output()
        .unwrap());
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(o.stderr.starts_with("error: line 3: "), "{}", o.stderr);
    assert_eq!(o.stderr.lines().count(), 1);
    assert_eq!(tree(dir.path()), before);
}

#[test]
fn run_reports_unparseable_line() {
    let dir = init_board();
    let before = tree(dir.path());
    let script = "{\"cmd\":\"card.add\",\"title\":\"One\"}\nnot json\n";
    let o = out(&bin()
        .arg("-C")
        .arg(dir.path())
        .args(["run", "-"])
        .write_stdin(script)
        .output()
        .unwrap());
    assert_eq!(o.code, 1);
    assert!(o.stderr.starts_with("error: line 2: "), "{}", o.stderr);
    assert_eq!(tree(dir.path()), before);
}

#[test]
fn same_script_gives_byte_identical_trees() {
    let script = "{\"cmd\":\"board.init\",\"title\":\"T\",\"columns\":[\"A\",\"B\"]}\n{\"cmd\":\"card.add\",\"title\":\"One\"}\n{\"cmd\":\"card.add\",\"title\":\"Two\"}\n{\"cmd\":\"card.move\",\"id\":\"two\",\"top\":true}\n{\"cmd\":\"card.move\",\"id\":\"one\",\"column\":\"B\"}\n";
    let run_in = |dir: &Path| {
        let o = out(&bin()
            .arg("-C")
            .arg(dir)
            .args(["run", "-"])
            .write_stdin(script)
            .output()
            .unwrap());
        assert_eq!(o.code, 0, "{}", o.stderr);
    };
    let (a, b) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    run_in(a.path());
    run_in(b.path());
    assert_eq!(tree(a.path()), tree(b.path()));
}

#[test]
fn discovery_via_flag_env_and_walk_up() {
    let dir = init_board();
    ok(dir.path(), &["add", "A", "--id", "a"]);
    let want = "# Demo\n## Todo (todo) 1\na  A\n## Doing (doing) 0\n## Done (done) 0\n";
    assert_eq!(ok(dir.path(), &["ls"]), want);

    let env = out(&bin()
        .env("MARKBAN_BOARD", dir.path())
        .current_dir(repo_root())
        .arg("ls")
        .output()
        .unwrap());
    assert_eq!(env.stdout, want, "{}", env.stderr);

    let deep = dir.path().join("cards");
    let up = out(&bin().current_dir(&deep).arg("ls").output().unwrap());
    assert_eq!(up.stdout, want, "{}", up.stderr);

    // -C beats the environment.
    let flag = out(&bin()
        .env("MARKBAN_BOARD", dir.path())
        .arg("-C")
        .arg(demo())
        .arg("ls")
        .output()
        .unwrap());
    assert!(flag.stdout.starts_with("# Markban roadmap\n"));
}

#[test]
fn discovery_failure_is_a_one_line_error() {
    let dir = TempDir::new().unwrap();
    let o = out(&bin().current_dir(dir.path()).arg("ls").output().unwrap());
    assert_eq!(o.code, 1);
    assert_eq!(o.stdout, "");
    assert!(
        o.stderr.starts_with("error: no board.md found"),
        "{}",
        o.stderr
    );
    assert_eq!(o.stderr.lines().count(), 1);
}

#[test]
fn edit_description_and_append_conflict() {
    let dir = init_board();
    ok(dir.path(), &["add", "A", "-c", "Todo", "--id", "a"]);
    let o = mb(dir.path(), &["edit", "a", "-d", "-", "--append", "-"]);
    assert_eq!(o.code, 2, "stderr {}", o.stderr);
    assert!(o.stderr.contains("cannot be used with"), "{}", o.stderr);
}
