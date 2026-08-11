//! Port of `ca.spottedleaf.dataconverter.converters.datatypes.DataType`.
//!
//! The `minecraft.datatypes` consumers (`MCDataType`/`MCValueType`) extend this
//! abstract class with their converter/hook/walker dispatch tables. The one
//! concrete behavior here is `convertOrOriginal`.

/// `DataType<T, R>` — the abstract data type.
///
/// `convert` borrows `data` (Java passes the reference; the concrete backings
/// mutate through interior storage), so `convert_or_original` can return the
/// owned `data` when conversion produced no replacement.
pub trait DataType<T, R> {
    /// `DataType.convert(T, long fromVersion, long toVersion)` — `None` means
    /// "no replacement" (Java null return).
    fn convert(&self, data: &T, from_version: i64, to_version: i64) -> Option<R>;

    /// `DataType.convertOrOriginal(T, long, long)` — the conversion result, or
    /// the original `data` when conversion produced no replacement.
    ///
    /// Java's `(R)data` fallback is an unchecked cast that is only sound because
    /// every concrete subtype uses `T = R` (e.g. `DataType<Object, Object>`,
    /// `DataType<MapType, MapType>`). The `T: Into<R>` bound is the identity for
    /// `T = R` and makes that soundness explicit in Rust.
    fn convert_or_original(&self, data: T, from_version: i64, to_version: i64) -> R
    where
        T: Into<R>,
    {
        match self.convert(&data, from_version, to_version) {
            Some(replaced) => replaced,
            None => data.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_or_original_keeps_original_when_null() {
        struct NullConverter;
        impl DataType<String, String> for NullConverter {
            fn convert(
                &self,
                _data: &String,
                _from_version: i64,
                _to_version: i64,
            ) -> Option<String> {
                None
            }
        }
        assert_eq!(
            NullConverter.convert_or_original("orig".into(), 1, 2),
            "orig"
        );
    }

    #[test]
    fn convert_or_original_replaces_when_present() {
        struct ReplacingConverter;
        impl DataType<String, String> for ReplacingConverter {
            fn convert(
                &self,
                _data: &String,
                _from_version: i64,
                _to_version: i64,
            ) -> Option<String> {
                Some("replaced".into())
            }
        }
        assert_eq!(
            ReplacingConverter.convert_or_original("orig".into(), 1, 2),
            "replaced"
        );
    }

    #[test]
    fn convert_direct_contract() {
        struct Fallthrough;
        impl DataType<i32, i32> for Fallthrough {
            fn convert(&self, data: &i32, _from_version: i64, _to_version: i64) -> Option<i32> {
                Some(data.wrapping_add(1))
            }
        }
        assert_eq!(Fallthrough.convert(&1, 0, 0), Some(2));
    }
}
