//! Port of `com.mojang.brigadier.exceptions` package (upstream + Paper patched
//! `CommandSyntaxException`).
//!
//! Java `CommandSyntaxException` extends `Exception` and carries `(type, message,
//! input, cursor)`. In Rust this is a value type holding the same fields. The Java
//! `getMessage()` post-processes `message.getString()` by appending the `" at
//! position N: ..."` context; that is reproduced by `Display`.

pub mod built_in_exception_provider;
pub mod built_in_exceptions;
pub mod command_exception_type;
pub mod command_syntax_exception;
pub mod dynamic2_command_exception_type;
pub mod dynamic3_command_exception_type;
pub mod dynamic4_command_exception_type;
pub mod dynamic_command_exception_type;
pub mod dynamic_n_command_exception_type;
pub mod simple_command_exception_type;
pub mod tag_parse_command_syntax_exception;

pub use built_in_exception_provider::BuiltInExceptionProvider;
pub use built_in_exceptions::BuiltInExceptions;
pub use command_exception_type::CommandExceptionType;
pub use command_syntax_exception::CommandSyntaxException;
pub use dynamic_command_exception_type::DynamicCommandExceptionType;
pub use dynamic_n_command_exception_type::DynamicNCommandExceptionType;
pub use dynamic2_command_exception_type::Dynamic2CommandExceptionType;
pub use dynamic3_command_exception_type::Dynamic3CommandExceptionType;
pub use dynamic4_command_exception_type::Dynamic4CommandExceptionType;
pub use simple_command_exception_type::SimpleCommandExceptionType;
pub use tag_parse_command_syntax_exception::is_tag_parse_exception;
pub use tag_parse_command_syntax_exception::tag_parse_exception;

/// Java's `type == other` identity comparison over `CommandExceptionType`
/// references (`getType() is type`). Rust `std::ptr::eq` compares the full fat
/// pointer, and the vtable pointer is not guaranteed to be deduplicated across
/// coercion sites; comparing only the data pointer reproduces the Java object
/// identity.
pub fn exception_type_eq(a: &dyn CommandExceptionType, b: &dyn CommandExceptionType) -> bool {
    (a as *const dyn CommandExceptionType as *const ())
        == (b as *const dyn CommandExceptionType as *const ())
}

#[cfg(test)]
mod dynamic_command_syntax_exception_type_tests;
#[cfg(test)]
mod simple_command_syntax_exception_type_tests;
#[cfg(test)]
mod tag_parse_command_syntax_exception_type_tests;
