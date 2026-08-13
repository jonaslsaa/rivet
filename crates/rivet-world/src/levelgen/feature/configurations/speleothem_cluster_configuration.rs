//! Port of `net.minecraft.world.level.levelgen.feature.configurations.SpeleothemClusterConfiguration`
//! (record, 26.2).
//!
//! Java: a fourteen-field record:
//! `record SpeleothemClusterConfiguration(BlockState baseBlock, BlockState
//! pointedBlock, HolderSet<Block> replaceableBlocks, int
//! floorToCeilingSearchRange, IntProvider height, IntProvider radius, int
//! maxStalagmiteStalactiteHeightDiff, int heightDeviation, IntProvider
//! speleothemBlockLayerThickness, FloatProvider density, FloatProvider wetness,
//! float chanceOfSpeleothemAtMaxDistanceFromCenter, int
//! maxDistanceFromEdgeAffectingChanceOfSpeleothem, int
//! maxDistanceFromCenterAffectingHeightBias)` whose `CODEC` is a
//! `RecordCodecBuilder` over ALL required fields: `"base_block"`/`"pointed_block"`
//! (`BlockState.CODEC`), `"replaceable_blocks"`
//! (`RegistryCodecs.homogeneousList(Registries.BLOCK)`), `"floor_to_ceiling_search_range"`
//! (`Codec.intRange(1, 512)`), `"height"`/`"radius"`
//! (`IntProviders.codec(1, 128)`), `"max_stalagmite_stalactite_height_diff"`
//! (`Codec.intRange(0, 64)`), `"height_deviation"` (`Codec.intRange(1, 64)`),
//! `"speleothem_block_layer_thickness"` (`IntProviders.codec(0, 128)`),
//! `"density"`/`"wetness"` (`FloatProviders.codec(0.0F, 2.0F)`),
//! `"chance_of_speleothem_at_max_distance_from_center"` (`Codec.floatRange(0.0F,
//! 1.0F)`), `"max_distance_from_edge_affecting_chance_of_speleothem"`
//! (`Codec.intRange(1, 64)`) and `"max_distance_from_center_affecting_height_bias"`
//! (`Codec.intRange(1, 64)`). DFU `Codec<T>` is `Codec<E, Ops>` in the port, so
//! the static Java constant is exposed as the ops-generic
//! `speleothem_cluster_configuration_codec::<Ops>()` factory.
//!
//! The fourteen-field group exceeds the port's `record_builder` `Group6` cap, so
//! the record codec is hand-composed with `map_encoder`/`map_decoder` mirroring
//! `Applicative.super.ap14` (`ap7(ap7(map(Function14.curry7, func), t1..t7),
//! t8..t14)`), flattened to the port's `ap3`/`ap4` helpers as
//! `ap4(ap4(ap3(map(curry3, func), t1, t2, t3), t4..t7), t8..t11), t12..t14)`:
//! the leading triple's results assemble a `Fn4` for `(t4..t7)`, whose results
//! assemble a `Fn4` for `(t8..t11)`, whose results assemble a `Fn3` for
//! `(t12..t14)`. Any left-to-right `ap`-chain preserves Java's
//! `DataResult` message ordering (the `ap2`/`ap3` Instance fast paths combine
//! lifecycles in declaration order), so the flattened shape is faithful.

