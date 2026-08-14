//! Port of `net.minecraft.world.level.levelgen.feature.configurations.GeodeConfiguration`
//! (record, 26.2) — the `mc.world.level.levelgen.feature.configurations.geode`
//! manifest unit.
//!
//! The thirteen-field geode record: the three nested geode settings records
//! (`blocks`/`layers`/`crack`, required), the nine geode-shape scalars
//! (`use_potential_placements_chance` / `use_alternate_layer0_chance` /
//! `noise_multiplier` as `CHANCE_RANGE` = `Codec.doubleRange(0.0, 1.0)`,
//! `placements_require_layer0_alternate` as `Codec.BOOL`,
//! `outer_wall_distance` / `distribution_points` /
//! `point_offset` as `IntProviders.codec(...)`, `min_gen_offset` /
//! `max_gen_offset` as `Codec.INT` — all non-lenient optional-with-default), and
//! the required `invalid_blocks_threshold` (`Codec.INT`).
//!
//! The thirteen-field group exceeds the port's `record_builder` `Group6` cap, so
//! the record codec is hand-composed with `map_encoder`/`map_decoder` as a
//! flattened, associatively-equivalent decomposition of `Applicative.super.ap13`
//! (`ap5(ap4(ap4(map(Function13::curry4, func), t1..t4), t5..t8), t9..t13)`).
//! DFU 10.0.21 defines `ap13` itself as
//! `ap7(ap6(map(Function13::curry6, func), t1..t6), t7..t13)` — the port's
//! decomposition encodes the fields in the same order and combines the same
//! `ap`-chained `DataResult`s. This is the same flattened pattern the
//! `GeodeBlockSettings` (ap8) and `LargeDripstoneConfiguration` (ap10) ports
//! use. The three `IntProvider` optional-with-default fields cannot use
//! [`codec::optional_field_of`] (which requires `JavaEquals`, unimplemented for
//! `IntProvider`), so they are composed here with the `Option`-typed
//! [`codec::optional_field`] + [`map_codec::xmap`] pair and a `PartialEq`
//! omission test — see [`optional_int_provider_field`].
//!
//! This unit ports the value layer only. The placement behavior
//! (`GeodeFeature`) writes blocks through
//! `WorldGenLevel.setBlock`/`getBlockState`, whose seams are not reachable on
//! the `WorldGenLevel` surface yet (RivetTodo #228/#399) — those defer.

