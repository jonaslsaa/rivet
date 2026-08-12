//! Port of `net.minecraft.world.level.levelgen.NoiseSettings` (record, 26.2).
//!
//! The `(minY, height, noiseSizeHorizontal, noiseSizeVertical)` record, its
//! `CODEC` (the four ranged fields `comapFlatMap`'d through `guardY`), the five
//! dimension constants, and the cell-height/width + `clampToHeightAccessor`
//! helpers.

use crate::level::LevelHeightAccessor;
use rivet_registry::core::QuartPos;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `DimensionType.MIN_Y`/`MAX_Y`/`Y_SIZE` — the height-clamp constants the
/// `NoiseSettings` fields range over. Inlined from
/// `rivet-world::level::dimension::dimension_type` to keep this value record
/// self-contained (they are `pub` there; the noise unit's `guardY` needs
/// `MAX_Y + 1`).
use crate::level::dimension::dimension_type::{MAX_Y, MIN_Y, Y_SIZE};

/// `net.minecraft.world.level.levelgen.NoiseSettings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoiseSettings {
    min_y: i32,
    height: i32,
    noise_size_horizontal: i32,
    noise_size_vertical: i32,
}

impl NoiseSettings {
    /// `NoiseSettings(int minY, int height, int noiseSizeHorizontal, int
    /// noiseSizeVertical)` — the record constructor. Java's canonical
    /// constructor is unchecked (the `CODEC`'s `comapFlatMap` applies
    /// `guardY`); callers use `create` for the validating path.
    pub fn new(
        min_y: i32,
        height: i32,
        noise_size_horizontal: i32,
        noise_size_vertical: i32,
    ) -> Self {
        NoiseSettings {
            min_y,
            height,
            noise_size_horizontal,
            noise_size_vertical,
        }
    }

    /// `minY()`.
    pub fn min_y(&self) -> i32 {
        self.min_y
    }

    /// `height()`.
    pub fn height(&self) -> i32 {
        self.height
    }

    /// `noiseSizeHorizontal()`.
    pub fn noise_size_horizontal(&self) -> i32 {
        self.noise_size_horizontal
    }

    /// `noiseSizeVertical()`.
    pub fn noise_size_vertical(&self) -> i32 {
        self.noise_size_vertical
    }

    /// `getCellHeight()` — `QuartPos.toBlock(this.noiseSizeVertical())`.
    pub fn get_cell_height(&self) -> i32 {
        QuartPos::to_block(self.noise_size_vertical)
    }

    /// `getCellWidth()` — `QuartPos.toBlock(this.noiseSizeHorizontal())`.
    pub fn get_cell_width(&self) -> i32 {
        QuartPos::to_block(self.noise_size_horizontal)
    }

    /// `clampToHeightAccessor(LevelHeightAccessor)` — clamps the `minY`/height
    /// window to the accessor's build height:
    ///
    /// ```java
    /// int newMinY = Math.max(this.minY, heightAccessor.getMinY());
    /// int newHeight = Math.min(this.minY + this.height, heightAccessor.getMaxY() + 1) - newMinY;
    /// ```
    ///
    /// `Math.max`/`Math.min` over `int` — no wrapping concern (the values are
    /// height bounds).
    pub fn clamp_to_height_accessor(
        &self,
        height_accessor: &dyn LevelHeightAccessor,
    ) -> NoiseSettings {
        let new_min_y = self.min_y.max(height_accessor.get_min_y());
        let new_height =
            (self.min_y + self.height).min(height_accessor.get_max_y() + 1) - new_min_y;
        NoiseSettings {
            min_y: new_min_y,
            height: new_height,
            noise_size_horizontal: self.noise_size_horizontal,
            noise_size_vertical: self.noise_size_vertical,
        }
    }
}

