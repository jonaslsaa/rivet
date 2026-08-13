//! `net.minecraft.world.level.biome.Biome` — the biome element value (issue
//! #178, `mc.world.level.biome.core` unit).
//!
//! Faithful port of the 26.2 `Biome.java` value surface: the `ClimateSettings`
//! record, the `Precipitation`/`TemperatureModifier` `StringRepresentable`
//! enums, the `DIRECT_CODEC`/`NETWORK_CODEC`/`CODEC`/`LIST_CODEC`, the
//! temperature/freeze/snow behavior, and the `BiomeBuilder`. The `Biome` value
//! carries the `EnvironmentAttributeMap` STUB (see below), the
//! `BiomeSpecialEffects`, `BiomeGenerationSettings`, and `MobSpawnSettings`.
//!
//! ## Fidelity notes
//!
//! - **Codec flattening.** Java's `RecordCodecBuilder.group` accepts bare
//!   `MapCodec`s (the `ClimateSettings`, `BiomeGenerationSettings`, and
//!   `MobSpawnSettings` codecs) whose fields flatten into the `Biome` map,
//!   while `EnvironmentAttributeMap.CODEC_ONLY_POSITIONAL` sits under
//!   `"attributes"` and `BiomeSpecialEffects.CODEC` under `"effects"` (`.fieldOf`).
//!   The `record_builder` compositor merges the field `MapCodec`s against the
//!   same builder/input, reproducing that shape exactly.
//! - **Noise statics.** `TEMPERATURE_NOISE`/`FROZEN_TEMPERATURE_NOISE`/
//!   `BIOME_INFO_NOISE` are `LazyLock<PerlinSimplexNoise>` over the seeded
//!   `WorldgenRandom(LegacyRandomSource)` — the `getValue`/octave behavior is
//!   exact (see `levelgen::synth`). `BIOME_INFO_NOISE` is `@Deprecated` in Java.
//! - **Color tables.** `GrassColor`/`FoliageColor`/`DryFoliageColor` are
//!   `init`-filled texture tables generated at startup (`ColorMapColorUtil.get`
//!   indexes a 65536-entry pixel array). Rivet has no such generator, so the
//!   table reads are STUBs returning the documented default colors
//!   (`-65281`/`-12012264`/`-10732494`).
//! - **`EnvironmentAttributeMap` STUB.** The attribute map is genuinely
//!   entangled with the `mc.world.attribute` unit (`EnvironmentAttribute`,
//!   `AttributeModifier`, the dispatched-map `CODEC_ONLY_POSITIONAL`). Vanilla
//!   biomes never set attributes (no `putAttributes`/`setAttribute` usage in
//!   `Biomes.java`/worldgen), so the port carries a unit `EMPTY`-only STUB:
//!   the `"attributes"` field is `optionalFieldOf("attributes", EMPTY)` — it
//!   omits on encode (the value is always `EMPTY`) and a present `"attributes"`
//!   key decodes through the STUB, which errors honestly (RivetTodo #178) rather
//!   than silently dropping the attribute data. Vanilla biomes never set
//!   attributes, so the honest strict boundary only fires on malformed input.
//! - **`shouldFreeze`/`shouldSnow` defer.** Both read the `LevelReader` block
//!   surface (`getBlockState`/`getFluidState`/`getBrightness`/`isWaterAt`/
//!   `isInsideBuildHeight`), which the #232 `mc.world.level` value slice has
//!   not ported — see the `RivetTodo(#232)` in `level::level_reader`. They land
//!   with that surface.

use crate::biome::biome_generation_settings::BiomeGenerationSettings;
use crate::biome::biome_special_effects::BiomeSpecialEffects;
use crate::biome::mob_spawn_settings::MobSpawnSettings;
use crate::levelgen::synth::perlin_simplex_noise::PerlinSimplexNoise;
use rivet_registry::core::BlockPos;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFileCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::{Identifier, Registry, ResourceKey};
use rivet_serialization::codec::{self, Codec, JavaEquals};
use rivet_serialization::decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::map_decoder;
use rivet_serialization::map_encoder;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth;
use rivet_util::random::LegacyRandomSource;
use rivet_util::string_representable::{self, EnumOrdinal, StringRepresentable};
use rivet_util::worldgen_random::WorldgenRandom;
use std::fmt;
use std::sync::{Arc, LazyLock};

/// `Registries.BIOME` — the typed registry key over the `Biome` element value.
/// `Registries.BIOME` in `rivet-registry` is declared over the `BiomeId`
/// id-handle (the pure-id model); the value type `Biome` is declared here
/// because it is a `rivet-world` type (the `registry_keys.rs` precedent). Both
/// keys share the `minecraft:worldgen/biome` wire identifier — the same Java
/// registry viewed through the id-handle and value lenses.
pub static BIOME: LazyLock<ResourceKey<Registry<Biome>>> = LazyLock::new(|| {
    ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome"))
});

// Java's `TEMPERATURE_CACHE_SIZE` (`1024`) and the frozen
// `ThreadLocal<Long2FloatLinkedOpenHashMap>` temperature cache are not ported:
// the pure `getValue` math is deterministic and the cache is a memoization
// only, so it would be dead state in the value model.

/// `Biome.TEMPERATURE_NOISE` — `PerlinSimplexNoise(WorldgenRandom(
/// LegacyRandomSource(1234L)), ImmutableList.of(0))`.
pub static TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(1234));
    PerlinSimplexNoise::new(&mut random, &[0])
});

