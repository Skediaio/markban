//! The `markban` binary: a thin layer that turns arguments into core commands.

mod cli;

use clap::Parser;
use cli::{Cli, Cmd, ColCmd, Position};
use markban_core::Command;
use markban_core::diagnostic::Severity;
use markban_core::fs::{self, ApplyError, RunError};
use markban_core::view::{self, BoardView};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Exit codes: 0 ok; 1 invalid input or command refused (fix the input); 2 usage (clap);
/// 3 contention (another writer changed the board; safe to retry).
const EXIT_INVALID: u8 = 1;
const EXIT_CONTENDED: u8 = 3;

/// A failure to report: a one-line message and the exit code.
struct Fail {
    message: String,
    code: u8,
}

impl From<String> for Fail {
    fn from(message: String) -> Self {
        Self {
            message,
            code: EXIT_INVALID,
        }
    }
}

/// The exit code for a failed run. Contention and conflicts are retryable (3); everything else
/// needs a different input (1).
///
/// NOTE: a `RunError::PartiallyApplied` variant is planned in `markban-core`. The wildcard arm
/// maps it to 1 for now; decide its code (and update spec section 8) when it lands.
fn exit_code_for(error: &RunError) -> u8 {
    match error {
        RunError::Contended { .. } | RunError::Apply(ApplyError::Conflict { .. }) => EXIT_CONTENDED,
        _ => EXIT_INVALID,
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => code,
        Err(Fail { message, code }) => {
            let flat = sanitize(&message.replace(['\n', '\r'], " "));
            let _ = writeln!(std::io::stderr(), "error: {flat}");
            ExitCode::from(code)
        }
    }
}

/// Replaces C0 and C1 control characters (except newline and tab) with U+FFFD, so board
/// content cannot inject terminal escape sequences. Text mode only; `--json` stays exact.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '\u{fffd}'
            } else {
                c
            }
        })
        .collect()
}

/// Writes text to stdout with control characters neutralised; a closed pipe is not an error
/// worth reporting.
fn emit(text: &str) {
    let _ = std::io::stdout().write_all(sanitize(text).as_bytes());
}

/// Writes JSON to stdout exactly as encoded.
fn emit_json(text: &str) {
    let _ = std::io::stdout().write_all(text.as_bytes());
}

fn cwd() -> Result<PathBuf, Fail> {
    Ok(std::env::current_dir().map_err(|e| format!("cannot read the current directory: {e}"))?)
}

/// `-C`, else `MARKBAN_BOARD`, else the nearest directory upwards with a `board.md`.
fn discover(flag: Option<&Path>) -> Result<PathBuf, Fail> {
    if let Some(dir) = flag {
        return Ok(dir.to_path_buf());
    }
    if let Some(dir) = std::env::var_os("MARKBAN_BOARD").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let start = cwd()?;
    start
        .ancestors()
        .find(|dir| dir.join("board.md").is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Fail::from(format!(
                "no board.md found in '{}' or any parent; pass -C <dir>, set MARKBAN_BOARD or run `markban init`",
                start.display()
            ))
        })
}

fn read_stdin() -> Result<String, Fail> {
    Ok(std::io::read_to_string(std::io::stdin()).map_err(|e| format!("cannot read stdin: {e}"))?)
}

/// `-` means: read the text from stdin.
fn text_arg(value: Option<String>) -> Result<Option<String>, Fail> {
    match value.as_deref() {
        Some("-") => read_stdin().map(Some),
        _ => Ok(value),
    }
}

/// Runs the commands through the core and prints one affected id per line.
/// `line_of` maps command index to script line, for `run`.
fn apply(dir: &Path, commands: &[Command], line_of: Option<&[usize]>) -> Result<(), Fail> {
    let outcomes = fs::run(dir, commands).map_err(|error| {
        let code = exit_code_for(&error);
        let message = match error {
            RunError::Command { index, source } => match line_of.and_then(|l| l.get(index)) {
                Some(line) => format!("line {line}: {source}"),
                None => source.to_string(),
            },
            other => other.to_string(),
        };
        Fail { message, code }
    })?;
    let mut text = String::new();
    for outcome in outcomes {
        text.push_str(&outcome.id);
        text.push('\n');
    }
    emit(&text);
    Ok(())
}

