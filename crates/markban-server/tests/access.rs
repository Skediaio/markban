#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The admin side: `access.toml`, users, tokens and boards, by direct file access.

use markban_server::access::{self, Access};
use tempfile::TempDir;

fn cols() -> Vec<String> {
    vec!["Todo".into(), "Done".into()]
}

fn read_access(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join("access.toml")).unwrap()
}

#[test]
fn init_creates_the_layout_and_one_admin() {
    let dir = TempDir::new().unwrap();
    let token = access::init(dir.path(), "alice").unwrap();
    assert!(dir.path().join("boards").is_dir());
    assert!(dir.path().join("log").is_dir());
    let text = read_access(&dir);
    assert!(text.contains("[users.alice]"), "{text}");
    assert!(text.contains("admin = true"), "{text}");
    assert!(text.contains(r#"boards = ["*"]"#), "{text}");
    let parsed = Access::load(dir.path()).unwrap();
    let user = parsed.authenticate(&token).unwrap();
    assert_eq!(user.name, "alice");
    assert!(user.admin);
}

#[test]
fn init_refuses_an_existing_access_file() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    let before = read_access(&dir);
    let err = access::init(dir.path(), "other").unwrap_err();
    assert!(err.contains("already"), "{err}");
    assert_eq!(read_access(&dir), before);
}

#[test]
fn tokens_are_random_long_and_stored_only_as_sha256() {
    let dir = TempDir::new().unwrap();
    let admin = access::init(dir.path(), "alice").unwrap();
    access::add_board(dir.path(), "acme", "Acme", &cols()).unwrap();
    let anna = access::add_user(dir.path(), "anna", &["acme".into()], false).unwrap();
    assert_ne!(admin, anna);
    for token in [&admin, &anna] {
        // 32 random bytes as hex after a short prefix.
        let hex = token.strip_prefix("mb_").unwrap();
        assert_eq!(hex.len(), 64, "{token}");
        assert!(hex.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    let text = read_access(&dir);
    for token in [&admin, &anna] {
        assert!(!text.contains(token.as_str()), "token stored in clear");
        assert!(!text.contains(token.strip_prefix("mb_").unwrap()));
        assert!(text.contains(&access::hash_token(token)));
    }
}

#[cfg(unix)]
#[test]
fn access_file_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    access::add_user(dir.path(), "bob", &["*".into()], false).unwrap();
    let mode = std::fs::metadata(dir.path().join("access.toml"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
}

#[test]
fn authenticate_rejects_wrong_and_near_tokens() {
    let dir = TempDir::new().unwrap();
    let token = access::init(dir.path(), "alice").unwrap();
    let parsed = Access::load(dir.path()).unwrap();
    assert!(parsed.authenticate(&token).is_some());
    let mut near = token.clone();
    near.pop();
    near.push('0');
    for bad in ["", "mb_", near.as_str(), &token[..10], &format!("{token}x")] {
        if bad != token {
            assert!(parsed.authenticate(bad).is_none(), "{bad}");
        }
    }
    // The stored hash itself is not a token.
    assert!(parsed.authenticate(&access::hash_token(&token)).is_none());
}

#[test]
fn user_boards_and_star() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    for id in ["a", "b"] {
        access::add_board(dir.path(), id, id, &cols()).unwrap();
    }
    let anna = access::add_user(dir.path(), "anna", &["a".into()], false).unwrap();
    let parsed = Access::load(dir.path()).unwrap();
    let user = parsed.authenticate(&anna).unwrap();
    assert!(user.may_use("a"));
    assert!(!user.may_use("b"));
    assert!(!user.admin);

    access::set_boards(dir.path(), "anna", &["*".into()]).unwrap();
    let user = Access::load(dir.path())
        .unwrap()
        .authenticate(&anna)
        .unwrap()
        .clone();
    assert!(user.may_use("a") && user.may_use("b"));

    access::set_boards(dir.path(), "anna", &["b".into()]).unwrap();
    let parsed = Access::load(dir.path()).unwrap();
    let user = parsed.authenticate(&anna).unwrap();
    assert!(!user.may_use("a") && user.may_use("b"));
}

#[test]
fn user_admin_commands_validate_their_input() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    access::add_board(dir.path(), "a", "A", &cols()).unwrap();
    let before = read_access(&dir);
    for (name, boards) in [
        ("Anna", vec!["a".to_string()]),
        ("../x", vec!["a".into()]),
        ("", vec!["a".into()]),
        ("anna", vec!["nope".into()]),
        ("anna", vec!["../a".into()]),
        ("alice", vec!["a".into()]),
    ] {
        let err = access::add_user(dir.path(), name, &boards, false).unwrap_err();
        assert!(!err.contains('\n'), "{err}");
    }
    assert!(access::remove_user(dir.path(), "ghost").is_err());
    assert!(access::set_boards(dir.path(), "ghost", &["a".into()]).is_err());
    assert!(access::set_boards(dir.path(), "alice", &["nope".into()]).is_err());
    assert_eq!(read_access(&dir), before, "refused edits change nothing");
}

#[test]
fn remove_user_revokes_the_token() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    let bob = access::add_user(dir.path(), "bob", &["*".into()], false).unwrap();
    access::remove_user(dir.path(), "bob").unwrap();
    assert!(
        Access::load(dir.path())
            .unwrap()
            .authenticate(&bob)
            .is_none()
    );
}

#[test]
fn edits_keep_comments_and_unknown_keys() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    let path = dir.path().join("access.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\n# keep me\n[users.alice.extra]\nnote = \"x\"\n");
    std::fs::write(&path, &text).unwrap();
    access::add_user(dir.path(), "bob", &["*".into()], false).unwrap();
    let after = std::fs::read_to_string(&path).unwrap();
    assert!(after.starts_with(&text), "{after}");
    assert!(after.contains("[users.bob]"));
}

