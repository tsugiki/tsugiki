//! Keeps Tsugiki's window on Tsugiki's own pages (`docs/devel/links.md`). The UI sends links to the system
//! browser, and this refuses whatever gets past it, such as a refresh GitHub's sanitizer missed.

use tauri::Url;

/// Whether the window may go to `url`: one of the pages Tauri serves Tsugiki from, or `dev_url` in a
/// development build.
pub(crate) fn stays_in_tsugiki(url: &Url, dev_url: Option<&Url>) -> bool {
    let tauris = match url.scheme() {
        // macOS and Linux.
        "tauri" => url.host_str() == Some("localhost"),
        // Windows.
        "http" | "https" => url.host_str() == Some("tauri.localhost"),
        _ => false,
    };
    tauris || dev_url.is_some_and(|dev| dev.origin() == url.origin())
}

#[cfg(test)]
mod tests;
