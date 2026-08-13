//! Port of `net.minecraft.world.level.levelgen.feature.configurations.LargeDripstoneConfiguration`
//! (class, 26.2).
//!
//! Java: a ten-field class (`HolderSet<Block> replaceableBlocks, int
//! floorToCeilingSearchRange, IntProvider columnRadius, FloatProvider heightScale,
//! float maxColumnRadiusToCaveHeightRatio, FloatProvider stalactiteBluntness,
//! FloatProvider stalagmiteBluntness, FloatProvider windSpeed, int
//! minRadiusForWind, float minBluntnessForWind`) whose `CODEC` is a
//! `RecordCodecBuilder` over the required `"replaceable_blocks"`
//! (`RegistryCodecs.homogeneousList(Registries.BLOCK)` — a `HolderSetCodec` over
//! `RegistryFixedCodec(Registries.BLOCK)`), the `"floor_to_ceiling_search_range"`
//! field (`Codec.intRange(1, 512).optionalFieldOf(..., 30)` — the NON-lenient
//! with-default optional), the required `"column_radius"`
//! (`IntProviders.codec(1, 16)`), `"height_scale"` (`FloatProviders.codec(0.0F,
//! 20.0F)`), `"max_column_radius_to_cave_height_ratio"` (`Codec.floatRange(0.1F,
//! 1.0F)`), `"stalactite_bluntness"`/`"stalagmite_bluntness"`
//! (`FloatProviders.codec(0.1F, 10.0F)` each), `"wind_speed"`
//! (`FloatProviders.codec(0.0F, 2.0F)`), `"min_radius_for_wind"`
//! (`Codec.intRange(0, 100)`) and `"min_bluntness_for_wind"`
//! (`Codec.floatRange(0.0F, 5.0F)`). DFU `Codec<T>` is `Codec<E, Ops>` in the
//! port, so the static Java constant is exposed as the ops-generic
//! `large_dripstone_configuration_codec::<Ops>()` factory.
//!
//! The ten-field group exceeds the port's `record_builder` `Group6` cap, so the
//! record codec is hand-composed with `map_encoder`/`map_decoder` exactly
//! mirroring `Applicative.super.ap10` (`ap5(ap5(map(Function10::curry5, func),
//! t1..t5), t6..t10)`); each `ap5` is the port's helper
//! (`ap3(ap2(map(curry2, f), a, b), c, d, e)`), so the nested curried function
//! types are `Fn5`/`Fn5`.
//!
//! `replaceableBlocks` is the value-semantic `HolderSet<BlockType>` (the
//! `BlockType` id-handle placeholder, the same surface `SpeleothemConfiguration`
//! uses). `PartialEq` mirrors `Objects.equals` on the record's fields: the
//! providers compare by value (`IntProvider`/`FloatProvider` derive a faithful
//! `PartialEq` in `rivet-util`) and the raw float fields via `Float.equals`
//! (`java_float_equals` — NaN payloads canonicalize, `-0.0` distinct from `0.0`).