/// `Biome.FROZEN_TEMPERATURE_NOISE` — `PerlinSimplexNoise(WorldgenRandom(
/// LegacyRandomSource(3456L)), ImmutableList.of(-2, -1, 0))`.
pub static FROZEN_TEMPERATURE_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(3456));
    PerlinSimplexNoise::new(&mut random, &[-2, -1, 0])
});

/// `Biome.BIOME_INFO_NOISE` (`@Deprecated(forRemoval = true)`) —
/// `PerlinSimplexNoise(WorldgenRandom(LegacyRandomSource(2345L)),
/// ImmutableList.of(0))`. Consumed by `TemperatureModifier.FROZEN` and
/// `BiomeSpecialEffects.GrassColorModifier.SWAMP`.
pub static BIOME_INFO_NOISE: LazyLock<PerlinSimplexNoise> = LazyLock::new(|| {
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(2345));
    PerlinSimplexNoise::new(&mut random, &[0])
});

/// `net.minecraft.world.level.biome.Biome` — the biome element value.
#[derive(Debug, Clone)]
pub struct Biome {
    /// `Biome.climateSettings` — public (Java `public final`).
    pub climate_settings: ClimateSettings,
    /// `Biome.attributes` — the `EnvironmentAttributeMap` STUB (always
    /// `EMPTY`; see the module doc).
    attributes: EnvironmentAttributeMap,
    /// `Biome.specialEffects`.
    special_effects: BiomeSpecialEffects,
    /// `Biome.generationSettings`.
    generation_settings: BiomeGenerationSettings,
    /// `Biome.mobSettings`.
    mob_settings: MobSpawnSettings,
}

impl Biome {
    /// `new Biome(ClimateSettings, EnvironmentAttributeMap, BiomeSpecialEffects,
    /// BiomeGenerationSettings, MobSpawnSettings)` — the private constructor
    /// (the codecs and builder construct through it).
    pub fn new(
        climate_settings: ClimateSettings,
        attributes: EnvironmentAttributeMap,
        special_effects: BiomeSpecialEffects,
        generation_settings: BiomeGenerationSettings,
        mob_settings: MobSpawnSettings,
    ) -> Self {
        Biome {
            climate_settings,
            attributes,
            special_effects,
            generation_settings,
            mob_settings,
        }
    }

