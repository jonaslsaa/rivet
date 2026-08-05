//! Port of `com.mojang.brigadier.arguments.ArgumentType` (upstream).
//!
//! // STUB(brigadier.builder): full port is the `com.mojang.brigadier.arguments`
//! unit; this is the surface the builder cluster references.

use crate::string_reader::StringReader;

/// Java `ArgumentType<T>` — parses a `StringReader` into a `T`.
pub trait ArgumentType<T>: Send + Sync {
    /// Java `parse(StringReader) throws CommandSyntaxException`.
    fn parse(
        &self,
        reader: &mut StringReader,
    ) -> Result<T, crate::exceptions::CommandSyntaxException<'static>>;
}
