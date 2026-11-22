//! The `markban` binary: a thin layer that turns arguments into core commands.
//!
//! Locally the commands run on a board directory; with `MARKBAN_URL` (remote mode) the same
//! commands go to a markban server, with the same output and exit codes.

mod cli;
mod remote;

use clap::Parser;
use cli::{BoardCmd, Cli, Cmd, ColCmd, Position, ServerCmd, UserCmd};
use markban_core::Command;
use markban_core::diagnostic::Severity;
use markban_core::fs::{self, RunError};
use markban_core::search::SearchResults;
use markban_core::view::{self, BoardView, CardDetail};
use markban_server::access;
use remote::{Remote, RemoteError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use markban_core::fs::EXIT_INVALID;
/// Exit codes: 0 ok; 1 invalid input or command refused (fix the input); 2 usage (clap);
/// 3 contention (another writer changed the board; safe to retry); 4 partially applied.
#[cfg(test)]
use markban_core::fs::{EXIT_CONTENDED, EXIT_PARTIAL};

/// Usage error, as clap's own rejections (spec section 8).
const EXIT_USAGE: u8 = 2;

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

impl From<RemoteError> for Fail {
    fn from(error: RemoteError) -> Self {
        Self {
            message: error.message,
            code: error.code,
        }
    }
}

/// The exit code for a failed run (spec section 8), see [`RunError::exit_code`].
fn exit_code_for(error: &RunError) -> u8 {
    error.exit_code()
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

/// Unicode bidirectional controls, which can reorder displayed text.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{61c}')
}

/// Replaces C0 and C1 control characters (except newline and tab) and bidi controls with U+FFFD, so board
/// content cannot inject terminal escape sequences. Text mode only; `--json` stays exact.
fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| {
            if (c.is_control() && c != '\n' && c != '\t') || is_bidi_control(c) {
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

fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
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

/// Where the board commands go: a directory (`-C`, discovered when absent) or a server board.
enum Target {
    Local(Option<PathBuf>),
    Remote(Remote),
}

/// Remote mode when `--url` or `MARKBAN_URL` is set; local mode otherwise.
fn target(
    dir: Option<PathBuf>,
    url: Option<String>,
    token: Option<String>,
    board: Option<String>,
    allow_insecure_http: bool,
) -> Result<Target, Fail> {
    let Some(url) = url.or_else(|| env_var("MARKBAN_URL")) else {
        if board.is_some() {
            return Err(Fail::from(
                "-b names a board on a markban server; set MARKBAN_URL, or use -C <dir> for a local board".to_string(),
            ));
        }
        return Ok(Target::Local(dir));
    };
    if dir.is_some() {
        return Err(Fail::from(
            "-C is a local board directory; in remote mode (MARKBAN_URL) pick the board with -b <id>"
                .to_string(),
        ));
    }
    let token = token.or_else(|| env_var("MARKBAN_TOKEN")).ok_or_else(|| {
        Fail::from(
            "remote mode needs a token; set MARKBAN_TOKEN (or unset MARKBAN_URL)".to_string(),
        )
    })?;
    let board = board.or_else(|| env_var("MARKBAN_BOARD"));
    let insecure =
        allow_insecure_http || env_var("MARKBAN_ALLOW_INSECURE_HTTP").as_deref() == Some("1");
    Ok(Target::Remote(Remote::new(&url, token, board, insecure)?))
}

/// The message for a refused command: `line N:` in front when it came from a script.
fn command_message(message: String, index: Option<usize>, line_of: Option<&[usize]>) -> String {
    match index.and_then(|i| line_of.and_then(|l| l.get(i))) {
        Some(line) => format!("line {line}: {message}"),
        None => message,
    }
}

fn print_outcomes(ids: impl IntoIterator<Item = String>) {
    let mut text = String::new();
    for id in ids {
        text.push_str(&id);
        text.push('\n');
    }
    emit(&text);
}

/// Runs the commands through the core and prints one affected id per line.
/// `line_of` maps command index to script line, for `run`.
fn apply(dir: &Path, commands: &[Command], line_of: Option<&[usize]>) -> Result<(), Fail> {
    let outcomes = fs::run(dir, commands).map_err(|error| {
        let code = exit_code_for(&error);
        let message = match error {
            RunError::Command { index, source } => {
                command_message(source.to_string(), Some(index), line_of)
            }
            other => other.to_string(),
        };
        Fail { message, code }
    })?;
    print_outcomes(outcomes.into_iter().map(|o| o.id));
    Ok(())
}

/// [`apply`] on a server board.
fn apply_remote(
    remote: &Remote,
    commands: &[Command],
    line_of: Option<&[usize]>,
) -> Result<(), Fail> {
    let ids = remote.run(commands).map_err(|error| Fail {
        code: error.code,
        message: command_message(error.message, error.index, line_of),
    })?;
    print_outcomes(ids);
    Ok(())
}

impl Target {
    fn run(&self, commands: &[Command], line_of: Option<&[usize]>) -> Result<(), Fail> {
        match self {
            Self::Local(dir) => apply(&discover(dir.as_deref())?, commands, line_of),
            Self::Remote(remote) => apply_remote(remote, commands, line_of),
        }
    }

    fn view(&self) -> Result<BoardView, Fail> {
        match self {
            Self::Local(dir) => {
                let (board, diagnostics) = load(&discover(dir.as_deref())?)?;
                let mut board_view = view::view(&board);
                board_view.diagnostics = diagnostics;
                Ok(board_view)
            }
            Self::Remote(remote) => Ok(remote.view()?),
        }
    }

    fn card(&self, id: &str) -> Result<CardDetail, Fail> {
        match self {
            Self::Local(dir) => {
                let (board, _) = load(&discover(dir.as_deref())?)?;
                view::card(&board, id).ok_or_else(|| {
                    Fail::from(format!(
                        "no card '{id}'; card ids are the file names in cards/ without .md"
                    ))
                })
            }
            Self::Remote(remote) => Ok(remote.card(id)?),
        }
    }

    /// The hits of a full-board search, with the board title (for the `board …` line; the
    /// title costs one extra view request in remote mode, so only when the board matched).
    fn search(&self, terms: &[String]) -> Result<(SearchResults, Option<String>), Fail> {
        match self {
            Self::Local(dir) => {
                let (board, _) = load(&discover(dir.as_deref())?)?;
                let title = board
                    .board
                    .frontmatter
                    .get("title")
                    .and_then(Result::ok)
                    .unwrap_or_default();
                let results = markban_core::search::search(&board, terms)
                    .map_err(|e| Fail::from(e.to_string()))?;
                Ok((results, Some(title)))
            }
            Self::Remote(remote) => {
                let results = remote.search(terms)?;
                let title = if results.board {
                    Some(remote.view()?.title)
                } else {
                    None
                };
                Ok((results, title))
            }
        }
    }

    fn remote(&self, what: &str) -> Result<&Remote, Fail> {
        match self {
            Self::Remote(remote) => Ok(remote),
            Self::Local(_) => Err(Fail::from(format!(
                "`markban {what}` is only available on a markban server; set MARKBAN_URL and MARKBAN_TOKEN"
            ))),
        }
    }
}

fn dispatch(cli: Cli) -> Result<ExitCode, Fail> {
    match cli.command {
        Cmd::Serve {
            data,
            listen,
            max_cards,
            max_card_bytes,
            max_requests,
            request_timeout,
        } => {
            let mut config = markban_server::Config::new(data_dir(data)?);
            config.allowed_hosts = markban_server::parse_hosts(
                &std::env::var("MARKBAN_ALLOWED_HOSTS").unwrap_or_default(),
            );
            let limits = &mut config.limits;
            limits.max_cards = max_cards;
            limits.max_card_bytes = max_card_bytes;
            limits.max_concurrent = max_requests;
            limits.request_timeout = std::time::Duration::from_secs(request_timeout.max(1));
            markban_server::serve(config, &listen)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Server { data, command } => {
            server_admin(data, command)?;
            Ok(ExitCode::SUCCESS)
        }
        command => {
            let target = target(
                cli.dir,
                cli.url,
                cli.token,
                cli.board,
                cli.allow_insecure_http,
            )?;
            board_command(&target, command)
        }
    }
}

fn board_command(target: &Target, command: Cmd) -> Result<ExitCode, Fail> {
    let one = |command: Command| target.run(&[command], None);
    match command {
        Cmd::Ls {
            column,
            archived,
            json,
        } => ls(&target.view()?, column.as_deref(), archived, json)?,
        Cmd::Show { id, json } => show(&target.card(&id)?, json)?,
        Cmd::Search { terms, json } => {
            let terms = search_terms(&terms)?;
            let (results, title) = target.search(&terms)?;
            search(&results, title.as_deref(), json)?;
        }
        Cmd::Add {
            title,
            column,
            description,
            id,
            position: Position { top, before, after },
        } => {
            let description = text_arg(description)?;
            one(Command::CardAdd {
                title,
                column,
                description,
                id,
                top,
                before,
                after,
            })?;
        }
        Cmd::Mv {
            id,
            column,
            position: Position { top, before, after },
        } => one(Command::CardMove {
            id,
            column,
            top,
            before,
            after,
        })?,
        Cmd::Edit {
            id,
            title,
            description,
            append,
            if_unchanged,
        } => {
            let description = text_arg(description)?;
            let append = text_arg(append)?;
            one(Command::CardEdit {
                id,
                title,
                description,
                append,
                if_unchanged,
            })?;
        }
        Cmd::Rm { id } => one(Command::CardRemove { id })?,
        Cmd::Archive { id } => one(Command::CardArchive { id })?,
        Cmd::Unarchive { id } => one(Command::CardUnarchive { id })?,
        Cmd::Col(col) => one(column_command(col))?,
        Cmd::Init { title, columns } => {
            let init = [Command::BoardInit { title, columns }];
            match target {
                Target::Local(dir) => {
                    let dir = match dir {
                        Some(d) => d.clone(),
                        None => cwd()?,
                    };
                    apply(&dir, &init, None)?;
                }
                Target::Remote(remote) => apply_remote(remote, &init, None)?,
            }
        }
        Cmd::Run { file } => run_script(target, &file)?,
        Cmd::Check { dir } => {
            return match (target, dir) {
                (Target::Local(flag), dir) => check(&discover(dir.as_deref().or(flag.as_deref()))?),
                (Target::Remote(_), Some(_)) => Err(Fail::from(
                    "in remote mode `check` takes no directory; it checks the board given with -b"
                        .to_string(),
                )),
                (Target::Remote(remote), None) => {
                    Ok(print_diagnostics(&remote.view()?.diagnostics))
                }
            };
        }
        Cmd::Boards { json } => boards(target.remote("boards")?, json)?,
        Cmd::Log { count } => {
            let entries = target.remote("log")?.log(count)?;
            let mut text = String::new();
            for entry in entries {
                text.push_str(entry.get());
                text.push('\n');
            }
            emit_json(&text);
        }
        Cmd::Serve { .. } | Cmd::Server { .. } => {
            return Err(Fail::from("not a board command".to_string()));
        }
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

fn ls(
    board_view: &BoardView,
    filter: Option<&str>,
    archived: bool,
    json: bool,
) -> Result<(), Fail> {
    if json {
        emit_json(&json_line(board_view)?);
        return Ok(());
    }
    emit(&render_ls(board_view, filter, archived)?);
    Ok(())
}

fn render_ls(view: &BoardView, filter: Option<&str>, archived: bool) -> Result<String, Fail> {
    if archived {
        let mut out = format!("# {}\n", view.title);
        out.push_str(&format!("## Archived ({})\n", view.archived.len()));
        for card in &view.archived {
            out.push_str(&format!("{}  {}\n", card.id, card.title));
        }
        return Ok(out);
    }
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
    if !view.archived.is_empty() {
        out.push_str(&format!(
            "{} archived card(s); markban ls --archived\n",
            view.archived.len()
        ));
    }
    if !view.diagnostics.is_empty() {
        out.push_str(&format!(
            "! {} problem(s); run markban check\n",
            view.diagnostics.len()
        ));
    }
    Ok(out)
}

fn show(card: &CardDetail, json: bool) -> Result<(), Fail> {
    if json {
        emit_json(&json_line(card)?);
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

/// The search terms: the whitespace-separated words of the arguments. Arguments that are
/// empty after trimming drop out; when nothing remains it is a usage error.
fn search_terms(args: &[String]) -> Result<Vec<String>, Fail> {
    let terms: Vec<String> = args
        .iter()
        .filter(|arg| !arg.trim().is_empty())
        .flat_map(|arg| arg.split_ascii_whitespace())
        .map(str::to_string)
        .collect();
    if terms.is_empty() {
        return Err(Fail {
            message: "search needs at least one term, e.g. `markban search roadmap`".to_string(),
            code: EXIT_USAGE,
        });
    }
    Ok(terms)
}

fn search(results: &SearchResults, board_title: Option<&str>, json: bool) -> Result<(), Fail> {
    if json {
        emit_json(&json_line(results)?);
        return Ok(());
    }
    emit(&render_search(results, board_title));
    Ok(())
}

/// `<n> hit(s)`, then the cards in `ls` order (archived ones flagged, with a description
/// snippet line when a term matched the description), then the columns, then the board.
fn render_search(results: &SearchResults, board_title: Option<&str>) -> String {
    let hits = results.cards.len() + results.columns.len() + usize::from(results.board);
    let mut out = format!("{hits} hit(s)\n");
    for card in &results.cards {
        out.push_str(&format!(
            "{}  {}  {}",
            card.id,
            card.title,
            card.column.as_deref().unwrap_or("unsorted")
        ));
        if card.archived {
            out.push_str(" (archived)");
        }
        out.push('\n');
        if let Some(snippet) = &card.snippet {
            out.push_str(&format!("  :{}: {}\n", snippet.line, snippet.text));
        }
    }
    for column in &results.columns {
        out.push_str(&format!("column {}  {}\n", column.id, column.name));
    }
    if let Some(title) = board_title.filter(|_| results.board) {
        out.push_str(&format!("board  {title}\n"));
    }
    out
}

/// Prints every diagnostic; failure when one is an error.
fn print_diagnostics(diagnostics: &[markban_core::diagnostic::Diagnostic]) -> ExitCode {
    let mut out = String::new();
    for diagnostic in diagnostics {
        out.push_str(&format!("{diagnostic}\n"));
    }
    emit(&out);
    if diagnostics.iter().any(|d| d.severity == Severity::Error) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn check(dir: &Path) -> Result<ExitCode, Fail> {
    let (_, diagnostics) = fs::load(dir).map_err(|e| e.to_string())?;
    Ok(print_diagnostics(&diagnostics))
}

fn run_script(target: &Target, file: &str) -> Result<(), Fail> {
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
    match target {
        Target::Local(dir) => {
            // A script may start with board.init in a directory that has no board yet.
            let dir = match discover(dir.as_deref()) {
                Ok(dir) => dir,
                Err(_) => cwd()?,
            };
            apply(&dir, &commands, Some(&line_of))
        }
        Target::Remote(remote) => apply_remote(remote, &commands, Some(&line_of)),
    }
}

fn boards(remote: &Remote, json: bool) -> Result<(), Fail> {
    let boards = remote.boards()?;
    if json {
        emit_json(&json_line(&boards)?);
        return Ok(());
    }
    emit(&board_lines(boards.into_iter().map(|b| (b.id, b.title))));
    Ok(())
}

fn board_lines(boards: impl IntoIterator<Item = (String, String)>) -> String {
    boards
        .into_iter()
        .map(|(id, title)| format!("{id}  {title}\n"))
        .collect()
}

/// `--data`, else `MARKBAN_DATA`, else the current directory.
fn data_dir(flag: Option<PathBuf>) -> Result<PathBuf, Fail> {
    match flag.or_else(|| env_var("MARKBAN_DATA").map(PathBuf::from)) {
        Some(dir) => Ok(dir),
        None => cwd(),
    }
}

/// Prints a new token on stdout (the only place it ever appears) and a reminder on stderr.
fn print_token(user: &str, token: &str) {
    emit(&format!("{token}\n"));
    let _ = writeln!(
        std::io::stderr(),
        "token for user '{user}', shown only once: store it now (e.g. as MARKBAN_TOKEN)"
    );
}

/// Refuses an existing `--token-file` before anything changes (writing it later also refuses).
fn check_token_file(file: Option<&Path>) -> Result<(), Fail> {
    match file {
        Some(file) if std::fs::symlink_metadata(file).is_ok() => Err(Fail::from(format!(
            "'{}' already exists; give a new file for the token",
            file.display()
        ))),
        _ => Ok(()),
    }
}

/// Hands a new token over: into a new private file with `--token-file`, else on stdout.
fn deliver_token(user: &str, token: &str, file: Option<&Path>) -> Result<(), Fail> {
    match file {
        Some(file) => {
            access::write_token_file(file, token)?;
            let _ = writeln!(
                std::io::stderr(),
                "token for user '{user}' written to '{}'",
                file.display()
            );
        }
        None => print_token(user, token),
    }
    Ok(())
}

/// The admin commands: direct file access to a data directory, never HTTP.
fn server_admin(data: Option<PathBuf>, command: ServerCmd) -> Result<(), Fail> {
    match command {
        ServerCmd::Init {
            dir,
            admin,
            token_file,
        } => {
            check_token_file(token_file.as_deref())?;
            let token = access::init(&dir, &admin)?;
            deliver_token(&admin, &token, token_file.as_deref())?;
        }
        ServerCmd::User(UserCmd::Add {
            name,
            boards,
            admin,
            token_file,
        }) => {
            check_token_file(token_file.as_deref())?;
            let token = access::add_user(&data_dir(data)?, &name, &boards, admin)?;
            deliver_token(&name, &token, token_file.as_deref())?;
        }
        ServerCmd::User(UserCmd::Remove { name }) => {
            access::remove_user(&data_dir(data)?, &name)?;
            emit(&format!("{name}\n"));
        }
        ServerCmd::User(UserCmd::Boards { name, boards }) => {
            access::set_boards(&data_dir(data)?, &name, &boards)?;
            emit(&format!("{name}\n"));
        }
        ServerCmd::Board(BoardCmd::Add { id, title, columns }) => {
            let id = access::add_board(&data_dir(data)?, &id, &title, &columns)?;
            emit(&format!("{id}\n"));
        }
        ServerCmd::Board(BoardCmd::Remove { id }) => {
            let moved = access::remove_board(&data_dir(data)?, &id)?;
            emit(&format!("{id}\n"));
            let _ = writeln!(
                std::io::stderr(),
                "board '{id}' moved to '{}'",
                moved.display()
            );
        }
        ServerCmd::Board(BoardCmd::List) => {
            emit(&board_lines(access::list_boards(&data_dir(data)?)?));
        }
    }
    Ok(())
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
        let locked = RunError::Locked {
            path: ".markban.lock".into(),
        };
        assert_eq!(exit_code_for(&locked), EXIT_CONTENDED);
    }

    #[test]
    fn partial_write_exits_4() {
        let partial = RunError::PartiallyApplied {
            written: vec!["board.md".into()],
            failed: "cards/a.md".into(),
            reason: "conflict".into(),
        };
        assert_eq!(exit_code_for(&partial), EXIT_PARTIAL);
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

    #[test]
    fn sanitize_replaces_bidi_controls() {
        let bidi = "\u{202a}\u{202b}\u{202c}\u{202d}\u{202e}\u{2066}\u{2067}\u{2068}\u{2069}\u{200e}\u{200f}\u{61c}";
        assert_eq!(
            sanitize(&format!("a{bidi}b")),
            format!("a{}b", "\u{fffd}".repeat(12))
        );
    }
}
