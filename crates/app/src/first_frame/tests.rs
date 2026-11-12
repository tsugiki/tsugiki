use std::cell::RefCell;

use tsugiki_core::error::ErrorKind;
use tsugiki_core::theme::Themes;

use super::*;

/// What `show_in` did to the window, in order.
#[derive(Debug, PartialEq, Eq)]
enum Step {
    Paint(u8, u8, u8, u8),
    Show,
}

/// Runs `show_in` on a window whose steps answer `painted` and `shown`, and says what it did, in order.
fn steps(
    background: Result<Color, Error>,
    painted: tauri::Result<()>,
    shown: tauri::Result<()>,
) -> Vec<Step> {
    let done = RefCell::new(Vec::new());
    show_in(
        background,
        |WindowColor(red, green, blue, alpha)| {
            done.borrow_mut().push(Step::Paint(red, green, blue, alpha));
            painted
        },
        || {
            done.borrow_mut().push(Step::Show);
            shown
        },
    );
    done.into_inner()
}

#[test]
fn a_dark_system_gives_the_window_the_dark_themes_bg_before_it_shows() {
    let background = Themes::built_in_background(side(Ok(Theme::Dark)));
    assert_eq!(
        steps(background, Ok(()), Ok(())),
        [Step::Paint(0x1f, 0x1f, 0x1f, 0xff), Step::Show]
    );
}

#[test]
fn a_light_system_gives_the_window_the_light_themes_bg_before_it_shows() {
    let background = Themes::built_in_background(side(Ok(Theme::Light)));
    assert_eq!(
        steps(background, Ok(()), Ok(())),
        [Step::Paint(0xff, 0xff, 0xff, 0xff), Step::Show]
    );
}

#[test]
fn a_system_that_cant_say_reads_as_light() {
    assert_eq!(side(Err(tauri::Error::WindowNotFound)), ThemeKind::Light);
}

#[test]
fn the_window_shows_even_when_it_cant_take_the_background() {
    let none = Err(Error::new(ErrorKind::Internal, "no background"));
    assert_eq!(steps(none, Ok(()), Ok(())), [Step::Show]);

    let light = Themes::built_in_background(ThemeKind::Light);
    let refused = steps(
        light,
        Err(tauri::Error::WindowNotFound),
        Err(tauri::Error::WindowNotFound),
    );
    assert_eq!(refused, [Step::Paint(0xff, 0xff, 0xff, 0xff), Step::Show]);
}
