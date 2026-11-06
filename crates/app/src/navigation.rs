//! Keeps Tsugiki's window on Tsugiki's own pages (`docs/scope.md`). The UI sends links to the system browser,
//! and this refuses whatever gets past it, such as a refresh GitHub's sanitizer missed.

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
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    /// The development server's address, from `tauri.conf.json`. Written here, it would be a host that
    /// `security.rs` has to allow in all of Rust.
    fn dev_url() -> Url {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        url(config["build"]["devUrl"].as_str().unwrap())
    }

    #[test]
    fn the_pages_tauri_serves_are_tsugikis() {
        let own = [
            "tauri://localhost/",
            "http://tauri.localhost/index.html",
            "https://tauri.localhost/",
        ];
        assert_eq!(
            own.map(|text| stays_in_tsugiki(&url(text), None)),
            [true; 3]
        );
    }

    #[test]
    fn a_web_page_or_another_scheme_isnt() {
        let others = [
            "https://github.com/python/cpython",
            "tauri://github.com/",
            "https://github.com/tauri.localhost/",
            "about:blank",
            "data:text/html,hello",
            "file:///C:/Windows/",
        ];
        assert_eq!(
            others.map(|text| stays_in_tsugiki(&url(text), None)),
            [false; 6]
        );
    }

    #[test]
    fn only_a_development_build_stays_on_its_dev_server() {
        let dev = dev_url();
        let mut other_port = dev.clone();
        other_port
            .set_port(dev.port().map(|port| port + 1))
            .unwrap();
        assert!(stays_in_tsugiki(&dev.join("?item=7").unwrap(), Some(&dev)));
        assert!(!stays_in_tsugiki(&dev, None));
        assert!(!stays_in_tsugiki(&other_port, Some(&dev)));
        assert!(!stays_in_tsugiki(&url("https://github.com/"), Some(&dev)));
    }
}