use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::float_format::java_float_equals;
use rivet_serialization::functions::{Fn3, Fn4};
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::{MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use rivet_util::valueproviders::float_provider::{FloatProvider, float_provider_codec_with_bounds};
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.SpeleothemClusterConfiguration`.
#[derive(Debug, Clone)]
pub struct SpeleothemClusterConfiguration {
    /// `baseBlock` — the block speleothem clusters grow from.
    pub base_block: BlockState,
    /// `pointedBlock` — the speleothem block (e.g. pointed dripstone).
    pub pointed_block: BlockState,
    /// `replaceableBlocks` — the blocks the cluster may replace.
    pub replaceable_blocks: HolderSet<BlockType>,
    /// `floorToCeilingSearchRange` — `[1, 512]`.
    pub floor_to_ceiling_search_range: i32,
    /// `height` — an `IntProvider` bounded to `[1, 128]`.
    pub height: IntProvider,
    /// `radius` — an `IntProvider` bounded to `[1, 128]`.
    pub radius: IntProvider,
    /// `maxStalagmiteStalactiteHeightDiff` — `[0, 64]`.
    pub max_stalagmite_stalactite_height_diff: i32,
    /// `heightDeviation` — `[1, 64]`.
    pub height_deviation: i32,
    /// `speleothemBlockLayerThickness` — an `IntProvider` bounded to `[0, 128]`.
    pub speleothem_block_layer_thickness: IntProvider,
    /// `density` — a `FloatProvider` bounded to `[0.0, 2.0]`.
    pub density: FloatProvider,
    /// `wetness` — a `FloatProvider` bounded to `[0.0, 2.0]`.
    pub wetness: FloatProvider,
    /// `chanceOfSpeleothemAtMaxDistanceFromCenter` — `[0.0, 1.0]`.
    pub chance_of_speleothem_at_max_distance_from_center: f32,
    /// `maxDistanceFromEdgeAffectingChanceOfSpeleothem` — `[1, 64]`.
    pub max_distance_from_edge_affecting_chance_of_speleothem: i32,
    /// `maxDistanceFromCenterAffectingHeightBias` — `[1, 64]`.
    pub max_distance_from_center_affecting_height_bias: i32,
}

impl PartialEq for SpeleothemClusterConfiguration {
    fn eq(&self, other: &Self) -> bool {
        // `Objects.equals` on the record's Float fields compares via
        // `Float.equals` (`Float.compare` semantics): NaN payloads canonicalize
        // and `-0.0` is distinct from `+0.0`.
        self.base_block == other.base_block
            && self.pointed_block == other.pointed_block
            && self.replaceable_blocks == other.replaceable_blocks
            && self.floor_to_ceiling_search_range == other.floor_to_ceiling_search_range
            && self.height == other.height
            && self.radius == other.radius
            && self.max_stalagmite_stalactite_height_diff
                == other.max_stalagmite_stalactite_height_diff
            && self.height_deviation == other.height_deviation
            && self.speleothem_block_layer_thickness == other.speleothem_block_layer_thickness
            && self.density == other.density
            && self.wetness == other.wetness
            && java_float_equals(
                self.chance_of_speleothem_at_max_distance_from_center,
                other.chance_of_speleothem_at_max_distance_from_center,
            )
            && self.max_distance_from_edge_affecting_chance_of_speleothem
                == other.max_distance_from_edge_affecting_chance_of_speleothem
            && self.max_distance_from_center_affecting_height_bias
                == other.max_distance_from_center_affecting_height_bias
    }
}

impl Eq for SpeleothemClusterConfiguration {}

impl SpeleothemClusterConfiguration {
    /// `new SpeleothemClusterConfiguration(BlockState, BlockState, HolderSet<Block>,
    /// int, IntProvider, IntProvider, int, int, IntProvider, FloatProvider,
    /// FloatProvider, float, int, int)` — the record constructor (the codec's
    /// `apply` function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_block: BlockState,
        pointed_block: BlockState,
        replaceable_blocks: HolderSet<BlockType>,
        floor_to_ceiling_search_range: i32,
        height: IntProvider,
        radius: IntProvider,
        max_stalagmite_stalactite_height_diff: i32,
        height_deviation: i32,
        speleothem_block_layer_thickness: IntProvider,
        density: FloatProvider,
        wetness: FloatProvider,
        chance_of_speleothem_at_max_distance_from_center: f32,
        max_distance_from_edge_affecting_chance_of_speleothem: i32,
        max_distance_from_center_affecting_height_bias: i32,
    ) -> Self {
        SpeleothemClusterConfiguration {
            base_block,
            pointed_block,
            replaceable_blocks,
            floor_to_ceiling_search_range,
            height,
            radius,
            max_stalagmite_stalactite_height_diff,
            height_deviation,
            speleothem_block_layer_thickness,
            density,
            wetness,
            chance_of_speleothem_at_max_distance_from_center,
            max_distance_from_edge_affecting_chance_of_speleothem,
            max_distance_from_center_affecting_height_bias,
        }
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the
/// `"replaceable_blocks"` field codec: a `HolderSetCodec` over the block
/// registry whose element codec is a `RegistryFixedCodec` (tag key
/// `#minecraft:...` or element-list form, `alwaysUseList=false`). The concrete
/// codec is not `Send + Sync`, so the `Arc` is held by the ops-parameterized
/// configuration codec and never crosses threads.
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

/// `SpeleothemClusterConfiguration.CODEC` — the ops-generic
/// `speleothem_cluster_configuration_codec::<Ops>()` factory (record codec over
/// the fourteen required fields). See the module doc for the flattened
/// `Applicative.super.ap14` decomposition; every field is a full `MapCodec`
/// (`codec::field_of`), whose `MapCodecEncoderHalf`/`MapCodecDecoderHalf`
/// adapters drive both directions.
pub fn speleothem_cluster_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<SpeleothemClusterConfiguration, Ops>> {
    let base_block_field = codec::field_of(block_state_codec::<Ops>(), "base_block".to_string());
    let pointed_block_field =
        codec::field_of(block_state_codec::<Ops>(), "pointed_block".to_string());
    let replaceable_blocks_field = replaceable_blocks_field_codec::<Ops>();
    let floor_to_ceiling_search_range_field = codec::field_of(
        codec::int_range::<Ops>(1, 512),
        "floor_to_ceiling_search_range".to_string(),
    );
    let height_field = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(1, 128),
        "height".to_string(),
    );
    let radius_field = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(1, 128),
        "radius".to_string(),
    );
    let max_stalagmite_stalactite_height_diff_field = codec::field_of(
        codec::int_range::<Ops>(0, 64),
        "max_stalagmite_stalactite_height_diff".to_string(),
    );
    let height_deviation_field = codec::field_of(
        codec::int_range::<Ops>(1, 64),
        "height_deviation".to_string(),
    );
    let speleothem_block_layer_thickness_field = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(0, 128),
        "speleothem_block_layer_thickness".to_string(),
    );
    let density_field = codec::field_of(
        float_provider_codec_with_bounds::<Ops>(0.0, 2.0),
        "density".to_string(),
    );
    let wetness_field = codec::field_of(
        float_provider_codec_with_bounds::<Ops>(0.0, 2.0),
        "wetness".to_string(),
    );
    let chance_of_speleothem_at_max_distance_from_center_field = codec::field_of(
        codec::float_range::<Ops>(0.0, 1.0),
        "chance_of_speleothem_at_max_distance_from_center".to_string(),
    );
    let max_distance_from_edge_affecting_chance_of_speleothem_field = codec::field_of(
        codec::int_range::<Ops>(1, 64),
        "max_distance_from_edge_affecting_chance_of_speleothem".to_string(),
    );
    let max_distance_from_center_affecting_height_bias_field = codec::field_of(
        codec::int_range::<Ops>(1, 64),
        "max_distance_from_center_affecting_height_bias".to_string(),
    );

    let base_block_encoder = Arc::new(MapCodecEncoderHalf(base_block_field.clone()));
    let base_block_decoder = Arc::new(MapCodecDecoderHalf(base_block_field));
    let pointed_block_encoder = Arc::new(MapCodecEncoderHalf(pointed_block_field.clone()));
    let pointed_block_decoder = Arc::new(MapCodecDecoderHalf(pointed_block_field));
    let replaceable_blocks_encoder =
        Arc::new(MapCodecEncoderHalf(replaceable_blocks_field.clone()));
    let replaceable_blocks_decoder = Arc::new(MapCodecDecoderHalf(replaceable_blocks_field));
    let floor_to_ceiling_search_range_encoder = Arc::new(MapCodecEncoderHalf(
        floor_to_ceiling_search_range_field.clone(),
    ));
    let floor_to_ceiling_search_range_decoder =
        Arc::new(MapCodecDecoderHalf(floor_to_ceiling_search_range_field));
    let height_encoder = Arc::new(MapCodecEncoderHalf(height_field.clone()));
    let height_decoder = Arc::new(MapCodecDecoderHalf(height_field));
    let radius_encoder = Arc::new(MapCodecEncoderHalf(radius_field.clone()));
    let radius_decoder = Arc::new(MapCodecDecoderHalf(radius_field));
    let max_stalagmite_stalactite_height_diff_encoder = Arc::new(MapCodecEncoderHalf(
        max_stalagmite_stalactite_height_diff_field.clone(),
    ));
    let max_stalagmite_stalactite_height_diff_decoder = Arc::new(MapCodecDecoderHalf(
        max_stalagmite_stalactite_height_diff_field,
    ));
    let height_deviation_encoder = Arc::new(MapCodecEncoderHalf(height_deviation_field.clone()));
    let height_deviation_decoder = Arc::new(MapCodecDecoderHalf(height_deviation_field));
    let speleothem_block_layer_thickness_encoder = Arc::new(MapCodecEncoderHalf(
        speleothem_block_layer_thickness_field.clone(),
    ));
    let speleothem_block_layer_thickness_decoder =
        Arc::new(MapCodecDecoderHalf(speleothem_block_layer_thickness_field));
    let density_encoder = Arc::new(MapCodecEncoderHalf(density_field.clone()));
    let density_decoder = Arc::new(MapCodecDecoderHalf(density_field));
    let wetness_encoder = Arc::new(MapCodecEncoderHalf(wetness_field.clone()));
    let wetness_decoder = Arc::new(MapCodecDecoderHalf(wetness_field));
    let chance_of_speleothem_at_max_distance_from_center_encoder = Arc::new(MapCodecEncoderHalf(
        chance_of_speleothem_at_max_distance_from_center_field.clone(),
    ));
    let chance_of_speleothem_at_max_distance_from_center_decoder = Arc::new(MapCodecDecoderHalf(
        chance_of_speleothem_at_max_distance_from_center_field,
    ));
    let max_distance_from_edge_affecting_chance_of_speleothem_encoder = Arc::new(
        MapCodecEncoderHalf(max_distance_from_edge_affecting_chance_of_speleothem_field.clone()),
    );
    let max_distance_from_edge_affecting_chance_of_speleothem_decoder = Arc::new(
        MapCodecDecoderHalf(max_distance_from_edge_affecting_chance_of_speleothem_field),
    );
    let max_distance_from_center_affecting_height_bias_encoder = Arc::new(MapCodecEncoderHalf(
        max_distance_from_center_affecting_height_bias_field.clone(),
    ));
    let max_distance_from_center_affecting_height_bias_decoder = Arc::new(MapCodecDecoderHalf(
        max_distance_from_center_affecting_height_bias_field,
    ));

    // Like `record_builder::build`'s `BuiltEncoder`, the encoder supplies no
    // keys and writes the fields in group declaration order.
    let encode = map_encoder::of(
        Arc::new(
            move |c: &SpeleothemClusterConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                base_block_encoder.encode(&c.base_block, ops, prefix);
                pointed_block_encoder.encode(&c.pointed_block, ops, prefix);
                replaceable_blocks_encoder.encode(&c.replaceable_blocks, ops, prefix);
                floor_to_ceiling_search_range_encoder.encode(
                    &c.floor_to_ceiling_search_range,
                    ops,
                    prefix,
                );
                height_encoder.encode(&c.height, ops, prefix);
                radius_encoder.encode(&c.radius, ops, prefix);
                max_stalagmite_stalactite_height_diff_encoder.encode(
                    &c.max_stalagmite_stalactite_height_diff,
                    ops,
                    prefix,
                );
                height_deviation_encoder.encode(&c.height_deviation, ops, prefix);
                speleothem_block_layer_thickness_encoder.encode(
                    &c.speleothem_block_layer_thickness,
                    ops,
                    prefix,
                );
                density_encoder.encode(&c.density, ops, prefix);
                wetness_encoder.encode(&c.wetness, ops, prefix);
                chance_of_speleothem_at_max_distance_from_center_encoder.encode(
                    &c.chance_of_speleothem_at_max_distance_from_center,
                    ops,
                    prefix,
                );
                max_distance_from_edge_affecting_chance_of_speleothem_encoder.encode(
                    &c.max_distance_from_edge_affecting_chance_of_speleothem,
                    ops,
                    prefix,
                );
                max_distance_from_center_affecting_height_bias_encoder.encode(
                    &c.max_distance_from_center_affecting_height_bias,
                    ops,
                    prefix,
                );
            },
        ),
        Arc::new(|_ops: &Ops| -> Vec<Ops::Output> { Vec::new() }),
    );

    // The decoder mirrors `Applicative.super.ap14` flattened to the port's
    // helpers: the leading triple forms a `Fn4` for `(t4..t7)`, which forms a
    // `Fn4` for `(t8..t11)`, which forms the final `Fn3` for `(t12..t14)`.
    #[allow(clippy::type_complexity)]
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<Fn3<BlockState, BlockState, HolderSet<BlockType>, Fn4_4_7>> =
                DataResult::success_with_lifecycle(
                    Arc::new(
                        move |c1: &BlockState, c2: &BlockState, c3: &HolderSet<BlockType>| {
                            let c1 = *c1;
                            let c2 = *c2;
                            let c3 = c3.clone();
                            let inner: Fn4_4_7 = Arc::new(
                                move |g1: &i32, g2: &IntProvider, g3: &IntProvider, g4: &i32| {
                                    let g1 = *g1;
                                    let g2 = g2.clone();
                                    let g3 = g3.clone();
                                    let g4 = *g4;
                                    // Fresh per-invocation locals to move into the
                                    // next closure — this closure is `Fn`, so it
                                    // cannot move its own captures.
                                    let c3 = c3.clone();
                                    let g2 = g2.clone();
                                    let g3 = g3.clone();
                                    let inner: Fn4_8_11 = Arc::new(
                                    move |h1: &i32,
                                          h2: &IntProvider,
                                          h3: &FloatProvider,
                                          h4: &FloatProvider| {
                                        let h1 = *h1;
                                        let h2 = h2.clone();
                                        let h3 = h3.clone();
                                        let h4 = h4.clone();
                                        let c3 = c3.clone();
                                        let g2 = g2.clone();
                                        let g3 = g3.clone();
                                        let h2 = h2.clone();
                                        let h3 = h3.clone();
                                        let h4 = h4.clone();
                                        let inner: Fn3<f32, i32, i32, SpeleothemClusterConfiguration> =
                                            Arc::new(
                                                move |k1: &f32, k2: &i32, k3: &i32| {
                                                    SpeleothemClusterConfiguration::new(
                                                        c1, c2, c3.clone(), g1, g2.clone(),
                                                        g3.clone(), g4, h1, h2.clone(),
                                                        h3.clone(), h4.clone(), *k1, *k2, *k3,
                                                    )
                                                },
                                            );
                                        inner
                                    },
                                );
                                    inner
                                },
                            );
                            inner
                        },
                    ),
                    Lifecycle::experimental(),
                );
            let step1 = data_result::ap3(
                fr,
                base_block_decoder.decode(ops, input),
                pointed_block_decoder.decode(ops, input),
                replaceable_blocks_decoder.decode(ops, input),
            );
            let step2 = data_result::ap4(
                step1,
                floor_to_ceiling_search_range_decoder.decode(ops, input),
                height_decoder.decode(ops, input),
                radius_decoder.decode(ops, input),
                max_stalagmite_stalactite_height_diff_decoder.decode(ops, input),
            );
            let step3 = data_result::ap4(
                step2,
                height_deviation_decoder.decode(ops, input),
                speleothem_block_layer_thickness_decoder.decode(ops, input),
                density_decoder.decode(ops, input),
                wetness_decoder.decode(ops, input),
            );
            data_result::ap3(
                step3,
                chance_of_speleothem_at_max_distance_from_center_decoder.decode(ops, input),
                max_distance_from_edge_affecting_chance_of_speleothem_decoder.decode(ops, input),
                max_distance_from_center_affecting_height_bias_decoder.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("base_block".to_string()),
                ops.create_string("pointed_block".to_string()),
                ops.create_string("replaceable_blocks".to_string()),
                ops.create_string("floor_to_ceiling_search_range".to_string()),
                ops.create_string("height".to_string()),
                ops.create_string("radius".to_string()),
                ops.create_string("max_stalagmite_stalactite_height_diff".to_string()),
                ops.create_string("height_deviation".to_string()),
                ops.create_string("speleothem_block_layer_thickness".to_string()),
                ops.create_string("density".to_string()),
                ops.create_string("wetness".to_string()),
                ops.create_string("chance_of_speleothem_at_max_distance_from_center".to_string()),
                ops.create_string(
                    "max_distance_from_edge_affecting_chance_of_speleothem".to_string(),
                ),
                ops.create_string("max_distance_from_center_affecting_height_bias".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

/// The nested `Fn4` for `(t4..t7)` in the flattened `ap14` decomposition:
/// `floor_to_ceiling_search_range` (i32), `height` (IntProvider), `radius`
/// (IntProvider), `max_stalagmite_stalactite_height_diff` (i32) — returning the
/// next-level `Fn4`.
type Fn4_4_7 = Fn4<i32, IntProvider, IntProvider, i32, Fn4_8_11>;

/// The nested `Fn4` for `(t8..t11)` in the flattened `ap14` decomposition:
/// `height_deviation` (i32), `speleothem_block_layer_thickness` (IntProvider),
/// `density` (FloatProvider), `wetness` (FloatProvider) — returning the final
/// `Fn3` for `(t12..t14)`.
type Fn4_8_11 = Fn4<
    i32,
    IntProvider,
    FloatProvider,
    FloatProvider,
    Fn3<f32, i32, i32, SpeleothemClusterConfiguration>,
>;

impl FeatureConfiguration for SpeleothemClusterConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::TagKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::uniform_float::UniformFloat;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `dripstone_block` (id 0) and `pointed_dripstone`
    /// (id 1), with the `minecraft:dripstone_replaceable_blocks` tag bound to
    /// both, wrapped in a `RegistryAccess` under `Registries.BLOCK`.
    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        let stone = builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:dripstone_block"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let pointed = builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:pointed_dripstone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        builder.bind_tags(vec![(
            TagKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:dripstone_replaceable_blocks"),
            ),
            vec![stone, pointed],
        )]);
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    /// A two-element direct holder set over the test registry, resolved through
    /// the SAME access the ops use.
    fn two_block_set(access: &RegistryAccess) -> HolderSet<BlockType> {
        let registry = RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry");
        HolderSet::direct(vec![
            Holder::reference(registry.registry_id(), 0),
            Holder::reference(registry.registry_id(), 1),
        ])
    }

    fn sample_config(replaceable: HolderSet<BlockType>) -> SpeleothemClusterConfiguration {
        SpeleothemClusterConfiguration::new(
            BlockState::of(BlockId::from_name("minecraft:dripstone_block").unwrap()),
            BlockState::of(BlockId::from_name("minecraft:pointed_dripstone").unwrap()),
            replaceable,
            12,
            IntProvider::Uniform(UniformInt::of(3, 6)),
            IntProvider::Uniform(UniformInt::of(2, 8)),
            1,
            3,
            IntProvider::Uniform(UniformInt::of(2, 4)),
            FloatProvider::Uniform(UniformFloat::of(0.3, 0.7)),
            FloatProvider::Uniform(UniformFloat::of(0.1, 0.5)),
            0.1,
            3,
            8,
        )
    }

    #[test]
    fn codec_round_trip() {
        let access = block_access();
        let config = sample_config(two_block_set(&access));
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_cluster_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded.get("floor_to_ceiling_search_range"),
            Some(&json!(12))
        );
        assert_eq!(
            encoded.get("replaceable_blocks"),
            Some(&json!([
                "minecraft:dripstone_block",
                "minecraft:pointed_dripstone"
            ]))
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_decodes_the_pinned_dripstone_cluster_fixture() {
        // The real `dripstone_cluster.json` config: every field present, with
        // `replaceable_blocks` in tag form (`#minecraft:dripstone_replaceable_blocks`)
        // — resolved through the tag-bound test registry. `pointed_block`
        // carries a `Properties` map (`BlockState.CODEC`).
        let fixture = json!({
            "base_block": {"Name": "minecraft:dripstone_block"},
            "chance_of_speleothem_at_max_distance_from_center": 0.1,
            "density": {"type": "minecraft:uniform", "max_exclusive": 0.7, "min_inclusive": 0.3},
            "floor_to_ceiling_search_range": 12,
            "height": {"type": "minecraft:uniform", "max_inclusive": 6, "min_inclusive": 3},
            "height_deviation": 3,
            "max_distance_from_center_affecting_height_bias": 8,
            "max_distance_from_edge_affecting_chance_of_speleothem": 3,
            "max_stalagmite_stalactite_height_diff": 1,
            "pointed_block": {
                "Name": "minecraft:pointed_dripstone",
                "Properties": {"thickness": "tip", "vertical_direction": "up", "waterlogged": "false"}
            },
            "radius": {"type": "minecraft:uniform", "max_inclusive": 8, "min_inclusive": 2},
            "replaceable_blocks": "#minecraft:dripstone_replaceable_blocks",
            "speleothem_block_layer_thickness": {"type": "minecraft:uniform", "max_inclusive": 4, "min_inclusive": 2},
            "wetness": {"type": "minecraft:uniform", "max_exclusive": 0.5, "min_inclusive": 0.1}
        });
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_cluster_configuration_codec::<TestOps>();
        let decoded = codec
            .parse(&ops, &fixture)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.floor_to_ceiling_search_range, 12);
        assert_eq!(decoded.height_deviation, 3);
        assert_eq!(decoded.max_stalagmite_stalactite_height_diff, 1);
        assert_eq!(
            decoded.max_distance_from_edge_affecting_chance_of_speleothem,
            3
        );
        assert_eq!(decoded.max_distance_from_center_affecting_height_bias, 8);
        // The tag form decodes to a Named set; re-encoding writes the tag key
        // back out (byte-for-byte the fixture's replaceable_blocks).
        let re_encoded = codec
            .encode_start(&ops, &decoded)
            .result()
            .expect("re-encode should succeed")
            .clone();
        assert_eq!(
            re_encoded.get("replaceable_blocks"),
            Some(&json!("#minecraft:dripstone_replaceable_blocks"))
        );
    }

    #[test]
    fn codec_requires_all_fields() {
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_cluster_configuration_codec::<TestOps>();
        // `fieldOf("base_block")` is required.
        let missing = json!({
            "replaceable_blocks": [],
            "floor_to_ceiling_search_range": 12,
            "height": 3,
            "radius": 2,
            "max_stalagmite_stalactite_height_diff": 1,
            "height_deviation": 3,
            "speleothem_block_layer_thickness": 2,
            "density": 0.5,
            "wetness": 0.2,
            "chance_of_speleothem_at_max_distance_from_center": 0.1,
            "max_distance_from_edge_affecting_chance_of_speleothem": 3,
            "max_distance_from_center_affecting_height_bias": 8,
        });
        assert!(codec.parse(&ops, &missing).is_error());
    }

    #[test]
    fn codec_rejects_out_of_range_values() {
        let access = block_access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = speleothem_cluster_configuration_codec::<TestOps>();
        // `floor_to_ceiling_search_range` above [1, 512].
        let mut bad = json!({
            "base_block": {"Name": "minecraft:dripstone_block"},
            "pointed_block": {"Name": "minecraft:pointed_dripstone"},
            "replaceable_blocks": [],
            "floor_to_ceiling_search_range": 600,
            "height": 3,
            "radius": 2,
            "max_stalagmite_stalactite_height_diff": 1,
            "height_deviation": 3,
            "speleothem_block_layer_thickness": 2,
            "density": 0.5,
            "wetness": 0.2,
            "chance_of_speleothem_at_max_distance_from_center": 0.1,
            "max_distance_from_edge_affecting_chance_of_speleothem": 3,
            "max_distance_from_center_affecting_height_bias": 8,
        });
        let result = codec.parse(&ops, &bad);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value 600 outside of range [1:512]");
        // A negative `speleothem_block_layer_thickness` provider fails the
        // `IntProviders.codec(0, 128)` validation.
        bad["speleothem_block_layer_thickness"] =
            json!({"type": "minecraft:uniform", "min_inclusive": -2, "max_inclusive": 2});
        bad["floor_to_ceiling_search_range"] = json!(12);
        assert!(codec.parse(&ops, &bad).is_error());
    }

    #[test]
    fn value_equality_semantics() {
        let access = block_access();
        let replaceable = two_block_set(&access);
        let config = sample_config(replaceable);
        // `Float.equals` — a NaN payload equals any other NaN payload.
        let nan = f32::from_bits(0x7fc0_0001);
        let nan_other = f32::from_bits(0x7fc0_0002);
        let with_nan = |chance: f32| {
            SpeleothemClusterConfiguration::new(
                config.base_block,
                config.pointed_block,
                config.replaceable_blocks.clone(),
                config.floor_to_ceiling_search_range,
                config.height.clone(),
                config.radius.clone(),
                config.max_stalagmite_stalactite_height_diff,
                config.height_deviation,
                config.speleothem_block_layer_thickness.clone(),
                config.density.clone(),
                config.wetness.clone(),
                chance,
                config.max_distance_from_edge_affecting_chance_of_speleothem,
                config.max_distance_from_center_affecting_height_bias,
            )
        };
        assert_eq!(with_nan(nan), with_nan(nan_other));
    }
}
