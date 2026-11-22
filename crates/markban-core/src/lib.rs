//! Markban core: a lossless model of a board directory, tolerant parsing,
//! validation (`check`) and a read-only `view`.
//!
//! The core has no UI, no network and no file watching. Parsing and writing
//! round-trip byte for byte; a mutation changes only the lines it must.

pub mod board;
pub mod command;
pub mod diagnostic;
pub mod frontmatter;
pub mod fs;
pub mod order;
pub mod search;

mod check;
mod id;
pub mod view;

pub use check::check;
pub use command::{Command, CommandError, Outcome, execute, execute_batch};
pub use id::{is_valid_id, slug};
pub use search::search;
pub use view::view;

/// Lower-case hex SHA-256 of `bytes`: the card hash in `show --json` and `card.edit`'s
/// `if_unchanged`.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}
