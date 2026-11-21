//! Markban core: a lossless model of a board directory, tolerant parsing,
//! validation (`check`) and a read-only `view`.
//!
//! The core has no UI, no network and no file watching. Parsing and writing
//! round-trip byte for byte; a mutation changes only the lines it must.

pub mod board;
pub mod diagnostic;
pub mod frontmatter;
pub mod fs;
pub mod order;

mod check;
mod id;
pub mod view;

pub use check::check;
pub use id::{is_valid_id, slug};
pub use view::view;
