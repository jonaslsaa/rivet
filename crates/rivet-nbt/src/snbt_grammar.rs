//! Port of `net.minecraft.nbt.SnbtGrammar`.
//!
//! The full grammar builds a packrat parser over `DynamicOps<T>` (the
//! `net.minecraft.util.parsing.packrat` machinery) and is owned by unit
//! mc.nbt.snbt. This module currently provides the parts the tag types and
//! visitors depend on (`escapeControlCharacters`) and will be extended with the
//! full `Grammar` port when `TagParser`/`SnbtOperations` are translated.

/// `SnbtGrammar.escapeControlCharacters(char)`.
pub fn escape_control_characters(c: char) -> Option<String> {
    match c {
        '\u{0008}' => Some("b".to_string()), // \b
        '\u{0009}' => Some("t".to_string()), // \t
        '\u{000A}' => Some("n".to_string()), // \n
        '\u{000C}' => Some("f".to_string()), // \f
        '\u{000D}' => Some("r".to_string()), // \r
        _ => {
            if (c as u32) < 0x20 {
                // `"x" + HEX_ESCAPE.toHexDigits((byte)c)` — uppercase hex, 2 digits.
                Some(format!("x{:02X}", (c as u8)))
            } else {
                None
            }
        }
    }
}