    /// `Biome.DIRECT_CODEC` — the ops-generic element codec (five fields, the
    /// three non-flattened ones under `"attributes"`/`"effects"`).
    ///
    /// Requires `RegistryOpsLookup` because the flattened
    /// `BiomeGenerationSettings.CODEC` resolves `ConfiguredWorldCarver` /
    /// `PlacedFeature` holders by name.
    pub fn direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn Codec<Biome, Ops>> {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.climate_settings),
                    ClimateSettings::map_codec_of::<Ops>(),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.attributes),
                    environment_attribute_map_codec::<Ops>(),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.special_effects.clone()),
                    field_of_map_codec(
                        "effects".to_string(),
                        BiomeSpecialEffects::map_codec_of::<Ops>(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.generation_settings.clone()),
                    BiomeGenerationSettings::map_codec_of::<Ops>(),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.mob_settings.clone()),
                    MobSpawnSettings::map_codec_of::<Ops>(),
                ))
                .apply(instance, Arc::new(biome_from_codec_fields))
        })
    }

    /// `Biome.NETWORK_CODEC` — the ops-generic network codec (three fields:
    /// climate settings, attributes, effects; generation/mob settings default to
    /// `EMPTY`).
    pub fn network_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Biome, Ops>> {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.climate_settings),
                    ClimateSettings::map_codec_of::<Ops>(),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.attributes),
                    environment_attribute_map_codec::<Ops>(),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|b: &Biome| b.special_effects.clone()),
                    field_of_map_codec(
                        "effects".to_string(),
                        BiomeSpecialEffects::map_codec_of::<Ops>(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |climate_settings: ClimateSettings,
                         attributes: EnvironmentAttributeMap,
                         special_effects: BiomeSpecialEffects| {
                            Biome::new(
                                climate_settings,
                                attributes,
                                special_effects,
                                BiomeGenerationSettings::EMPTY,
                                MobSpawnSettings::empty(),
                            )
                        },
                    ),
                )
        })
    }

    /// `Biome.CODEC` — `RegistryFileCodec.create(Registries.BIOME,
    /// DIRECT_CODEC)`, the ops-generic holder codec.
    pub fn codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn Codec<Holder<Biome>, Ops>> {
        Arc::new(RegistryFileCodec::create(
            &BIOME,
            Biome::direct_codec::<Ops>(),
        ))
    }

    /// `Biome.LIST_CODEC` — `RegistryCodecs.homogeneousList(Registries.BIOME,
    /// DIRECT_CODEC)` (`alwaysUseList = false`).
    pub fn list_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn Codec<HolderSet<Biome>, Ops>> {
        Arc::new(HolderSetCodec::create(&BIOME, Biome::codec::<Ops>(), false))
    }

    /// `Biome.getMobSettings()`.
    pub fn get_mob_settings(&self) -> &MobSpawnSettings {
        &self.mob_settings
    }

    /// `Biome.getGenerationSettings()`.
    pub fn get_generation_settings(&self) -> &BiomeGenerationSettings {
        &self.generation_settings
    }

    /// `Biome.getAttributes()`.
    pub fn get_attributes(&self) -> &EnvironmentAttributeMap {
        &self.attributes
    }

    /// `Biome.getSpecialEffects()`.
    pub fn get_special_effects(&self) -> &BiomeSpecialEffects {
        &self.special_effects
    }

    /// `Biome.getWaterColor()`.
    pub fn get_water_color(&self) -> i32 {
        self.special_effects.water_color
    }

    /// `Biome.getBaseTemperature()`.
    pub fn get_base_temperature(&self) -> f32 {
        self.climate_settings.temperature
    }

    /// `Biome.hasPrecipitation()`.
    pub fn has_precipitation(&self) -> bool {
        self.climate_settings.has_precipitation
    }

    /// `Biome.getPrecipitationAt(BlockPos, int seaLevel)`.
    pub fn get_precipitation_at(&self, pos: &BlockPos, sea_level: i32) -> Precipitation {
        if !self.has_precipitation() {
            Precipitation::None
        } else if self.cold_enough_to_snow(pos, sea_level) {
            Precipitation::Snow
        } else {
            Precipitation::Rain
        }
    }

    /// `Biome.getHeightAdjustedTemperature(BlockPos, int seaLevel)`.
    fn get_height_adjusted_temperature(&self, pos: &BlockPos, sea_level: i32) -> f32 {
        let adjusted_temperature = self
            .climate_settings
            .temperature_modifier
            .modify_temperature(pos, self.get_base_temperature());
        let snow_level = sea_level + 17;
        if pos.get_y() > snow_level {
            // Java: `(float)(TEMPERATURE_NOISE.getValue(pos.getX() / 8.0F,
            // pos.getZ() / 8.0F, false) * 8.0)` — int / float promotes to
            // float, then widens to the double parameter.
            let v = (TEMPERATURE_NOISE.get_value(
                (pos.get_x() as f32 / 8.0f32) as f64,
                (pos.get_z() as f32 / 8.0f32) as f64,
                false,
            ) * 8.0) as f32;
            // Java: `adjustedTemperature - (v + pos.getY() - snowLevel) *
            // 0.05F / 40.0F` — the whole `v + pos.getY() - snowLevel` sum runs
            // left-to-right in float (each int widened at its operand), so the
            // i32 difference must NOT be formed first.
            adjusted_temperature - (v + pos.get_y() as f32 - snow_level as f32) * 0.05f32 / 40.0f32
        } else {
            adjusted_temperature
        }
    }

    /// `Biome.getTemperature(BlockPos, int seaLevel)` (`@Deprecated` in Java).
    pub fn get_temperature(&self, pos: &BlockPos, sea_level: i32) -> f32 {
        self.get_height_adjusted_temperature(pos, sea_level)
    }

    /// `Biome.coldEnoughToSnow(BlockPos, int seaLevel)`.
    pub fn cold_enough_to_snow(&self, pos: &BlockPos, sea_level: i32) -> bool {
        !self.warm_enough_to_rain(pos, sea_level)
    }

    /// `Biome.warmEnoughToRain(BlockPos, int seaLevel)`.
    pub fn warm_enough_to_rain(&self, pos: &BlockPos, sea_level: i32) -> bool {
        self.get_temperature(pos, sea_level) >= 0.15
    }

    /// `Biome.shouldMeltFrozenOceanIcebergSlightly(BlockPos, int seaLevel)`.
    pub fn should_melt_frozen_ocean_iceberg_slightly(
        &self,
        pos: &BlockPos,
        sea_level: i32,
    ) -> bool {
        self.get_temperature(pos, sea_level) > 0.1
    }

    /// `Biome.getGrassColor(double x, double z)`.
    pub fn get_grass_color(&self, x: f64, z: f64) -> i32 {
        let base_grass_color = self.get_base_grass_color();
        self.special_effects
            .grass_color_modifier
            .modify_color(x, z, base_grass_color)
    }

    /// `Biome.getBaseGrassColor()`.
    fn get_base_grass_color(&self) -> i32 {
        match self.special_effects.grass_color_override {
            Some(color) => color,
            None => self.get_grass_color_from_texture(),
        }
    }

    /// `Biome.getGrassColorFromTexture()`.
    fn get_grass_color_from_texture(&self) -> i32 {
        let temp = mth::clamp_f32(self.climate_settings.temperature, 0.0, 1.0) as f64;
        let rain = mth::clamp_f32(self.climate_settings.downfall, 0.0, 1.0) as f64;
        grass_color::get(temp, rain)
    }

    /// `Biome.getFoliageColor()`.
    pub fn get_foliage_color(&self) -> i32 {
        match self.special_effects.foliage_color_override {
            Some(color) => color,
            None => self.get_foliage_color_from_texture(),
        }
    }

    /// `Biome.getFoliageColorFromTexture()`.
    fn get_foliage_color_from_texture(&self) -> i32 {
        let temp = mth::clamp_f32(self.climate_settings.temperature, 0.0, 1.0) as f64;
        let rain = mth::clamp_f32(self.climate_settings.downfall, 0.0, 1.0) as f64;
        foliage_color::get(temp, rain)
    }

    /// `Biome.getDryFoliageColor()`.
    pub fn get_dry_foliage_color(&self) -> i32 {
        match self.special_effects.dry_foliage_color_override {
            Some(color) => color,
            None => self.get_dry_foliage_color_from_texture(),
        }
    }

    /// `Biome.getDryFoliageColorFromTexture()`.
    fn get_dry_foliage_color_from_texture(&self) -> i32 {
        let temp = mth::clamp_f32(self.climate_settings.temperature, 0.0, 1.0) as f64;
        let rain = mth::clamp_f32(self.climate_settings.downfall, 0.0, 1.0) as f64;
        dry_foliage_color::get(temp, rain)
    }
}