/// `NoiseSettings.create(int minY, int height, int noiseSizeHorizontal, int
/// noiseSizeVertical)` — the validating constructor: `new` + `guardY`, throwing
/// `IllegalStateException` on error (Java's `error().ifPresent(error -> { throw
/// new IllegalStateException(error.message()); })`).
pub fn create(
    min_y: i32,
    height: i32,
    noise_size_horizontal: i32,
    noise_size_vertical: i32,
) -> NoiseSettings {
    let settings = NoiseSettings::new(min_y, height, noise_size_horizontal, noise_size_vertical);
    if let Some(err) = guard_y(&settings).error_ref() {
        panic!("{}", err.message());
    }
    settings
}

/// `NoiseSettings.OVERWORLD_NOISE_SETTINGS` — `create(-64, 384, 1, 2)`.
pub static OVERWORLD_NOISE_SETTINGS: NoiseSettings = NoiseSettings {
    min_y: -64,
    height: 384,
    noise_size_horizontal: 1,
    noise_size_vertical: 2,
};
/// `NoiseSettings.NETHER_NOISE_SETTINGS` — `create(0, 128, 1, 2)`.
pub static NETHER_NOISE_SETTINGS: NoiseSettings = NoiseSettings {
    min_y: 0,
    height: 128,
    noise_size_horizontal: 1,
    noise_size_vertical: 2,
};
/// `NoiseSettings.END_NOISE_SETTINGS` — `create(0, 128, 2, 1)`.
pub static END_NOISE_SETTINGS: NoiseSettings = NoiseSettings {
    min_y: 0,
    height: 128,
    noise_size_horizontal: 2,
    noise_size_vertical: 1,
};
/// `NoiseSettings.CAVES_NOISE_SETTINGS` — `create(-64, 192, 1, 2)`.
pub static CAVES_NOISE_SETTINGS: NoiseSettings = NoiseSettings {
    min_y: -64,
    height: 192,
    noise_size_horizontal: 1,
    noise_size_vertical: 2,
};
/// `NoiseSettings.FLOATING_ISLANDS_NOISE_SETTINGS` — `create(0, 256, 2, 1)`.
pub static FLOATING_ISLANDS_NOISE_SETTINGS: NoiseSettings = NoiseSettings {
    min_y: 0,
    height: 256,
    noise_size_horizontal: 2,
    noise_size_vertical: 1,
};

/// `NoiseSettings.guardY(NoiseSettings)` — the three `DataResult` guards:
/// `minY + height > MAX_Y + 1`, `height % 16 != 0`, `minY % 16 != 0`.
fn guard_y(settings: &NoiseSettings) -> DataResult<NoiseSettings> {
    if settings.min_y + settings.height > MAX_Y + 1 {
        DataResult::error(format!(
            "min_y + height cannot be higher than: {}",
            MAX_Y + 1
        ))
    } else if settings.height % 16 != 0 {
        DataResult::error("height has to be a multiple of 16".to_string())
    } else if settings.min_y % 16 != 0 {
        DataResult::error("min_y has to be a multiple of 16".to_string())
    } else {
        DataResult::success(*settings)
    }
}

