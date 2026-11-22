//! Read-only viewer for a markban board (Dioxus fullstack).
//!
//! The server reads the board directory named by `MARKBAN_BOARD` and never writes to it.

mod api;
#[cfg(feature = "server")]
mod auth;
#[cfg(feature = "server")]
mod git;
#[cfg(feature = "server")]
mod server;

use api::{auth_enabled, get_card, search, wait_for_change};
use dioxus::prelude::*;
use markban_core::search::SearchResults;
use markban_core::view::{BoardView, CardSummary};

const STYLE: Asset = asset!("/assets/main.css");
const DRAG_SCROLL: Asset = asset!("/assets/drag-scroll.js");

#[cfg(not(feature = "server"))]
fn main() {
    dioxus::launch(App);
}

/// The server serves the app through a custom router so every request passes the `Host` check
/// and, with `MARKBAN_PASSWORD` set, the password guard. `--healthcheck` probes a running server.
#[cfg(feature = "server")]
fn main() {
    use dioxus::server::axum::middleware::{from_fn, from_fn_with_state};
    use dioxus::server::axum::routing::get;

    if std::env::args().any(|a| a == "--healthcheck") {
        std::process::exit(i32::from(!server::healthcheck()));
    }
    if let Err(e) = server::start_git() {
        eprintln!("markban: {e}");
        std::process::exit(2);
    }
    let auth = auth::Auth::from_env();
    dioxus::serve(move || {
        let auth = auth.clone();
        async move {
            let mut router = dioxus::server::router(App).route("/healthz", get(|| async { "ok" }));
            if let Some(auth) = auth {
                router = router.layer(from_fn_with_state(auth, auth::guard));
            }
            Ok(router.layer(from_fn(server::host_guard)))
        }
    });
}

#[cfg(target_arch = "wasm32")]
async fn sleep_ms(ms: u32) {
    gloo_timers::future::TimeoutFuture::new(ms).await;
}

// Only the browser runs the refresh loop, so there is nothing to wait for elsewhere.
#[cfg(not(target_arch = "wasm32"))]
async fn sleep_ms(_ms: u32) {}

#[component]
fn App() -> Element {
    let mut board = use_signal(|| None::<BoardView>);
    let mut rev = use_signal(|| 0_u64);
    let mut error = use_signal(|| None::<String>);
    let selected = use_signal(|| None::<String>);
    let login = use_resource(auth_enabled);

    // Long poll: each call returns when the server's revision changes (or after ~25 s).
    use_future(move || async move {
        loop {
            match wait_for_change(rev()).await {
                Ok((new_rev, view)) => {
                    error.set(None);
                    rev.set(new_rev);
                    board.set(Some(view));
                }
                Err(e) => {
                    error.set(Some(e.to_string()));
                    sleep_ms(2000).await;
                }
            }
        }
    });

    rsx! {
        document::Stylesheet { href: STYLE }
        document::Script { src: DRAG_SCROLL, defer: true }
        document::Title { "Markban" }
        main { class: "app",
            if let Some(Ok(true)) = *login.read() {
                a { class: "logout", href: "/logout", "Log out" }
            }
            if let Some(message) = error() {
                p { class: "banner error", "Cannot reach the server: {message}" }
            }
            match board() {
                Some(view) => rsx! { BoardPage { view, selected } },
                None => rsx! { p { class: "loading", "Loading board..." } },
            }
        }
        if let Some(id) = selected() {
            CardModal { id, rev: rev(), selected }
        }
    }
}

#[component]
fn BoardPage(view: BoardView, selected: Signal<Option<String>>) -> Element {
    // Viewer state only: whether archived cards show, and the search query.
    // Nothing is written.
    let mut show_archived = use_signal(|| false);
    let mut query = use_signal(String::new);
    // Debounced search: every keystroke restarts the resource, so only a query that settles
    // for 200 ms reaches the server. An empty query never asks, and clearing it brings the
    // board back without any refetch of its own.
    let results = use_resource(move || {
        let query = query().trim().to_string();
        async move {
            if query.is_empty() {
                None
            } else {
                sleep_ms(200).await;
                Some(search(query).await)
            }
        }
    });
    let title = if view.title.is_empty() {
        "Untitled board".to_string()
    } else {
        view.title.clone()
    };
    let unsorted = visible(&view.unsorted, show_archived());
    rsx! {
        header { class: "board-head",
            h1 { "{title}" }
            if !view.description.is_empty() {
                p { class: "description", "{view.description}" }
            }
            div { class: "search",
                input {
                    class: "search-input",
                    r#type: "search",
                    placeholder: "Search…",
                    "aria-label": "Search the board",
                    value: "{query()}",
                    oninput: move |e| query.set(e.value()),
                    onkeydown: move |e| {
                        if e.key() == Key::Escape {
                            query.set(String::new());
                        }
                    },
                }
                if !query().is_empty() {
                    button {
                        class: "search-clear",
                        "aria-label": "Clear search",
                        onclick: move |_| query.set(String::new()),
                        "\u{d7}"
                    }
                }
            }
            label { class: "archived-toggle",
                input {
                    r#type: "checkbox",
                    checked: show_archived(),
                    onchange: move |e| show_archived.set(e.checked()),
                }
                "Show archived"
            }
        }
        if !view.diagnostics.is_empty() {
            div { class: "banner diagnostics", role: "alert",
                strong { "{view.diagnostics.len()} problem(s) in this board" }
                ul {
                    for d in view.diagnostics.iter() {
                        li { "{d}" }
                    }
                }
            }
        }
        if query().trim().is_empty() {
            div { class: "columns",
                for column in view.columns.iter() {
                    Column {
                        key: "{column.id}",
                        name: column.name.clone(),
                        cards: visible(&column.cards, show_archived()),
                        selected,
                    }
                }
                if !unsorted.is_empty() {
                    Column { name: "Unsorted", cards: unsorted, selected, muted: true }
                }
            }
        } else {
            match &*results.read() {
                Some(Some(Ok(hits))) => rsx! {
                    SearchResultsPanel { hits: hits.clone(), view: view.clone(), selected }
                },
                Some(Some(Err(e))) => rsx! { p { class: "banner error", "Search failed: {e}" } },
                _ => rsx! { p { class: "search-hint", "Searching…" } },
            }
        }
    }
}

