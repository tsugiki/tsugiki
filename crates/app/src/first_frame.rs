//! The window's background, in the theme in force (#516). The webview paints white until the page loads, so
//! the window opens hidden, takes the `bg` of the theme for the side the system is on, and only then shows.
//! When the system changes sides, the window takes the other side's `bg`, so a resize doesn't show the old
//! one.

use tauri::Theme;
use tauri::window::Color;
use tsugiki_core::theme::{ThemeKind, Themes};

/// The side the system is on, as `theme` gives it. One it can't tell reads as light, the side Windows
/// starts on.
fn side(theme: tauri::Result<Theme>) -> ThemeKind {
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

/// The window's background for the side `theme` names: the `bg` of that side's built-in theme.
pub fn background(theme: tauri::Result<Theme>) -> Color {
    let [red, green, blue, alpha] = Themes::built_in_background(side(theme)).rgba();
    Color(red, green, blue, alpha)
}

/// Gives the window the background for the side `theme` names through `paint`. A window that can't take it
/// keeps the webview's white, which the log says.
pub fn paint(theme: tauri::Result<Theme>, paint: impl FnOnce(Color) -> tauri::Result<()>) {
    if let Err(cause) = paint(background(theme)) {
        log::warn!("Tsugiki can't give its window the theme's background: {cause}");
    }
}

/// Paints the window through `paint`, then shows it through `show`. A window that won't show fails the start,
/// as one that won't open does, since the user would have no window to close.
pub fn show_in(
    theme: tauri::Result<Theme>,
    paint_with: impl FnOnce(Color) -> tauri::Result<()>,
    show: impl FnOnce() -> tauri::Result<()>,
) -> tauri::Result<()> {
    paint(theme, paint_with);
    show()
}

#[cfg(test)]
mod tests;