use crate::levelgen::settings::geode_block_settings::{
    GeodeBlockSettings, geode_block_settings_codec,
};
use crate::levelgen::settings::geode_crack_settings::{
    GeodeCrackSettings, geode_crack_settings_codec,
};
use crate::levelgen::settings::geode_layer_settings::{
    GeodeLayerSettings, geode_layer_settings_codec,
};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::functions::{Fn4, Fn5};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::{MapCodec, MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use rivet_util::valueproviders::uniform_int::UniformInt;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.GeodeConfiguration`.
///
/// `GeodeBlockSettings` holds the erased `Arc<dyn ErasedBlockStateProvider>`
/// carriers, so the record is `Clone`+`Debug` only — the same shape the other
/// provider-carrying configuration value types take.
#[derive(Debug, Clone)]
pub struct GeodeConfiguration {
    /// `geodeBlockSettings`.
    pub geode_block_settings: GeodeBlockSettings,
    /// `geodeLayerSettings`.
    pub geode_layer_settings: GeodeLayerSettings,
    /// `geodeCrackSettings`.
    pub geode_crack_settings: GeodeCrackSettings,
    /// `usePotentialPlacementsChance` — `[0.0, 1.0]`, default `0.35`.
    pub use_potential_placements_chance: f64,
    /// `useAlternateLayer0Chance` — `[0.0, 1.0]`, default `0.0`.
    pub use_alternate_layer0_chance: f64,
    /// `placementsRequireLayer0Alternate` — default `true`.
    pub placements_require_layer0_alternate: bool,
    /// `outerWallDistance` — `IntProviders.codec(1, 20)`, default
    /// `UniformInt(4, 5)`.
    pub outer_wall_distance: IntProvider,
    /// `distributionPoints` — `IntProviders.codec(1, 20)`, default
    /// `UniformInt(3, 4)`.
    pub distribution_points: IntProvider,
    /// `pointOffset` — `IntProviders.codec(0, 10)`, default `UniformInt(1, 2)`.
    pub point_offset: IntProvider,
    /// `minGenOffset` — default `-16`.
    pub min_gen_offset: i32,
    /// `maxGenOffset` — default `16`.
    pub max_gen_offset: i32,
    /// `noiseMultiplier` — `[0.0, 1.0]`, default `0.05`.
    pub noise_multiplier: f64,
    /// `invalidBlocksThreshold` — required.
    pub invalid_blocks_threshold: i32,
}

impl GeodeConfiguration {
    /// The record constructor (the codec's `apply` function).
    #[allow(clippy::too_many_arguments)] // Java's 13-field record constructor.
    pub fn new(
        geode_block_settings: GeodeBlockSettings,
        geode_layer_settings: GeodeLayerSettings,
        geode_crack_settings: GeodeCrackSettings,
        use_potential_placements_chance: f64,
        use_alternate_layer0_chance: f64,
        placements_require_layer0_alternate: bool,
        outer_wall_distance: IntProvider,
        distribution_points: IntProvider,
        point_offset: IntProvider,
        min_gen_offset: i32,
        max_gen_offset: i32,
        noise_multiplier: f64,
        invalid_blocks_threshold: i32,
    ) -> Self {
        GeodeConfiguration {
            geode_block_settings,
            geode_layer_settings,
            geode_crack_settings,
            use_potential_placements_chance,
            use_alternate_layer0_chance,
            placements_require_layer0_alternate,
            outer_wall_distance,
            distribution_points,
            point_offset,
            min_gen_offset,
            max_gen_offset,
            noise_multiplier,
            invalid_blocks_threshold,
        }
    }

    /// `GeodeConfiguration.CHANCE_RANGE` — `Codec.doubleRange(0.0, 1.0)`, the
    /// public `Codec<Double>` constant the geode record's chance fields
    /// (`use_potential_placements_chance`, `use_alternate_layer0_chance`,
    /// `noise_multiplier`) and `GeodeCrackSettings.generate_crack_chance`
    /// share. Owned by this unit (see the `geode_crack_settings` unit note).
    pub fn chance_range_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<f64, Ops>> {
        codec::double_range::<Ops>(0.0, 1.0)
    }
}

/// `Codec.optionalFieldOf(name, codec, default)` for the three `IntProvider`
/// fields.
///
/// The shared [`codec::optional_field_of`] requires `JavaEquals`, which
/// `IntProvider` does not implement (it is a `PartialEq` enum). The decode side
/// is identical (`Optional.orElse(default)`); the encode-side omission test here
/// uses Rust `PartialEq ==` instead of `JavaEquals`. For `IntProvider` that is
/// value equality, and Java's `Objects.equals` against the record defaults
/// (`UniformInt` records) is also value equality — so for these fields the two
/// are equivalent. This mirrors [`codec::lenient_optional_field_of`]'s use of
/// `PartialEq` for the same omission test, but stays NON-lenient (a
/// present-but-malformed value is still a decode error, like
/// `optionalField(name, codec, false)`).
fn optional_int_provider_field<Ops: DynamicOps + 'static>(
    name: &str,
    element_codec: Arc<dyn Codec<IntProvider, Ops>>,
    default: IntProvider,
) -> Arc<dyn MapCodec<IntProvider, Ops>> {
    let inner = codec::optional_field(name.to_string(), element_codec, false);
    let default_for_decode = default.clone();
    let default_for_encode = default;
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<IntProvider>| {
            o.clone().unwrap_or_else(|| default_for_decode.clone())
        }),
        Arc::new(move |a: &IntProvider| {
            if a == &default_for_encode {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

/// The nested `Fn4` for `(t5..t8)` in the flattened `ap13` decomposition:
/// `use_alternate_layer0_chance` (f64), `placements_require_layer0_alternate`
/// (bool), `outer_wall_distance` (IntProvider), `distribution_points`
/// (IntProvider) — returning the final-group `Fn5`.
type Group2Fn = Fn4<f64, bool, IntProvider, IntProvider, Group3Fn>;

/// The nested `Fn5` for `(t9..t13)` in the flattened `ap13` decomposition:
/// `point_offset` (IntProvider), `min_gen_offset` (i32), `max_gen_offset` (i32),
/// `noise_multiplier` (f64), `invalid_blocks_threshold` (i32) — returning the
/// final `GeodeConfiguration`.
type Group3Fn = Fn5<IntProvider, i32, i32, f64, i32, GeodeConfiguration>;

/// The flattened-`ap13` seed: `map(Function13::curry4, func)` — the leading
/// quadruple `(blocks, layers, crack, use_potential_placements_chance)` whose
/// result is [`Group2Fn`].
type SeedFn = Fn4<GeodeBlockSettings, GeodeLayerSettings, GeodeCrackSettings, f64, Group2Fn>;

/// `GeodeConfiguration.CODEC` — the ops-generic
/// `geode_configuration_codec::<Ops>()` factory (record codec over the thirteen
/// fields). See the module doc for the flattened `ap13` decomposition.
pub fn geode_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<GeodeConfiguration, Ops>> {
    let blocks_field = codec::field_of(geode_block_settings_codec::<Ops>(), "blocks".to_string());
    let layers_field = codec::field_of(geode_layer_settings_codec::<Ops>(), "layers".to_string());
    let crack_field = codec::field_of(geode_crack_settings_codec::<Ops>(), "crack".to_string());
    let chance_range = GeodeConfiguration::chance_range_codec::<Ops>();
    let use_potential_placements_field = codec::optional_field_of(
        "use_potential_placements_chance",
        chance_range.clone(),
        0.35,
    );
    let use_alternate_layer0_field =
        codec::optional_field_of("use_alternate_layer0_chance", chance_range.clone(), 0.0);
    let placements_require_layer0_alternate_field = codec::optional_field_of(
        "placements_require_layer0_alternate",
        codec::bool_codec::<Ops>(),
        true,
    );
    let outer_wall_distance_field = optional_int_provider_field(
        "outer_wall_distance",
        int_provider_codec_with_bounds::<Ops>(1, 20),
        IntProvider::Uniform(UniformInt::of(4, 5)),
    );
    let distribution_points_field = optional_int_provider_field(
        "distribution_points",
        int_provider_codec_with_bounds::<Ops>(1, 20),
        IntProvider::Uniform(UniformInt::of(3, 4)),
    );
    let point_offset_field = optional_int_provider_field(
        "point_offset",
        int_provider_codec_with_bounds::<Ops>(0, 10),
        IntProvider::Uniform(UniformInt::of(1, 2)),
    );
    let min_gen_offset_field =
        codec::optional_field_of("min_gen_offset", codec::int_codec::<Ops>(), -16);
    let max_gen_offset_field =
        codec::optional_field_of("max_gen_offset", codec::int_codec::<Ops>(), 16);
    let noise_multiplier_field = codec::optional_field_of("noise_multiplier", chance_range, 0.05);
    let invalid_blocks_threshold_field = codec::field_of(
        codec::int_codec::<Ops>(),
        "invalid_blocks_threshold".to_string(),
    );

    let f1 = Arc::new(MapCodecEncoderHalf(blocks_field.clone()));
    let d1 = Arc::new(MapCodecDecoderHalf(blocks_field));
    let f2 = Arc::new(MapCodecEncoderHalf(layers_field.clone()));
    let d2 = Arc::new(MapCodecDecoderHalf(layers_field));
    let f3 = Arc::new(MapCodecEncoderHalf(crack_field.clone()));
    let d3 = Arc::new(MapCodecDecoderHalf(crack_field));
    let f4 = Arc::new(MapCodecEncoderHalf(use_potential_placements_field.clone()));
    let d4 = Arc::new(MapCodecDecoderHalf(use_potential_placements_field));
    let f5 = Arc::new(MapCodecEncoderHalf(use_alternate_layer0_field.clone()));
    let d5 = Arc::new(MapCodecDecoderHalf(use_alternate_layer0_field));
    let f6 = Arc::new(MapCodecEncoderHalf(
        placements_require_layer0_alternate_field.clone(),
    ));
    let d6 = Arc::new(MapCodecDecoderHalf(
        placements_require_layer0_alternate_field,
    ));
    let f7 = Arc::new(MapCodecEncoderHalf(outer_wall_distance_field.clone()));
    let d7 = Arc::new(MapCodecDecoderHalf(outer_wall_distance_field));
    let f8 = Arc::new(MapCodecEncoderHalf(distribution_points_field.clone()));
    let d8 = Arc::new(MapCodecDecoderHalf(distribution_points_field));
    let f9 = Arc::new(MapCodecEncoderHalf(point_offset_field.clone()));
    let d9 = Arc::new(MapCodecDecoderHalf(point_offset_field));
    let f10 = Arc::new(MapCodecEncoderHalf(min_gen_offset_field.clone()));
    let d10 = Arc::new(MapCodecDecoderHalf(min_gen_offset_field));
    let f11 = Arc::new(MapCodecEncoderHalf(max_gen_offset_field.clone()));
    let d11 = Arc::new(MapCodecDecoderHalf(max_gen_offset_field));
    let f12 = Arc::new(MapCodecEncoderHalf(noise_multiplier_field.clone()));
    let d12 = Arc::new(MapCodecDecoderHalf(noise_multiplier_field));
    let f13 = Arc::new(MapCodecEncoderHalf(invalid_blocks_threshold_field.clone()));
    let d13 = Arc::new(MapCodecDecoderHalf(invalid_blocks_threshold_field));

    // The encoder writes the fields in group declaration order (like
    // `record_builder::build`'s `BuiltEncoder`).
    let encode = map_encoder::of(
        Arc::new(
            move |c: &GeodeConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                f1.encode(&c.geode_block_settings, ops, prefix);
                f2.encode(&c.geode_layer_settings, ops, prefix);
                f3.encode(&c.geode_crack_settings, ops, prefix);
                f4.encode(&c.use_potential_placements_chance, ops, prefix);
                f5.encode(&c.use_alternate_layer0_chance, ops, prefix);
                f6.encode(&c.placements_require_layer0_alternate, ops, prefix);
                f7.encode(&c.outer_wall_distance, ops, prefix);
                f8.encode(&c.distribution_points, ops, prefix);
                f9.encode(&c.point_offset, ops, prefix);
                f10.encode(&c.min_gen_offset, ops, prefix);
                f11.encode(&c.max_gen_offset, ops, prefix);
                f12.encode(&c.noise_multiplier, ops, prefix);
                f13.encode(&c.invalid_blocks_threshold, ops, prefix);
            },
        ),
        // The encoder `keys()` mirrors Java's `RecordEncoder.keys()` — the
        // `ap`-chained field-name keys (the function encoders contribute none).
        // This matters if the encoder half is ever used standalone via
        // `map_encoder::encoder()`/`MapEncoderAsEncoder` under a compress-maps
        // ops: a zero-slot `keys()` would build a 0-sized `KeyCompressor` and
        // the field writes would panic. Under the merged `map_codec::of` keys
        // path the list is deduplicated by first occurrence, so it is a no-op
        // there (encoder keys come first, in the same order as the decoder's).
        Arc::new(|ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("blocks".to_string()),
                ops.create_string("layers".to_string()),
                ops.create_string("crack".to_string()),
                ops.create_string("use_potential_placements_chance".to_string()),
                ops.create_string("use_alternate_layer0_chance".to_string()),
                ops.create_string("placements_require_layer0_alternate".to_string()),
                ops.create_string("outer_wall_distance".to_string()),
                ops.create_string("distribution_points".to_string()),
                ops.create_string("point_offset".to_string()),
                ops.create_string("min_gen_offset".to_string()),
                ops.create_string("max_gen_offset".to_string()),
                ops.create_string("noise_multiplier".to_string()),
                ops.create_string("invalid_blocks_threshold".to_string()),
            ]
        }),
    );

    // The decoder mirrors `Applicative.super.ap13` flattened to
    // `ap5(ap4(ap4(map(Function13::curry4, func), t1..t4), t5..t8), t9..t13)`
    // (the `GeodeBlockSettings` ap8 pattern extended by one group).
    #[allow(clippy::type_complexity)]
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<SeedFn> = DataResult::success_with_lifecycle(
                Arc::new(
                    move |a: &GeodeBlockSettings,
                          b: &GeodeLayerSettings,
                          c: &GeodeCrackSettings,
                          d: &f64| {
                        let a = a.clone();
                        let b = *b;
                        let c = *c;
                        let d = *d;
                        let inner: Group2Fn =
                            Arc::new(move |e: &f64, f: &bool, g: &IntProvider, h: &IntProvider| {
                                let e = *e;
                                let f = *f;
                                let g = g.clone();
                                let h = h.clone();
                                // Re-clone the non-`Copy` outer capture so the
                                // innermost `move` closure owns it (this closure is
                                // `Fn`, so it cannot hand it away); `b`/`c` are
                                // `Copy` and captured by value.
                                let a = a.clone();
                                let inner2: Group3Fn = Arc::new(
                                    move |i: &IntProvider, j: &i32, k: &i32, l: &f64, m: &i32| {
                                        GeodeConfiguration::new(
                                            a.clone(),
                                            b,
                                            c,
                                            d,
                                            e,
                                            f,
                                            g.clone(),
                                            h.clone(),
                                            i.clone(),
                                            *j,
                                            *k,
                                            *l,
                                            *m,
                                        )
                                    },
                                );
                                inner2
                            });
                        inner
                    },
                ),
                Lifecycle::experimental(),
            );
            let step1 = data_result::ap4(
                fr,
                d1.decode(ops, input),
                d2.decode(ops, input),
                d3.decode(ops, input),
                d4.decode(ops, input),
            );
            let step2 = data_result::ap4(
                step1,
                d5.decode(ops, input),
                d6.decode(ops, input),
                d7.decode(ops, input),
                d8.decode(ops, input),
            );
            data_result::ap5(
                step2,
                d9.decode(ops, input),
                d10.decode(ops, input),
                d11.decode(ops, input),
                d12.decode(ops, input),
                d13.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("blocks".to_string()),
                ops.create_string("layers".to_string()),
                ops.create_string("crack".to_string()),
                ops.create_string("use_potential_placements_chance".to_string()),
                ops.create_string("use_alternate_layer0_chance".to_string()),
                ops.create_string("placements_require_layer0_alternate".to_string()),
                ops.create_string("outer_wall_distance".to_string()),
                ops.create_string("distribution_points".to_string()),
                ops.create_string("point_offset".to_string()),
                ops.create_string("min_gen_offset".to_string()),
                ops.create_string("max_gen_offset".to_string()),
                ops.create_string("noise_multiplier".to_string()),
                ops.create_string("invalid_blocks_threshold".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

impl crate::levelgen::feature::configurations::FeatureConfiguration for GeodeConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
    use crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider;
    use rivet_registry::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::holder_set::HolderSet;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;
    use std::sync::Arc;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn empty_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn provider(state: BlockState) -> Arc<dyn ErasedBlockStateProvider> {
        let simple = SimpleStateProvider::new(state);
        let erased: Arc<dyn ErasedBlockStateProvider> = Arc::new(simple);
        erased
    }

    fn default_block_settings() -> GeodeBlockSettings {
        let stone = crate::block::blocks::Blocks::STONE.default_block_state();
        let air = crate::block::blocks::Blocks::AIR.default_block_state();
        GeodeBlockSettings::new(
            provider(air),
            provider(stone),
            provider(air),
            provider(stone),
            provider(air),
            vec![stone],
            HolderSet::empty(),
            HolderSet::empty(),
        )
    }

    /// A decodable base payload: the three required settings records at their
    /// defaults plus a valid `invalid_blocks_threshold` — produced by encoding
    /// [`default_config`] (all optional fields omitted at their defaults). Tests
    /// that inject a bad optional field start from this base so the decode only
    /// fails on the field under test, never on a malformed settings record.
    fn base_json() -> serde_json::Value {
        let codec = geode_configuration_codec::<TestOps>();
        codec
            .encode_start(&empty_ops(), &default_config())
            .result()
            .expect("encode default config")
            .clone()
    }

    /// A `GeodeConfiguration` with all optional fields at their Java defaults
    /// (so the encode omits them all).
    fn default_config() -> GeodeConfiguration {
        GeodeConfiguration::new(
            default_block_settings(),
            GeodeLayerSettings::new(1.7, 2.2, 3.2, 4.2),
            GeodeCrackSettings::new(1.0, 2.0, 2),
            0.35,
            0.0,
            true,
            IntProvider::Uniform(UniformInt::of(4, 5)),
            IntProvider::Uniform(UniformInt::of(3, 4)),
            IntProvider::Uniform(UniformInt::of(1, 2)),
            -16,
            16,
            0.05,
            1,
        )
    }

    #[test]
    fn codec_round_trip_defaults() {
        let codec = geode_configuration_codec::<TestOps>();
        let config = default_config();
        let encoded = codec
            .encode_start(&empty_ops(), &config)
            .result()
            .expect("encode should succeed")
            .clone();
        let json = encoded.as_object().expect("record object");
        // All optional-with-default fields are omitted; the three required
        // settings records plus `invalid_blocks_threshold` remain.
        assert!(json.contains_key("blocks"));
        assert!(json.contains_key("layers"));
        assert!(json.contains_key("crack"));
        assert_eq!(json["invalid_blocks_threshold"], json!(1));
        assert!(!json.contains_key("use_potential_placements_chance"));
        assert!(!json.contains_key("outer_wall_distance"));
        assert!(!json.contains_key("point_offset"));

        let decoded = codec
            .parse(&empty_ops(), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.use_potential_placements_chance, 0.35);
        assert_eq!(decoded.use_alternate_layer0_chance, 0.0);
        assert!(decoded.placements_require_layer0_alternate);
        assert_eq!(
            decoded.outer_wall_distance,
            IntProvider::Uniform(UniformInt::of(4, 5))
        );
        assert_eq!(
            decoded.distribution_points,
            IntProvider::Uniform(UniformInt::of(3, 4))
        );
        assert_eq!(
            decoded.point_offset,
            IntProvider::Uniform(UniformInt::of(1, 2))
        );
        assert_eq!(decoded.min_gen_offset, -16);
        assert_eq!(decoded.max_gen_offset, 16);
        assert_eq!(decoded.noise_multiplier, 0.05);
        assert_eq!(decoded.invalid_blocks_threshold, 1);
        // The block settings round-trip structurally; their `inner_placements`
        // pins the decode.
        assert_eq!(decoded.geode_block_settings.inner_placements.len(), 1);
        assert_eq!(
            decoded.geode_block_settings.inner_placements[0],
            crate::block::blocks::Blocks::STONE.default_block_state()
        );
        assert_eq!(
            decoded.geode_layer_settings,
            GeodeLayerSettings::new(1.7, 2.2, 3.2, 4.2)
        );
        assert_eq!(
            decoded.geode_crack_settings,
            GeodeCrackSettings::new(1.0, 2.0, 2)
        );
    }

    #[test]
    fn codec_round_trip_custom_optionals() {
        let codec = geode_configuration_codec::<TestOps>();
        let mut config = default_config();
        config.use_potential_placements_chance = 0.6;
        config.use_alternate_layer0_chance = 0.2;
        config.placements_require_layer0_alternate = false;
        config.outer_wall_distance = IntProvider::Uniform(UniformInt::of(2, 7));
        config.distribution_points = IntProvider::Uniform(UniformInt::of(1, 3));
        config.point_offset = IntProvider::Uniform(UniformInt::of(0, 1));
        config.min_gen_offset = -8;
        config.max_gen_offset = 9;
        config.noise_multiplier = 0.1;
        config.invalid_blocks_threshold = 2;

        let encoded = codec
            .encode_start(&empty_ops(), &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "blocks": {
                    "filling_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
                    "inner_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}},
                    "alternate_inner_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
                    "middle_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:stone"}},
                    "outer_layer_provider": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:air"}},
                    "inner_placements": [{"Name": "minecraft:stone"}],
                    "cannot_replace": [],
                    "invalid_blocks": []
                },
                "layers": {},
                "crack": {},
                "use_potential_placements_chance": 0.6,
                "use_alternate_layer0_chance": 0.2,
                "placements_require_layer0_alternate": false,
                "outer_wall_distance": {"type": "minecraft:uniform", "min_inclusive": 2, "max_inclusive": 7},
                "distribution_points": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 3},
                "point_offset": {"type": "minecraft:uniform", "min_inclusive": 0, "max_inclusive": 1},
                "min_gen_offset": -8,
                "max_gen_offset": 9,
                "noise_multiplier": 0.1,
                "invalid_blocks_threshold": 2
            })
        );

        let decoded = codec
            .parse(&empty_ops(), &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.use_potential_placements_chance, 0.6);
        assert_eq!(
            decoded.outer_wall_distance,
            IntProvider::Uniform(UniformInt::of(2, 7))
        );
        assert_eq!(
            decoded.distribution_points,
            IntProvider::Uniform(UniformInt::of(1, 3))
        );
        assert_eq!(
            decoded.point_offset,
            IntProvider::Uniform(UniformInt::of(0, 1))
        );
        assert_eq!(decoded.min_gen_offset, -8);
        assert_eq!(decoded.max_gen_offset, 9);
        assert_eq!(decoded.noise_multiplier, 0.1);
        assert_eq!(decoded.invalid_blocks_threshold, 2);
    }

    #[test]
    fn codec_requires_the_settings_records_and_threshold() {
        let codec = geode_configuration_codec::<TestOps>();
        // Missing required `blocks`/`layers`/`crack`/`invalid_blocks_threshold`.
        assert!(codec.parse(&empty_ops(), &json!({})).result().is_none());
        // A settings record with no fields fails the record codec (each field is
        // required `fieldOf`), so `blocks: {}` is not a decodable payload.
        assert!(
            codec
                .parse(
                    &empty_ops(),
                    &json!({"blocks": {}, "layers": {}, "crack": {}})
                )
                .result()
                .is_none()
        );
        // Dropping only the required `invalid_blocks_threshold` from an otherwise
        // decodable base also fails.
        let mut missing_threshold = base_json();
        missing_threshold
            .as_object_mut()
            .expect("base object")
            .remove("invalid_blocks_threshold");
        assert!(
            codec
                .parse(&empty_ops(), &missing_threshold)
                .result()
                .is_none()
        );
    }

    #[test]
    fn codec_rejects_out_of_bounds_int_provider_optionals() {
        let codec = geode_configuration_codec::<TestOps>();
        // `outer_wall_distance` is `IntProviders.codec(1, 20)`: a constant 25
        // fails the bounds validation even though the field is optional. The
        // base payload's settings records decode fine, so the failure comes from
        // the out-of-range provider, not a malformed `blocks`.
        let mut bad = base_json();
        bad.as_object_mut().expect("base object").insert(
            "outer_wall_distance".to_string(),
            json!({"type": "minecraft:constant", "value": 25}),
        );
        assert!(codec.parse(&empty_ops(), &bad).result().is_none());
        // `point_offset` is `IntProviders.codec(0, 10)`: a negative uniform min.
        let mut bad2 = base_json();
        bad2.as_object_mut().expect("base object").insert(
            "point_offset".to_string(),
            json!({"type": "minecraft:uniform", "min_inclusive": -1, "max_inclusive": 2}),
        );
        assert!(codec.parse(&empty_ops(), &bad2).result().is_none());
        // A present-but-malformed optional is still a decode error (non-lenient).
        let mut bad3 = base_json();
        bad3.as_object_mut()
            .expect("base object")
            .insert("use_potential_placements_chance".to_string(), json!(1.5));
        assert!(codec.parse(&empty_ops(), &bad3).result().is_none());
    }

    #[test]
    fn decode_lifecycle_is_experimental() {
        // Like the flattened-`ap8` `GeodeBlockSettings` decode, the unstamped
        // 13-field `RecordCodecBuilder.create(...).apply(i, new)` decode is
        // Experimental (verified against the pinned DFU 10.0.21 jar).
        let codec = geode_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&empty_ops(), &default_config())
            .result()
            .expect("encode")
            .clone();
        let result = codec.parse(&empty_ops(), &encoded);
        assert_eq!(
            result.lifecycle(),
            rivet_serialization::lifecycle::Lifecycle::experimental()
        );
    }
}
