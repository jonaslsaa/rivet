//! Port of `com.mojang.brigadier.arguments.IntegerArgumentType` (upstream
//! brigadier-1.3.10).

use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::arguments::ArgumentType;
use crate::context::CommandContext;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;

/// Java `IntegerArgumentType`.
pub struct IntegerArgumentType {
    minimum: i32,
    maximum: i32,
}

impl IntegerArgumentType {
    fn new(minimum: i32, maximum: i32) -> Self {
        IntegerArgumentType { minimum, maximum }
    }

    /// Java `IntegerArgumentType.integer()`.
    pub fn integer() -> Arc<dyn ArgumentType<i32>> {
        Self::integer_with_bounds(i32::MIN, i32::MAX)
    }

    /// Java `IntegerArgumentType.integer(int min)`.
    pub fn integer_min(min: i32) -> Arc<dyn ArgumentType<i32>> {
        Self::integer_with_bounds(min, i32::MAX)
    }

    /// Java `IntegerArgumentType.integer(int min, int max)`.
    pub fn integer_with_bounds(min: i32, max: i32) -> Arc<dyn ArgumentType<i32>> {
        Arc::new(IntegerArgumentType::new(min, max))
    }

    /// Java `IntegerArgumentType.getInteger(CommandContext, String)`.
    pub fn get_integer(context: &CommandContext<impl Clone + std::any::Any>, name: &str) -> i32 {
        context.get_argument::<i32>(name)
    }

    /// Java `getMinimum()`.
    pub fn get_minimum(&self) -> i32 {
        self.minimum
    }

    /// Java `getMaximum()`.
    pub fn get_maximum(&self) -> i32 {
        self.maximum
    }
}

impl ArgumentType<i32> for IntegerArgumentType {
    fn parse(&self, reader: &mut StringReader) -> Result<i32, CommandSyntaxException<'static>> {
        let start = reader.get_cursor();
        let result = reader.read_int()?;
        if result < self.minimum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .integer_too_low()
                .create_with_context(reader, &result.to_string(), &self.minimum.to_string()));
        }
        if result > self.maximum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .integer_too_high()
                .create_with_context(reader, &result.to_string(), &self.maximum.to_string()));
        }
        Ok(result)
    }

    fn to_string(&self) -> String {
        if self.minimum == i32::MIN && self.maximum == i32::MAX {
            "integer()".to_string()
        } else if self.maximum == i32::MAX {
            format!("integer({})", self.minimum)
        } else {
            format!("integer({}, {})", self.minimum, self.maximum)
        }
    }

    fn get_examples(&self) -> Vec<String> {
        ["0", "123", "-123"].iter().map(|s| s.to_string()).collect()
    }

    fn type_equals(&self, other: &dyn ArgumentType<i32>) -> bool {
        match other.as_any().downcast_ref::<IntegerArgumentType>() {
            Some(that) => self.minimum == that.minimum && self.maximum == that.maximum,
            None => false,
        }
    }

    fn type_hash_code(&self) -> i32 {
        // Java: `31 * minimum + maximum`.
        31_i32.wrapping_mul(self.minimum).wrapping_add(self.maximum)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