/// The `DIRECT_CODEC`/`NETWORK_CODEC` field constructor.
fn biome_from_codec_fields(
    climate_settings: ClimateSettings,
    attributes: EnvironmentAttributeMap,
    special_effects: BiomeSpecialEffects,
    generation_settings: BiomeGenerationSettings,
    mob_settings: MobSpawnSettings,
) -> Biome {
    Biome::new(
        climate_settings,
        attributes,
        special_effects,
        generation_settings,
        mob_settings,
    )
}

/// Wrap a `MapCodec<A>` under a field name — Java `MapCodec.fieldOf(name)`.
fn field_of_map_codec<A, Ops: DynamicOps + 'static>(
    name: String,
    codec: Arc<dyn MapCodec<A, Ops>>,
) -> Arc<dyn MapCodec<A, Ops>>
where
    A: 'static + Send + Sync,
{
    map_codec::of(
        map_encoder::field_encoder(
            name.clone(),
            map_encoder::encoder(Arc::new(map_codec::MapCodecEncoderHalf(codec.clone()))),
        ),
        map_decoder::field_decoder(
            name.clone(),
            map_decoder::decoder(Arc::new(map_codec::MapCodecDecoderHalf(codec))),
        ),
        format!("Field[{name}: MapCodec]"),
    )
}

/// `Biome.ClimateSettings` — the record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClimateSettings {
    /// `hasPrecipitation`.
    pub has_precipitation: bool,
    /// `temperature`.
    pub temperature: f32,
    /// `temperatureModifier` — defaults to `NONE`.
    pub temperature_modifier: TemperatureModifier,
    /// `downfall`.
    pub downfall: f32,
}

impl ClimateSettings {
    /// `ClimateSettings.CODEC` — the ops-generic `MapCodec` (four fields;
    /// `temperature_modifier` defaults to `NONE`).
    pub fn map_codec_of<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClimateSettings, Ops>> {
        record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|c: &ClimateSettings| c.has_precipitation),
                    codec::field_of(codec::bool_codec::<Ops>(), "has_precipitation".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|c: &ClimateSettings| c.temperature),
                    codec::field_of(codec::float_codec::<Ops>(), "temperature".to_string()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|c: &ClimateSettings| c.temperature_modifier),
                    codec::optional_field_of(
                        "temperature_modifier",
                        TemperatureModifier::codec::<Ops>(),
                        TemperatureModifier::None,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|c: &ClimateSettings| c.downfall),
                    codec::field_of(codec::float_codec::<Ops>(), "downfall".to_string()),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |has_precipitation: bool,
                         temperature: f32,
                         temperature_modifier: TemperatureModifier,
                         downfall: f32| {
                            ClimateSettings {
                                has_precipitation,
                                temperature,
                                temperature_modifier,
                                downfall,
                            }
                        },
                    ),
                )
        })
    }
}

/// `Biome.Precipitation` — the `StringRepresentable` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Precipitation {
    /// `NONE("none")`.
    None,
    /// `RAIN("rain")`.
    Rain,
    /// `SNOW("snow")`.
    Snow,
}

impl Precipitation {
    /// `Precipitation.CODEC` — the ops-generic enum codec.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Precipitation, Ops>> {
        Arc::new(string_representable::from_enum(PRECIPITATION_VALUES))
    }
}

/// `Precipitation.values()` — declaration order.
pub const PRECIPITATION_VALUES: &[Precipitation] = &[
    Precipitation::None,
    Precipitation::Rain,
    Precipitation::Snow,
];

impl StringRepresentable for Precipitation {
    fn get_serialized_name(&self) -> &str {
        match self {
            Precipitation::None => "none",
            Precipitation::Rain => "rain",
            Precipitation::Snow => "snow",
        }
    }
}

impl EnumOrdinal for Precipitation {
    fn ordinal(&self) -> usize {
        match self {
            Precipitation::None => 0,
            Precipitation::Rain => 1,
            Precipitation::Snow => 2,
        }
    }
}

impl fmt::Display for Precipitation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get_serialized_name())
    }
}

/// `Biome.TemperatureModifier` — the `StringRepresentable` enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TemperatureModifier {
    /// `NONE("none")` — identity.
    None,
    /// `FROZEN("frozen")` — the frozen-temperature noise adjustment.
    Frozen,
}

impl JavaEquals for TemperatureModifier {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

impl TemperatureModifier {
    /// `TemperatureModifier.CODEC` — the ops-generic enum codec.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TemperatureModifier, Ops>> {
        Arc::new(string_representable::from_enum(TEMPERATURE_MODIFIER_VALUES))
    }