#[test]
fn malformed_access_file_is_refused_not_guessed() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    let path = dir.path().join("access.toml");
    for bad in [
        "[users.x]\ntoken_sha256 = \"short\"\nboards = [\"*\"]\n",
        "[users.x]\nboards = [\"*\"]\n",
        "[users.x]\ntoken_sha256 = \"00\"\nboards = \"*\"\n",
        "[users.Bad]\ntoken_sha256 = \"0000000000000000000000000000000000000000000000000000000000000000\"\nboards = [\"*\"]\n",
        "not toml [[[",
    ] {
        std::fs::write(&path, bad).unwrap();
        let err = Access::load(dir.path()).unwrap_err();
        assert!(err.contains("access.toml"), "{bad}: {err}");
        assert!(!err.contains('\n'), "{err}");
    }
}

#[test]
fn board_add_and_list() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    access::add_board(dir.path(), "zeta", "Zeta board", &cols()).unwrap();
    access::add_board(dir.path(), "alpha", "Alpha", &cols()).unwrap();
    assert!(dir.path().join("boards/alpha/board.md").is_file());
    assert_eq!(
        access::list_boards(dir.path()).unwrap(),
        vec![
            ("alpha".to_string(), "Alpha".to_string()),
            ("zeta".to_string(), "Zeta board".to_string())
        ]
    );
    let err = access::add_board(dir.path(), "alpha", "Again", &cols()).unwrap_err();
    assert!(err.contains("already"), "{err}");
    for bad in ["Alpha", "../x", "a/b", "", "a--b"] {
        assert!(
            access::add_board(dir.path(), bad, "X", &cols()).is_err(),
            "{bad}"
        );
    }
    assert!(!dir.path().join("x").exists());
}

