//! `net.minecraft.world.level.biome.BiomeSpecialEffects` — the biome's
//! water/foliage/grass color record (issue #178, `mc.world.level.biome.core`
//! unit).
//!
//! Java is a record `(int waterColor, Optional<Integer> foliageColorOverride,
//! Optional<Integer> dryFoliageColorOverride, Optional<Integer>
//! grassColorOverride, GrassColorModifier grassColorModifier)` with a
//! `RecordCodecBuilder` `CODEC` (five fields, water_color required, the three
//! color overrides optional via `STRING_RGB_COLOR`, grass_color_modifier
//! defaulted to `NONE`), a `Builder`, and the `GrassColorModifier`
//! `StringRepresentable` enum.
//!
//! The `CODEC` is the ops-generic `codec::<Ops>()` factory (the
//! `record_builder` compositor caps at 5 fields, so `map_codec` is used
//! directly). `GrassColorModifier.SWAMP.modifyColor` reads
//! `Biome.BIOME_INFO_NOISE` — the shared `PerlinSimplexNoise` (seeded 2345L,
//! octaves `[0]`) defined on the `Biome` value type (see [`crate::biome::biome`]);
//! `DARK_FOREST` applies `ARGB.opaque((baseColor & 16711422) + 2634762 >> 1)`
//! (Java's `>>` binds looser than `+`, so the shift applies to the sum).

use crate::biome::biome::BIOME_INFO_NOISE;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::extra_codecs;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use rivet_util::string_representable::{self, EnumOrdinal, StringRepresentable};
use std::fmt;
use std::sync::Arc;

/// `net.minecraft.world.level.biome.BiomeSpecialEffects` — the record fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeSpecialEffects {
    /// `waterColor`.
    pub water_color: i32,
    /// `foliageColorOverride` — `Optional<Integer>`.
    pub foliage_color_override: Option<i32>,
    /// `dryFoliageColorOverride` — `Optional<Integer>`.
    pub dry_foliage_color_override: Option<i32>,
    /// `grassColorOverride` — `Optional<Integer>`.
    pub grass_color_override: Option<i32>,
    /// `grassColorModifier`.
    pub grass_color_modifier: GrassColorModifier,
}

impl BiomeSpecialEffects {
    /// `BiomeSpecialEffects.CODEC` — the ops-generic factory.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BiomeSpecialEffects, Ops>> {
        map_codec::codec_of(Self::map_codec_of::<Ops>())
    }

    /// The `MapCodec` half — `RecordCodecBuilder.mapCodec(...)`.
    pub fn map_codec_of<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<BiomeSpecialEffects, Ops>>
    {
        let string_rgb = extra_codecs::string_rgb_color::<Ops>();
        record_builder::map_codec(|instance| {
            instance
                .group(record_builder::RecordCodecBuilder::of(
                    Arc::new(|e: &BiomeSpecialEffects| e.water_color),
                    codec::field_of(string_rgb.clone(), "water_color".to_string()),
                ))
                .and(record_builder::RecordCodecBuilder::of(
                    Arc::new(|e: &BiomeSpecialEffects| e.foliage_color_override),
                    codec::optional_field("foliage_color".to_string(), string_rgb.clone(), false),
                ))
                .and(record_builder::RecordCodecBuilder::of(
                    Arc::new(|e: &BiomeSpecialEffects| e.dry_foliage_color_override),
                    codec::optional_field(
                        "dry_foliage_color".to_string(),
                        string_rgb.clone(),
                        false,
                    ),
                ))
                .and(record_builder::RecordCodecBuilder::of(
                    Arc::new(|e: &BiomeSpecialEffects| e.grass_color_override),
                    codec::optional_field("grass_color".to_string(), string_rgb.clone(), false),
                ))
                .and(record_builder::RecordCodecBuilder::of(
                    Arc::new(|e: &BiomeSpecialEffects| e.grass_color_modifier),
                    codec::lenient_optional_field_of(
                        "grass_color_modifier",
                        GrassColorModifier::codec::<Ops>(),
                        GrassColorModifier::None,
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |water_color: i32,
                         foliage_color_override: Option<i32>,
                         dry_foliage_color_override: Option<i32>,
                         grass_color_override: Option<i32>,
                         grass_color_modifier: GrassColorModifier| {
                            BiomeSpecialEffects {
                                water_color,
                                foliage_color_override,
                                dry_foliage_color_override,
                                grass_color_override,
                                grass_color_modifier,
                            }
                        },
                    ),
                )
        })
    }
}

