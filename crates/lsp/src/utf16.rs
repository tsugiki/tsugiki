//! Converts between LSP's columns, which count UTF-16 code units, and bytes in a line of Rust text.

#[cfg(test)]
mod tests;

/// The byte in `line` where UTF-16 column `column` falls. A column past the line's end is the end, as LSP
/// says, and one inside a character, between the two halves of an emoji, is that character's start.
pub fn byte_of(line: &str, column: u32) -> usize {
    let mut units = 0_u32;
    for (byte, character) in line.char_indices() {
        units = units.saturating_add(width(character));
        if units > column {
            return byte;
        }
    }
    line.len()
}

/// The UTF-16 column of byte `byte` in `line`. A byte past the line's end is its end, and one inside a
/// character is that character's start.
pub fn column_of(line: &str, byte: usize) -> u32 {
    line.char_indices()
        .take_while(|&(start, character)| start.saturating_add(character.len_utf8()) <= byte)
        .map(|(_, character)| width(character))
        .sum()
}

/// One code unit, or two for a character outside the Basic Multilingual Plane.
fn width(character: char) -> u32 {
    u32::try_from(character.len_utf16()).unwrap_or(2)
}
