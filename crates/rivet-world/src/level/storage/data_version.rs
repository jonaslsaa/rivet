//! Port of `net.minecraft.world.level.storage.DataVersion` — the
//! `(version, series)` record.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! storage/DataVersion.java`. A two-field record (`int version, String series`)
//! with the `MAIN_SERIES` constant and the `isSideSeries`/`isCompatible`
//! accessors. The record is `PartialEq`-equivalent via derived `Eq`/`Hash`
//! (both components are `Eq`).

/// `net.minecraft.world.level.storage.DataVersion` — the `(version, series)`
/// record.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DataVersion {
    /// The data version (`version`).
    pub version: i32,
    /// The release series (`series`).
    pub series: String,
}

impl DataVersion {
    /// `DataVersion.MAIN_SERIES` — `"main"`.
    pub const MAIN_SERIES: &'static str = "main";

    /// `new DataVersion(int version, String series)` — the record constructor.
    pub fn new(version: i32, series: String) -> Self {
        DataVersion { version, series }
    }

    /// `DataVersion.isSideSeries()` — `!series.equals("main")`.
    pub fn is_side_series(&self) -> bool {
        self.series != Self::MAIN_SERIES
    }

    /// `DataVersion.isCompatible(DataVersion other)` — the series match.
    ///
    /// Java: `SharedConstants.DEBUG_OPEN_INCOMPATIBLE_WORLDS ||
    /// series.equals(other.series)`. `DEBUG_OPEN_INCOMPATIBLE_WORLDS` is a
    /// debug flag (default false) not ported, so this is the plain series
    /// equality — the same behavior in a release build.
    pub fn is_compatible(&self, other: &DataVersion) -> bool {
        self.series == other.series
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn main_series_defaults() {
        let dv = DataVersion::new(4903, DataVersion::MAIN_SERIES.to_string());
        assert_eq!(dv.version, 4903);
        assert_eq!(dv.series, "main");
        assert!(!dv.is_side_series());
    }

    #[test]
    fn side_series_and_compatibility() {
        let main = DataVersion::new(4903, "main".to_string());
        let side = DataVersion::new(4903, "side".to_string());
        assert!(side.is_side_series());
        assert!(main.is_compatible(&main));
        assert!(side.is_compatible(&side));
        assert!(!main.is_compatible(&side));
    }
}