    /// `TemperatureModifier.modifyTemperature(BlockPos, float baseTemperature)`.
    pub fn modify_temperature(self, pos: &BlockPos, base_temperature: f32) -> f32 {
        match self {
            TemperatureModifier::None => base_temperature,
            TemperatureModifier::Frozen => {
                // Java `pos.getX() * 0.05` — int * double promotes to double.
                let ground_value_large_variation = FROZEN_TEMPERATURE_NOISE.get_value(
                    pos.get_x() as f64 * 0.05,
                    pos.get_z() as f64 * 0.05,
                    false,
                ) * 7.0;
                let ground_value_edge_variation = BIOME_INFO_NOISE.get_value(
                    pos.get_x() as f64 * 0.2,
                    pos.get_z() as f64 * 0.2,
                    false,
                );
                let ice_patches = ground_value_large_variation + ground_value_edge_variation;
                if ice_patches < 0.3 {
                    let ground_value_small_variation = BIOME_INFO_NOISE.get_value(
                        pos.get_x() as f64 * 0.09,
                        pos.get_z() as f64 * 0.09,
                        false,
                    );
                    if ground_value_small_variation < 0.8 {
                        return 0.2;
                    }
                }
                base_temperature
            }
        }
    }
}

/// `TemperatureModifier.values()` — declaration order.
pub const TEMPERATURE_MODIFIER_VALUES: &[TemperatureModifier] =
    &[TemperatureModifier::None, TemperatureModifier::Frozen];

impl StringRepresentable for TemperatureModifier {
    fn get_serialized_name(&self) -> &str {
        match self {
            TemperatureModifier::None => "none",
            TemperatureModifier::Frozen => "frozen",
        }
    }
}

impl EnumOrdinal for TemperatureModifier {
    fn ordinal(&self) -> usize {
        match self {
            TemperatureModifier::None => 0,
            TemperatureModifier::Frozen => 1,
        }
    }
}

impl fmt::Display for TemperatureModifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.get_serialized_name())
    }
}

/// `net.minecraft.world.attribute.EnvironmentAttributeMap` — STUB (see the
/// module doc). Only the empty value is representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnvironmentAttributeMap;

impl JavaEquals for EnvironmentAttributeMap {
    fn java_equals(&self, _other: &Self) -> bool {
        true
    }
}

impl EnvironmentAttributeMap {
    /// `EnvironmentAttributeMap.EMPTY`.
    pub const EMPTY: EnvironmentAttributeMap = EnvironmentAttributeMap;
}

/// The `"attributes"` field codec — `optionalFieldOf("attributes", EMPTY)`.
/// Encode omits the field (the value is always `EMPTY`, equal to the default).
/// A present `"attributes"` key decodes strictly: absent → `EMPTY`, but a
/// present value routes through the STUB element codec, which errors honestly
/// (RivetTodo #178) rather than silently discarding the attribute data. This
/// matches Java's strict `optionalFieldOf` shape; because the STUB always
/// errors, any present attributes key fails the decode with the STUB's message
/// (the real dispatched-map codec defers with the `mc.world.attribute` unit).
fn environment_attribute_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<EnvironmentAttributeMap, Ops>> {
    let stub: Arc<dyn Codec<EnvironmentAttributeMap, Ops>> = codec::of(
        encoder::error(
            "EnvironmentAttributeMap.CODEC_ONLY_POSITIONAL is a STUB (RivetTodo #178)".to_string(),
        ),
        decoder::error(
            "EnvironmentAttributeMap.CODEC_ONLY_POSITIONAL is a STUB (RivetTodo #178)".to_string(),
        ),
        "EnvironmentAttributeMap[STUB]".to_string(),
    );
    codec::optional_field_of("attributes", stub, EnvironmentAttributeMap::EMPTY)
}

/// `Biome.BiomeBuilder`.
#[derive(Debug, Clone)]
pub struct BiomeBuilder {
    /// `BiomeBuilder.hasPrecipitation` — defaults to `true`.
    has_precipitation: bool,
    /// `BiomeBuilder.temperature` — `@Nullable Float`.
    temperature: Option<f32>,
    /// `BiomeBuilder.temperatureModifier` — defaults to `NONE`.
    temperature_modifier: TemperatureModifier,
    /// `BiomeBuilder.downfall` — `@Nullable Float`.
    downfall: Option<f32>,
    /// `BiomeBuilder.attributes` — the `EnvironmentAttributeMap.Builder` STUB
    /// (always `EMPTY`).
    attributes: EnvironmentAttributeMap,
    /// `BiomeBuilder.specialEffects` — `@Nullable`.
    special_effects: Option<BiomeSpecialEffects>,
    /// `BiomeBuilder.mobSpawnSettings` — `@Nullable`.
    mob_spawn_settings: Option<MobSpawnSettings>,
    /// `BiomeBuilder.generationSettings` — `@Nullable`.
    generation_settings: Option<BiomeGenerationSettings>,
}

impl Default for BiomeBuilder {
    fn default() -> Self {
        BiomeBuilder {
            has_precipitation: true,
            temperature: None,
            temperature_modifier: TemperatureModifier::None,
            downfall: None,
            attributes: EnvironmentAttributeMap::EMPTY,
            special_effects: None,
            mob_spawn_settings: None,
            generation_settings: None,
        }
    }
}

impl BiomeBuilder {
    /// `new BiomeBuilder()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `BiomeBuilder.hasPrecipitation(boolean)`.
    pub fn has_precipitation(mut self, has_precipitation: bool) -> Self {
        self.has_precipitation = has_precipitation;
        self
    }

    /// `BiomeBuilder.temperature(float)`.
    pub fn temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// `BiomeBuilder.downfall(float)`.
    pub fn downfall(mut self, downfall: f32) -> Self {
        self.downfall = Some(downfall);
        self
    }

