//! Port of `net.minecraft.world.level.levelgen.feature.configurations.VegetationPatchConfiguration`
//! (record, 26.2).
//!
//! Java: a ten-field record `record VegetationPatchConfiguration(HolderSet<Block>
//! replaceable, BlockStateProvider groundState, Holder<PlacedFeature>
//! vegetationFeature, CaveSurface surface, IntProvider depth, float
//! extraBottomBlockChance, int verticalRange, float vegetationChance, IntProvider
//! xzRadius, float extraEdgeColumnChance)` whose `CODEC` is a `RecordCodecBuilder`
//! over ALL required fields: `"replaceable"`
//! (`RegistryCodecs.homogeneousList(Registries.BLOCK)`), `"ground_state"`
//! (`BlockStateProvider.CODEC` — the `"type"` by-name dispatch),
//! `"vegetation_feature"` (`PlacedFeature.CODEC` — a `RegistryFileCodec` over
//! `Registries.PLACED_FEATURE` with inline definitions allowed, element
//! `DIRECT_CODEC`), `"surface"` (`CaveSurface.CODEC`), `"depth"`
//! (`IntProviders.codec(1, 128)`), `"extra_bottom_block_chance"`
//! (`Codec.floatRange(0.0F, 1.0F)`), `"vertical_range"` (`Codec.intRange(1,
//! 256)`), `"vegetation_chance"` (`Codec.floatRange(0.0F, 1.0F)`),
//! `"xz_radius"` (`IntProviders.CODEC` — the PLAIN dispatch, no bounds) and
//! `"extra_edge_column_chance"` (`Codec.floatRange(0.0F, 1.0F)`). DFU
//! `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant is
//! exposed as the ops-generic `vegetation_patch_configuration_codec::<Ops>()`
//! factory.
//!
//! The ten-field group exceeds the port's `record_builder` `Group6` cap, so the
//! record codec is hand-composed with `map_encoder`/`map_decoder` mirroring
//! `Applicative.super.ap10` (`ap5(ap5(map(Function10.curry5, func), t1..t5),
//! t6..t10)`): the leading quintuple's results assemble a `Fn5` returning the
//! trailing `Fn5`, which the outer `ap5` applies.
//!
//! ## The `vegetationFeature` holder — `PlacedFeature.CODEC` STUB
//!
//! The `Holder<PlacedFeature>` field codec is `PlacedFeature.CODEC` = a
//! `RegistryFileCodec` over `Registries.PLACED_FEATURE` (`"worldgen/placed_feature"`)
//! with inline definitions allowed (the pinned `moss_patch.json` inlines the
//! feature: `{"feature": "minecraft:moss_vegetation", "placement": []}`). The
//! element codec is `DIRECT_CODEC`, a record over `"feature"`
//! (`ConfiguredFeature.CODEC` — itself a `RegistryFileCodec` over
//! `Registries.CONFIGURED_FEATURE`) and `"placement"`
//! (`PlacementModifier.CODEC.listOf()`).
//!
//! Both `ConfiguredFeature.CODEC`'s direct form (the `"type"`/`"config"`
//! dispatch — deferred to issue #126) and `PlacementModifier.CODEC` (the
//! per-modifier dispatch — deferred to the owning placement unit) are
//! cross-unit surfaces, so this module STUBs them:
//! - [`configured_feature_holder_codec`] keeps the identifier-reference form
//!   (`RegistryFileCodec` with `allow_inline`), which is the wire shape this
//!   unit's fixtures exercise; the inline direct form errors (the feature
//!   dispatch that builds a `ConfiguredFeatureErased` from a map defers).
//! - [`placement_modifier_codec`] errors on any element, so the fixture's empty
//!   `"placement": []` list decodes and a non-empty list fails loudly instead of
//!   silently mis-decoding.

