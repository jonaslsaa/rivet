//! Port of `com.mojang.brigadier.arguments.FloatArgumentType` (upstream
//! brigadier-1.3.10).

use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::arguments::ArgumentType;
use crate::context::CommandContext;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;

/// Java `FloatArgumentType`.
pub struct FloatArgumentType {
    minimum: f32,
    maximum: f32,
}

impl FloatArgumentType {
    fn new(minimum: f32, maximum: f32) -> Self {
        FloatArgumentType { minimum, maximum }
    }

    /// Java `FloatArgumentType.floatArg()`.
    pub fn float_arg() -> Arc<dyn ArgumentType<f32>> {
        Self::float_arg_with_bounds(-f32::MAX, f32::MAX)
    }

    /// Java `FloatArgumentType.floatArg(float min)`.
    pub fn float_arg_min(min: f32) -> Arc<dyn ArgumentType<f32>> {
        Self::float_arg_with_bounds(min, f32::MAX)
    }

    /// Java `FloatArgumentType.floatArg(float min, float max)`.
    pub fn float_arg_with_bounds(min: f32, max: f32) -> Arc<dyn ArgumentType<f32>> {
        Arc::new(FloatArgumentType::new(min, max))
    }

    /// Java `FloatArgumentType.getFloat(CommandContext, String)`.
    pub fn get_float(context: &CommandContext<impl Clone + std::any::Any>, name: &str) -> f32 {
        context.get_argument::<f32>(name)
    }

    /// Java `getMinimum()`.
    pub fn get_minimum(&self) -> f32 {
        self.minimum
    }

    /// Java `getMaximum()`.
    pub fn get_maximum(&self) -> f32 {
        self.maximum
    }
}

impl ArgumentType<f32> for FloatArgumentType {
    fn parse(&self, reader: &mut StringReader) -> Result<f32, CommandSyntaxException<'static>> {
        let start = reader.get_cursor();
        let result = reader.read_float()?;
        if result < self.minimum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .float_too_low()
                .create_with_context(
                    reader,
                    &crate::java_float_format::java_float_to_string(result),
                    &crate::java_float_format::java_float_to_string(self.minimum),
                ));
        }
        if result > self.maximum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .float_too_high()
                .create_with_context(
                    reader,
                    &crate::java_float_format::java_float_to_string(result),
                    &crate::java_float_format::java_float_to_string(self.maximum),
                ));
        }
        Ok(result)
    }

    fn to_string(&self) -> String {
        let fmt = crate::java_float_format::java_float_to_string;
        if self.minimum == -f32::MAX && self.maximum == f32::MAX {
            "float()".to_string()
        } else if self.maximum == f32::MAX {
            format!("float({})", fmt(self.minimum))
        } else {
            format!("float({}, {})", fmt(self.minimum), fmt(self.maximum))
        }
    }

    fn get_examples(&self) -> Vec<String> {
        ["0", "1.2", ".5", "-1", "-.5", "-1234.56"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn type_equals(&self, other: &dyn ArgumentType<f32>) -> bool {
        match other.as_any().downcast_ref::<FloatArgumentType>() {
            Some(that) => self.minimum == that.minimum && self.maximum == that.maximum,
            None => false,
        }
    }

    fn type_hash_code(&self) -> i32 {
        // Java: `(int) (31 * minimum + maximum)` with float math.
        (31.0_f32 * self.minimum + self.maximum) as i32
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
