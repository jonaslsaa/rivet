//! Port of `com.mojang.brigadier.exceptions.CommandExceptionType` (upstream).
//!
//! Java marker interface. In Rust it is a marker trait; identity of exception
//! types (the test `ex.getType() is type`) is reproduced because each `Dynamic*` /
//! `SimpleCommandExceptionType` is a singleton behind the `BuiltInExceptions`
//! `LazyLock`. Compare instances with `std::ptr::eq` on the `&dyn
//! CommandExceptionType` refs, since the types are not `PartialEq`.

/// Java `CommandExceptionType` marker interface.
pub trait CommandExceptionType: std::fmt::Debug + Send + Sync {}
