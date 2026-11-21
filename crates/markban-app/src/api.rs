//! Types and server functions shared by the server and the browser.

use dioxus::prelude::*;
use markban_core::view::BoardView;
use serde::{Deserialize, Serialize};

/// One card with its description rendered to sanitised HTML.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDetail {
    pub id: String,
    pub title: String,
    pub column: String,
    pub html: String,
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
