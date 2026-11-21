//! Serves a board that lives in a git repository (`MARKBAN_GIT_URL`).
//!
//! At start the repository is cloned shallowly; then every interval it is fetched (`--depth 1`)
//! and reset hard to the remote branch. The viewer never writes, so the reset is safe. The file
//! watcher sees the changed files and the page refreshes as for a local board.
//!
//! A token (`MARKBAN_GIT_TOKEN`) goes to git as an `http.extraHeader` through the
//! `GIT_CONFIG_*` environment variables, never through the URL or the arguments. Every message
//! from git is redacted before it is shown.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// Where and how to fetch the board.
#[derive(Debug, Clone)]
pub struct GitSource {
    pub url: String,
    pub token: Option<String>,
    pub branch: String,
    /// The board directory inside the repository (relative, no `..`).
    pub subdir: PathBuf,
    pub interval: Duration,
    /// The local clone.
    pub dir: PathBuf,
}

impl GitSource {
    /// Reads the source from the environment. `None` when `MARKBAN_GIT_URL` is unset or empty.
    ///
    /// # Errors
    /// A message when a variable has an invalid value.
    pub fn from_env() -> Result<Option<Self>, String> {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let Some(url) = var("MARKBAN_GIT_URL") else {
            return Ok(None);
        };
        let interval = match var("MARKBAN_GIT_INTERVAL") {
            None => 30,
            Some(v) => v
                .trim()
                .parse::<u64>()
                .ok()
                .filter(|s| *s > 0)
                .ok_or_else(|| {
                    format!(
                        "MARKBAN_GIT_INTERVAL must be a whole number of seconds above 0, got '{v}'"
                    )
                })?,
        };
        let subdir = PathBuf::from(var("MARKBAN_GIT_SUBDIR").unwrap_or_else(|| ".".to_string()));
        check_subdir(&subdir)?;
        let dir = var("MARKBAN_GIT_DIR")
            .map_or_else(|| std::env::temp_dir().join("markban-git"), PathBuf::from);
        Ok(Some(Self {
            url,
            token: var("MARKBAN_GIT_TOKEN"),
            branch: var("MARKBAN_GIT_BRANCH").unwrap_or_else(|| "main".to_string()),
            subdir,
            interval: Duration::from_secs(interval),
            dir,
        }))
    }

    /// The board directory: the clone plus the subdirectory.
    #[must_use]
    pub fn board_dir(&self) -> PathBuf {
        self.dir.join(&self.subdir)
    }

    /// A `git` command with the token (if any) passed through the environment.
    #[must_use]
    pub fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.args(args).env("GIT_TERMINAL_PROMPT", "0");
        if let Some(token) = &self.token {
            use base64::Engine;
            let basic =
                base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
            cmd.env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", "http.extraHeader")
                .env(
                    "GIT_CONFIG_VALUE_0",
                    format!("Authorization: Basic {basic}"),
                );
        }
        cmd
    }

    /// The commands one sync runs, in order: a shallow clone when there is no clone yet,
    /// otherwise a shallow fetch of the branch and a hard reset to it.
    #[must_use]
    pub fn sync_commands(&self) -> Vec<Command> {
        let dir = self.dir.to_string_lossy();
        let branch = self.branch.as_str();
        if !self.dir.join(".git").exists() {
            return vec![self.command(&[
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--single-branch",
                "--branch",
                branch,
                "--",
                &self.url,
                &dir,
            ])];
        }
        let refspec = format!("+refs/heads/{branch}:refs/remotes/origin/{branch}");
        let target = format!("refs/remotes/origin/{branch}");
        vec![
            self.command(&["-C", &dir, "remote", "set-url", "origin", &self.url]),
            self.command(&[
                "-C", &dir, "fetch", "--quiet", "--depth", "1", "origin", "--", &refspec,
            ]),
            self.command(&["-C", &dir, "reset", "--quiet", "--hard", &target]),
        ]
    }

    /// Brings the local clone up to date with the remote branch.
    ///
    /// # Errors
    /// The redacted message of the git command that failed.
    pub fn sync(&self) -> Result<(), String> {
        if let Some(parent) = self.dir.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
        }
        for mut cmd in self.sync_commands() {
            let out = cmd.output().map_err(|e| format!("cannot run git: {e}"))?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(self.redact(&format!("git failed: {}", stderr.trim())));
            }
        }
        Ok(())
    }

    /// Removes the token and any credentials in URLs from `text`.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let text = strip_url_credentials(text);
        match &self.token {
            Some(token) if !token.is_empty() => text.replace(token.as_str(), "***"),
            _ => text,
        }
    }
}

