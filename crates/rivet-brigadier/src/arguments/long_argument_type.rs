//! Port of `com.mojang.brigadier.arguments.LongArgumentType` (upstream
//! brigadier-1.3.10).

use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::arguments::ArgumentType;
use crate::context::CommandContext;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;

/// Java `LongArgumentType`.
pub struct LongArgumentType {
    minimum: i64,
    maximum: i64,
}

impl LongArgumentType {
    fn new(minimum: i64, maximum: i64) -> Self {
        LongArgumentType { minimum, maximum }
    }

    /// Java `LongArgumentType.longArg()`.
    pub fn long_arg() -> Arc<dyn ArgumentType<i64>> {
        Self::long_arg_with_bounds(i64::MIN, i64::MAX)
    }

    /// Java `LongArgumentType.longArg(long min)`.
    pub fn long_arg_min(min: i64) -> Arc<dyn ArgumentType<i64>> {
        Self::long_arg_with_bounds(min, i64::MAX)
    }

    /// Java `LongArgumentType.longArg(long min, long max)`.
    pub fn long_arg_with_bounds(min: i64, max: i64) -> Arc<dyn ArgumentType<i64>> {
        Arc::new(LongArgumentType::new(min, max))
    }

    /// Java `LongArgumentType.getLong(CommandContext, String)`.
    pub fn get_long(context: &CommandContext<impl Clone + std::any::Any>, name: &str) -> i64 {
        context.get_argument::<i64>(name)
    }

    /// Java `getMinimum()`.
    pub fn get_minimum(&self) -> i64 {
        self.minimum
    }

    /// Java `getMaximum()`.
    pub fn get_maximum(&self) -> i64 {
        self.maximum
    }
}

impl ArgumentType<i64> for LongArgumentType {
    fn parse(&self, reader: &mut StringReader) -> Result<i64, CommandSyntaxException<'static>> {
        let start = reader.get_cursor();
        let result = reader.read_long()?;
        if result < self.minimum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .long_too_low()
                .create_with_context(reader, &result.to_string(), &self.minimum.to_string()));
        }
        if result > self.maximum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .long_too_high()
                .create_with_context(reader, &result.to_string(), &self.maximum.to_string()));
        }
        Ok(result)
    }

    fn to_string(&self) -> String {
        if self.minimum == i64::MIN && self.maximum == i64::MAX {
            "longArg()".to_string()
        } else if self.maximum == i64::MAX {
            format!("longArg({})", self.minimum)
        } else {
            format!("longArg({}, {})", self.minimum, self.maximum)
        }
    }

    fn get_examples(&self) -> Vec<String> {
        ["0", "123", "-123"].iter().map(|s| s.to_string()).collect()
    }

    fn type_equals(&self, other: &dyn ArgumentType<i64>) -> bool {
        match other.as_any().downcast_ref::<LongArgumentType>() {
            Some(that) => self.minimum == that.minimum && self.maximum == that.maximum,
            None => false,
        }
    }

    fn type_hash_code(&self) -> i32 {
        // Java: `31 * Long.hashCode(minimum) + Long.hashCode(maximum)`.
        31_i32
            .wrapping_mul(crate::java_hash::long_hash(self.minimum))
            .wrapping_add(crate::java_hash::long_hash(self.maximum))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