use crate::levelgen::feature::ConfiguredFeatureErased;
use crate::levelgen::feature::configurations::FeatureConfiguration;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec,
};
use crate::levelgen::placement::CaveSurface;
use crate::levelgen::placement::ErasedPlacementModifier;
use crate::levelgen::placement::PlacedFeature;
use crate::levelgen::placement::cave_surface_codec;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_file_codec::RegistryFileCodec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::resource_key::ResourceKey;
use rivet_registry::{Identifier, Registry};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::{self, DataResult};
use rivet_serialization::decoder;
use rivet_serialization::dynamic_ops::{DynamicOps, MapLike, RecordBuilder};
use rivet_serialization::encoder;
use rivet_serialization::functions::Fn5;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::{MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::{self, MapDecoder};
use rivet_serialization::map_encoder::{self, MapEncoder};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::valueproviders::int_provider::{
    IntProvider, int_provider_codec, int_provider_codec_with_bounds,
};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.VegetationPatchConfiguration`.
///
/// The `groundState` half is held as the erased `Arc<dyn ErasedBlockStateProvider>`
/// carrier and `vegetationFeature` as the `Holder<PlacedFeature>` value (the
/// `PlacedFeature` value record holds an erased modifier list and derives no
/// `PartialEq`), so the configuration is `Clone`+`Debug` only — the same shape
/// `BlockColumnConfiguration` takes.
#[derive(Debug, Clone)]
pub struct VegetationPatchConfiguration {
    /// `replaceable` — the blocks the vegetation patch may replace.
    pub replaceable: HolderSet<BlockType>,
    /// `groundState` — the block state provider for the patch's ground blocks.
    pub ground_state: Arc<dyn ErasedBlockStateProvider>,
    /// `vegetationFeature` — the placed feature scattered over the patch.
    pub vegetation_feature: Holder<PlacedFeature>,
    /// `surface` — the cave surface the patch grows on.
    pub surface: CaveSurface,
    /// `depth` — an `IntProvider` bounded to `[1, 128]`.
    pub depth: IntProvider,
    /// `extraBottomBlockChance` — `[0.0, 1.0]`.
    pub extra_bottom_block_chance: f32,
    /// `verticalRange` — `[1, 256]`.
    pub vertical_range: i32,
    /// `vegetationChance` — `[0.0, 1.0]`.
    pub vegetation_chance: f32,
    /// `xzRadius` — an `IntProvider` (plain `IntProviders.CODEC`, no bounds).
    pub xz_radius: IntProvider,
    /// `extraEdgeColumnChance` — `[0.0, 1.0]`.
    pub extra_edge_column_chance: f32,
}

impl VegetationPatchConfiguration {
    /// `new VegetationPatchConfiguration(HolderSet<Block>, BlockStateProvider,
    /// Holder<PlacedFeature>, CaveSurface, IntProvider, float, int, float,
    /// IntProvider, float)` — the record constructor (the codec's `apply`
    /// function).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        replaceable: HolderSet<BlockType>,
        ground_state: Arc<dyn ErasedBlockStateProvider>,
        vegetation_feature: Holder<PlacedFeature>,
        surface: CaveSurface,
        depth: IntProvider,
        extra_bottom_block_chance: f32,
        vertical_range: i32,
        vegetation_chance: f32,
        xz_radius: IntProvider,
        extra_edge_column_chance: f32,
    ) -> Self {
        VegetationPatchConfiguration {
            replaceable,
            ground_state,
            vegetation_feature,
            surface,
            depth,
            extra_bottom_block_chance,
            vertical_range,
            vegetation_chance,
            xz_radius,
            extra_edge_column_chance,
        }
    }
}

/// `Registries.CONFIGURED_FEATURE` — `"worldgen/configured_feature"`, the
/// registry key `ConfiguredFeature.CODEC` is a `RegistryFileCodec` over. Not yet
/// a `rivet-registry` constant, so it is declared here for the codec STUB.
fn configured_feature_registry_key() -> ResourceKey<Registry<ConfiguredFeatureErased>> {
    ResourceKey::create_registry_key(Identifier::with_default_namespace(
        "worldgen/configured_feature",
    ))
}

/// `Registries.PLACED_FEATURE` — `"worldgen/placed_feature"`, the registry key
/// `PlacedFeature.CODEC` is a `RegistryFileCodec` over. Not yet a
/// `rivet-registry` constant, so it is declared here for the codec STUB.
fn placed_feature_registry_key() -> ResourceKey<Registry<PlacedFeature>> {
    ResourceKey::create_registry_key(Identifier::with_default_namespace(
        "worldgen/placed_feature",
    ))
}

