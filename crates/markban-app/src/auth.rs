//! One shared password in front of the whole viewer (`MARKBAN_PASSWORD`).
//!
//! The password is checked only here, on the server, against a SHA-256 of the configured value
//! in constant time. A successful login gets a random session id in an `HttpOnly` cookie; the
//! sessions live in memory only. Failed logins are throttled per client.

use dioxus::server::axum::body::to_bytes;
use dioxus::server::axum::extract::{Request, State};
use dioxus::server::axum::http::{HeaderValue, Method, StatusCode, header};
use dioxus::server::axum::middleware::Next;
use dioxus::server::axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a session stays valid.
pub const SESSION_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Failed logins allowed per client within [`THROTTLE_WINDOW`] before answering 429.
pub const MAX_FAILURES: u32 = 5;
/// The window over which failed logins are counted.
pub const THROTTLE_WINDOW: Duration = Duration::from_secs(60);
/// The session cookie's name.
pub const COOKIE: &str = "markban_session";

/// The configured password, kept only as its SHA-256.
pub struct Password {
    hash: [u8; 32],
}

fn sha256(text: &str) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(text.as_bytes()).into()
}

impl Password {
    #[must_use]
    pub fn new(password: &str) -> Self {
        Self {
            hash: sha256(password),
        }
    }

    /// Whether `candidate` is the password. Both sides are hashed first, so the comparison is
    /// constant time in the candidate's content and length.
    #[must_use]
    pub fn matches(&self, candidate: &str) -> bool {
        use subtle::ConstantTimeEq;
        sha256(candidate).ct_eq(&self.hash).into()
    }
}

/// Sessions in memory: id to expiry.
pub struct Sessions {
    ttl: Duration,
    map: Mutex<HashMap<String, Instant>>,
}

impl Sessions {
    #[must_use]
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a session valid from `now` for the configured time and returns its id
    /// (32 random bytes, hex). Expired sessions are dropped on the way.
    ///
    /// # Errors
    /// A message when no random bytes are available or the store is unusable.
    pub fn create(&self, now: Instant) -> Result<String, String> {
        let mut bytes = [0_u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| format!("cannot get random bytes: {e}"))?;
        let id: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let expiry = now
            .checked_add(self.ttl)
            .ok_or_else(|| "session expiry out of range".to_string())?;
        let mut map = self
            .map
            .lock()
            .map_err(|_| "session store is unavailable".to_string())?;
        map.retain(|_, until| *until > now);
        map.insert(id.clone(), expiry);
        Ok(id)
    }

    /// Whether `id` names a session that has not expired at `now`.
    #[must_use]
    pub fn valid(&self, id: &str, now: Instant) -> bool {
        self.map
            .lock()
            .is_ok_and(|map| map.get(id).is_some_and(|until| *until > now))
    }

    /// Ends the session `id`, if any.
    pub fn remove(&self, id: &str) {
        if let Ok(mut map) = self.map.lock() {
            map.remove(id);
        }
    }
}

/// Counts failed logins per client key within a fixed window.
pub struct Throttle {
    max: u32,
    window: Duration,
    failures: Mutex<HashMap<String, (Instant, u32)>>,
}

impl Throttle {
    #[must_use]
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            failures: Mutex::new(HashMap::new()),
        }
    }

    /// Whether `key` has used up its failed attempts at `now`. An unusable store blocks.
    #[must_use]
    pub fn blocked(&self, key: &str, now: Instant) -> bool {
        self.failures.lock().map_or(true, |map| {
            map.get(key).is_some_and(|(start, count)| {
                now.duration_since(*start) < self.window && *count >= self.max
            })
        })
    }

    /// Records a failed login for `key` at `now`. Windows that are over are dropped.
    pub fn fail(&self, key: &str, now: Instant) {
        let Ok(mut map) = self.failures.lock() else {
            return;
        };
        map.retain(|_, (start, _)| now.duration_since(*start) < self.window);
        let entry = map.entry(key.to_string()).or_insert((now, 0));
        entry.1 = entry.1.saturating_add(1);
    }

    /// Forgets the failures of `key` (after a successful login).
    pub fn clear(&self, key: &str) {
        if let Ok(mut map) = self.failures.lock() {
            map.remove(key);
        }
    }
}

/// The value of cookie `name` in a `Cookie` header, if present.
#[must_use]
pub fn cookie_value<'a>(header: &'a str, name: &str) -> Option<&'a str> {
    header.split(';').find_map(|pair| {
        let (key, value) = pair.trim().split_once('=')?;
        (key == name).then_some(value)
    })
}

