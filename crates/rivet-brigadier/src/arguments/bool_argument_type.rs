//! Port of `com.mojang.brigadier.arguments.BoolArgumentType` (upstream
//! brigadier-1.3.10).

use std::sync::Arc;

use crate::arguments::ArgumentType;
use crate::context::CommandContext;
use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;
use crate::suggestion::{Suggestions, SuggestionsBuilder};

/// Java `BoolArgumentType`.
pub struct BoolArgumentType;

impl BoolArgumentType {
    /// Java `BoolArgumentType.bool()`.
    pub fn bool() -> Arc<dyn ArgumentType<bool>> {
        Arc::new(BoolArgumentType)
    }

    /// Java `BoolArgumentType.getBool(CommandContext, String)`.
    pub fn get_bool(context: &CommandContext<impl Clone + std::any::Any>, name: &str) -> bool {
        context.get_argument::<bool>(name)
    }
}

impl ArgumentType<bool> for BoolArgumentType {
    fn parse(&self, reader: &mut StringReader) -> Result<bool, CommandSyntaxException<'static>> {
        reader.read_boolean()
    }

    fn list_suggestions(
        &self,
        _context: &dyn std::any::Any,
        builder: &mut SuggestionsBuilder,
    ) -> Suggestions {
        if "true".starts_with(builder.get_remaining_lower_case()) {
            builder.suggest("true");
        }
        if "false".starts_with(builder.get_remaining_lower_case()) {
            builder.suggest("false");
        }
        builder.build()
    }

    fn get_examples(&self) -> Vec<String> {
        ["true", "false"].iter().map(|s| s.to_string()).collect()
    }

    fn to_string(&self) -> String {
        "bool()".to_string()
    }

    fn type_equals(&self, other: &dyn ArgumentType<bool>) -> bool {
        other.as_any().downcast_ref::<BoolArgumentType>().is_some()
    }

    fn type_hash_code(&self) -> i32 {
        // Java `BoolArgumentType` does not override hashCode — identity (single
        // static instance `bool()`), reproduced by a fixed constant.
        0
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
