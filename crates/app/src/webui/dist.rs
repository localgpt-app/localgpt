//! The built page (`npm run build` in `crates/app/ui`): read from disk in a
//! debug build and inside the binary in a release one. The window's web view
//! reads it through the `lgui://` scheme (`mac.rs`), and a paired device's
//! browser over HTTP (`remote.rs`). A build without it still runs, and the
//! page says how to build it.

use std::borrow::Cow;

#[derive(rust_embed::RustEmbed)]
#[folder = "ui/dist"]
#[allow_missing = true]
struct Dist;

/// A file of the built page by its path, with its type: the index for `/`,
/// a page that says how to build it when there is none, and never anything
/// above `dist`.
pub fn file(path: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let path = match path.trim_start_matches('/') {
        "" => "index.html",
        path => path,
    };
    if path.split('/').any(|part| part == "..") {
        return None;
    }
    match Dist::get(path) {
        Some(file) => Some((file.data, mime(path))),
        None if path == "index.html" => Some((Cow::Borrowed(NOT_BUILT.as_bytes()), "text/html")),
        None => None,
    }
}

const NOT_BUILT: &str = "<body style='font:14px system-ui;color:#ddd;background:#111;padding:2em'>\
    The interface is not built: <code>npm ci &amp;&amp; npm run build</code> in \
    <code>crates/app/ui</code>, then build the app again — or set LOCALGPT_APP_UI_URL \
    to its dev server.</body>";

fn mime(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html",
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_served_but_never_above_its_folder() {
        assert!(file("/../Cargo.toml").is_none());
        // With or without a built page, the index always answers.
        let (_, kind) = file("/").expect("the index");
        assert_eq!(kind, "text/html");
        assert_eq!(mime("assets/index-abc.js"), "text/javascript");
    }
}
