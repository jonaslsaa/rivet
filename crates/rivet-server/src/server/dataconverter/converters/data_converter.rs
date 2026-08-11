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
pub trait ConverterBehavior<T, R> {
    /// `convert(T, long sourceVersion, long toVersion)` — `None` means "no
    /// replacement" (Java null return), which the dispatch layer skips.
    fn convert(&self, data: &T, source_version: i64, to_version: i64) -> Option<R>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_versions_round_trip_matches_probe() {
        // Directly from the probe `encodeVersions` golden rows.
        for (version, step, expected_encoded) in [
            (0i32, 0i32, 0i64),
            (1, 0, 4_294_967_296),
            (1, 1, 4_294_967_297),
            (2, 0, 8_589_934_592),
            (-1, 0, -4_294_967_296),
            (-1, -1, -1),
            (99, 0, 425_201_762_304),
            (1344, 0, 5_772_436_045_824),
            (1344, 1, 5_772_436_045_825),
            (1344, 2_147_483_647, 5_774_583_529_471),
            (1344, -2_147_483_648, 5_774_583_529_472),
            (268_435_456, 1234, 1_152_921_504_606_848_210),
            (-2_147_483_648, 0, i64::MIN),
            (-2_147_483_648, -2_147_483_648, -9_223_372_034_707_292_160),
        ] {
            assert_eq!(
                encode_versions(version, step),
                expected_encoded,
                "encodeVersions({version}, {step})"
            );
        }
    }

    #[test]
    fn get_version_step_round_trip() {
        for (version, step) in [
            (0i32, 0i32),
            (1, 1),
            (1344, 2_147_483_647),
            (1344, -2_147_483_648),
            (-1, -1),
            (268_435_456, 1234),
        ] {
            let encoded = encode_versions(version, step);
            assert_eq!(get_version(encoded), version);
            assert_eq!(get_step(encoded), step);
        }
    }

    #[test]
    fn step_monotonic() {
        // encodeVersions(version, step) < encodeVersions(version, step + 1).
        let a = encode_versions(5, 100);
        let b = encode_versions(5, 101);
        assert!(a < b);
        // The probe's exact monotonic row.
        assert_eq!(encode_versions(5, 100), 21_474_836_580);
        assert_eq!(encode_versions(5, 101), 21_474_836_581);
    }

    #[test]
    fn encoded_to_string_matches_probe() {
        assert_eq!(encoded_to_string(encode_versions(0, 0)), "0.0");
        assert_eq!(encoded_to_string(encode_versions(1, 0)), "1.0");
        assert_eq!(encoded_to_string(encode_versions(1, 1)), "1.1");
        assert_eq!(encoded_to_string(encode_versions(-1, -1)), "-1.-1");
        assert_eq!(
            encoded_to_string(encode_versions(1344, 2_147_483_647)),
            "1344.2147483647"
        );
    }

    #[test]
    fn lowest_version_comparator_orders_by_encoded() {
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
        // Expected ascending encoded order: 3.0 < 3.2147483647 < 5.0 < 5.1 <
        // 5.2 < 5.-1 < 100.0 — note 5.-1 has the largest low bits of the 5.x
        // group because `step & 0xFFFFFFFFL` is unsigned.
        let expected_encoded = [
            encode_versions(3, 0),
            encode_versions(3, 2_147_483_647),
            encode_versions(5, 0),
            encode_versions(5, 1),
            encode_versions(5, 2),
            encode_versions(5, -1),
            encode_versions(100, 0),
        ];
        let actual: Vec<i64> = convs.iter().map(|c| c.get_encoded_version()).collect();
        assert_eq!(actual, expected_encoded);
    }
}