/// The cards to show: archived ones only when the viewer asked for them.
fn visible(cards: &[CardSummary], show_archived: bool) -> Vec<CardSummary> {
    cards
        .iter()
        .filter(|c| show_archived || !c.archived)
        .cloned()
        .collect()
}

/// The displayed column name of a card hit: its column id resolved through the view, or
/// "Unsorted" (a hit's `column` is an id, `None` for a card without a valid one).
fn hit_column_name(view: &BoardView, column: Option<&str>) -> String {
    column
        .and_then(|id| view.columns.iter().find(|c| c.id == id))
        .map_or_else(|| "Unsorted".to_string(), |c| c.name.clone())
}

#[component]
fn SearchResultsPanel(
    hits: SearchResults,
    view: BoardView,
    selected: Signal<Option<String>>,
) -> Element {
    let total = hits.cards.len() + hits.columns.len() + usize::from(hits.board);
    let title = if view.title.is_empty() {
        "Untitled board".to_string()
    } else {
        view.title.clone()
    };
    rsx! {
        section { class: "search-results",
            p { class: "search-count", "{total} hit(s)" }
            if !hits.cards.is_empty() {
                ul { class: "cards search-cards",
                    for hit in hits.cards.iter() {
                        li { key: "{hit.id}",
                            button {
                                class: "card",
                                onclick: {
                                    let id = hit.id.clone();
                                    move |_| selected.set(Some(id.clone()))
                                },
                                span { class: "card-id", "{hit.id}" }
                                "{hit.title}"
                                span { class: "hit-column", "{hit_column_name(&view, hit.column.as_deref())}" }
                                if hit.archived {
                                    span { class: "hit-archived", "(archived)" }
                                }
                            }
                        }
                    }
                }
            }
            for column in hits.columns.iter() {
                p { class: "search-extra", key: "column-{column.id}", "Column: {column.name}" }
            }
            if hits.board {
                p { class: "search-extra", "Board: {title}" }
            }
        }
    }
}

#[component]
fn Column(
    name: String,
    cards: Vec<CardSummary>,
    selected: Signal<Option<String>>,
    #[props(default)] muted: bool,
) -> Element {
    let class = if muted { "column muted" } else { "column" };
    rsx! {
        section { class,
            h2 {
                "{name}"
                span { class: "count", "{cards.len()}" }
            }
            ul { class: "cards",
                for card in cards {
                    li { key: "{card.id}",
                        button {
                            class: if card.archived { "card archived" } else { "card" },
                            onclick: {
                                let id = card.id.clone();
                                move |_| selected.set(Some(id.clone()))
                            },
                            "{card.title}"
                            if card.archived {
                                span { class: "badge-archived", "archived" }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn CardModal(id: String, rev: u64, selected: Signal<Option<String>>) -> Element {
    // Re-fetch when the card or the board revision changes, so an open card stays current.
    let detail = use_resource(use_reactive!(|(id, rev)| async move {
        let _ = rev;
        get_card(id).await
    }));
    rsx! {
        div {
            class: "overlay",
            tabindex: "0",
            onmounted: move |e| async move {
                let _ = e.set_focus(true).await;
            },
            onclick: move |_| selected.set(None),
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    selected.set(None);
                }
            },
            div {
                class: "modal",
                role: "dialog",
                "aria-modal": "true",
                onclick: move |e| e.stop_propagation(),
                button {
                    class: "close",
                    "aria-label": "Close",
                    onclick: move |_| selected.set(None),
                    "\u{d7}"
                }
                match &*detail.read() {
                    Some(Ok(card)) => rsx! {
                        h2 { "{card.title}" }
                        p { class: "meta",
                            "Column: {card.column}"
                            if card.archived {
                                span { class: "badge-archived", "archived" }
                            }
                        }
                        div { class: "markdown", dangerous_inner_html: "{card.html}" }
                    },
                    Some(Err(e)) => rsx! { p { class: "banner error", "{e}" } },
                    None => rsx! { p { class: "loading", "Loading..." } },
                }
            }
        }
    }
}
