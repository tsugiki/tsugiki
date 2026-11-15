//! The window's background, in the theme in force (#516). The webview paints white until the page loads, so
//! the window opens hidden, takes the `bg` of the theme for the side the system is on, and only then shows.
//! When the system changes sides, the window takes the other side's `bg`, so a resize doesn't show the old
//! one. A Theme that `settings.toml` names keeps the window on its side whatever the system says, its title
//! bar and native controls with it (#755).

use tauri::Theme;
use tauri::window::Color;
use tsugiki_core::theme::{ThemeKind, Themes};

/// The side of Tauri's `theme`. Tauri may add themes, and one it adds reads as light.
fn kind(theme: Theme) -> ThemeKind {
    match theme {
        Theme::Dark => ThemeKind::Dark,
        Theme::Light | _ => ThemeKind::Light,
    }
}

/// The side the system is on, as `theme` gives it. One it can't tell reads as light, the side Windows
/// starts on.
fn side(theme: tauri::Result<Theme>) -> ThemeKind {
    match theme {
        Ok(theme) => kind(theme),
        Err(cause) => {
            log::warn!(
                "Tsugiki can't tell which side the system is on, so it takes the light side: {cause}"
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

/// The theme Tauri gives the window that keeps to `side`: none to follow the system.
pub fn window_theme(side: Option<ThemeKind>) -> Option<Theme> {
    side.map(|side| match side {
        ThemeKind::Light => Theme::Light,
        ThemeKind::Dark => Theme::Dark,
    })
}

/// Turns the window to `side` through `set_theme`, and its title bar and native controls with it, or back to
/// the system's side with none. A window that can't turn keeps the side it was on, which the log says.
///
/// On Windows the webview's `prefers-color-scheme` follows the window, so the page's media query reports
/// `side` too until the window follows the system again.
///
/// `set_theme` isn't generic, so the app and the tests run one copy of this, and the coverage gate sees each
/// line run.
pub fn turn(side: Option<ThemeKind>, set_theme: &dyn Fn(Option<Theme>) -> tauri::Result<()>) {
    if let Err(cause) = set_theme(window_theme(side)) {
        log::warn!("Tsugiki can't turn its window to the side its settings ask for: {cause}");
    }
}

/// The window, as `turn_window` reaches it: `set_theme` turns it, and `theme` says the side it is on.
pub struct ThemedWindow<'a> {
    pub set_theme: &'a dyn Fn(Option<Theme>) -> tauri::Result<()>,
    pub theme: &'a dyn Fn() -> tauri::Result<Theme>,
}

/// Turns `window` to `side`, as `turn` does, for the page that shows a theme of that side (#755). Back to
/// following the system, it says the side the window is on then, which the page's media query reports only
/// a moment later: a theme that kept the window on one side kept the query there too. A window that can't
/// say gives none, which the log says, and the page goes on with what its query reports.
pub fn turn_window(side: Option<ThemeKind>, window: &ThemedWindow<'_>) -> Option<ThemeKind> {
    turn(side, window.set_theme);
    if side.is_some() {
        return None;
    }
    match (window.theme)() {
        Ok(theme) => Some(kind(theme)),
        Err(cause) => {
            log::warn!("Tsugiki can't tell which side its window turned back to: {cause}");
            None
        }
    }
}

#[cfg(test)]
mod tests;
