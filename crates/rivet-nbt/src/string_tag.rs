//! Port of `net.minecraft.nbt.StringTag` — `record StringTag(String value)`.

pub const SELF_SIZE_IN_BYTES: i32 = 36;

/// `StringTag` — value struct (Java record).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StringTag {
    pub value: String,
}

impl StringTag {
    /// `StringTag.valueOf(String)`.
    pub fn value_of(data: String) -> Self {
        StringTag { value: data }
    }

    /// `StringTag.sizeInBytes()` — `36 + 2 * value.length()` (UTF-16 units).
    pub fn size_in_bytes(&self) -> i32 {
        36 + 2 * self.value.encode_utf16().count() as i32
    }
}

impl StringTag {
    /// `StringTag.quoteAndEscape(String)` (static).
    pub fn quote_and_escape(input: &str) -> String {
        quote_and_escape(input)
    }

    /// `StringTag.quoteAndEscape(input, result)` (static, buffer form).
    pub fn quote_and_escape_into(input: &str, result: &mut String) {
        quote_and_escape_into(input, result);
    }

    /// `StringTag.escapeWithoutQuotes(String)` (static).
    pub fn escape_without_quotes(input: &str) -> String {
        escape_without_quotes(input)
    }

    /// `StringTag.escapeWithoutQuotes(input, result)` (static, buffer form).
    pub fn escape_without_quotes_into(input: &str, result: &mut String) {
        escape_without_quotes_into(input, result);
    }
}

impl std::fmt::Display for StringTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", quote_and_escape(&self.value))
    }
}

/// `StringTag.quoteAndEscape(String)`.
pub fn quote_and_escape(input: &str) -> String {
    let mut result = String::new();
    quote_and_escape_into(input, &mut result);
    result
}

/// `StringTag.quoteAndEscape(input, result)` — appends into an existing
/// buffer (used by `StringTagVisitor.handleKeyEscape`).
pub fn quote_and_escape_into(input: &str, result: &mut String) {
    let quote_mark_index = result.len();
    result.push(' ');
    let mut quote: char = '\0';
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            result.push_str("\\\\");
        } else if c != '"' && c != '\'' {
            if let Some(escaped) = crate::snbt_grammar::escape_control_characters(c) {
                result.push('\\');
                result.push_str(&escaped);
            } else {
                result.push(c);
            }
        } else {
            if quote == '\0' {
                quote = if c == '"' { '\'' } else { '"' };
            }
            if quote == c {
                result.push('\\');
            }
            result.push(c);
        }
        i += 1;
    }
    if quote == '\0' {
        quote = '"';
    }
    result.replace_range(quote_mark_index..=quote_mark_index, &quote.to_string());
    result.push(quote);
}

/// `StringTag.escapeWithoutQuotes(String)`.
pub fn escape_without_quotes(input: &str) -> String {
    let mut result = String::new();
    escape_without_quotes_into(input, &mut result);
    result
}

/// `StringTag.escapeWithoutQuotes(input, result)`.
pub fn escape_without_quotes_into(input: &str, result: &mut String) {
    for c in input.chars() {
        match c {
            '"' | '\'' | '\\' => {
                result.push('\\');
                result.push(c);
            }
            _ => {
                if let Some(escaped) = crate::snbt_grammar::escape_control_characters(c) {
                    result.push('\\');
                    result.push_str(&escaped);
                } else {
                    result.push(c);
                }
            }
        }
    }
}
