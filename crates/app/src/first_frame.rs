//! The window's first frame, in the theme in force (#516). The webview paints white until the page loads, so
//! the window opens hidden, takes the `bg` of the theme for the side the system is on, and only then shows.

use tauri::Theme;
use tauri::window::Color as WindowColor;
use tsugiki_core::error::Error;
use tsugiki_core::theme::{Color, ThemeKind};

/// The side the system is on, as `theme` gives it. One it can't tell reads as light, the side Windows
/// starts on.
pub fn side(theme: tauri::Result<Theme>) -> ThemeKind {
    match theme {
        Ok(Theme::Dark) => ThemeKind::Dark,
        Ok(_) => ThemeKind::Light,
        Err(cause) => {
            log::warn!(
                "Tsugiki can't tell which side the system is on, so it opens light: {cause}"
            );
            ThemeKind::Light
        }
    }
}

/// Gives the window `background` through `paint`, then shows it through `show`. A step that fails is logged,
/// and the window shows all the same: a white flash beats no window.
pub fn show_in(
    background: Result<Color, Error>,
    paint: impl FnOnce(WindowColor) -> tauri::Result<()>,
    show: impl FnOnce() -> tauri::Result<()>,
) {
    match background {
        Ok(color) => {
            let [red, green, blue, alpha] = color.rgba();
            if let Err(cause) = paint(WindowColor(red, green, blue, alpha)) {
                log::warn!("Tsugiki can't give its window the theme's background: {cause}");
            }
        }
        Err(cause) => log::warn!("{}", cause.message),
    }
    if let Err(cause) = show() {
        log::error!("Tsugiki can't show its window: {cause}");
    }
}

#[cfg(test)]
mod tests;
