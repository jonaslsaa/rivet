//! Port of `ca.spottedleaf.dataconverter.converters.datatypes.DataType`.
//!
//! The `minecraft.datatypes` consumers (`MCDataType`/`MCValueType`) extend this
//! abstract class with their converter/hook/walker dispatch tables. The one
//! concrete behavior here is `convertOrOriginal`.

/// `DataType<T, R>` — the abstract data type.
///
/// `convert` takes `&mut T`: Java's `convert(T data, ...)` passes a reference
/// that the concrete `MCDataType`/`MCValueType` conversions mutate in place and
/// (for `MCDataType`) often return null after mutating. `MCValueType.convert`
/// keeps its own `ret`/`data` binding that the dispatcher reassigns from a
/// non-null result, so `convert_or_original` owns `data` and hands it to
/// `convert` mutably, returning the original only when no replacement was
/// produced.
pub trait DataType<T, R> {
    /// `DataType.convert(T, long fromVersion, long toVersion)` — `None` means
    /// "no replacement" (Java null return).
    fn convert(&self, data: &mut T, from_version: i64, to_version: i64) -> Option<R>;

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
        let mut data = data;
        match self.convert(&mut data, from_version, to_version) {
            Some(replaced) => replaced,
            None => data.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_or_original_matches_paper_golden() {
        // `dataTypeConvertOrOriginal` from the `dataconverter-foundation`
        // golden: null-conversion keeps the original; a replacement is kept.
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../../../../tools/rivet-oracle/fixtures/dataconverter/dataconverter-foundation.json"
        ))
        .expect("dataconverter-foundation.json parses");
        let golden = &fixture["dataTypeConvertOrOriginal"];

        struct NullConverter;
        impl DataType<String, String> for NullConverter {
            fn convert(
                &self,
                _data: &mut String,
                _from_version: i64,
                _to_version: i64,
            ) -> Option<String> {
                None
            }
        }
        assert_eq!(
            NullConverter.convert_or_original("orig".into(), 1, 2),
            golden["nullConverterKeepsOriginal"].as_str().unwrap()
        );

        struct ReplacingConverter;
        impl DataType<String, String> for ReplacingConverter {
            fn convert(
                &self,
                _data: &mut String,
                _from_version: i64,
                _to_version: i64,
            ) -> Option<String> {
                Some("replaced".into())
            }
        }
        assert_eq!(
            ReplacingConverter.convert_or_original("orig".into(), 1, 2),
            golden["replacingConverterReplaces"].as_str().unwrap()
        );
    }

    #[test]
    fn convert_or_original_keeps_original_when_null() {
        struct NullConverter;
        impl DataType<String, String> for NullConverter {
            fn convert(
                &self,
                _data: &mut String,
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
                _data: &mut String,
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
            fn convert(&self, data: &mut i32, _from_version: i64, _to_version: i64) -> Option<i32> {
                Some(data.wrapping_add(1))
            }
        }
        assert_eq!(Fallthrough.convert(&mut 1, 0, 0), Some(2));
    }

    /// The concrete conversion shape: `convert` mutates the data argument in
    /// place and returns null (`None`) — exactly how `ConverterAbstractBlockRename`
    /// does `data.setString("Name", converted); return null;`. The mutation must
    /// be visible to the caller even when no replacement is produced, so the
    /// signature must be `&mut T`, not `&T`.
    #[test]
    fn convert_mutates_data_in_place_and_returns_none() {
        struct Rename;
        impl DataType<String, String> for Rename {
            fn convert(
                &self,
                data: &mut String,
                _from_version: i64,
                _to_version: i64,
            ) -> Option<String> {
                data.push_str("_v2");
                None
            }
        }
        let mut data = "name".to_string();
        assert!(Rename.convert(&mut data, 1, 2).is_none());
        assert_eq!(data, "name_v2");
    }
}