/// Logs a failed sync, keeps the last good clone and tries again every interval.
pub fn spawn(source: GitSource) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(source.interval);
            if let Err(e) = source.sync() {
                eprintln!("markban: git sync failed, serving the last good board: {e}");
            }
        }
    });
}

/// Rejects an absolute subdirectory or one that leaves the clone.
fn check_subdir(subdir: &Path) -> Result<(), String> {
    if subdir
        .components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
    {
        Ok(())
    } else {
        Err(format!(
            "MARKBAN_GIT_SUBDIR must be a relative path inside the repository, got '{}'",
            subdir.display()
        ))
    }
}

/// Removes the user-info part (`user:password@`) of every URL in `text`.
#[must_use]
pub fn strip_url_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at.saturating_add(3));
        out.push_str(head);
        // The authority ends at the first '/', quote, whitespace or other delimiter.
        let end = tail
            .find(|c: char| c == '/' || c == '\'' || c == '"' || c.is_whitespace())
            .unwrap_or(tail.len());
        let (authority, after) = tail.split_at(end);
        match authority.rfind('@') {
            Some(i) => out.push_str(authority.get(i.saturating_add(1)..).unwrap_or_default()),
            None => out.push_str(authority),
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)] // tests may expect and index
mod tests {
    use super::*;
    use std::ffi::OsStr;

