//! Port of the `com.mojang.brigadier.arguments` package (upstream brigadier-1.3.10).

pub mod bool_argument_type;
pub mod double_argument_type;
pub mod float_argument_type;
pub mod integer_argument_type;
pub mod long_argument_type;
pub mod string_argument_type;

pub use bool_argument_type::BoolArgumentType;
pub use double_argument_type::DoubleArgumentType;
pub use float_argument_type::FloatArgumentType;
pub use integer_argument_type::IntegerArgumentType;
pub use long_argument_type::LongArgumentType;
pub use string_argument_type::{StringArgumentType, escape_if_required};

use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestions, SuggestionsBuilder};

/// Java `ArgumentType<T>`.
///
/// Java's methods are generic over the command source `S` (`parse(reader,
/// source)`, `listSuggestions(CommandContext<S>, builder)`). The built-in types in
/// this crate never read the source, so their `parse`/`list_suggestions` overrides
/// take but ignore it. `ArgumentCommandNode.parse` calls
/// `parse_with_source(reader, source)`, reproducing Java's
/// `type.parse(reader, contextBuilder.getSource())`, and its `list_suggestions`
/// threads the context through, so a custom `ArgumentType` that reads the command
/// source still receives it.
pub trait ArgumentType<T>: Send + Sync {
    /// Java `parse(StringReader) throws CommandSyntaxException`.
    fn parse(&self, reader: &mut StringReader) -> Result<T, CommandSyntaxException<'static>>;

    /// Java `default <S> T parse(StringReader, S source)` — forwards to `parse`.
    /// No built-in type overrides it; `ArgumentCommandNode.parse` calls this so a
    /// custom override can read the command source.
    fn parse_with_source(
        &self,
        reader: &mut StringReader,
        _source: &dyn std::any::Any,
    ) -> Result<T, CommandSyntaxException<'static>> {
        self.parse(reader)
    }

    /// Java `default <S> listSuggestions(CommandContext<S>, SuggestionsBuilder)`.
    /// The context is erased to `&dyn Any` (like `parse_with_source`) because the
    /// trait is object-safe over `T` only — Java's `S` is a method-level generic a
    /// vtable cannot dispatch. A custom `ArgumentType` downcasts to recover the
    /// `CommandContext<S>`. The only built-in override (`BoolArgumentType`) does not
    /// read it.
    fn list_suggestions(
        &self,
        _context: &dyn std::any::Any,
        _builder: &mut SuggestionsBuilder,
    ) -> Suggestions {
        Suggestions::empty()
    }

    /// Java `default getExamples()`.
    fn get_examples(&self) -> Vec<String> {
        Vec::new()
    }

    /// Java `toString()` — rendered by `ArgumentCommandNode.toString` as
    /// `<argument name:type>`. The concrete types override it.
    fn to_string(&self) -> String;

    /// Java `equals(Object)` — value equality for the concrete types (Java
    /// `Object.equals` identity otherwise). Identity on the data pointer matches
    /// Java's `==` for types without an `equals` override.
    fn type_equals(&self, other: &dyn ArgumentType<T>) -> bool {
        let a = (self as *const Self) as *const ();
        let b = (other as *const dyn ArgumentType<T>) as *const ();
        a == b
    }

    /// Java `hashCode()` — identity for types without an override; a constant keeps
    /// equal-implies-equal-hash for identity-equal instances.
    fn type_hash_code(&self) -> i32 {
        0
    }

    /// Downcast helper (`instanceof` in Java), for the value `type_equals`
    /// implementations.
    fn as_any(&self) -> &dyn std::any::Any;
}

#[cfg(test)]
mod tests {
    pub mod bool_argument_type;
    pub mod double_argument_type;
    pub mod float_argument_type;
    pub mod integer_argument_type;
    pub mod long_argument_type;
    pub mod string_argument_type;
}
