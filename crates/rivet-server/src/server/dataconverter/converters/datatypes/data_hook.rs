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

#[cfg(test)]
mod tests {
    use super::*;

    /// `hookWalker` from the `dataconverter-foundation` golden: a pre-hook that
    /// passes its data through, a post-hook that returns null, and the no-op
    /// walker's null.
    #[test]
    fn hook_contract_matches_paper_golden() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../../../tools/rivet-oracle/fixtures/dataconverter/dataconverter-foundation.json"
        ))
        .expect("dataconverter-foundation.json parses");
        let golden = &fixture["hookWalker"];

        struct Passthrough;
        impl DataHook<&'static str, &'static str> for Passthrough {
            fn pre_hook(&self, data: &&'static str, _from: i64, _to: i64) -> Option<&'static str> {
                Some(data)
            }
            fn post_hook(
                &self,
                _data: &&'static str,
                _from: i64,
                _to: i64,
            ) -> Option<&'static str> {
                None
            }
        }
        assert_eq!(
            Passthrough.pre_hook(&"d", 1, 2),
            Some(golden["preHookPassthrough"].as_str().unwrap())
        );
        assert_eq!(
            Passthrough.post_hook(&"d", 1, 2).is_none(),
            golden["postHookNull"].as_bool().unwrap()
        );
    }
}