    const TOKEN: &str = "s3cr3t-t0ken";

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args([
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A bare repository with one commit holding `board/board.md`, and a work tree to push from.
    fn remote() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().expect("tempdir");
        let bare = tmp.path().join("remote.git");
        let work = tmp.path().join("work");
        std::fs::create_dir_all(&work).expect("work");
        git(tmp.path(), &["init", "--bare", "-b", "main", "remote.git"]);
        git(&work, &["init", "-b", "main"]);
        std::fs::create_dir_all(work.join("board/cards")).expect("board");
        std::fs::write(
            work.join("board/board.md"),
            "---\nmarkban: 0.1\ntitle: One\n---\n",
        )
        .expect("board.md");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-m", "one"]);
        git(
            &work,
            &["remote", "add", "origin", bare.to_str().expect("utf8")],
        );
        git(&work, &["push", "origin", "main"]);
        (tmp, bare, work)
    }

    fn source(url: &str, dir: PathBuf, token: Option<&str>) -> GitSource {
        GitSource {
            url: url.to_string(),
            token: token.map(str::to_string),
            branch: "main".to_string(),
            subdir: PathBuf::from("board"),
            interval: Duration::from_secs(30),
            dir,
        }
    }

    #[test]
    fn sync_clones_then_follows_new_commits() {
        let (tmp, bare, work) = remote();
        let url = format!("file://{}", bare.display());
        let src = source(&url, tmp.path().join("clone"), None);
        src.sync().expect("first sync clones");
        let board = src.board_dir().join("board.md");
        assert!(
            std::fs::read_to_string(&board)
                .expect("cloned")
                .contains("title: One"),
            "cloned board"
        );
        std::fs::write(
            work.join("board/board.md"),
            "---\nmarkban: 0.1\ntitle: Two\n---\n",
        )
        .expect("edit");
        std::fs::write(work.join("board/cards/a.md"), "---\ntitle: A\n---\n").expect("card");
        git(&work, &["add", "."]);
        git(&work, &["commit", "-m", "two"]);
        git(&work, &["push", "origin", "main"]);
        src.sync().expect("second sync fetches");
        assert!(
            std::fs::read_to_string(&board)
                .expect("synced")
                .contains("title: Two"),
            "synced board"
        );
        assert!(src.board_dir().join("cards/a.md").exists(), "new card");
    }

    #[test]
    fn failed_sync_keeps_the_last_good_clone() {
        let (tmp, bare, _work) = remote();
        let url = format!("file://{}", bare.display());
        let src = source(&url, tmp.path().join("clone"), None);
        src.sync().expect("clone");
        std::fs::rename(&bare, tmp.path().join("gone.git")).expect("move remote away");
        assert!(src.sync().is_err(), "fetch from a missing remote fails");
        assert!(
            src.board_dir().join("board.md").exists(),
            "last good board stays"
        );
    }

    #[test]
    fn token_is_passed_in_the_environment_never_in_args() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = source(
            "https://git.example/o/r.git",
            tmp.path().join("clone"),
            Some(TOKEN),
        );
        let check = |cmd: &Command| {
            assert_eq!(cmd.get_program(), OsStr::new("git"));
            for arg in cmd.get_args() {
                assert!(
                    !arg.to_string_lossy().contains(TOKEN),
                    "token in argument {arg:?}"
                );
            }
            let envs: Vec<(String, String)> = cmd
                .get_envs()
                .filter_map(|(k, v)| {
                    Some((
                        k.to_string_lossy().into_owned(),
                        v?.to_string_lossy().into_owned(),
                    ))
                })
                .collect();
            let get = |k: &str| {
                envs.iter()
                    .find(|(key, _)| key == k)
                    .map(|(_, v)| v.clone())
            };
            assert_eq!(get("GIT_CONFIG_COUNT").as_deref(), Some("1"));
            assert_eq!(get("GIT_CONFIG_KEY_0").as_deref(), Some("http.extraHeader"));
            let value = get("GIT_CONFIG_VALUE_0").expect("header value");
            assert!(value.starts_with("Authorization: "), "{value}");
            assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
        };
        // No clone yet: the clone command.
        let commands = src.sync_commands();
        assert_eq!(commands.len(), 1);
        assert!(commands[0].get_args().any(|a| a == "clone"));
        commands.iter().for_each(check);
        // With a clone: set-url, fetch and reset.
        std::fs::create_dir_all(src.dir.join(".git")).expect("fake clone");
        let commands = src.sync_commands();
        assert!(commands.iter().any(|c| c.get_args().any(|a| a == "fetch")));
        assert!(commands.iter().any(|c| c.get_args().any(|a| a == "reset")));
        commands.iter().for_each(check);
    }

    #[test]
    fn no_token_means_no_auth_header() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = source(
            "https://git.example/o/r.git",
            tmp.path().join("clone"),
            None,
        );
        let cmd = src.command(&["status"]);
        assert!(!cmd.get_envs().any(|(k, _)| k == "GIT_CONFIG_VALUE_0"));
    }

    #[test]
    fn errors_are_redacted() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = source(
            "https://git.example/o/r.git",
            tmp.path().join("clone"),
            Some(TOKEN),
        );
        let message = format!(
            "fatal: unable to access 'https://user:{TOKEN}@git.example/o/r.git/': header {TOKEN}"
        );
        let redacted = src.redact(&message);
        assert!(!redacted.contains(TOKEN), "{redacted}");
        assert!(!redacted.contains("user:"), "{redacted}");
        assert!(redacted.contains("git.example/o/r.git"), "{redacted}");
    }

    #[test]
    fn url_credentials_are_stripped() {
        assert_eq!(
            strip_url_credentials("see https://a:b@h.example/x and http://c@h2/y."),
            "see https://h.example/x and http://h2/y."
        );
        assert_eq!(strip_url_credentials("no url here"), "no url here");
        assert_eq!(
            strip_url_credentials("'https://h.example/p@q'"),
            "'https://h.example/p@q'",
            "an @ in the path is not user info"
        );
    }

    #[test]
    fn subdir_must_stay_inside_the_clone() {
        assert!(check_subdir(Path::new(".")).is_ok());
        assert!(check_subdir(Path::new("boards/main")).is_ok());
        assert!(check_subdir(Path::new("../x")).is_err());
        assert!(check_subdir(Path::new("/etc")).is_err());
    }
}