    /// `BiomeBuilder.putAttributes(EnvironmentAttributeMap)` — a no-op on the
    /// STUB (only `EMPTY` is representable).
    pub fn put_attributes(mut self, attributes: EnvironmentAttributeMap) -> Self {
        self.attributes = attributes;
        self
    }

    /// `BiomeBuilder.specialEffects(BiomeSpecialEffects)`.
    pub fn special_effects(mut self, special_effects: BiomeSpecialEffects) -> Self {
        self.special_effects = Some(special_effects);
        self
    }

    /// `BiomeBuilder.mobSpawnSettings(MobSpawnSettings)`.
    pub fn mob_spawn_settings(mut self, mob_spawn_settings: MobSpawnSettings) -> Self {
        self.mob_spawn_settings = Some(mob_spawn_settings);
        self
    }

    /// `BiomeBuilder.generationSettings(BiomeGenerationSettings)`.
    pub fn generation_settings(mut self, generation_settings: BiomeGenerationSettings) -> Self {
        self.generation_settings = Some(generation_settings);
        self
    }

    /// `BiomeBuilder.temperatureAdjustment(TemperatureModifier)`.
    pub fn temperature_adjustment(mut self, temperature_modifier: TemperatureModifier) -> Self {
        self.temperature_modifier = temperature_modifier;
        self
    }

    /// `BiomeBuilder.build()` — throws the exact
    /// `IllegalStateException("You are missing parameters to build a proper
    /// biome\n" + this)` when any of temperature/downfall/specialEffects/
    /// mobSpawnSettings/generationSettings is unset.
    pub fn build(self) -> Biome {
        // Clone for the panic message: the destructure below moves the `Option`
        // fields out of `self`, so the error path prints a pristine copy.
        let for_error = self.clone();
        if let (
            Some(temperature),
            Some(downfall),
            Some(special_effects),
            Some(mob_spawn_settings),
            Some(generation_settings),
        ) = (
            self.temperature,
            self.downfall,
            self.special_effects,
            self.mob_spawn_settings,
            self.generation_settings,
        ) {
            Biome::new(
                ClimateSettings {
                    has_precipitation: self.has_precipitation,
                    temperature,
                    temperature_modifier: self.temperature_modifier,
                    downfall,
                },
                self.attributes,
                special_effects,
                generation_settings,
                mob_spawn_settings,
            )
        } else {
            panic!(
                "You are missing parameters to build a proper biome\n{}",
                for_error
            );
        }
    }
}

impl fmt::Display for BiomeBuilder {
    /// `BiomeBuilder.toString()`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "BiomeBuilder{{\nhasPrecipitation={},\ntemperature={:?},\ntemperatureModifier={},\ndownfall={:?},\nspecialEffects={:?},\nmobSpawnSettings={:?},\ngenerationSettings={:?},\n}}",
            self.has_precipitation,
            self.temperature,
            self.temperature_modifier,
            self.downfall,
            self.special_effects,
            self.mob_spawn_settings,
            self.generation_settings,
        )
    }
}

/// `net.minecraft.world.level.GrassColor` — STUB: the texture pixel table is
/// `init`-filled at startup and Rivet has no generator, so `get` returns the
/// documented default color (`-65281`).
///
/// RivetTodo(#178): the `ColorMapColorUtil.get` table read (and the `init`
/// surface) lands with a grass/foliage color-map source.
mod grass_color {
    pub fn get(_temp: f64, _rain: f64) -> i32 {
        -65281
    }
}

/// `net.minecraft.world.level.FoliageColor` — STUB (default `-12012264`).
mod foliage_color {
    pub fn get(_temp: f64, _rain: f64) -> i32 {
        -12012264
    }
}