/// `NoiseSettings.CODEC` — the four ranged fields `comapFlatMap(guardY,
/// identity)`, as the ops-generic `noise_settings_codec::<Ops>()` factory.
pub fn noise_settings_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<NoiseSettings, Ops>> {
    let base = record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of_named(
                Arc::new(|s: &NoiseSettings| s.min_y),
                "min_y".to_string(),
                codec::int_range::<Ops>(MIN_Y, MAX_Y),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|s: &NoiseSettings| s.height),
                "height".to_string(),
                codec::int_range::<Ops>(0, Y_SIZE),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|s: &NoiseSettings| s.noise_size_horizontal),
                "size_horizontal".to_string(),
                codec::int_range::<Ops>(1, 4),
            ))
            .and(RecordCodecBuilder::of_named(
                Arc::new(|s: &NoiseSettings| s.noise_size_vertical),
                "size_vertical".to_string(),
                codec::int_range::<Ops>(1, 4),
            ))
            .apply(instance, Arc::new(NoiseSettings::new))
    });
    codec::comap_flat_map(
        base,
        Arc::new(|s: &NoiseSettings| guard_y(s)),
        Arc::new(|s: &NoiseSettings| *s),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimension_constants_match_java() {
        assert_eq!(
            (
                OVERWORLD_NOISE_SETTINGS.min_y,
                OVERWORLD_NOISE_SETTINGS.height
            ),
            (-64, 384)
        );
        assert_eq!(
            (NETHER_NOISE_SETTINGS.min_y, NETHER_NOISE_SETTINGS.height),
            (0, 128)
        );
        assert_eq!(
            (END_NOISE_SETTINGS.min_y, END_NOISE_SETTINGS.height),
            (0, 128)
        );
        assert_eq!(
            (CAVES_NOISE_SETTINGS.min_y, CAVES_NOISE_SETTINGS.height),
            (-64, 192)
        );
        assert_eq!(
            (
                FLOATING_ISLANDS_NOISE_SETTINGS.min_y,
                FLOATING_ISLANDS_NOISE_SETTINGS.height
            ),
            (0, 256)
        );
        assert_eq!(OVERWORLD_NOISE_SETTINGS.noise_size_horizontal, 1);
        assert_eq!(OVERWORLD_NOISE_SETTINGS.noise_size_vertical, 2);
        assert_eq!(END_NOISE_SETTINGS.noise_size_horizontal, 2);
        assert_eq!(END_NOISE_SETTINGS.noise_size_vertical, 1);
    }

    #[test]
    fn cell_helpers_use_quart() {
        // QuartPos.toBlock(n) = n << 2.
        assert_eq!(OVERWORLD_NOISE_SETTINGS.get_cell_width(), 4);
        assert_eq!(OVERWORLD_NOISE_SETTINGS.get_cell_height(), 8);
        assert_eq!(END_NOISE_SETTINGS.get_cell_width(), 8);
        assert_eq!(END_NOISE_SETTINGS.get_cell_height(), 4);
    }

    #[test]
    fn create_validates_like_java() {
        // Valid windows pass.
        let _ = create(-64, 384, 1, 2);
        let _ = create(0, 128, 1, 2);
        // Invalid: minY not a multiple of 16.
        assert!(std::panic::catch_unwind(|| create(1, 128, 1, 2)).is_err());
        // Invalid: height not a multiple of 16.
        assert!(std::panic::catch_unwind(|| create(0, 100, 1, 2)).is_err());
        // Invalid: minY + height beyond MAX_Y + 1.
        assert!(std::panic::catch_unwind(|| create(2000, 400, 1, 2)).is_err());
    }

    #[test]
    fn codec_round_trips_valid_settings() {
        use rivet_serialization::json_ops::JsonOps;
        let codec = noise_settings_codec::<JsonOps>();
        let settings = create(-64, 384, 1, 2);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &settings)
            .result()
            .expect("encode should succeed")
            .clone();
        let (decoded, _rest) = codec
            .decode(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, settings);
    }

    #[test]
    fn codec_rejects_invalid_settings_on_decode() {
        use rivet_serialization::json_ops::JsonOps;
        let codec = noise_settings_codec::<JsonOps>();
        // minY not a multiple of 16.
        let json = serde_json::json!({
            "min_y": 1,
            "height": 128,
            "size_horizontal": 1,
            "size_vertical": 2
        });
        let result = codec.decode(&JsonOps::INSTANCE, &json);
        assert!(result.error_ref().is_some());
        assert!(
            result
                .error_ref()
                .unwrap()
                .message()
                .contains("min_y has to be a multiple of 16")
        );
    }

    #[test]
    fn clamp_to_height_accessor_clamps_window() {
        // A simple 0..256 accessor (LevelHeightAccessor.create(0, 256)).
        let accessor = crate::level::create(0, 256);
        let clamped = OVERWORLD_NOISE_SETTINGS.clamp_to_height_accessor(&accessor);
        assert_eq!(clamped.min_y, 0);
        assert_eq!(clamped.height, 256);
        // Cell sizes carry through.
        assert_eq!(clamped.noise_size_horizontal, 1);
        assert_eq!(clamped.noise_size_vertical, 2);
    }
}
