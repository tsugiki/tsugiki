//! The window's background, in the theme in force (#516). The webview paints white until the page loads, so
//! the window opens hidden, takes the `bg` of the theme for the side the system is on, and only then shows.
//! When the system changes sides, the window takes the other side's `bg`, so a resize doesn't show the old
//! one. A Theme that `settings.toml` names keeps the window on its side whatever the system says, its title
//! bar and native controls with it (#755).

use tauri::Theme;
use tauri::window::Color;
use tsugiki_core::error::Error;
use tsugiki_core::settings::{AppearanceSettings, SettingsOwner};
use tsugiki_core::theme::{GivenTheme, ThemeKind, ThemeLibrary, Themes};

/// The side the system is on, as `theme` gives it. One it can't tell reads as light, the side Windows
/// starts on.
fn side(theme: tauri::Result<Theme>) -> ThemeKind {
    match theme {
        Ok(Theme::Dark) => ThemeKind::Dark,
        Ok(_) => ThemeKind::Light,
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

/// The window, as the `themes` command reaches it: `set_theme` turns it, and `theme` says the side it is on.
pub struct ThemedWindow<'a> {
    pub set_theme: &'a dyn Fn(Option<Theme>) -> tauri::Result<()>,
    pub theme: &'a dyn Fn() -> tauri::Result<Theme>,
}

/// Every theme for the window, with the theme in force, from the `themes` folder and the settings `owner`
/// holds, and turns `window` to the side of the theme in force (#755).
///
/// While the window follows the system, it says the side the window is on once it has turned back, which the
/// page's media query reports only a moment later.
pub fn themes(owner: &SettingsOwner, window: &ThemedWindow<'_>) -> Result<Themes, Error> {
    let view = owner.view();
    let library = ThemeLibrary::read(&view.themes_folder)?;
    let AppearanceSettings {
        theme,
        light_theme,
        dark_theme,
    } = &view.loaded.settings.appearance;
    let given = GivenTheme {
        choice: theme,
        place: &view.theme_place,
    };
    let mut themes = library.themes(given, light_theme, dark_theme);
    let keep = themes.theme.side();
    turn(keep, window.set_theme);
    if keep.is_none() {
        themes.system = Some(side((window.theme)()));
    }
    Ok(themes)
}

#[cfg(test)]
mod tests;
