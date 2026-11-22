//! Types and server functions shared by the server and the browser.

use dioxus::prelude::*;
use markban_core::search::SearchResults;
use markban_core::view::BoardView;
use serde::{Deserialize, Serialize};

/// One card with its description rendered to sanitised HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDetail {
    pub id: String,
    pub title: String,
    pub column: String,
    pub html: String,
    /// True when the card is archived; the modal marks it.
    pub archived: bool,
}

/// The current board.
#[server]
pub async fn get_board() -> Result<BoardView, ServerFnError> {
    let state = crate::server::state().map_err(ServerFnError::new)?;
    Ok(state.board_view())
}

/// One card by id, with its description rendered to sanitised HTML.
#[server]
pub async fn get_card(id: String) -> Result<CardDetail, ServerFnError> {
    let state = crate::server::state().map_err(ServerFnError::new)?;
    state.card(&id).map_err(ServerFnError::new)
}

/// Long poll: returns `(revision, board)` as soon as the board's revision differs from `since`,
/// or after about 25 seconds with the unchanged revision.
#[server]
pub async fn wait_for_change(since: u64) -> Result<(u64, BoardView), ServerFnError> {
    let state = crate::server::state().map_err(ServerFnError::new)?;
    Ok(state.wait_for_change(since).await)
}

/// Full-text search over the watched board: the words of `query` (split on ASCII whitespace)
/// are the terms, and a hit must contain every one, case-insensitively.
#[server]
pub async fn search(query: String) -> Result<SearchResults, ServerFnError> {
    let state = crate::server::state().map_err(ServerFnError::new)?;
    state.search(&query).await.map_err(ServerFnError::new)
}

/// Whether the viewer sits behind a password, so the page offers a logout link.
#[server]
pub async fn auth_enabled() -> Result<bool, ServerFnError> {
    Ok(std::env::var_os("MARKBAN_PASSWORD").is_some_and(|p| !p.is_empty()))
}
