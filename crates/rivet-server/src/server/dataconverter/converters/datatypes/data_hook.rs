//! Port of `ca.spottedleaf.dataconverter.converters.datatypes.DataHook`.

/// `DataHook<T, R>` — a pre/post hook invoked by the datatype dispatch layer
/// around a converter (the `minecraft.datatypes` consumers drive them). The
/// hook's `T`-typed data argument is the current value being converted; `R` is
/// the (nullable) replacement — `None` means "no replacement", so the dispatch
/// keeps the prior value.
pub trait DataHook<T, R> {
    /// `DataHook.preHook(T, long fromVersion, long toVersion)`.
    fn pre_hook(&self, data: &T, from_version: i64, to_version: i64) -> Option<R>;

    /// `DataHook.postHook(T, long fromVersion, long toVersion)`.
    fn post_hook(&self, data: &T, from_version: i64, to_version: i64) -> Option<R>;
}
