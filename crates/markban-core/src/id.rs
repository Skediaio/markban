//! Card and column ids.

/// Longest allowed card id.
pub const MAX_ID_LEN: usize = 64;

/// True when `id` matches `[a-z0-9]+(-[a-z0-9]+)*`.
#[must_use]
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// Slug of a title (spec section 4): lowercase ASCII letters and digits, every run of other
/// characters becomes `-`, trimmed of `-`, at most 64 characters. An empty result is `card`.
#[must_use]
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in title.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else {
            pending_dash = true;
        }
    }
    out.truncate(MAX_ID_LEN);
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "card".to_string()
    } else {
        out
    }
}