fn one(cli_dir: Option<&Path>, command: Command) -> Result<(), Fail> {
    apply(&discover(cli_dir)?, &[command], None)
}

fn dispatch(cli: Cli) -> Result<ExitCode, Fail> {
    let dir = cli.dir.as_deref();
    match cli.command {
        Cmd::Ls { column, json } => ls(&discover(dir)?, column.as_deref(), json)?,
        Cmd::Show { id, json } => show(&discover(dir)?, &id, json)?,
        Cmd::Add {
            title,
            column,
            description,
            id,
            position: Position { top, before, after },
        } => {
            let description = text_arg(description)?;
            one(
                dir,
                Command::CardAdd {
                    title,
                    column,
                    description,
                    id,
                    top,
                    before,
                    after,
                },
            )?;
        }
        Cmd::Mv {
            id,
            column,
            position: Position { top, before, after },
        } => one(
            dir,
            Command::CardMove {
                id,
                column,
                top,
                before,
                after,
            },
        )?,
        Cmd::Edit {
            id,
            title,
            description,
            append,
        } => {
            let description = text_arg(description)?;
            let append = text_arg(append)?;
            one(
                dir,
                Command::CardEdit {
                    id,
                    title,
                    description,
                    append,
                },
            )?;
        }
        Cmd::Rm { id } => one(dir, Command::CardRemove { id })?,
        Cmd::Col(col) => one(dir, column_command(col))?,
        Cmd::Init { title, columns } => {
            let target = match dir {
                Some(d) => d.to_path_buf(),
                None => cwd()?,
            };
            apply(&target, &[Command::BoardInit { title, columns }], None)?;
        }
        Cmd::Run { file } => run_script(dir, &file)?,
        Cmd::Check { dir: target } => return check(&discover(target.as_deref().or(dir))?),
    }
    Ok(ExitCode::SUCCESS)
}

fn column_command(col: ColCmd) -> Command {
    match col {
        ColCmd::Add { name, id, before } => Command::ColumnAdd { name, id, before },
        ColCmd::Rename { column, name } => Command::ColumnRename { column, name },
        ColCmd::Mv { column, before } => Command::ColumnMove { column, before },
        ColCmd::Rm { column, move_to } => Command::ColumnRemove { column, move_to },
    }
}

fn load(
    dir: &Path,
) -> Result<
    (
        markban_core::board::Board,
        Vec<markban_core::diagnostic::Diagnostic>,
    ),
    Fail,
> {
    let (board, diagnostics) = fs::load(dir).map_err(|e| e.to_string())?;
    match board {
        Some(board) => Ok((board, diagnostics)),
        None => Err(diagnostics
            .first()
            .map_or_else(
                || format!("no usable board in '{}'", dir.display()),
                ToString::to_string,
            )
            .into()),
    }
}

fn json_line<T: serde::Serialize>(value: &T) -> Result<String, Fail> {
    Ok(serde_json::to_string(value)
        .map(|s| s + "\n")
        .map_err(|e| format!("cannot encode JSON: {e}"))?)
}

fn ls(dir: &Path, filter: Option<&str>, json: bool) -> Result<(), Fail> {
    let (board, diagnostics) = load(dir)?;
    let mut board_view: BoardView = view::view(&board);
    board_view.diagnostics = diagnostics;
    if json {
        emit_json(&json_line(&board_view)?);
        return Ok(());
    }
    emit(&render_ls(&board_view, filter)?);
    Ok(())
}

