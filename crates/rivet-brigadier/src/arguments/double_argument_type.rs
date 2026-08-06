//! Port of `com.mojang.brigadier.arguments.DoubleArgumentType` (upstream
//! brigadier-1.3.10).

use std::sync::Arc;

use crate::ImmutableStringReader;
use crate::arguments::ArgumentType;
use crate::context::CommandContext;
use crate::exceptions::BuiltInExceptionProvider;
use crate::exceptions::CommandSyntaxException;
use crate::string_reader::StringReader;

/// Java `DoubleArgumentType`.
pub struct DoubleArgumentType {
    minimum: f64,
    maximum: f64,
}

impl DoubleArgumentType {
    fn new(minimum: f64, maximum: f64) -> Self {
        DoubleArgumentType { minimum, maximum }
    }

    /// Java `DoubleArgumentType.doubleArg()`.
    pub fn double_arg() -> Arc<dyn ArgumentType<f64>> {
        Self::double_arg_with_bounds(-f64::MAX, f64::MAX)
    }

    /// Java `DoubleArgumentType.doubleArg(double min)`.
    pub fn double_arg_min(min: f64) -> Arc<dyn ArgumentType<f64>> {
        Self::double_arg_with_bounds(min, f64::MAX)
    }

    /// Java `DoubleArgumentType.doubleArg(double min, double max)`.
    pub fn double_arg_with_bounds(min: f64, max: f64) -> Arc<dyn ArgumentType<f64>> {
        Arc::new(DoubleArgumentType::new(min, max))
    }

    /// Java `DoubleArgumentType.getDouble(CommandContext, String)`.
    pub fn get_double(context: &CommandContext<impl Clone + std::any::Any>, name: &str) -> f64 {
        context.get_argument::<f64>(name)
    }

    /// Java `getMinimum()`.
    pub fn get_minimum(&self) -> f64 {
        self.minimum
    }

    /// Java `getMaximum()`.
    pub fn get_maximum(&self) -> f64 {
        self.maximum
    }
}

impl ArgumentType<f64> for DoubleArgumentType {
    fn parse(&self, reader: &mut StringReader) -> Result<f64, CommandSyntaxException<'static>> {
        let start = reader.get_cursor();
        let result = reader.read_double()?;
        if result < self.minimum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .double_too_low()
                .create_with_context(
                    reader,
                    &crate::java_float_format::java_double_to_string(result),
                    &crate::java_float_format::java_double_to_string(self.minimum),
                ));
        }
        if result > self.maximum {
            reader.set_cursor(start);
            return Err(CommandSyntaxException::built_in_exceptions()
                .double_too_high()
                .create_with_context(
                    reader,
                    &crate::java_float_format::java_double_to_string(result),
                    &crate::java_float_format::java_double_to_string(self.maximum),
                ));
        }
        Ok(result)
    }

    fn to_string(&self) -> String {
        let fmt = crate::java_float_format::java_double_to_string;
        if self.minimum == -f64::MAX && self.maximum == f64::MAX {
            "double()".to_string()
        } else if self.maximum == f64::MAX {
            format!("double({})", fmt(self.minimum))
        } else {
            format!("double({}, {})", fmt(self.minimum), fmt(self.maximum))
        }
    }

    fn get_examples(&self) -> Vec<String> {
        ["0", "1.2", ".5", "-1", "-.5", "-1234.56"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn type_equals(&self, other: &dyn ArgumentType<f64>) -> bool {
        match other.as_any().downcast_ref::<DoubleArgumentType>() {
            Some(that) => self.minimum == that.minimum && self.maximum == that.maximum,
            None => false,
        }
    }

    fn type_hash_code(&self) -> i32 {
        // Java: `(int) (31 * minimum + maximum)` with double math.
        (31.0_f64 * self.minimum + self.maximum) as i32
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
