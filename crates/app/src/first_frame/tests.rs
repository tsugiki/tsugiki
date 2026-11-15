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

#[test]
fn following_the_system_leaves_the_window_to_follow_the_system() {
    assert_eq!(window_theme(None), None);
}

#[test]
fn a_side_kept_keeps_the_window_on_it_whatever_the_system_says() {
    assert_eq!(window_theme(Some(ThemeKind::Light)), Some(Theme::Light));
    assert_eq!(window_theme(Some(ThemeKind::Dark)), Some(Theme::Dark));
}

#[test]
fn the_window_turns_to_the_side_kept() {
    let asked = RefCell::new(Vec::new());
    for side in [Some(ThemeKind::Dark), None] {
        turn(side, &|theme| {
            asked.borrow_mut().push(theme);
            Ok(())
        });
    }
    // A window that can't turn keeps the side it was on, and the page goes on.
    turn(Some(ThemeKind::Light), &|theme| {
        asked.borrow_mut().push(theme);
        Err(tauri::Error::WindowNotFound)
    });
    assert_eq!(
        asked.into_inner(),
        [Some(Theme::Dark), None, Some(Theme::Light)]
    );
}

/// Turns a window whose `theme` answers `system` to `side`, and gives what `turn_window` said and each side
/// the window was turned to.
fn turned_window(
    side: Option<ThemeKind>,
    system: tauri::Result<Theme>,
) -> (Option<ThemeKind>, Vec<Option<Theme>>) {
    let turned = RefCell::new(Vec::new());
    let system = RefCell::new(Some(system));
    let window = ThemedWindow {
        set_theme: &|theme| {
            turned.borrow_mut().push(theme);
            Ok(())
        },
        theme: &|| system.borrow_mut().take().unwrap(),
    };
    let said = turn_window(side, &window);
    (said, turned.into_inner())
}

#[test]
fn a_window_turned_to_a_side_says_nothing_of_the_system() {
    assert_eq!(
        turned_window(Some(ThemeKind::Dark), Ok(Theme::Light)),
        (None, vec![Some(Theme::Dark)])
    );
}

/// The page's media query still reports the side a theme kept until the webview catches up, so the window
/// says the side it turned back to.
#[test]
fn a_window_turned_back_to_the_system_says_the_side_it_is_on() {
    assert_eq!(
        turned_window(None, Ok(Theme::Dark)),
        (Some(ThemeKind::Dark), vec![None])
    );
    assert_eq!(
        turned_window(None, Ok(Theme::Light)),
        (Some(ThemeKind::Light), vec![None])
    );
}

/// The page then goes on with what its media query reports.
#[test]
fn a_window_that_cant_say_its_side_says_none() {
    assert_eq!(
        turned_window(None, Err(tauri::Error::WindowNotFound)),
        (None, vec![None])
    );
}