/// `PlacementModifier.CODEC` — the `"type"`-dispatch codec for the placement
/// modifier list elements.
///
/// STUB(mc.world.level.levelgen.placement.core): the per-modifier
/// dispatch defers with the owning `mc.world.level.levelgen.placement.core`
/// unit; this stub errors on any element, so an empty `"placement"` list
/// decodes and a non-empty list fails loudly instead of silently mis-decoding.
fn placement_modifier_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<dyn ErasedPlacementModifier>, Ops>> {
    codec::of(
        encoder::error::<Arc<dyn ErasedPlacementModifier>, Ops>(
            "STUB: placement modifier encoding defers with the placement unit".to_string(),
        ),
        decoder::error::<Arc<dyn ErasedPlacementModifier>, Ops>(
            "STUB: placement modifier decoding defers with the placement unit".to_string(),
        ),
        "PlacementModifier".to_string(),
    )
}

/// `ConfiguredFeature.CODEC` — the `RegistryFileCodec` over
/// `Registries.CONFIGURED_FEATURE` whose identifier-reference form this unit's
/// fixtures exercise.
///
/// STUB(mc.world.level.levelgen.feature.core): the reference
/// form (a `"minecraft:..."` identifier resolving a configured feature) is kept
/// faithful; the inline direct form (a `{"type": ..., "config": ...}` map built
/// through the `Feature.CODEC` dispatch) defers with issue #126 and errors via
/// [`configured_feature_direct_codec`].
fn configured_feature_holder_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Holder<ConfiguredFeatureErased>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    Arc::new(RegistryFileCodec::create(
        &configured_feature_registry_key(),
        configured_feature_direct_codec::<Ops>(),
    ))
}

/// `ConfiguredFeature.DIRECT_CODEC` — the inline `{"type", "config"}` map form.
///
/// STUB(mc.world.level.levelgen.feature.core): building a
/// `ConfiguredFeatureErased` from the map needs `Feature.CODEC` (the `"type"`
/// by-name dispatch) and the per-feature config codec dispatch, which defer
/// with issue #126; this stub errors on both directions.
fn configured_feature_direct_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ConfiguredFeatureErased, Ops>> {
    codec::of(
        encoder::error::<ConfiguredFeatureErased, Ops>(
            "STUB: inline configured feature encoding defers with the feature dispatch (issue #126)"
                .to_string(),
        ),
        decoder::error::<ConfiguredFeatureErased, Ops>(
            "STUB: inline configured feature decoding defers with the feature dispatch (issue #126)"
                .to_string(),
        ),
        "ConfiguredFeature".to_string(),
    )
}

/// `PlacedFeature.DIRECT_CODEC` — a record over the required `"feature"` field
/// (`ConfiguredFeature.CODEC`) and the required `"placement"` field
/// (`PlacementModifier.CODEC.listOf()`).
fn placed_feature_direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<PlacedFeature, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &PlacedFeature| p.feature.clone()),
                codec::field_of(
                    configured_feature_holder_codec::<Ops>(),
                    "feature".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &PlacedFeature| p.placement.clone()),
                codec::field_of(
                    codec::list(placement_modifier_codec::<Ops>()),
                    "placement".to_string(),
                ),
            ))
            .apply(instance, Arc::new(PlacedFeature::new))
    })
}