fn render_ls(view: &BoardView, filter: Option<&str>) -> Result<String, Fail> {
    let unsorted_only = filter.is_some_and(|f| f.eq_ignore_ascii_case("unsorted"));
    let only = match filter {
        Some(f) if !unsorted_only => Some(
            view.columns
                .iter()
                .find(|c| {
                    c.id == f || c.name.eq_ignore_ascii_case(f) || c.id.eq_ignore_ascii_case(f)
                })
                .map(|c| c.id.as_str())
                .ok_or_else(|| {
                    let valid: Vec<&str> = view.columns.iter().map(|c| c.id.as_str()).collect();
                    Fail::from(format!(
                        "unknown column '{f}'; valid columns: {}",
                        valid.join(", ")
                    ))
                })?,
        ),
        _ => None,
    };
    let mut out = format!("# {}\n", view.title);
    if !unsorted_only {
        for column in &view.columns {
            if only.is_some_and(|id| id != column.id) {
                continue;
            }
            out.push_str(&format!(
                "## {} ({}) {}\n",
                column.name,
                column.id,
                column.cards.len()
            ));
            for card in &column.cards {
                out.push_str(&format!("{}  {}\n", card.id, card.title));
            }
        }
    }
    if (only.is_none() || unsorted_only) && !view.unsorted.is_empty() {
        out.push_str(&format!("## Unsorted {}\n", view.unsorted.len()));
        for card in &view.unsorted {
            out.push_str(&format!("{}  {}\n", card.id, card.title));
        }
    }
    if !view.diagnostics.is_empty() {
        out.push_str(&format!(
            "! {} problem(s); run markban check\n",
            view.diagnostics.len()
        ));
    }
    Ok(out)
}

fn show(dir: &Path, id: &str, json: bool) -> Result<(), Fail> {
    let (board, _) = load(dir)?;
    let card = view::card(&board, id).ok_or_else(|| {
        Fail::from(format!(
            "no card '{id}'; card ids are the file names in cards/ without .md"
        ))
    })?;
    if json {
        emit_json(&json_line(&card)?);
        return Ok(());
    }
    let mut out = format!(
        "{}  {}\ncolumn: {}\n",
        card.id,
        card.title,
        card.column.as_deref().unwrap_or("")
    );
    if !card.description.is_empty() {
        out.push('\n');
        out.push_str(&card.description);
        out.push('\n');
    }
    emit(&out);
    Ok(())
}

fn check(dir: &Path) -> Result<ExitCode, Fail> {
    let (_, diagnostics) = fs::load(dir).map_err(|e| e.to_string())?;
    let mut out = String::new();
    for diagnostic in &diagnostics {
        out.push_str(&format!("{diagnostic}\n"));
    }
    emit(&out);
    let failed = diagnostics.iter().any(|d| d.severity == Severity::Error);
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn run_script(dir: Option<&Path>, file: &str) -> Result<(), Fail> {
    let script = if file == "-" {
        read_stdin()?
    } else {
        std::fs::read_to_string(file).map_err(|e| format!("cannot read '{file}': {e}"))?
    };
    let mut commands = Vec::new();
    let mut line_of = Vec::new();
    for (number, line) in script.lines().enumerate().map(|(i, l)| (i + 1, l.trim())) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let command = Command::parse_line(line).map_err(|e| format!("line {number}: {e}"))?;
        commands.push(command);
        line_of.push(number);
    }
    // A script may start with board.init in a directory that has no board yet.
    let target = match discover(dir) {
        Ok(target) => target,
        Err(_) => cwd()?,
    };
    apply(&target, &commands, Some(&line_of))
}

#[cfg(test)]
mod tests {
    use super::*;
    use markban_core::fs::ApplyError;

    #[test]
    fn contention_and_conflict_exit_3() {
        let contended = RunError::Contended {
            attempts: 3,
            path: "board.md".into(),
        };
        assert_eq!(exit_code_for(&contended), EXIT_CONTENDED);
        let conflict = RunError::Apply(ApplyError::Conflict {
            path: "cards/a.md".into(),
        });
        assert_eq!(exit_code_for(&conflict), EXIT_CONTENDED);
    }

    #[test]
    fn other_run_errors_exit_1() {
        let no_board = RunError::NoBoard {
            dir: PathBuf::from("."),
            reason: "x".into(),
        };
        assert_eq!(exit_code_for(&no_board), EXIT_INVALID);
    }

    #[test]
    fn sanitize_replaces_controls_but_keeps_newline_and_tab() {
        assert_eq!(
            sanitize("a\u{1b}[0m\tb\nc\u{9b}d\r"),
            "a\u{fffd}[0m\tb\nc\u{fffd}d\u{fffd}"
        );
    }
}