use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::float_format::java_float_equals;
use rivet_serialization::functions::Fn5;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::{MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use rivet_util::valueproviders::float_provider::{FloatProvider, float_provider_codec_with_bounds};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.LargeDripstoneConfiguration`.
#[derive(Debug, Clone)]
pub struct LargeDripstoneConfiguration {
    /// `replaceableBlocks` — the blocks large dripstone may replace.
    pub replaceable_blocks: HolderSet<BlockType>,
    /// `floorToCeilingSearchRange` — `[1, 512]`, default `30`.
    pub floor_to_ceiling_search_range: i32,
    /// `columnRadius` — an `IntProvider` bounded to `[1, 16]`.
    pub column_radius: IntProvider,
    /// `heightScale` — a `FloatProvider` bounded to `[0.0, 20.0]`.
    pub height_scale: FloatProvider,
    /// `maxColumnRadiusToCaveHeightRatio` — `[0.1, 1.0]`.
    pub max_column_radius_to_cave_height_ratio: f32,
    /// `stalactiteBluntness` — a `FloatProvider` bounded to `[0.1, 10.0]`.
    pub stalactite_bluntness: FloatProvider,
    /// `stalagmiteBluntness` — a `FloatProvider` bounded to `[0.1, 10.0]`.
    pub stalagmite_bluntness: FloatProvider,
    /// `windSpeed` — a `FloatProvider` bounded to `[0.0, 2.0]`.
    pub wind_speed: FloatProvider,
    /// `minRadiusForWind` — `[0, 100]`.
    pub min_radius_for_wind: i32,
    /// `minBluntnessForWind` — `[0.0, 5.0]`.
    pub min_bluntness_for_wind: f32,
}

impl PartialEq for LargeDripstoneConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // `Objects.equals` on the class's Float fields compares via
        // `Float.equals` (`Float.compare` semantics); the providers and holder
        // set compare by value.
        self.replaceable_blocks == other.replaceable_blocks
            && self.floor_to_ceiling_search_range == other.floor_to_ceiling_search_range
            && self.column_radius == other.column_radius
            && self.height_scale == other.height_scale
            && java_float_equals(
                self.max_column_radius_to_cave_height_ratio,
                other.max_column_radius_to_cave_height_ratio,
            )
            && self.stalactite_bluntness == other.stalactite_bluntness
            && self.stalagmite_bluntness == other.stalagmite_bluntness
            && self.wind_speed == other.wind_speed
            && self.min_radius_for_wind == other.min_radius_for_wind
            && java_float_equals(self.min_bluntness_for_wind, other.min_bluntness_for_wind)
    }
}

impl Eq for LargeDripstoneConfiguration {}

impl LargeDripstoneConfiguration {
    /// `new LargeDripstoneConfiguration(HolderSet<Block>, int, IntProvider,
    /// FloatProvider, float, FloatProvider, FloatProvider, FloatProvider, int,
    /// float)` — the constructor (the codec's `apply` function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        replaceable_blocks: HolderSet<BlockType>,
        floor_to_ceiling_search_range: i32,
        column_radius: IntProvider,
        height_scale: FloatProvider,
        max_column_radius_to_cave_height_ratio: f32,
        stalactite_bluntness: FloatProvider,
        stalagmite_bluntness: FloatProvider,
        wind_speed: FloatProvider,
        min_radius_for_wind: i32,
        min_bluntness_for_wind: f32,
    ) -> Self {
        LargeDripstoneConfiguration {
            replaceable_blocks,
            floor_to_ceiling_search_range,
            column_radius,
            height_scale,
            max_column_radius_to_cave_height_ratio,
            stalactite_bluntness,
            stalagmite_bluntness,
            wind_speed,
            min_radius_for_wind,
            min_bluntness_for_wind,
        }
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the
/// `"replaceable_blocks"` field codec: a `HolderSetCodec` over the block
/// registry whose element codec is a `RegistryFixedCodec` (tag key
/// `#minecraft:...` or element-list form, `alwaysUseList=false`).
///
/// The concrete codec is not `Send + Sync` (its `RegistryOps` carries the
/// single-threaded `HolderLookupAdapter` `RefCell` memo); the `Arc` is held by
/// the ops-parameterized configuration codec and never crosses threads.
fn replaceable_blocks_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(RegistryFixedCodec::create(
        &rivet_registry::registries::BLOCK,
    ));
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> = Arc::new(HolderSetCodec::create(
        &rivet_registry::registries::BLOCK,
        element,
        false,
    ));
    codec::field_of(holder_set, "replaceable_blocks".to_string())
}

/// `LargeDripstoneConfiguration.CODEC` — the ops-generic
/// `large_dripstone_configuration_codec::<Ops>()` factory (record codec over the
/// ten fields: nine required, one optional-with-default). The ten-field group
/// exceeds the port's `record_builder` `Group6` cap, so the decode side is
/// hand-composed with the `Applicative.super.ap10` decomposition
/// `ap5(ap5(map(Function10::curry5, func), t1..t5), t6..t10)`: the leading
/// quintuple's results assemble a `Fn5` returning the trailing `Fn5`, which the
/// outer `ap5` applies to the last five field results.
///
/// Each field is a full `MapCodec` (`codec::field_of(...)` for the required
/// fields, `codec::optional_field_of` for the search-range optional); the
/// encoder/decoder halves are the `MapCodecEncoderHalf`/`MapCodecDecoderHalf`
/// adapters, so the field codec's own `encode`/`decode` (which knows the key
/// and — for the optional — the default/omission logic) drives both directions.
pub fn large_dripstone_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<LargeDripstoneConfiguration, Ops>> {
    let replaceable_blocks_field = replaceable_blocks_field_codec::<Ops>();
    let floor_to_ceiling_search_range_field = codec::optional_field_of::<i32, Ops>(
        "floor_to_ceiling_search_range",
        codec::int_range::<Ops>(1, 512),
        30,
    );
    let column_radius_field = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(1, 16),
        "column_radius".to_string(),
    );
    let height_scale_field = codec::field_of(
        float_provider_codec_with_bounds::<Ops>(0.0, 20.0),
        "height_scale".to_string(),
    );
    let max_column_radius_to_cave_height_ratio_field = codec::field_of(
        codec::float_range::<Ops>(0.1, 1.0),
        "max_column_radius_to_cave_height_ratio".to_string(),
    );
    let stalactite_bluntness_field = codec::field_of(
        float_provider_codec_with_bounds::<Ops>(0.1, 10.0),
        "stalactite_bluntness".to_string(),
    );
    let stalagmite_bluntness_field = codec::field_of(
        float_provider_codec_with_bounds::<Ops>(0.1, 10.0),
        "stalagmite_bluntness".to_string(),
    );
    let wind_speed_field = codec::field_of(
        float_provider_codec_with_bounds::<Ops>(0.0, 2.0),
        "wind_speed".to_string(),
    );
    let min_radius_for_wind_field = codec::field_of(
        codec::int_range::<Ops>(0, 100),
        "min_radius_for_wind".to_string(),
    );
    let min_bluntness_for_wind_field = codec::field_of(
        codec::float_range::<Ops>(0.0, 5.0),
        "min_bluntness_for_wind".to_string(),
    );

    let replaceable_blocks_encoder =
        Arc::new(MapCodecEncoderHalf(replaceable_blocks_field.clone()));
    let replaceable_blocks_decoder = Arc::new(MapCodecDecoderHalf(replaceable_blocks_field));
    let floor_to_ceiling_search_range_encoder = Arc::new(MapCodecEncoderHalf(
        floor_to_ceiling_search_range_field.clone(),
    ));
    let floor_to_ceiling_search_range_decoder =
        Arc::new(MapCodecDecoderHalf(floor_to_ceiling_search_range_field));
    let column_radius_encoder = Arc::new(MapCodecEncoderHalf(column_radius_field.clone()));
    let column_radius_decoder = Arc::new(MapCodecDecoderHalf(column_radius_field));
    let height_scale_encoder = Arc::new(MapCodecEncoderHalf(height_scale_field.clone()));
    let height_scale_decoder = Arc::new(MapCodecDecoderHalf(height_scale_field));
    let max_column_radius_to_cave_height_ratio_encoder = Arc::new(MapCodecEncoderHalf(
        max_column_radius_to_cave_height_ratio_field.clone(),
    ));
    let max_column_radius_to_cave_height_ratio_decoder = Arc::new(MapCodecDecoderHalf(
        max_column_radius_to_cave_height_ratio_field,
    ));
    let stalactite_bluntness_encoder =
        Arc::new(MapCodecEncoderHalf(stalactite_bluntness_field.clone()));
    let stalactite_bluntness_decoder = Arc::new(MapCodecDecoderHalf(stalactite_bluntness_field));
    let stalagmite_bluntness_encoder =
        Arc::new(MapCodecEncoderHalf(stalagmite_bluntness_field.clone()));
    let stalagmite_bluntness_decoder = Arc::new(MapCodecDecoderHalf(stalagmite_bluntness_field));
    let wind_speed_encoder = Arc::new(MapCodecEncoderHalf(wind_speed_field.clone()));
    let wind_speed_decoder = Arc::new(MapCodecDecoderHalf(wind_speed_field));
    let min_radius_for_wind_encoder =
        Arc::new(MapCodecEncoderHalf(min_radius_for_wind_field.clone()));
    let min_radius_for_wind_decoder = Arc::new(MapCodecDecoderHalf(min_radius_for_wind_field));
    let min_bluntness_for_wind_encoder =
        Arc::new(MapCodecEncoderHalf(min_bluntness_for_wind_field.clone()));
    let min_bluntness_for_wind_decoder =
        Arc::new(MapCodecDecoderHalf(min_bluntness_for_wind_field));

    // Like `record_builder::build`'s `BuiltEncoder`, the encoder supplies no
    // keys and writes the fields in group declaration order.
    let encode = map_encoder::of(
        Arc::new(
            move |c: &LargeDripstoneConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                replaceable_blocks_encoder.encode(&c.replaceable_blocks, ops, prefix);
                floor_to_ceiling_search_range_encoder.encode(
                    &c.floor_to_ceiling_search_range,
                    ops,
                    prefix,
                );
                column_radius_encoder.encode(&c.column_radius, ops, prefix);
                height_scale_encoder.encode(&c.height_scale, ops, prefix);
                max_column_radius_to_cave_height_ratio_encoder.encode(
                    &c.max_column_radius_to_cave_height_ratio,
                    ops,
                    prefix,
                );
                stalactite_bluntness_encoder.encode(&c.stalactite_bluntness, ops, prefix);
                stalagmite_bluntness_encoder.encode(&c.stalagmite_bluntness, ops, prefix);
                wind_speed_encoder.encode(&c.wind_speed, ops, prefix);
                min_radius_for_wind_encoder.encode(&c.min_radius_for_wind, ops, prefix);
                min_bluntness_for_wind_encoder.encode(&c.min_bluntness_for_wind, ops, prefix);
            },
        ),
        Arc::new(|_ops: &Ops| -> Vec<Ops::Output> { Vec::new() }),
    );

    // The decoder mirrors `Applicative.super.ap10`: the leading quintuple forms
    // a `Fn5` returning the trailing `Fn5`, which the outer `ap5` applies.
    #[allow(clippy::type_complexity)]
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<
                Fn5<
                    HolderSet<BlockType>,
                    i32,
                    IntProvider,
                    FloatProvider,
                    f32,
                    Fn5<
                        FloatProvider,
                        FloatProvider,
                        FloatProvider,
                        i32,
                        f32,
                        LargeDripstoneConfiguration,
                    >,
                >,
            > = DataResult::success_with_lifecycle(
                Arc::new(
                    move |c1: &HolderSet<BlockType>,
                          c2: &i32,
                          c3: &IntProvider,
                          c4: &FloatProvider,
                          c5: &f32| {
                        let c1 = c1.clone();
                        let c2 = *c2;
                        let c3 = c3.clone();
                        let c4 = c4.clone();
                        let c5 = *c5;
                        let inner: Fn5<
                            FloatProvider,
                            FloatProvider,
                            FloatProvider,
                            i32,
                            f32,
                            LargeDripstoneConfiguration,
                        > = Arc::new(
                            move |g1: &FloatProvider,
                                  g2: &FloatProvider,
                                  g3: &FloatProvider,
                                  g4: &i32,
                                  g5: &f32| {
                                LargeDripstoneConfiguration::new(
                                    c1.clone(),
                                    c2,
                                    c3.clone(),
                                    c4.clone(),
                                    c5,
                                    g1.clone(),
                                    g2.clone(),
                                    g3.clone(),
                                    *g4,
                                    *g5,
                                )
                            },
                        );
                        inner
                    },
                ),
                Lifecycle::experimental(),
            );
            let step1 = data_result::ap5(
                fr,
                replaceable_blocks_decoder.decode(ops, input),
                floor_to_ceiling_search_range_decoder.decode(ops, input),
                column_radius_decoder.decode(ops, input),
                height_scale_decoder.decode(ops, input),
                max_column_radius_to_cave_height_ratio_decoder.decode(ops, input),
            );
            data_result::ap5(
                step1,
                stalactite_bluntness_decoder.decode(ops, input),
                stalagmite_bluntness_decoder.decode(ops, input),
                wind_speed_decoder.decode(ops, input),
                min_radius_for_wind_decoder.decode(ops, input),
                min_bluntness_for_wind_decoder.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("replaceable_blocks".to_string()),
                ops.create_string("floor_to_ceiling_search_range".to_string()),
                ops.create_string("column_radius".to_string()),
                ops.create_string("height_scale".to_string()),
                ops.create_string("max_column_radius_to_cave_height_ratio".to_string()),
                ops.create_string("stalactite_bluntness".to_string()),
                ops.create_string("stalagmite_bluntness".to_string()),
                ops.create_string("wind_speed".to_string()),
                ops.create_string("min_radius_for_wind".to_string()),
                ops.create_string("min_bluntness_for_wind".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

impl FeatureConfiguration for LargeDripstoneConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::clamped_int::ClampedInt;
    use rivet_util::valueproviders::constant_float::ConstantFloat;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_float::UniformFloat;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:stone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:deepslate"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn two_block_set(access: &RegistryAccess) -> HolderSet<BlockType> {
        let registry = RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry");
        HolderSet::direct(vec![
            Holder::reference(registry.registry_id(), 0),
            Holder::reference(registry.registry_id(), 1),
        ])
    }

    fn sample_config(replaceable: HolderSet<BlockType>) -> LargeDripstoneConfiguration {
        // The pinned `large_dripstone.json` wire shape (all values explicit,
        // `floor_to_ceiling_search_range` defaulted to 30). `column_radius` is
        // the fixture's `clamped` provider: source `uniform(3, 19)` clamped to
        // `[3, 16]` — within the codec's `IntProviders.codec(1, 16)` bound.
        LargeDripstoneConfiguration::new(
            replaceable,
            30,
            IntProvider::Clamped(ClampedInt::of(
                IntProvider::Uniform(UniformInt::of(3, 19)),
                3,
                16,
            )),
            FloatProvider::Uniform(UniformFloat::of(0.4, 2.0)),
            0.33,
            FloatProvider::Uniform(UniformFloat::of(0.3, 0.9)),
            FloatProvider::Uniform(UniformFloat::of(0.4, 1.0)),
            FloatProvider::Uniform(UniformFloat::of(0.0, 0.3)),
            4,
            0.6,
        )
    }

    #[test]
    fn codec_round_trip() {
        let access = block_access();
        let config = sample_config(two_block_set(&access));
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = large_dripstone_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "replaceable_blocks": ["minecraft:stone", "minecraft:deepslate"],
                "column_radius": {
                    "type": "minecraft:clamped",
                    "min_inclusive": 3,
                    "max_inclusive": 16,
                    "source": {
                        "type": "minecraft:uniform",
                        "min_inclusive": 3,
                        "max_inclusive": 19,
                    },
                },
                "height_scale": {
                    "type": "minecraft:uniform",
                    "min_inclusive": 0.4,
                    "max_exclusive": 2.0,
                },
                "max_column_radius_to_cave_height_ratio": 0.33,
                "stalactite_bluntness": {
                    "type": "minecraft:uniform",
                    "min_inclusive": 0.3,
                    "max_exclusive": 0.9,
                },
                "stalagmite_bluntness": {
                    "type": "minecraft:uniform",
                    "min_inclusive": 0.4,
                    "max_exclusive": 1.0,
                },
                "wind_speed": {
                    "type": "minecraft:uniform",
                    "min_inclusive": 0.0,
                    "max_exclusive": 0.3,
                },
                "min_radius_for_wind": 4,
                "min_bluntness_for_wind": 0.6,
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_omits_the_defaulted_search_range() {
        // `floor_to_ceiling_search_range` equals its default 30, so the
        // with-default optional is omitted on encode and restored on decode.
        let access = block_access();
        let config = sample_config(two_block_set(&access));
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = large_dripstone_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert!(encoded.get("floor_to_ceiling_search_range").is_none());
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.floor_to_ceiling_search_range, 30);
    }

    #[test]
    fn codec_encodes_an_explicit_search_range() {
        let access = block_access();
        let config = LargeDripstoneConfiguration::new(
            two_block_set(&access),
            100,
            IntProvider::Constant(ConstantInt::of(5)),
            FloatProvider::Constant(ConstantFloat::of(1.0)),
            0.5,
            FloatProvider::Constant(ConstantFloat::of(0.5)),
            FloatProvider::Constant(ConstantFloat::of(0.5)),
            FloatProvider::Constant(ConstantFloat::of(0.1)),
            10,
            1.0,
        );
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = large_dripstone_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded.get("floor_to_ceiling_search_range"),
            Some(&json!(100))
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_all_fields() {
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = large_dripstone_configuration_codec::<TestOps>();
        assert!(codec.parse(&ops, &json!({})).is_error());
        // `column_radius` missing.
        let missing = json!({
            "replaceable_blocks": [],
            "height_scale": 1.0,
            "max_column_radius_to_cave_height_ratio": 0.3,
            "stalactite_bluntness": 0.5,
            "stalagmite_bluntness": 0.5,
            "wind_speed": 0.1,
            "min_radius_for_wind": 4,
            "min_bluntness_for_wind": 0.6,
        });
        assert!(codec.parse(&ops, &missing).is_error());
    }

    #[test]
    fn codec_rejects_out_of_range_values() {
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = large_dripstone_configuration_codec::<TestOps>();
        // `min_radius_for_wind` above [0, 100].
        let high = json!({
            "replaceable_blocks": [],
            "column_radius": 5,
            "height_scale": 1.0,
            "max_column_radius_to_cave_height_ratio": 0.3,
            "stalactite_bluntness": 0.5,
            "stalagmite_bluntness": 0.5,
            "wind_speed": 0.1,
            "min_radius_for_wind": 101,
            "min_bluntness_for_wind": 0.6,
        });
        let result = codec.parse(&ops, &high);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value 101 outside of range [0:100]");
        // `max_column_radius_to_cave_height_ratio` below [0.1, 1.0].
        let low_ratio = json!({
            "replaceable_blocks": [],
            "column_radius": 5,
            "height_scale": 1.0,
            "max_column_radius_to_cave_height_ratio": 0.05,
            "stalactite_bluntness": 0.5,
            "stalagmite_bluntness": 0.5,
            "wind_speed": 0.1,
            "min_radius_for_wind": 4,
            "min_bluntness_for_wind": 0.6,
        });
        assert!(codec.parse(&ops, &low_ratio).is_error());
    }

    #[test]
    fn value_equality_semantics() {
        let access = block_access();
        let replaceable = two_block_set(&access);
        let config = sample_config(replaceable.clone());
        assert_eq!(
            config,
            LargeDripstoneConfiguration::new(
                replaceable.clone(),
                30,
                IntProvider::Clamped(ClampedInt::of(
                    IntProvider::Uniform(UniformInt::of(3, 19)),
                    3,
                    16,
                )),
                FloatProvider::Uniform(UniformFloat::of(0.4, 2.0)),
                0.33,
                FloatProvider::Uniform(UniformFloat::of(0.3, 0.9)),
                FloatProvider::Uniform(UniformFloat::of(0.4, 1.0)),
                FloatProvider::Uniform(UniformFloat::of(0.0, 0.3)),
                4,
                0.6
            )
        );
        // `Float.equals` canonicalizes every NaN payload.
        let nan_a = f32::from_bits(0x7fc0_0001);
        let nan_b = f32::from_bits(0x7fc0_0002);
        assert_eq!(
            LargeDripstoneConfiguration::new(
                replaceable.clone(),
                30,
                IntProvider::Constant(ConstantInt::of(5)),
                FloatProvider::Constant(ConstantFloat::of(1.0)),
                nan_a,
                FloatProvider::Constant(ConstantFloat::of(0.5)),
                FloatProvider::Constant(ConstantFloat::of(0.5)),
                FloatProvider::Constant(ConstantFloat::of(0.1)),
                10,
                1.0
            ),
            LargeDripstoneConfiguration::new(
                replaceable,
                30,
                IntProvider::Constant(ConstantInt::of(5)),
                FloatProvider::Constant(ConstantFloat::of(1.0)),
                nan_b,
                FloatProvider::Constant(ConstantFloat::of(0.5)),
                FloatProvider::Constant(ConstantFloat::of(0.5)),
                FloatProvider::Constant(ConstantFloat::of(0.1)),
                10,
                1.0
            )
        );
    }
}
