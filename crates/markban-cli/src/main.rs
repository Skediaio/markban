//! The `markban` binary: a thin layer that turns arguments into core commands.

mod cli;

use clap::Parser;
use cli::{Cli, Cmd, ColCmd, Position};
use markban_core::Command;
use markban_core::diagnostic::Severity;
use markban_core::fs::{self, RunError};
use markban_core::view::{self, BoardView};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

type Fail = String;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => code,
        Err(message) => {
            let flat = message.replace(['\n', '\r'], " ");
            let _ = writeln!(std::io::stderr(), "error: {flat}");
            ExitCode::FAILURE
        }
    }
}

/// Writes to stdout; a closed pipe is not an error worth reporting.
fn emit(text: &str) {
    let _ = std::io::stdout().write_all(text.as_bytes());
}

fn cwd() -> Result<PathBuf, Fail> {
    std::env::current_dir().map_err(|e| format!("cannot read the current directory: {e}"))
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
            format!(
                "no board.md found in '{}' or any parent; pass -C <dir>, set MARKBAN_BOARD or run `markban init`",
                start.display()
            )
        })
}

fn read_stdin() -> Result<String, Fail> {
    std::io::read_to_string(std::io::stdin()).map_err(|e| format!("cannot read stdin: {e}"))
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
    let outcomes = fs::run(dir, commands).map_err(|error| match error {
        RunError::Command { index, source } => match line_of.and_then(|l| l.get(index)) {
            Some(line) => format!("line {line}: {source}"),
            None => source.to_string(),
        },
        other => other.to_string(),
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
        None => Err(diagnostics.first().map_or_else(
            || format!("no usable board in '{}'", dir.display()),
            ToString::to_string,
        )),
    }
}

fn json_line<T: serde::Serialize>(value: &T) -> Result<String, Fail> {
    serde_json::to_string(value)
        .map(|s| s + "\n")
        .map_err(|e| format!("cannot encode JSON: {e}"))
}

fn ls(dir: &Path, filter: Option<&str>, json: bool) -> Result<(), Fail> {
    let (board, diagnostics) = load(dir)?;
    let mut board_view: BoardView = view::view(&board);
    board_view.diagnostics = diagnostics;
    if json {
        emit(&json_line(&board_view)?);
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
                    format!("unknown column '{f}'; valid columns: {}", valid.join(", "))
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
    for diagnostic in &view.diagnostics {
        out.push_str(&format!("! {diagnostic}\n"));
    }
    Ok(out)
}

fn show(dir: &Path, id: &str, json: bool) -> Result<(), Fail> {
    let (board, _) = load(dir)?;
    let card = view::card(&board, id).ok_or_else(|| {
        format!("no card '{id}'; card ids are the file names in cards/ without .md")
    })?;
    if json {
        emit(&json_line(&card)?);
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
