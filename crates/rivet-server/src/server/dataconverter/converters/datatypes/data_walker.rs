//! Port of `ca.spottedleaf.dataconverter.converters.datatypes.DataWalker`.

use std::marker::PhantomData;

/// `DataWalker<T>` — a structure walker that may replace the value it walks
/// (the `minecraft.walkers.*` units implement it). `None` means "no
/// replacement" (Java null return).
pub trait DataWalker<T> {
    /// `DataWalker.walk(T, long fromVersion, long toVersion)`.
    fn walk(&self, data: &T, from_version: i64, to_version: i64) -> Option<T>;
}

/// `DataWalker.NO_OP` — the singleton walker that never replaces.
pub struct NoOpWalker<T> {
    _marker: PhantomData<T>,
}

impl<T> NoOpWalker<T> {
    /// `DataWalker.noOp()` — the `NO_OP` singleton cast to `DataWalker<T>`.
    pub fn no_op() -> NoOpWalker<T> {
        NoOpWalker {
            _marker: PhantomData,
        }
    }
}

impl<T> DataWalker<T> for NoOpWalker<T> {
    fn walk(&self, _data: &T, _from_version: i64, _to_version: i64) -> Option<T> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_op_walk_returns_none() {
        let walker: NoOpWalker<&'static str> = NoOpWalker::no_op();
        assert!(walker.walk(&"data", 1, 2).is_none());
    }

    #[test]
    fn walker_replacement_contract() {
        struct Replacer;
        impl DataWalker<&'static str> for Replacer {
            fn walk(
                &self,
                _data: &&'static str,
                _from_version: i64,
                _to_version: i64,
            ) -> Option<&'static str> {
                Some("replaced")
            }
        }
        assert_eq!(Replacer.walk(&"orig", 1, 2), Some("replaced"));
    }
}