/// `BiomeSpecialEffects.Builder`.
#[derive(Debug, Clone)]
pub struct BiomeSpecialEffectsBuilder {
    /// `Builder.waterColor` — `OptionalInt`; `None` until `waterColor` is set.
    water_color: Option<i32>,
    /// `Builder.foliageColorOverride`.
    foliage_color_override: Option<i32>,
    /// `Builder.dryFoliageColorOverride`.
    dry_foliage_color_override: Option<i32>,
    /// `Builder.grassColorOverride`.
    grass_color_override: Option<i32>,
    /// `Builder.grassColorModifier` — defaults to `NONE`.
    grass_color_modifier: GrassColorModifier,
}

impl Default for BiomeSpecialEffectsBuilder {
    fn default() -> Self {
        BiomeSpecialEffectsBuilder {
            water_color: None,
            foliage_color_override: None,
            dry_foliage_color_override: None,
            grass_color_override: None,
            grass_color_modifier: GrassColorModifier::None,
        }
    }
}

impl BiomeSpecialEffectsBuilder {
    /// `Builder.waterColor(int)`.
    pub fn water_color(mut self, water_color: i32) -> Self {
        self.water_color = Some(water_color);
        self
    }

    /// `Builder.foliageColorOverride(int)`.
    pub fn foliage_color_override(mut self, foliage_color: i32) -> Self {
        self.foliage_color_override = Some(foliage_color);
        self
    }

    /// `Builder.dryFoliageColorOverride(int)`.
    pub fn dry_foliage_color_override(mut self, dry_foliage_color: i32) -> Self {
        self.dry_foliage_color_override = Some(dry_foliage_color);
        self
    }

    /// `Builder.grassColorOverride(int)`.
    pub fn grass_color_override(mut self, grass_color: i32) -> Self {
        self.grass_color_override = Some(grass_color);
        self
    }

    /// `Builder.grassColorModifier(GrassColorModifier)`.
    pub fn grass_color_modifier(mut self, grass_modifier: GrassColorModifier) -> Self {
        self.grass_color_modifier = grass_modifier;
        self
    }

    /// `Builder.build()` — throws the exact
    /// `IllegalStateException("Missing 'water' color.")` when `waterColor` was
    /// never set.
    pub fn build(self) -> BiomeSpecialEffects {
        BiomeSpecialEffects {
            water_color: self
                .water_color
                .unwrap_or_else(|| panic!("Missing 'water' color.")),
            foliage_color_override: self.foliage_color_override,
            dry_foliage_color_override: self.dry_foliage_color_override,
            grass_color_override: self.grass_color_override,
            grass_color_modifier: self.grass_color_modifier,
        }
    }
}

/// `BiomeSpecialEffects.GrassColorModifier` — the `StringRepresentable` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GrassColorModifier {
    /// `NONE("none")` — identity.
    None,
    /// `DARK_FOREST("dark_forest")` — `ARGB.opaque((baseColor & 16711422) +
    /// 2634762 >> 1)`.
    DarkForest,
    /// `SWAMP("swamp")` — reads `Biome.BIOME_INFO_NOISE` at `(x*0.0225,
    /// z*0.0225)`; `< -0.1` yields `-11766212`, else `-9801671`.
    Swamp,
}

impl GrassColorModifier {
    /// `GrassColorModifier.CODEC` — the ops-generic `StringRepresentable` enum
    /// codec.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<GrassColorModifier, Ops>> {
        Arc::new(string_representable::from_enum(GRASS_COLOR_MODIFIER_VALUES))
    }

    /// `GrassColorModifier.modifyColor(double x, double z, int baseColor)`.
    pub fn modify_color(self, x: f64, z: f64, base_color: i32) -> i32 {
        match self {
            GrassColorModifier::None => base_color,
            GrassColorModifier::DarkForest => {
                // Java `ARGB.opaque((baseColor & 16711422) + 2634762 >> 1)` —
                // `>>` binds looser than `+`, so the shift applies to the sum.
                argb_opaque((base_color & 16711422).wrapping_add(2634762) >> 1)
            }
            GrassColorModifier::Swamp => {
                let ground_value = BIOME_INFO_NOISE.get_value(x * 0.0225, z * 0.0225, false);
                if ground_value < -0.1 {
                    -11766212
                } else {
                    -9801671
                }
            }
        }
    }
}

/// `GrassColorModifier.values()` — declaration order.
pub const GRASS_COLOR_MODIFIER_VALUES: &[GrassColorModifier] = &[
    GrassColorModifier::None,
    GrassColorModifier::DarkForest,
    GrassColorModifier::Swamp,
];

impl StringRepresentable for GrassColorModifier {
    fn get_serialized_name(&self) -> &str {
        match self {
            GrassColorModifier::None => "none",
            GrassColorModifier::DarkForest => "dark_forest",
            GrassColorModifier::Swamp => "swamp",
        }
    }
}

impl EnumOrdinal for GrassColorModifier {
    fn ordinal(&self) -> usize {
        match self {
            GrassColorModifier::None => 0,
            GrassColorModifier::DarkForest => 1,
            GrassColorModifier::Swamp => 2,
        }
    }
}

