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

use api::{auth_enabled, get_card, wait_for_change};
use dioxus::prelude::*;
use markban_core::view::{BoardView, CardSummary};

const STYLE: Asset = asset!("/assets/main.css");

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
    let title = if view.title.is_empty() {
        "Untitled board".to_string()
    } else {
        view.title.clone()
    };
    rsx! {
        header { class: "board-head",
            h1 { "{title}" }
            if !view.description.is_empty() {
                p { class: "description", "{view.description}" }
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
        div { class: "columns",
            for column in view.columns.iter() {
                Column { key: "{column.id}", name: column.name.clone(), cards: column.cards.clone(), selected }
            }
            if !view.unsorted.is_empty() {
                Column { name: "Unsorted", cards: view.unsorted.clone(), selected, muted: true }
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
                            class: "card",
                            onclick: {
                                let id = card.id.clone();
                                move |_| selected.set(Some(id.clone()))
                            },
                            "{card.title}"
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
                        p { class: "meta", "Column: {card.column}" }
                        div { class: "markdown", dangerous_inner_html: "{card.html}" }
                    },
                    Some(Err(e)) => rsx! { p { class: "banner error", "{e}" } },
                    None => rsx! { p { class: "loading", "Loading..." } },
                }
            }
        }
    }
}