/// `PlacedFeature.CODEC` — `RegistryCodecs.create(Registries.PLACED_FEATURE,
/// DIRECT_CODEC)`, a `RegistryFileCodec` over the placed-feature registry with
/// inline definitions allowed, as the ops-generic `placed_feature_codec::<Ops>()`
/// factory.
///
/// STUB(mc.world.level.levelgen.placement.core): the codec surface
/// is owned by the pending `mc.world.level.levelgen.placement.core` unit;
/// this stub keeps the wire shape faithful (identifier reference or inline
/// `DIRECT_CODEC`) with the two inner dispatches deferred (see
/// [`configured_feature_direct_codec`] and [`placement_modifier_codec`]).
pub fn placed_feature_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Holder<PlacedFeature>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    Arc::new(RegistryFileCodec::create(
        &placed_feature_registry_key(),
        placed_feature_direct_codec::<Ops>(),
    ))
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the `"replaceable"`
/// field codec: a `HolderSetCodec` over the block registry (tag key
/// `#minecraft:...` or element-list form).
fn replaceable_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(
        rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::BLOCK,
        ),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::BLOCK,
            element,
            false,
        ));
    codec::field_of(holder_set, "replaceable".to_string())
}

/// `VegetationPatchConfiguration.CODEC` — the ops-generic
/// `vegetation_patch_configuration_codec::<Ops>()` factory (record codec over
/// the ten required fields). See the module doc for the flattened
/// `Applicative.super.ap10` decomposition; every field is a full `MapCodec`
/// (`codec::field_of`), whose `MapCodecEncoderHalf`/`MapCodecDecoderHalf`
/// adapters drive both directions.
pub fn vegetation_patch_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<VegetationPatchConfiguration, Ops>> {
    vegetation_patch_configuration_codec_impl::<Ops>()
}