/// The throttle key for a request: the last `X-Forwarded-For` entry when the proxy is trusted
/// (the address the proxy itself saw), otherwise one shared key. The viewer does not see the
/// socket's peer address, so without a trusted proxy all clients share one bucket.
#[must_use]
pub fn client_key(forwarded_for: Option<&str>, trust_proxy: bool) -> String {
    forwarded_for
        .filter(|_| trust_proxy)
        .and_then(|list| list.rsplit(',').next())
        .map(str::trim)
        .filter(|ip| !ip.is_empty())
        .map_or_else(|| "direct".to_string(), str::to_string)
}

/// The `password` field of an `application/x-www-form-urlencoded` body.
#[must_use]
pub fn form_password(body: &[u8]) -> Option<String> {
    form_urlencoded::parse(body)
        .find(|(key, _)| key == "password")
        .map(|(_, value)| value.into_owned())
}

/// The `Set-Cookie` value that stores session `id`.
#[must_use]
pub fn session_cookie(id: &str) -> String {
    format!(
        "{COOKIE}={id}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_TTL.as_secs()
    )
}

/// The `Set-Cookie` value that removes the session cookie.
#[must_use]
pub fn clear_cookie() -> String {
    format!("{COOKIE}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0")
}

/// The login page: one password field, no script. `error` is shown above the field; it is a
/// fixed message from the server, never user input.
#[must_use]
pub fn login_page(error: Option<&str>) -> String {
    let error = error.map_or_else(String::new, |e| format!("<p class=\"error\">{e}</p>"));
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Markban</title>
<style>
body {{ font-family: system-ui, sans-serif; display: grid; place-items: center; min-height: 100vh; margin: 0; background: #f5f5f4; color: #1c1917; }}
form {{ display: grid; gap: .75rem; padding: 1.5rem; background: #fff; border-radius: .5rem; box-shadow: 0 1px 3px #0002; min-width: 16rem; }}
input, button {{ font: inherit; padding: .5rem; }}
.error {{ color: #b91c1c; margin: 0; }}
</style>
</head>
<body>
<form method="post" action="/login">
<h1>Markban</h1>
{error}<label for="password">Password</label>
<input id="password" name="password" type="password" autocomplete="current-password" required autofocus>
<button type="submit">Sign in</button>
</form>
</body>
</html>
"#
    )
}

/// Everything the password guard needs.
pub struct Auth {
    password: Password,
    sessions: Sessions,
    throttle: Throttle,
    trust_proxy: bool,
}

impl Auth {
    /// The guard configured from `MARKBAN_PASSWORD` and `MARKBAN_TRUST_PROXY`; `None` when no
    /// password is set (local use, no login).
    #[must_use]
    pub fn from_env() -> Option<Arc<Self>> {
        let password = std::env::var("MARKBAN_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty())?;
        let trust_proxy = std::env::var("MARKBAN_TRUST_PROXY").is_ok_and(|v| v == "1");
        Some(Arc::new(Self::new(&password, trust_proxy)))
    }

    #[must_use]
    pub fn new(password: &str, trust_proxy: bool) -> Self {
        Self {
            password: Password::new(password),
            sessions: Sessions::new(SESSION_TTL),
            throttle: Throttle::new(MAX_FAILURES, THROTTLE_WINDOW),
            trust_proxy,
        }
    }
}

/// Middleware: lets a request through only with a valid session; serves `/login`, `/logout`
/// and an open `/healthz`. Without a session a page request gets the login form and anything
/// else (server functions, assets) a bare 401.
pub async fn guard(State(auth): State<Arc<Auth>>, request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let is_get = matches!(*request.method(), Method::GET | Method::HEAD);
    if path == "/healthz" {
        return next.run(request).await;
    }
    if path == "/login" && request.method() == Method::POST {
        return auth.login(request).await;
    }
    let session = request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|c| cookie_value(c, COOKIE))
        .map(str::to_string);
    if path == "/logout" {
        if let Some(id) = &session {
            auth.sessions.remove(id);
        }
        return redirect_home(&clear_cookie());
    }
    if session.is_some_and(|id| auth.sessions.valid(&id, Instant::now())) {
        return next.run(request).await;
    }
    if is_get {
        login_response(StatusCode::UNAUTHORIZED, None)
    } else {
        (StatusCode::UNAUTHORIZED, "unauthorized: log in first\n").into_response()
    }
}

/// The largest login form body read.
const LOGIN_BODY_LIMIT: usize = 4096;

impl Auth {
    async fn login(&self, request: Request) -> Response {
        let forwarded = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok());
        let key = client_key(forwarded, self.trust_proxy);
        if self.throttle.blocked(&key, Instant::now()) {
            return login_response(
                StatusCode::TOO_MANY_REQUESTS,
                Some("Too many failed attempts. Wait a minute and try again."),
            );
        }
        let body = to_bytes(request.into_body(), LOGIN_BODY_LIMIT)
            .await
            .unwrap_or_default();
        let password = form_password(&body).unwrap_or_default();
        if !self.password.matches(&password) {
            self.throttle.fail(&key, Instant::now());
            return login_response(StatusCode::UNAUTHORIZED, Some("Wrong password."));
        }
        self.throttle.clear(&key);
        match self.sessions.create(Instant::now()) {
            Ok(id) => redirect_home(&session_cookie(&id)),
            Err(_) => login_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some("Cannot start a session. Try again."),
            ),
        }
    }
}

fn login_response(status: StatusCode, error: Option<&str>) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'",
            ),
        ],
        login_page(error),
    )
        .into_response()
}