impl fmt::Display for GrassColorModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get_serialized_name())
    }
}

/// `ARGB.opaque(int color)` — `color | 0xFF000000`.
fn argb_opaque(color: i32) -> i32 {
    color | 0xFF000000u32 as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn builder_defaults_and_water_required() {
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(0x3F76E4)
            .build();
        assert_eq!(effects.water_color, 0x3F76E4);
        assert_eq!(effects.foliage_color_override, None);
        assert_eq!(effects.grass_color_modifier, GrassColorModifier::None);
    }

    #[test]
    #[should_panic(expected = "Missing 'water' color.")]
    fn builder_missing_water_panics_with_exact_message() {
        let _ = BiomeSpecialEffectsBuilder::default().build();
    }

    #[test]
    fn builder_sets_all_fields() {
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(1)
            .foliage_color_override(2)
            .dry_foliage_color_override(3)
            .grass_color_override(4)
            .grass_color_modifier(GrassColorModifier::DarkForest)
            .build();
        assert_eq!(effects.water_color, 1);
        assert_eq!(effects.foliage_color_override, Some(2));
        assert_eq!(effects.dry_foliage_color_override, Some(3));
        assert_eq!(effects.grass_color_override, Some(4));
        assert_eq!(effects.grass_color_modifier, GrassColorModifier::DarkForest);
    }

    #[test]
    fn grass_color_modifier_none_is_identity() {
        assert_eq!(
            GrassColorModifier::None.modify_color(0.5, 0.5, 0x123456),
            0x123456
        );
    }

    #[test]
    fn dark_forest_modifies_color() {
        // Java `ARGB.opaque((baseColor & 16711422) + 2634762 >> 1)`. The
        // golden `0xFF506763` was produced by the Java reference — pin it to
        // guard the operator-precedence (the shift binds looser than `+`).
        assert_eq!(
            GrassColorModifier::DarkForest.modify_color(0.0, 0.0, 0x789ABC),
            0xFF506763u32 as i32
        );
    }

    #[test]
    fn swamp_uses_biome_info_noise() {
        // The swamp threshold: BIOME_INFO_NOISE(x,z) at (0,0) is the noise
        // sample for the seeded 2345L PerlinSimplexNoise. The expected color
        // is derived from the SAME noise read the modifier performs, so the
        // test pins the branch against the seeded noise deterministically.
        let color = GrassColorModifier::Swamp.modify_color(0.0, 0.0, 0x123456);
        let expected = if BIOME_INFO_NOISE.get_value(0.0, 0.0, false) < -0.1 {
            -11766212
        } else {
            -9801671
        };
        assert_eq!(color, expected);
    }

    #[test]
    fn codec_round_trips() {
        let codec = BiomeSpecialEffects::codec::<JsonOps>();
        // Use an opaque water color: STRING_RGB_COLOR's hex form decodes via
        // `ARGB.opaque` (| 0xFF000000), so only an already-opaque value
        // round-trips identically.
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(0xFF3F76E4u32 as i32)
            .grass_color_modifier(GrassColorModifier::DarkForest)
            .build();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &effects)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(
            encoded,
            json!({"water_color": "#3f76e4", "grass_color_modifier": "dark_forest"})
        );
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded, effects);
    }

    #[test]
    fn codec_decodes_hex_and_int_water_color() {
        let codec = BiomeSpecialEffects::codec::<JsonOps>();
        // Hex form — `hexColor(6).xmap(ARGB::opaque, ARGB::transparent)`, so
        // the decode forces the alpha byte on.
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!({"water_color": "#3F76E4"}))
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.water_color, 0xFF3F76E4u32 as i32);
        // Int form — the `RGB_COLOR_CODEC` alternative decodes the int as-is
        // (no alpha forcing).
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!({"water_color": 4155620}))
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.water_color, 4155620);
    }

    #[test]
    fn codec_missing_water_color_errors() {
        let codec = BiomeSpecialEffects::codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.contains("No key water_color"), "got: {msg}");
    }

    #[test]
    fn enum_codec_round_trips_and_maps_names() {
        let codec = GrassColorModifier::codec::<JsonOps>();
        for (name, variant) in [
            ("none", GrassColorModifier::None),
            ("dark_forest", GrassColorModifier::DarkForest),
            ("swamp", GrassColorModifier::Swamp),
        ] {
            let decoded = codec
                .parse(&JsonOps::INSTANCE, &json!(name))
                .result()
                .cloned()
                .expect("decode");
            assert_eq!(decoded, variant);
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &variant)
                .result()
                .expect("encode")
                .clone();
            assert_eq!(encoded, json!(name));
        }
    }
}
