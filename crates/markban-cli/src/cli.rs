//! The command line definition (clap). No logic lives here.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Plain-Markdown kanban boards, built for agents. Success prints the affected id only.
#[derive(Debug, Parser)]
#[command(name = "markban", version, about)]
pub struct Cli {
    /// Board directory (default: $MARKBAN_BOARD, else the nearest parent of the current
    /// directory that contains board.md).
    #[arg(short = 'C', global = true, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Cmd,
}

/// Where a card goes in its column; default is the end.
#[derive(Debug, Args)]
pub struct Position {
    /// Put the card at the top of its column.
    #[arg(long, conflicts_with_all = ["before", "after"])]
    pub top: bool,
    /// Put the card right before this card.
    #[arg(long, value_name = "ID", conflicts_with = "after")]
    pub before: Option<String>,
    /// Put the card right after this card.
    #[arg(long, value_name = "ID")]
    pub after: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// List the board: one line per card.
    Ls {
        /// Only this column (id or name, or "unsorted").
        column: Option<String>,
        /// Print the board view as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one card with its description.
    Show {
        id: String,
        /// Print the card as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Add a card.
    Add {
        title: String,
        /// Column id or name (default: the first column).
        #[arg(short, long, value_name = "COL")]
        column: Option<String>,
        /// Description text, or `-` to read it from stdin.
        #[arg(short, long, value_name = "TEXT")]
        description: Option<String>,
        /// Card id (default: derived from the title).
        #[arg(long)]
        id: Option<String>,
        #[command(flatten)]
        position: Position,
    },
    /// Move a card to another column and/or position.
    Mv {
        id: String,
        /// Target column id or name (default: stay in the column).
        column: Option<String>,
        #[command(flatten)]
        position: Position,
    },
    /// Change a card's title or description.
    Edit {
        id: String,
        #[arg(long)]
        title: Option<String>,
        /// Replace the description with TEXT, or `-` to read it from stdin.
        #[arg(short, long, value_name = "TEXT")]
        description: Option<String>,
        /// Append a line to the description, or `-` to read it from stdin.
        #[arg(long, value_name = "TEXT", conflicts_with = "description")]
        append: Option<String>,
    },
    /// Remove a card.
    Rm { id: String },
    /// Add, rename, move or remove columns.
    #[command(subcommand)]
    Col(ColCmd),
    /// Create a board in the target directory (default: the current directory).
    Init {
        title: String,
        #[arg(required = true, value_name = "COL")]
        columns: Vec<String>,
    },
    /// Run JSON-lines commands from FILE or stdin (`-`), all or nothing.
    Run {
        #[arg(value_name = "FILE", default_value = "-")]
        file: String,
    },
    /// Report all diagnostics; exit 1 when the board has errors.
    Check {
        /// Board directory (default: discovered like the other commands).
        dir: Option<PathBuf>,
    },
}

#[derive(Debug, Subcommand)]
pub enum ColCmd {
    /// Add a column.
    Add {
        name: String,
        #[arg(long)]
        id: Option<String>,
        /// Insert before this column (default: at the end).
        #[arg(long, value_name = "COL")]
        before: Option<String>,
    },
    /// Rename a column (its id stays).
    Rename { column: String, name: String },
    /// Move a column.
    Mv {
        column: String,
        /// Insert before this column (default: at the end).
        #[arg(long, value_name = "COL")]
        before: Option<String>,
    },
    /// Remove a column.
    Rm {
        column: String,
        /// Move its cards to this column first.
        #[arg(long, value_name = "COL")]
        move_to: Option<String>,
    },
}