/// `net.minecraft.world.level.DryFoliageColor` — STUB (default `-10732494`).
mod dry_foliage_color {
    pub fn get(_temp: f64, _rain: f64) -> i32 {
        -10732494
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_generation_settings::{
        BiomeGenerationSettings, CONFIGURED_CARVER, PLACED_FEATURE,
    };
    use crate::biome::biome_special_effects::BiomeSpecialEffectsBuilder;
    use crate::biome::mob_spawn_settings::MobSpawnSettings;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A minimal full `Biome` for codec/behavior tests.
    fn plains() -> Biome {
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(0x3F76E4)
            .build();
        BiomeBuilder::new()
            .has_precipitation(true)
            .temperature(0.8)
            .downfall(0.4)
            .special_effects(effects)
            .mob_spawn_settings(MobSpawnSettings::empty())
            .generation_settings(BiomeGenerationSettings::EMPTY)
            .build()
    }

    #[test]
    fn builder_defaults_and_round_trip() {
        let biome = plains();
        assert!(biome.has_precipitation());
        assert_eq!(biome.get_base_temperature(), 0.8);
        assert_eq!(biome.climate_settings.downfall, 0.4);
        assert_eq!(
            biome.climate_settings.temperature_modifier,
            TemperatureModifier::None
        );
        assert_eq!(biome.get_water_color(), 0x3F76E4);
        assert_eq!(biome.get_attributes(), &EnvironmentAttributeMap::EMPTY);
    }

    #[test]
    #[should_panic(expected = "You are missing parameters to build a proper biome")]
    fn builder_missing_parameters_panics_with_exact_prefix() {
        let _ = BiomeBuilder::new().temperature(0.5).build();
    }

    #[test]
    fn precipitation_logic() {
        let biome = plains();
        // temp 0.8 >= 0.15 -> warm enough to rain; has precipitation -> RAIN.
        let pos = BlockPos::new(0, 64, 0);
        assert_eq!(biome.get_precipitation_at(&pos, 63), Precipitation::Rain);
        assert!(biome.warm_enough_to_rain(&pos, 63));
        assert!(!biome.cold_enough_to_snow(&pos, 63));
        // temp 0.8 > 0.1 -> the frozen-ocean iceberg would melt slightly.
        assert!(biome.should_melt_frozen_ocean_iceberg_slightly(&pos, 63));

        // A freezing biome (temp 0.0, no modifier at low elevation).
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(0x3F76E4)
            .build();
        let freezing = BiomeBuilder::new()
            .has_precipitation(true)
            .temperature(0.0)
            .downfall(0.5)
            .special_effects(effects)
            .mob_spawn_settings(MobSpawnSettings::empty())
            .generation_settings(BiomeGenerationSettings::EMPTY)
            .build();
        assert_eq!(freezing.get_precipitation_at(&pos, 63), Precipitation::Snow);
        assert!(freezing.cold_enough_to_snow(&pos, 63));
        assert!(!freezing.warm_enough_to_rain(&pos, 63));

        // No precipitation -> always NONE regardless of temperature.
        let arid = BiomeBuilder::new()
            .has_precipitation(false)
            .temperature(-1.0)
            .downfall(0.0)
            .special_effects(BiomeSpecialEffectsBuilder::default().water_color(1).build())
            .mob_spawn_settings(MobSpawnSettings::empty())
            .generation_settings(BiomeGenerationSettings::EMPTY)
            .build();
        assert_eq!(arid.get_precipitation_at(&pos, 63), Precipitation::None);
    }

    #[test]
    fn temperature_adjustment_frozen_uses_noise() {
        // FROZEN at a position whose noise pushes below 0.3 ice-patches and
        // 0.8 small-variation returns 0.2F; the (0,0) sample on the pinned
        // seeded noise is deterministic.
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(0x3F76E4)
            .build();
        let frozen = BiomeBuilder::new()
            .has_precipitation(true)
            .temperature(0.7)
            .temperature_adjustment(TemperatureModifier::Frozen)
            .downfall(0.5)
            .special_effects(effects)
            .mob_spawn_settings(MobSpawnSettings::empty())
            .generation_settings(BiomeGenerationSettings::EMPTY)
            .build();
        let pos = BlockPos::new(0, 64, 0);
        let base = frozen.get_base_temperature();
        let adjusted = frozen.get_temperature(&pos, 63);
        // The adjusted temperature is either 0.2 (FROZEN kicked in) or the
        // noise-adjusted base — both deterministic on the pinned seeds.
        assert!(adjusted == 0.2 || adjusted != base);
    }

    #[test]
    fn grass_color_override_wins_over_texture() {
        let effects = BiomeSpecialEffectsBuilder::default()
            .water_color(1)
            .grass_color_override(0x123456)
            .build();
        let biome = BiomeBuilder::new()
            .has_precipitation(true)
            .temperature(0.5)
            .downfall(0.5)
            .special_effects(effects)
            .mob_spawn_settings(MobSpawnSettings::empty())
            .generation_settings(BiomeGenerationSettings::EMPTY)
            .build();
        assert_eq!(biome.get_grass_color(0.0, 0.0), 0x123456);
    }

    #[test]
    fn grass_color_falls_back_to_texture_stub() {
        let biome = plains();
        // No grass override -> the GrassColor STUB default.
        assert_eq!(biome.get_grass_color(0.0, 0.0), -65281);
        assert_eq!(biome.get_foliage_color(), -12012264);
        assert_eq!(biome.get_dry_foliage_color(), -10732494);
    }

    #[test]
    fn climate_settings_codec_round_trips_with_default_modifier() {
        let codec = map_codec::codec_of(ClimateSettings::map_codec_of::<JsonOps>());
        let value = ClimateSettings {
            has_precipitation: true,
            temperature: 0.8,
            temperature_modifier: TemperatureModifier::None,
            downfall: 0.4,
        };
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &value)
            .result()
            .expect("encode")
            .clone();
        // `temperature_modifier` is omitted when equal to NONE.
        assert_eq!(
            encoded,
            json!({"has_precipitation": true, "temperature": 0.8, "downfall": 0.4})
        );
        let decoded = *codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode");
        assert_eq!(decoded, value);

        // A present "temperature_modifier" decodes.
        let decoded = *codec
            .parse(
                &JsonOps::INSTANCE,
                &json!({"has_precipitation": false, "temperature": -0.5, "temperature_modifier": "frozen", "downfall": 0.2}),
            )
            .result()
            .expect("decode");
        assert_eq!(decoded.temperature_modifier, TemperatureModifier::Frozen);
    }