fn redirect_home(cookie: &str) -> Response {
    let mut response = (
        StatusCode::SEE_OTHER,
        [(header::LOCATION, "/"), (header::CACHE_CONTROL, "no-store")],
    )
        .into_response();
    if let Ok(value) = HeaderValue::from_str(cookie) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
    response
}

#[cfg(test)]
#[allow(clippy::expect_used)] // tests may expect
mod tests {
    use super::*;

    #[test]
    fn password_matches_only_itself() {
        let p = Password::new("correct horse");
        assert!(p.matches("correct horse"));
        for wrong in [
            "",
            "correct",
            "correct horse ",
            "Correct horse",
            "correct horsf",
        ] {
            assert!(!p.matches(wrong), "{wrong:?}");
        }
    }

    #[test]
    fn password_is_not_stored_in_plain_text() {
        let p = Password::new("hunter2");
        assert_ne!(&p.hash[..7], b"hunter2");
        assert_ne!(p.hash, [0; 32]);
    }

    #[test]
    fn sessions_are_random_hex_and_expire() {
        let sessions = Sessions::new(Duration::from_secs(10));
        let now = Instant::now();
        let a = sessions.create(now).expect("a");
        let b = sessions.create(now).expect("b");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "{a}");
        assert_ne!(a, b);
        assert!(sessions.valid(&a, now));
        assert!(sessions.valid(&a, now + Duration::from_secs(9)));
        assert!(!sessions.valid(&a, now + Duration::from_secs(11)));
        assert!(!sessions.valid("unknown", now));
        assert!(!sessions.valid("", now));
    }

    #[test]
    fn removed_session_is_invalid() {
        let sessions = Sessions::new(SESSION_TTL);
        let now = Instant::now();
        let id = sessions.create(now).expect("id");
        sessions.remove(&id);
        assert!(!sessions.valid(&id, now));
    }

    #[test]
    fn throttle_blocks_after_max_failures_within_window() {
        let throttle = Throttle::new(5, Duration::from_secs(60));
        let now = Instant::now();
        for _ in 0..5 {
            assert!(!throttle.blocked("ip", now));
            throttle.fail("ip", now);
        }
        assert!(throttle.blocked("ip", now));
        assert!(throttle.blocked("ip", now + Duration::from_secs(59)));
        assert!(!throttle.blocked("other", now), "keys are separate");
        assert!(
            !throttle.blocked("ip", now + Duration::from_secs(61)),
            "window over"
        );
    }

    #[test]
    fn throttle_clear_forgets_failures() {
        let throttle = Throttle::new(2, Duration::from_secs(60));
        let now = Instant::now();
        throttle.fail("ip", now);
        throttle.fail("ip", now);
        assert!(throttle.blocked("ip", now));
        throttle.clear("ip");
        assert!(!throttle.blocked("ip", now));
    }

    #[test]
    fn cookie_value_finds_the_named_cookie() {
        let header = "a=1; markban_session=abc; b=2";
        assert_eq!(cookie_value(header, COOKIE), Some("abc"));
        assert_eq!(cookie_value("markban_session=x", COOKIE), Some("x"));
        assert_eq!(cookie_value("xmarkban_session=x", COOKIE), None);
        assert_eq!(cookie_value("", COOKIE), None);
    }

    #[test]
    fn client_key_trusts_forwarded_for_only_when_told() {
        assert_eq!(
            client_key(Some("1.1.1.1, 2.2.2.2"), true),
            "2.2.2.2",
            "the entry the proxy added"
        );
        assert_eq!(client_key(Some("1.1.1.1"), false), "direct");
        assert_eq!(client_key(None, true), "direct");
        assert_eq!(client_key(Some(" "), true), "direct");
    }

    #[test]
    fn form_password_decodes_the_field() {
        assert_eq!(
            form_password(b"password=a%20b%26c+d").as_deref(),
            Some("a b&c d")
        );
        assert_eq!(form_password(b"x=1&password=p").as_deref(), Some("p"));
        assert_eq!(form_password(b"x=1"), None);
    }

    #[test]
    fn cookies_carry_the_safe_attributes() {
        let set = session_cookie("abc");
        assert!(set.starts_with("markban_session=abc;"), "{set}");
        for attr in [
            "HttpOnly",
            "Secure",
            "SameSite=Strict",
            "Path=/",
            "Max-Age=2592000",
        ] {
            assert!(set.contains(attr), "{attr} in {set}");
        }
        let clear = clear_cookie();
        assert!(clear.starts_with("markban_session=;"), "{clear}");
        assert!(clear.contains("Max-Age=0"), "{clear}");
    }

    #[test]
    fn login_page_has_one_password_field_and_no_script() {
        let page = login_page(Some("Wrong password."));
        assert_eq!(page.matches("<input").count(), 1, "{page}");
        assert!(page.contains("type=\"password\""), "{page}");
        assert!(page.contains("method=\"post\""), "{page}");
        assert!(page.contains("Wrong password."), "{page}");
        assert!(!page.contains("<script"), "{page}");
    }

    mod guard {
        use super::super::*;
        use dioxus::server::axum::Router;
        use dioxus::server::axum::body::Body;
        use dioxus::server::axum::http::{StatusCode, header};
        use dioxus::server::axum::routing::{get, post};
        use tower::ServiceExt;

        fn app(auth: &Arc<Auth>) -> Router {
            Router::new()
                .route("/", get(|| async { "the board" }))
                .route("/api/get_board", post(|| async { "board json" }))
                .route("/healthz", get(|| async { "ok" }))
                .layer(dioxus::server::axum::middleware::from_fn_with_state(
                    Arc::clone(auth),
                    guard,
                ))
        }

        async fn send(auth: &Arc<Auth>, request: Request) -> (StatusCode, Option<String>, String) {
            let response = app(auth).oneshot(request).await.expect("infallible");
            let status = response.status();
            let cookie = response
                .headers()
                .get(header::SET_COOKIE)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let body = dioxus::server::axum::body::to_bytes(response.into_body(), 1 << 20)
                .await
                .expect("body");
            (status, cookie, String::from_utf8_lossy(&body).into_owned())
        }

        fn get_req(path: &str, cookie: Option<&str>) -> Request {
            let mut b = Request::builder().uri(path);
            if let Some(c) = cookie {
                b = b.header(header::COOKIE, c);
            }
            b.body(Body::empty()).expect("request")
        }

        fn login(password: &str) -> Request {
            Request::builder()
                .method("POST")
                .uri("/login")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("password={password}")))
                .expect("request")
        }

        #[tokio::test]
        async fn pages_and_server_functions_need_a_session() {
            let auth = Arc::new(Auth::new("pw", false));
            let (status, _, body) = send(&auth, get_req("/", None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(body.contains("type=\"password\""), "{body}");
            assert!(!body.contains("the board"));
            let request = Request::builder()
                .method("POST")
                .uri("/api/get_board")
                .body(Body::empty())
                .expect("request");
            let (status, _, body) = send(&auth, request).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(!body.contains("board json"));
            let (status, _, body) = send(&auth, get_req("/", Some("markban_session=forged"))).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        }

        #[tokio::test]
        async fn healthz_stays_open() {
            let auth = Arc::new(Auth::new("pw", false));
            let (status, _, body) = send(&auth, get_req("/healthz", None)).await;
            assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
        }

        #[tokio::test]
        async fn right_password_gives_a_working_session_and_logout_ends_it() {
            let auth = Arc::new(Auth::new("pw", false));
            let (status, cookie, body) = send(&auth, login("wrong")).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(cookie.is_none());
            assert!(body.contains("Wrong password"), "{body}");
            assert!(!body.contains("wrong"), "the attempt is not echoed: {body}");

            let (status, cookie, _) = send(&auth, login("pw")).await;
            assert_eq!(status, StatusCode::SEE_OTHER);
            let cookie = cookie.expect("set-cookie");
            assert!(cookie.contains("HttpOnly"), "{cookie}");
            let pair = cookie.split(';').next().expect("pair").to_string();
            let (status, _, body) = send(&auth, get_req("/", Some(&pair))).await;
            assert_eq!((status, body.as_str()), (StatusCode::OK, "the board"));

            let (status, cleared, _) = send(&auth, get_req("/logout", Some(&pair))).await;
            assert_eq!(status, StatusCode::SEE_OTHER);
            assert!(cleared.expect("clear").contains("Max-Age=0"));
            let (status, _, _) = send(&auth, get_req("/", Some(&pair))).await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "session ended server side"
            );
        }

        #[tokio::test]
        async fn too_many_failures_answer_429_even_for_the_right_password() {
            let auth = Arc::new(Auth::new("pw", false));
            for _ in 0..MAX_FAILURES {
                let (status, _, _) = send(&auth, login("nope")).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED);
            }
            let (status, cookie, _) = send(&auth, login("pw")).await;
            assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
            assert!(cookie.is_none());
        }
    }
}
