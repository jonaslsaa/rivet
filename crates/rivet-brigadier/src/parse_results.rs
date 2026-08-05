//! Port of `com.mojang.brigadier.ParseResults` (upstream).
//!
//! // STUB(brigadier): full port is the root `com.mojang.brigadier` unit; this is a
//! placeholder so the module path exists.

/// Java `ParseResults<S>`.
pub struct ParseResults<S> {
    _marker: std::marker::PhantomData<S>,
}