#[test]
fn concurrent_admin_commands_lose_no_edit() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    let per_thread = 12;
    let tokens: Vec<Vec<String>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|who| {
                let data = dir.path();
                scope.spawn(move || {
                    (0..per_thread)
                        .map(|i| {
                            access::add_user(data, &format!("{who}{i}"), &["*".into()], false)
                                .unwrap_or_else(|e| panic!("{who}{i}: {e}"))
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let parsed = Access::load(dir.path()).unwrap();
    for (who, tokens) in ["a", "b"].into_iter().zip(&tokens) {
        for (i, token) in tokens.iter().enumerate() {
            let user = parsed.authenticate(token);
            assert_eq!(
                user.map(|u| u.name.as_str()),
                Some(format!("{who}{i}").as_str()),
                "lost {who}{i}"
            );
        }
    }
    // The lock file is tool-owned and stays private.
    assert!(dir.path().join("access.toml.lock").is_file());
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[cfg(unix)]
#[test]
fn the_data_directory_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let parent = TempDir::new().unwrap();
    let data = parent.path().join("data");
    access::init(&data, "alice").unwrap();
    for sub in ["", "boards", "log"] {
        let mode = std::fs::metadata(data.join(sub))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700, "{sub:?}: mode {mode:o}");
    }
}

#[test]
fn board_remove_moves_the_board_and_its_log_and_drops_the_grants() {
    let dir = TempDir::new().unwrap();
    access::init(dir.path(), "alice").unwrap();
    for id in ["a", "b"] {
        access::add_board(dir.path(), id, id, &cols()).unwrap();
    }
    let anna = access::add_user(dir.path(), "anna", &["a".into(), "b".into()], false).unwrap();
    let bob = access::add_user(dir.path(), "bob", &["b".into()], false).unwrap();
    std::fs::write(dir.path().join("log/b.jsonl"), "{}\n").unwrap();

    let moved = access::remove_board(dir.path(), "b").unwrap();
    assert!(!dir.path().join("boards/b").exists());
    assert!(!dir.path().join("log/b.jsonl").exists());
    assert_eq!(moved.parent().unwrap(), dir.path().join("removed"));
    let name = moved.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with("b-") && name.len() > 10, "{name}");
    assert!(moved.join("board.md").is_file());
    assert_eq!(
        std::fs::read_to_string(moved.join("log.jsonl")).unwrap(),
        "{}\n"
    );

    let parsed = Access::load(dir.path()).unwrap();
    let a = parsed.authenticate(&anna).unwrap();
    assert!(a.may_use("a") && !a.may_use("b"));
    // Bob keeps his token but no board.
    let b = parsed.authenticate(&bob).unwrap();
    assert!(!b.may_use("b"));
    // The admin with "*" keeps "*".
    assert!(read_access(&dir).contains(r#"boards = ["*"]"#));

    // A board of the same id later is a new board without the old grants.
    access::add_board(dir.path(), "b", "New b", &cols()).unwrap();
    let parsed = Access::load(dir.path()).unwrap();
    assert!(!parsed.authenticate(&bob).unwrap().may_use("b"));

    // Removing twice moves to a distinct place; an unknown board is an error.
    let again = access::remove_board(dir.path(), "b").unwrap();
    assert_ne!(again, moved);
    for bad in ["b", "ghost", "../a", ""] {
        let err = access::remove_board(dir.path(), bad).unwrap_err();
        assert!(!err.contains('\n'), "{err}");
    }
    assert!(dir.path().join("boards/a/board.md").is_file());
}

#[test]
fn a_token_can_be_written_to_a_new_private_file() {
    let parent = TempDir::new().unwrap();
    let data = parent.path().join("data");
    let file = parent.path().join("admin.token");
    let token = access::init(&data, "alice").unwrap();
    access::write_token_file(&file, &token).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("{token}\n")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let err = access::write_token_file(&file, "mb_other").unwrap_err();
    assert!(err.contains("exists"), "{err}");
    assert!(!err.contains("mb_other"));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        format!("{token}\n")
    );
}
