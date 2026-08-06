//! Port of `com.mojang.brigadier.arguments.StringArgumentType` (upstream
//! brigadier-1.3.10).

use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::arguments::ArgumentType;
use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;

/// Java `StringArgumentType.StringType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringType {
    SingleWord,
    QuotablePhrase,
    GreedyPhrase,
}

impl StringType {
    fn examples(self) -> Vec<String> {
        match self {
            StringType::SingleWord => vec!["word", "words_with_underscores"],
            StringType::QuotablePhrase => vec!["\"quoted phrase\"", "word", "\"\""],
            StringType::GreedyPhrase => vec!["word", "words with spaces", "\"and symbols\""],
        }
        .iter()
        .map(|s| s.to_string())
        .collect()
    }
}

/// Java `StringArgumentType`.
pub struct StringArgumentType {
    type_: StringType,
}

impl StringArgumentType {
    fn new(type_: StringType) -> Self {
        StringArgumentType { type_ }
    }

    /// Java `StringArgumentType.word()`.
    pub fn word() -> Arc<dyn ArgumentType<String>> {
        Arc::new(StringArgumentType::new(StringType::SingleWord))
    }

    /// Java `StringArgumentType.string()`.
    pub fn string() -> Arc<dyn ArgumentType<String>> {
        Arc::new(StringArgumentType::new(StringType::QuotablePhrase))
    }

    /// Java `StringArgumentType.greedyString()`.
    pub fn greedy_string() -> Arc<dyn ArgumentType<String>> {
        Arc::new(StringArgumentType::new(StringType::GreedyPhrase))
    }

    /// Java `StringArgumentType.getString(CommandContext, String)`.
    pub fn get_string(context: &CommandContext<impl Clone + std::any::Any>, name: &str) -> String {
        context.get_argument::<String>(name)
    }

    /// Java `getType()`.
    pub fn get_type(&self) -> StringType {
        self.type_
    }
}

impl ArgumentType<String> for StringArgumentType {
    fn parse(&self, reader: &mut StringReader) -> Result<String, CommandSyntaxException<'static>> {
        match self.type_ {
            StringType::GreedyPhrase => {
                let text = reader.get_remaining();
                reader.set_cursor(reader.get_total_length());
                Ok(text)
            }
            StringType::SingleWord => Ok(reader.read_unquoted_string()),
            StringType::QuotablePhrase => reader.read_string(),
        }
    }

    fn to_string(&self) -> String {
        "string()".to_string()
    }

    fn get_examples(&self) -> Vec<String> {
        self.type_.examples()
    }

    fn type_equals(&self, other: &dyn ArgumentType<String>) -> bool {
        match other.as_any().downcast_ref::<StringArgumentType>() {
            Some(that) => self.type_ == that.type_,
            None => false,
        }
    }

    fn type_hash_code(&self) -> i32 {
        // Java `StringArgumentType` does not override hashCode — identity. The enum
        // value is used so equal types hash equal.
        self.type_ as i32
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Java `StringArgumentType.escapeIfRequired(String)` — escape the input if it
/// contains any char not allowed in an unquoted string, else return it unchanged.
pub fn escape_if_required(input: &str) -> String {
    if input
        .chars()
        .any(|c| !StringReader::is_allowed_in_unquoted_string(c))
    {
        escape(input)
    } else {
        input.to_string()
    }
}

/// Java `StringArgumentType.escape(String)`.
fn escape(input: &str) -> String {
    let mut result = String::from("\"");
    for c in input.chars() {
        if c == '\\' || c == '"' {
            result.push('\\');
        }
        result.push(c);
    }
    result.push('"');
    result
}
