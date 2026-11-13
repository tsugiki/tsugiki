use super::{byte_of, column_of};

/// Each UTF-16 column of `line` with the byte it lands on, and back.
fn round_trip(line: &str, pairs: &[(u32, usize)]) {
    for &(column, byte) in pairs {
        assert_eq!(byte_of(line, column), byte, "{line:?}: column {column}");
        assert_eq!(column_of(line, byte), column, "{line:?}: byte {byte}");
    }
}

#[test]
fn ascii_counts_the_same_either_way() {
    round_trip("def f():", &[(0, 0), (4, 4), (8, 8)]);
}

#[test]
fn an_accented_letter_is_one_column_and_two_bytes() {
    round_trip("café = 1", &[(0, 0), (3, 3), (4, 5), (5, 6), (8, 9)]);
}

#[test]
fn a_cjk_character_is_one_column_and_three_bytes() {
    round_trip("名前 = 2", &[(0, 0), (1, 3), (2, 6), (3, 7), (6, 10)]);
}

#[test]
fn an_emoji_outside_the_bmp_is_two_columns_and_four_bytes() {
    round_trip("x = '😀'", &[(5, 5), (7, 9), (8, 10)]);
}

#[test]
fn a_column_or_byte_past_the_end_is_the_end() {
    assert_eq!(byte_of("café", 40), 5);
    assert_eq!(column_of("café", 40), 4);
    assert_eq!(byte_of("", 1), 0);
}

#[test]
fn a_place_inside_a_character_is_its_start() {
    // Column 6 falls between the emoji's two halves, and byte 7 inside its four bytes.
    assert_eq!(byte_of("x = '😀'", 6), 5);
    assert_eq!(column_of("x = '😀'", 7), 5);
    assert_eq!(column_of("café", 4), 3);
}
