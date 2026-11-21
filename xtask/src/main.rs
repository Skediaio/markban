//! Developer tasks: `cargo xtask ci` runs the full gate.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("ci") => match ci() {
            Ok(()) => {
                println!("xtask ci: all green");
                ExitCode::SUCCESS
            }
            Err(message) => {
                eprintln!("xtask ci: {message}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: cargo xtask ci");
            ExitCode::FAILURE
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

fn run(root: &Path, args: &[&str]) -> Result<(), String> {
    println!("+ cargo {}", args.join(" "));
    let status = Command::new("cargo")
        .args(args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cannot start cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("`cargo {}` failed", args.join(" ")))
    }
}

fn ci() -> Result<(), String> {
    let root = workspace_root();
    run(&root, &["fmt", "--all", "--check"])?;
    run(
        &root,
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    run(&root, &["test", "--workspace"])?;
    run(
        &root,
        &[
            "build",
            "-p",
            "markban-wasm",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
        ],
    )?;
    check_examples(&root)
}

fn check_examples(root: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(root.join("examples"))
        .map_err(|e| format!("cannot read examples/: {e}"))?;
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for dir in dirs {
        let dir = dir.to_string_lossy().into_owned();
        run(
            root,
            &["run", "--quiet", "-p", "markban-cli", "--", "check", &dir],
        )?;
    }
    Ok(())
}
