//! Port of `ca.spottedleaf.dataconverter.converters.DataConverter`.
//!
//! Java's abstract class carries the version fields and the static version
//! encoding/decoding; the abstract `convert` is modeled as the
//! [`ConverterBehavior`] trait per PORTING.md's abstract-class rule. Concrete
//! converters in later units embed a [`DataConverter`] base and implement the
//! behavior trait.

/// `DataConverter.LOWEST_VERSION_COMPARATOR` — ascending by encoded version.
pub fn lowest_version_cmp(a: &DataConverter, b: &DataConverter) -> std::cmp::Ordering {
    a.get_encoded_version().cmp(&b.get_encoded_version())
}

/// `DataConverter.encodeVersions(int, int)` — packs the version into the top 32
/// bits and the (unsigned) step into the low 32 bits:
/// `((long)version << 32) | (step & 0xFFFFFFFFL)`.
///
/// The step is masked with `0xFFFFFFFFL`, so a *negative* step is encoded as its
/// unsigned 32-bit value (`step & 0xFFFFFFFFL`); `getStep` then recovers the
/// signed value by casting the low 32 bits back to `int`. This is exactly why
/// `encodeVersions(v, step) < encodeVersions(v, step + 1)` holds — the step
/// lives entirely in the low bits (probe `stepMonotonic`).
pub fn encode_versions(version: i32, step: i32) -> i64 {
    ((version as i64) << 32) | ((step as u32) as i64)
}

/// `DataConverter.getVersion(long)` — `(int)(encoded >>> 32)`.
pub fn get_version(encoded: i64) -> i32 {
    ((encoded as u64) >> 32) as i32
}

/// `DataConverter.getStep(long)` — `(int)encoded` (low 32 bits, signed).
pub fn get_step(encoded: i64) -> i32 {
    encoded as i32
}

/// `DataConverter.encodedToString(long)` — `getVersion + "." + getStep`.
pub fn encoded_to_string(encoded: i64) -> String {
    format!("{}.{}", get_version(encoded), get_step(encoded))
}

/// The `DataConverter<T, R>` base — the version fields and getters.
///
/// `T`/`R` are the converter's data/replacement types; they do not affect the
/// version arithmetic, so the base is not generic (the behavior trait carries
/// the type parameters, exactly as Java's erased `DataConverter<?, ?>` does for
/// the comparator).
#[derive(Debug, Clone, Copy)]
pub struct DataConverter {
    to_version: i32,
    version_step: i32,
}

impl DataConverter {
    /// `new DataConverter(int toVersion)` — step defaults to 0.
    pub fn new(to_version: i32) -> Self {
        DataConverter {
            to_version,
            version_step: 0,
        }
    }

    /// `new DataConverter(int toVersion, int versionStep)`.
    pub fn with_step(to_version: i32, version_step: i32) -> Self {
        DataConverter {
            to_version,
            version_step,
        }
    }

    /// `DataConverter.getToVersion()`.
    pub fn get_to_version(&self) -> i32 {
        self.to_version
    }

    /// `DataConverter.getVersionStep()`.
    pub fn get_version_step(&self) -> i32 {
        self.version_step
    }

    /// `DataConverter.getEncodedVersion()`.
    pub fn get_encoded_version(&self) -> i64 {
        encode_versions(self.to_version, self.version_step)
    }
}

/// `DataConverter.convert` — the abstract behavior trait.
///
/// `data` is `&mut T`: Java's `convert(T data, ...)` receives a reference that
/// the concrete converters mutate in place and then typically return null (e.g.
/// `ConverterAbstractBlockRename` does `data.setString("Name", converted)` and
/// returns null). The dispatcher rebinds the running value from the returned
/// `Some` (`ret = data = replace`).
pub trait ConverterBehavior<T, R> {
    /// `convert(T, long sourceVersion, long toVersion)` — `None` means "no
    /// replacement" (Java null return), which the dispatch layer skips.
    fn convert(&self, data: &mut T, source_version: i64, to_version: i64) -> Option<R>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The committed `dataconverter-foundation` oracle golden — the same file
    /// `rivet-oracle verify` hash-validates — so a re-pin that changes the
    /// version-encoding arithmetic fails this test instead of going stale.
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../../../tools/rivet-oracle/fixtures/dataconverter/dataconverter-foundation.json"
        ))
        .expect("dataconverter-foundation.json parses")
    }

    #[test]
    fn encode_versions_matches_paper_golden() {
        // `encodeVersions` rows: version/step/encoded + the round-trip decode.
        for row in fixture()["encodeVersions"].as_array().unwrap() {
            let version = row["version"].as_i64().unwrap() as i32;
            let step = row["step"].as_i64().unwrap() as i32;
            let expected_encoded = row["encoded"].as_str().unwrap().parse::<i64>().unwrap();
            let encoded = encode_versions(version, step);
            assert_eq!(
                encoded, expected_encoded,
                "encodeVersions({version}, {step})"
            );
            assert_eq!(get_version(encoded), version);
            assert_eq!(get_step(encoded), step);
            assert_eq!(
                encoded_to_string(encoded),
                row["encodedToString"].as_str().unwrap()
            );
        }

        let monotonic = &fixture()["stepMonotonic"];
        let a = encode_versions(5, 100);
        let b = encode_versions(5, 101);
        assert_eq!(a.to_string(), monotonic["a"].as_str().unwrap());
        assert_eq!(b.to_string(), monotonic["b"].as_str().unwrap());
        assert_eq!(a < b, monotonic["aLessThanB"].as_bool().unwrap());
    }

    #[test]
    fn lowest_version_comparator_matches_paper_golden() {
        // The probe's `convs` list, in its original insertion order.
        let convs = [
            DataConverter::with_step(5, 0),
            DataConverter::with_step(5, 2),
            DataConverter::with_step(5, 1),
            DataConverter::with_step(3, 0),
            DataConverter::with_step(100, 0),
            DataConverter::with_step(5, -1),
            DataConverter::with_step(3, 2_147_483_647),
        ];
        let mut convs = convs.to_vec();
        convs.sort_by(lowest_version_cmp);

        // The golden records the probe's sorted `comparatorOrder` rows; compare
        // the sorted encoded versions against the recorded `encoded` values.
        let golden: Vec<i64> = fixture()["comparatorOrder"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["encoded"].as_str().unwrap().parse::<i64>().unwrap())
            .collect();
        let actual: Vec<i64> = convs.iter().map(|c| c.get_encoded_version()).collect();
        assert_eq!(actual, golden);
    }
}
