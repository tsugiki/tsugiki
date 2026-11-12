use std::cell::RefCell;

use super::*;

/// What `show_in` did to the window, in order.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Paint(Color),
    Show,
}

/// The background of the built-in theme of side `kind`, as the window takes it.
fn bg(kind: ThemeKind) -> Color {
    let [red, green, blue, alpha] = Themes::built_in_background(kind).rgba();
    Color(red, green, blue, alpha)
}

/// Runs `show_in` for `theme` on a window whose steps answer `painted` and `shown`, and says what it did, in
/// order, and what it returned.
fn steps(
    theme: tauri::Result<Theme>,
    painted: tauri::Result<()>,
    shown: tauri::Result<()>,
) -> (Vec<Step>, bool) {
    let done = RefCell::new(Vec::new());
    let result = show_in(
        theme,
        |color| {
            done.borrow_mut().push(Step::Paint(color));
            painted
        },
        || {
            done.borrow_mut().push(Step::Show);
            shown
        },
    );
    (done.into_inner(), result.is_ok())
}

#[test]
fn a_dark_system_gives_the_window_the_dark_themes_bg_before_it_shows() {
    assert_eq!(
        steps(Ok(Theme::Dark), Ok(()), Ok(())),
        (vec![Step::Paint(bg(ThemeKind::Dark)), Step::Show], true)
    );
}

#[test]
fn a_light_system_gives_the_window_the_light_themes_bg_before_it_shows() {
    assert_eq!(
        steps(Ok(Theme::Light), Ok(()), Ok(())),
        (vec![Step::Paint(bg(ThemeKind::Light)), Step::Show], true)
    );
}

#[test]
fn a_system_that_cant_say_reads_as_light() {
    assert_eq!(
        background(Err(tauri::Error::WindowNotFound)),
        bg(ThemeKind::Light)
    );
}

#[test]
fn the_window_shows_even_when_it_cant_take_the_background() {
    let (done, shown) = steps(Ok(Theme::Dark), Err(tauri::Error::WindowNotFound), Ok(()));
    assert_eq!(done, [Step::Paint(bg(ThemeKind::Dark)), Step::Show]);
    assert!(shown);
}

/// The page's background until the theme arrives is the same pair, so the load keeps one shade.
#[test]
fn the_page_starts_on_the_same_backgrounds() {
    let page = include_str!("../../../../ui/src/app.html");
    let pair = format!(
        "--bg: light-dark({}, {});",
        Themes::built_in_background(ThemeKind::Light).css(),
        Themes::built_in_background(ThemeKind::Dark).css()
    );
    assert!(page.contains(&pair), "app.html has no `{pair}`");
}

#[test]
fn a_window_that_wont_show_fails_the_start() {
    let (_, shown) = steps(Ok(Theme::Dark), Ok(()), Err(tauri::Error::WindowNotFound));
    assert!(!shown);
}