    #[test]
    fn direct_codec_flattens_and_nests() {
        let access = direct_codec_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = Biome::direct_codec::<TestOps>();
        let biome = plains();
        let encoded = codec
            .encode_start(&ops, &biome)
            .result()
            .expect("encode")
            .clone();
        // Climate settings, generation settings, and mob settings flatten;
        // effects nests under "effects"; attributes is omitted (EMPTY default).
        let obj = encoded.as_object().expect("object");
        assert!(obj.contains_key("has_precipitation"));
        assert!(obj.contains_key("temperature"));
        assert!(obj.contains_key("downfall"));
        assert!(obj.contains_key("effects"));
        assert!(obj.contains_key("carvers"));
        assert!(obj.contains_key("features"));
        assert!(obj.contains_key("spawners"));
        assert!(obj.contains_key("spawn_costs"));
        assert!(!obj.contains_key("attributes"));
        assert!(!obj.contains_key("climate_settings"));
        assert!(!obj.contains_key("generation_settings"));
        assert!(!obj.contains_key("mob_settings"));

        // Round-trip: decoded biome matches field-by-field. The special
        // effects' water color comes back through `ARGB.opaque` (the hex form
        // forces alpha), so compare the alpha-adjusted value.
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.climate_settings, biome.climate_settings);
        assert_eq!(
            decoded.special_effects.water_color,
            biome.special_effects.water_color | 0xFF000000u32 as i32
        );
        assert_eq!(
            decoded.special_effects.grass_color_modifier,
            biome.special_effects.grass_color_modifier
        );
        assert_eq!(decoded.get_generation_settings().get_carvers().size(), 0);
        assert!(decoded.get_generation_settings().features().is_empty());
        assert_eq!(decoded.get_mob_settings(), biome.get_mob_settings());
    }

    #[test]
    fn direct_codec_attributes_present_errors_honestly() {
        let access = direct_codec_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = Biome::direct_codec::<TestOps>();
        // A present "attributes" key decodes through the STUB element codec,
        // which errors honestly (RivetTodo #178) — the field is Java's strict
        // `optionalFieldOf("attributes", EMPTY)`, so a present value is routed
        // to the STUB rather than silently swallowed to EMPTY. Vanilla biomes
        // never set attributes, so only malformed input reaches this boundary.
        let mut json = codec
            .encode_start(&ops, &plains())
            .result()
            .expect("encode")
            .clone();
        json.as_object_mut().expect("object").insert(
            "attributes".to_string(),
            json!({"minecraft:generic.max_health": 20}),
        );
        let parsed = codec.parse(&ops, &json);
        let result = parsed.result();
        assert!(
            result.is_none(),
            "present attributes must error (STUB #178), not fall back to EMPTY"
        );
    }

    #[test]
    fn network_codec_uses_three_fields_and_empty_settings() {
        let codec = Biome::network_codec::<JsonOps>();
        let biome = plains();
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &biome)
            .result()
            .expect("encode")
            .clone();
        // Network codec omits carvers/features/spawners/spawn_costs.
        let obj = encoded.as_object().expect("object");
        assert!(!obj.contains_key("carvers"));
        assert!(!obj.contains_key("features"));
        assert!(!obj.contains_key("spawners"));
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &encoded)
            .result()
            .expect("decode")
            .clone();
        assert_eq!(decoded.get_generation_settings().get_carvers().size(), 0);
        assert!(decoded.get_generation_settings().features().is_empty());
        assert_eq!(*decoded.get_mob_settings(), MobSpawnSettings::empty());
    }

    /// A `RegistryAccess` carrying empty CONFIGURED_CARVER and PLACED_FEATURE
    /// registries — enough for `direct_codec` (the flattened generation-settings
    /// codec resolves those registries; EMPTY settings round-trip).
    fn direct_codec_access() -> RegistryAccess {
        let carver_registry = RegistryBuilder::new(&*CONFIGURED_CARVER).freeze();
        let feature_registry = RegistryBuilder::new(&*PLACED_FEATURE).freeze();
        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/configured_carver",
                )),
                Box::new(carver_registry) as rivet_registry::root::AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/placed_feature",
                )),
                Box::new(feature_registry) as rivet_registry::root::AnyBox,
            ),
        ])
    }

    fn biome_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*BIOME);
        builder.register(
            &ResourceKey::create(&*BIOME, Identifier::parse("minecraft:plains")),
            Arc::new(plains()),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome")),
            Box::new(registry) as rivet_registry::root::AnyBox,
        )])
    }

    #[test]
    fn codec_encodes_reference_by_identifier() {
        let access = biome_access();
        let owner = access
            .lookup(&*BIOME)
            .expect("frozen registry")
            .registry_id();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = Biome::codec::<TestOps>();
        let holder: Holder<Biome> = Holder::reference(owner, 0);
        let encoded = codec
            .encode_start(&ops, &holder)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!("minecraft:plains"));
    }

    #[test]
    fn list_codec_round_trips() {
        let access = biome_access();
        let owner = access
            .lookup(&*BIOME)
            .expect("frozen registry")
            .registry_id();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = Biome::list_codec::<TestOps>();
        let set = HolderSet::direct(vec![Holder::reference(owner, 0)]);
        let encoded = codec
            .encode_start(&ops, &set)
            .result()
            .expect("encode")
            .clone();
        // A single element compacts to the bare identifier.
        assert_eq!(encoded, json!("minecraft:plains"));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode")
            .clone();
        // The holder set round-trips as the same reference ids.
        let refs: Vec<_> = decoded
            .iter()
            .map(|h| match h {
                Holder::Reference { registry, id } => (*registry, *id),
                Holder::Direct(_) => panic!("expected a reference holder"),
            })
            .collect();
        assert_eq!(refs, vec![(owner, 0)]);
    }
}