fn vegetation_patch_configuration_codec_impl<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<VegetationPatchConfiguration, Ops>> {
    let replaceable_field = replaceable_field_codec::<Ops>();
    let ground_state_field = codec::field_of(
        block_state_provider_codec::<Ops>(),
        "ground_state".to_string(),
    );
    let vegetation_feature_field = codec::field_of(
        placed_feature_codec::<Ops>(),
        "vegetation_feature".to_string(),
    );
    let surface_field =
        codec::field_of(Arc::new(cave_surface_codec::<Ops>()), "surface".to_string());

    let depth_field = codec::field_of(
        int_provider_codec_with_bounds::<Ops>(1, 128),
        "depth".to_string(),
    );
    let extra_bottom_block_chance_field = codec::field_of(
        codec::float_range::<Ops>(0.0, 1.0),
        "extra_bottom_block_chance".to_string(),
    );
    let vertical_range_field = codec::field_of(
        codec::int_range::<Ops>(1, 256),
        "vertical_range".to_string(),
    );
    let vegetation_chance_field = codec::field_of(
        codec::float_range::<Ops>(0.0, 1.0),
        "vegetation_chance".to_string(),
    );
    let xz_radius_field = codec::field_of(int_provider_codec::<Ops>(), "xz_radius".to_string());
    let extra_edge_column_chance_field = codec::field_of(
        codec::float_range::<Ops>(0.0, 1.0),
        "extra_edge_column_chance".to_string(),
    );

    let replaceable_encoder = Arc::new(MapCodecEncoderHalf(replaceable_field.clone()));
    let replaceable_decoder = Arc::new(MapCodecDecoderHalf(replaceable_field));
    let ground_state_encoder = Arc::new(MapCodecEncoderHalf(ground_state_field.clone()));
    let ground_state_decoder = Arc::new(MapCodecDecoderHalf(ground_state_field));
    let vegetation_feature_encoder =
        Arc::new(MapCodecEncoderHalf(vegetation_feature_field.clone()));
    let vegetation_feature_decoder = Arc::new(MapCodecDecoderHalf(vegetation_feature_field));
    let surface_encoder = Arc::new(MapCodecEncoderHalf(surface_field.clone()));
    let surface_decoder = Arc::new(MapCodecDecoderHalf(surface_field));
    let depth_encoder = Arc::new(MapCodecEncoderHalf(depth_field.clone()));
    let depth_decoder = Arc::new(MapCodecDecoderHalf(depth_field));
    let extra_bottom_block_chance_encoder =
        Arc::new(MapCodecEncoderHalf(extra_bottom_block_chance_field.clone()));
    let extra_bottom_block_chance_decoder =
        Arc::new(MapCodecDecoderHalf(extra_bottom_block_chance_field));
    let vertical_range_encoder = Arc::new(MapCodecEncoderHalf(vertical_range_field.clone()));
    let vertical_range_decoder = Arc::new(MapCodecDecoderHalf(vertical_range_field));
    let vegetation_chance_encoder = Arc::new(MapCodecEncoderHalf(vegetation_chance_field.clone()));
    let vegetation_chance_decoder = Arc::new(MapCodecDecoderHalf(vegetation_chance_field));
    let xz_radius_encoder = Arc::new(MapCodecEncoderHalf(xz_radius_field.clone()));
    let xz_radius_decoder = Arc::new(MapCodecDecoderHalf(xz_radius_field));
    let extra_edge_column_chance_encoder =
        Arc::new(MapCodecEncoderHalf(extra_edge_column_chance_field.clone()));
    let extra_edge_column_chance_decoder =
        Arc::new(MapCodecDecoderHalf(extra_edge_column_chance_field));

    // Like `record_builder::build`'s `BuiltEncoder`, the encoder supplies no
    // keys and writes the fields in group declaration order.
    let encode = map_encoder::of(
        Arc::new(
            move |c: &VegetationPatchConfiguration,
                  ops: &Ops,
                  prefix: &mut dyn RecordBuilder<Output = Ops::Output>| {
                replaceable_encoder.encode(&c.replaceable, ops, prefix);
                ground_state_encoder.encode(&c.ground_state, ops, prefix);
                vegetation_feature_encoder.encode(&c.vegetation_feature, ops, prefix);
                surface_encoder.encode(&c.surface, ops, prefix);
                depth_encoder.encode(&c.depth, ops, prefix);
                extra_bottom_block_chance_encoder.encode(&c.extra_bottom_block_chance, ops, prefix);
                vertical_range_encoder.encode(&c.vertical_range, ops, prefix);
                vegetation_chance_encoder.encode(&c.vegetation_chance, ops, prefix);
                xz_radius_encoder.encode(&c.xz_radius, ops, prefix);
                extra_edge_column_chance_encoder.encode(&c.extra_edge_column_chance, ops, prefix);
            },
        ),
        Arc::new(|_ops: &Ops| -> Vec<Ops::Output> { Vec::new() }),
    );

    // The decoder mirrors `Applicative.super.ap10`: the leading quintuple forms
    // a `Fn5` returning the trailing `Fn5`, which `ap5` applies.
    #[allow(clippy::type_complexity)]
    let decode = map_decoder::of(
        Arc::new(move |ops: &Ops, input: &dyn MapLike<Ops::Output>| {
            let fr: DataResult<
                Fn5<
                    HolderSet<BlockType>,
                    Arc<dyn ErasedBlockStateProvider>,
                    Holder<PlacedFeature>,
                    CaveSurface,
                    IntProvider,
                    Fn5<f32, i32, f32, IntProvider, f32, VegetationPatchConfiguration>,
                >,
            > = DataResult::success_with_lifecycle(
                Arc::new(
                    move |c1: &HolderSet<BlockType>,
                          c2: &Arc<dyn ErasedBlockStateProvider>,
                          c3: &Holder<PlacedFeature>,
                          c4: &CaveSurface,
                          c5: &IntProvider| {
                        let c1 = c1.clone();
                        let c2 = c2.clone();
                        let c3 = c3.clone();
                        let c4 = *c4;
                        let c5 = c5.clone();
                        let inner: Fn5<
                            f32,
                            i32,
                            f32,
                            IntProvider,
                            f32,
                            VegetationPatchConfiguration,
                        > = Arc::new(
                            move |g1: &f32, g2: &i32, g3: &f32, g4: &IntProvider, g5: &f32| {
                                VegetationPatchConfiguration::new(
                                    c1.clone(),
                                    c2.clone(),
                                    c3.clone(),
                                    c4,
                                    c5.clone(),
                                    *g1,
                                    *g2,
                                    *g3,
                                    g4.clone(),
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
                replaceable_decoder.decode(ops, input),
                ground_state_decoder.decode(ops, input),
                vegetation_feature_decoder.decode(ops, input),
                surface_decoder.decode(ops, input),
                depth_decoder.decode(ops, input),
            );
            data_result::ap5(
                step1,
                extra_bottom_block_chance_decoder.decode(ops, input),
                vertical_range_decoder.decode(ops, input),
                vegetation_chance_decoder.decode(ops, input),
                xz_radius_decoder.decode(ops, input),
                extra_edge_column_chance_decoder.decode(ops, input),
            )
        }),
        Arc::new(move |ops: &Ops| -> Vec<Ops::Output> {
            vec![
                ops.create_string("replaceable".to_string()),
                ops.create_string("ground_state".to_string()),
                ops.create_string("vegetation_feature".to_string()),
                ops.create_string("surface".to_string()),
                ops.create_string("depth".to_string()),
                ops.create_string("extra_bottom_block_chance".to_string()),
                ops.create_string("vertical_range".to_string()),
                ops.create_string("vegetation_chance".to_string()),
                ops.create_string("xz_radius".to_string()),
                ops.create_string("extra_edge_column_chance".to_string()),
            ]
        }),
    );

    map_codec::codec_of(map_codec::of(
        encode,
        decode.clone(),
        format!("RecordCodec[{:?}]", decode),
    ))
}

impl FeatureConfiguration for VegetationPatchConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::stateproviders::block_state_provider::simple;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::float_format::java_float_equals;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A `RegistryAccess` with:
    /// - a block registry holding `stone`/`moss_block`, the `minecraft:moss_replaceable`
    ///   tag bound to both;
    /// - a configured-feature registry holding `minecraft:moss_vegetation`.
    fn access() -> RegistryAccess {
        let mut blocks = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        let stone = blocks.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:stone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let moss = blocks.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:moss_block"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        blocks.bind_tags(vec![(
            rivet_registry::TagKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:moss_replaceable"),
            ),
            vec![stone, moss],
        )]);
        let blocks = blocks.freeze();

        let mut features = RegistryBuilder::new(&configured_feature_registry_key());
        features.register(
            &ResourceKey::create(
                &configured_feature_registry_key(),
                Identifier::parse("minecraft:moss_vegetation"),
            ),
            Arc::new(ConfiguredFeatureErased {
                feature: crate::levelgen::feature::FeatureId::new(0),
                // A placeholder configuration value (never decoded/encoded: the
                // holder is a reference).
                config: Arc::new(super::tests::PlaceholderConfig),
            }),
            RegistrationInfo::BUILT_IN,
        );
        let features = features.freeze();

        // `PlacedFeature.CODEC` is a `RegistryFileCodec` over the placed-feature
        // registry whose decode requires the registry to exist (even for the
        // inline direct form); an empty registry suffices here because the
        // fixture's `vegetation_feature` is an inline `{"feature", "placement"}`
        // map, never a placed-feature identifier.
        let placed = RegistryBuilder::new(&placed_feature_registry_key());
        let placed = placed.freeze();

        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
                Box::new(blocks) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/configured_feature",
                )),
                Box::new(features) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(Identifier::with_default_namespace(
                    "worldgen/placed_feature",
                )),
                Box::new(placed) as AnyBox,
            ),
        ])
    }

    /// A do-nothing `FeatureConfiguration` placeholder for the registry's
    /// configured-feature value.
    #[derive(Debug)]
    struct PlaceholderConfig;
    impl FeatureConfiguration for PlaceholderConfig {}

    /// A two-element direct holder set over the test block registry, resolved
    /// through the SAME access the ops use.
    fn two_block_set(access: &RegistryAccess) -> HolderSet<BlockType> {
        let registry = RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry");
        HolderSet::direct(vec![
            Holder::reference(registry.registry_id(), 0),
            Holder::reference(registry.registry_id(), 1),
        ])
    }

    /// The `minecraft:moss_vegetation` configured feature as a reference holder
    /// through the SAME access the ops use.
    fn moss_vegetation(access: &RegistryAccess) -> Holder<ConfiguredFeatureErased> {
        let registry = RegistryAccess::lookup(access, &configured_feature_registry_key())
            .expect("configured feature registry");
        Holder::reference(registry.registry_id(), 0)
    }

    fn moss_block_state() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:moss_block").unwrap())
    }

    fn sample_config(access: &RegistryAccess) -> VegetationPatchConfiguration {
        VegetationPatchConfiguration::new(
            two_block_set(access),
            Arc::new(simple(moss_block_state())),
            Holder::direct(PlacedFeature::new(moss_vegetation(access), Vec::new())),
            CaveSurface::Floor,
            IntProvider::Uniform(UniformInt::of(1, 1)),
            0.0,
            5,
            0.8,
            IntProvider::Uniform(UniformInt::of(4, 7)),
            0.3,
        )
    }

    #[test]
    fn codec_round_trip() {
        let access = access();
        let config = sample_config(&access);
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = vegetation_patch_configuration_codec::<TestOps>();
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "replaceable": ["minecraft:stone", "minecraft:moss_block"],
                "ground_state": {
                    "type": "minecraft:simple_state_provider",
                    "state": {"Name": "minecraft:moss_block"},
                },
                "vegetation_feature": {
                    "feature": "minecraft:moss_vegetation",
                    "placement": [],
                },
                "surface": "floor",
                "depth": {"type": "minecraft:uniform", "min_inclusive": 1, "max_inclusive": 1},
                "extra_bottom_block_chance": 0.0,
                "vertical_range": 5,
                "vegetation_chance": 0.8,
                "xz_radius": {"type": "minecraft:uniform", "min_inclusive": 4, "max_inclusive": 7},
                "extra_edge_column_chance": 0.3,
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.replaceable, config.replaceable);
        assert_eq!(decoded.surface, CaveSurface::Floor);
        assert_eq!(decoded.depth, config.depth);
        assert_eq!(decoded.vertical_range, 5);
        assert_eq!(decoded.xz_radius, config.xz_radius);
        assert!(java_float_equals(decoded.extra_bottom_block_chance, 0.0));
        assert!(java_float_equals(decoded.vegetation_chance, 0.8));
        assert!(java_float_equals(decoded.extra_edge_column_chance, 0.3));
        // The inline feature round-trips as a direct holder whose configured
        // feature is a reference into the configured-feature registry.
        match &decoded.vegetation_feature {
            Holder::Direct(pf) => match &pf.feature {
                Holder::Reference { id, .. } => assert_eq!(*id, 0),
                other => panic!("expected a reference configured feature, got {other:?}"),
            },
            other => panic!("expected an inline placed feature, got {other:?}"),
        }
    }

    #[test]
    fn codec_decodes_the_pinned_moss_patch_fixture() {
        // The real `moss_patch.json` config: every field present, `replaceable`
        // in tag form (`#minecraft:moss_replaceable`), the vegetation feature
        // inlined, `surface` as the enum string.
        let fixture = json!({
            "depth": 1,
            "extra_bottom_block_chance": 0.0,
            "extra_edge_column_chance": 0.3,
            "ground_state": {
                "type": "minecraft:simple_state_provider",
                "state": {"Name": "minecraft:moss_block"}
            },
            "replaceable": "#minecraft:moss_replaceable",
            "surface": "floor",
            "vegetation_chance": 0.8,
            "vegetation_feature": {
                "feature": "minecraft:moss_vegetation",
                "placement": []
            },
            "vertical_range": 5,
            "xz_radius": {"type": "minecraft:uniform", "max_inclusive": 7, "min_inclusive": 4}
        });
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = vegetation_patch_configuration_codec::<TestOps>();
        let decoded = codec
            .parse(&ops, &fixture)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded.depth, IntProvider::Constant(ConstantInt::of(1)));
        assert_eq!(decoded.vertical_range, 5);
        assert_eq!(decoded.surface, CaveSurface::Floor);
        assert!(java_float_equals(decoded.extra_bottom_block_chance, 0.0));
        assert!(java_float_equals(decoded.vegetation_chance, 0.8));
        assert!(java_float_equals(decoded.extra_edge_column_chance, 0.3));
        // The tag form decodes to a Named set; re-encoding writes the tag key
        // back out, and the inline feature/`"surface"`/empty placement round-trip.
        let re_encoded = codec
            .encode_start(&ops, &decoded)
            .result()
            .expect("re-encode should succeed")
            .clone();
        assert_eq!(
            re_encoded.get("replaceable"),
            Some(&json!("#minecraft:moss_replaceable"))
        );
        assert_eq!(
            re_encoded.get("vegetation_feature"),
            Some(&json!({"feature": "minecraft:moss_vegetation", "placement": []}))
        );
        assert_eq!(re_encoded.get("surface"), Some(&json!("floor")));
    }

    #[test]
    fn codec_requires_all_fields() {
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = vegetation_patch_configuration_codec::<TestOps>();
        // `fieldOf("replaceable")` is required.
        let missing = json!({
            "ground_state": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:moss_block"}},
            "vegetation_feature": {"feature": "minecraft:moss_vegetation", "placement": []},
            "surface": "floor",
            "depth": 1,
            "extra_bottom_block_chance": 0.0,
            "vertical_range": 5,
            "vegetation_chance": 0.8,
            "xz_radius": 5,
            "extra_edge_column_chance": 0.3,
        });
        let result = codec.parse(&ops, &missing);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key replaceable"), "got: {msg}");
    }

    #[test]
    fn codec_rejects_out_of_range_values() {
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = vegetation_patch_configuration_codec::<TestOps>();
        // `vertical_range` above [1, 256].
        let mut bad = json!({
            "replaceable": [],
            "ground_state": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:moss_block"}},
            "vegetation_feature": {"feature": "minecraft:moss_vegetation", "placement": []},
            "surface": "floor",
            "depth": 1,
            "extra_bottom_block_chance": 0.0,
            "vertical_range": 257,
            "vegetation_chance": 0.8,
            "xz_radius": 5,
            "extra_edge_column_chance": 0.3,
        });
        let result = codec.parse(&ops, &bad);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert_eq!(msg, "Value 257 outside of range [1:256]");
        // `vegetation_chance` below [0.0, 1.0].
        bad["vertical_range"] = json!(5);
        bad["vegetation_chance"] = json!(-0.1);
        assert!(codec.parse(&ops, &bad).is_error());
    }

    #[test]
    fn non_empty_placement_list_errors_via_the_stub() {
        // The `PlacementModifier.CODEC` STUB errors on any element, so a
        // non-empty `"placement"` list fails loudly instead of mis-decoding.
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = vegetation_patch_configuration_codec::<TestOps>();
        let fixture = json!({
            "replaceable": "#minecraft:moss_replaceable",
            "ground_state": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:moss_block"}},
            "vegetation_feature": {"feature": "minecraft:moss_vegetation", "placement": [{"type": "minecraft:in_square"}]},
            "surface": "floor",
            "depth": 1,
            "extra_bottom_block_chance": 0.0,
            "vertical_range": 5,
            "vegetation_chance": 0.8,
            "xz_radius": 5,
            "extra_edge_column_chance": 0.3,
        });
        let result = codec.parse(&ops, &fixture);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("placement modifier decoding defers"),
            "got: {msg}"
        );
    }

    #[test]
    fn inline_configured_feature_map_errors_via_the_stub() {
        // `ConfiguredFeature.DIRECT_CODEC` defers (issue #126), so an inline
        // configured-feature map errors rather than mis-decoding. This also
        // exercises the reference form's sibling: `"feature"` as a map.
        let access = access();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let codec = vegetation_patch_configuration_codec::<TestOps>();
        let fixture = json!({
            "replaceable": "#minecraft:moss_replaceable",
            "ground_state": {"type": "minecraft:simple_state_provider", "state": {"Name": "minecraft:moss_block"}},
            "vegetation_feature": {
                "feature": {"type": "minecraft:moss_vegetation", "config": {}},
                "placement": []
            },
            "surface": "floor",
            "depth": 1,
            "extra_bottom_block_chance": 0.0,
            "vertical_range": 5,
            "vegetation_chance": 0.8,
            "xz_radius": 5,
            "extra_edge_column_chance": 0.3,
        });
        let result = codec.parse(&ops, &fixture);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("inline configured feature decoding defers"),
            "got: {msg}"
        );
    }
}
