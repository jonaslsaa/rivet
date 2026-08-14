//! Port of `net.minecraft.world.level.levelgen.feature.rootplacers.
//! RootPlacer` (abstract class, 26.2) — the dispatch root of the
//! root-placer framework.
//!
//! Java is the abstract base of the (single, 26.2) concrete root placer, with
//! the dispatch codec `CODEC`:
//!
//! ```text
//! CODEC = BuiltInRegistries.ROOT_PLACER_TYPE.byNameCodec()
//!     .dispatch(RootPlacer::type, RootPlacerType::codec);
//! ```
//!
//! The port splits identity from behavior the same way `FoliagePlacer` and
//! `TrunkPlacer` do: [`RootPlacer`] is the behavior contract generic over the
//! random source, and [`ErasedRootPlacer`] is the object-safe carrier the placer
//! codec graph stores each placer as (`Arc<dyn ErasedRootPlacer>`). Every
//! concrete placer implements `RootPlacer`, so the erased carrier is
//! blanket-derived; `as_any` is the explicit downcast seam the dispatch codec
//! uses on encode.
//!
//! The `BiConsumer<BlockPos, BlockState>` root setter Java passes in is a
//! `&mut dyn FnMut(&BlockPos, BlockState)` closure (the `RootSystemFeature`
//! captures the level write). Java `protected` instance helpers
//! virtual-dispatch on `this` (`canPlaceRoot`, `placeRoot`,
//! `getPotentiallyWaterloggedState`, `getTrunkOrigin`), so they are default
//! trait methods; the `protected final` fields are read through accessor
//! methods. The `"trunk_offset_y"` field is `IntProviders.CODEC` (the plain
//! dispatch), `"root_provider"` the `BlockStateProvider.CODEC` dispatch, and
//! `"above_root_placement"` the optional `AboveRootPlacement.CODEC` field.
//!
//! The `root_provider` field therefore carries the `RegistryOpsLookup` ops
//! surface (`BlockStateProvider.CODEC`'s requirement), so unlike the foliage/
//! trunk dispatches this codec factory is bounded over `RegistryOpsLookup`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::rootplacers::above_root_placement::{
    AboveRootPlacement, above_root_placement_map_codec,
};
use crate::levelgen::feature::rootplacers::root_placer_type::{
    RootPlacerTypeId, root_placer_type_by_name,
};
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec, block_state_provider_get_state,
};
use crate::levelgen::feature::tree_feature::valid_tree_pos;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::BlockPos;
use rivet_registry::fluid_id::FluidId;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{
    self, Instance, RecordCodecBuilder,
};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `rootPlacerParts(Instance)` — the shared three-field record group
/// (`IntProviders.CODEC.fieldOf("trunk_offset_y")`,
/// `BlockStateProvider.CODEC.fieldOf("root_provider")`, and
/// `AboveRootPlacement.CODEC.optionalFieldOf("above_root_placement")`), the
/// `P3` every concrete placer codec starts from.
///
/// The group is heterogeneous (IntProvider, provider, Optional), which the
/// port's `record_builder` `Group3` expresses directly; each concrete placer
/// codec chains its own trailing field onto this group (`MangroveRootPlacer`
/// adds `"mangrove_root_placement"`).
#[allow(clippy::type_complexity)]
pub(crate) fn root_placer_parts<P, Ops>(
    instance: &Instance<P, Ops>,
    get_trunk_offset_y: Arc<dyn Fn(&P) -> IntProvider + Send + Sync>,
    get_root_provider: Arc<dyn Fn(&P) -> Arc<dyn ErasedBlockStateProvider> + Send + Sync>,
    get_above_root_placement: Arc<dyn Fn(&P) -> Option<AboveRootPlacement> + Send + Sync>,
) -> record_builder::Group3<
    P,
    Ops,
    IntProvider,
    Arc<dyn ErasedBlockStateProvider>,
    Option<AboveRootPlacement>,
>
where
    P: 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    instance
        .group(RecordCodecBuilder::of_named(
            get_trunk_offset_y,
            "trunk_offset_y".to_string(),
            int_provider_codec::<Ops>(),
        ))
        .and(RecordCodecBuilder::of_named(
            get_root_provider,
            "root_provider".to_string(),
            block_state_provider_codec::<Ops>(),
        ))
        .and(RecordCodecBuilder::of(
            get_above_root_placement,
            codec::optional_field(
                "above_root_placement".to_string(),
                map_codec::codec_of(above_root_placement_map_codec::<Ops>()),
                false,
            ),
        ))
}

/// `net.minecraft.world.level.levelgen.feature.rootplacers.RootPlacer` — the
/// behavior contract of a root placer (Java's abstract `placeRoots` +
/// `type()`).
///
/// The Java `protected` instance helpers virtual-dispatch on `this`, so they
/// are default trait methods here exactly as Java's inheritance reaches them:
/// `canPlaceRoot`, `placeRoot`, `getPotentiallyWaterloggedState`,
/// `getTrunkOrigin`. The `protected final` fields are read through accessor
/// methods (`trunk_offset_y`, `root_provider`, `above_root_placement`).
pub trait RootPlacer: Any + Debug + Send + Sync + 'static {
    /// `RootPlacer.type()` — the registry-held `RootPlacerType<?>` identity.
    fn type_id(&self) -> RootPlacerTypeId;

    /// `RootPlacer.placeRoots(...)` — the abstract per-placer root placement,
    /// returning whether the whole root system could be placed.
    fn place_roots<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        root_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        origin: &BlockPos,
        trunk_origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> bool;

    /// `RootPlacer.canPlaceRoot(LevelSimulatedReader, BlockPos)` —
    /// `TreeFeature.validTreePos`.
    fn can_place_root(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        valid_tree_pos(level, pos)
    }

    /// `RootPlacer.placeRoot(...)` — place the root at `pos` when placeable,
    /// plus the above-root block when the chance passes and the position above
    /// is air.
    fn place_root<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        root_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        pos: &BlockPos,
        config: &TreeConfiguration,
    ) {
        if self.can_place_root(level, pos) {
            let state = block_state_provider_get_state(&self.root_provider(), level, random, pos);
            root_setter(pos, self.get_potentially_waterlogged_state(level, pos, state));
            if let Some(above_placement) = self.above_root_placement() {
                let above = pos.above();
                if random.next_float() < above_placement.above_root_placement_chance
                    && level.is_state_at_position(&above, &|s: &BlockState| s.is_air())
                {
                    let above_state = block_state_provider_get_state(
                        &above_placement.above_root_provider,
                        level,
                        random,
                        &above,
                    );
                    root_setter(
                        &above,
                        self.get_potentially_waterlogged_state(level, &above, above_state),
                    );
                }
            }
        }
    }

    /// `RootPlacer.getPotentiallyWaterloggedState(...)` — waterlog the state
    /// when it carries `WATERLOGGED` and the position holds water.
    fn get_potentially_waterlogged_state(
        &self,
        level: &dyn WorldGenLevel,
        pos: &BlockPos,
        state: BlockState,
    ) -> BlockState {
        if state.has_property(BlockStateProperties::WATERLOGGED) {
            let waterlogged = level.is_fluid_at_position(pos, &|fluid: &FluidId| {
                *fluid == FluidId::WATER
            });
            state
                .set_value(BlockStateProperties::WATERLOGGED, waterlogged)
                .expect("RootPlacer waterlogged a state that has the property")
        } else {
            state
        }
    }

    /// `RootPlacer.getTrunkOrigin(BlockPos, RandomSource)` — the trunk origin
    /// is `origin.above(this.trunkOffsetY.sample(random))`.
    fn get_trunk_origin<R: RandomSource>(&self, origin: &BlockPos, random: &mut R) -> BlockPos {
        origin.above_steps(self.trunk_offset_y().sample(random))
    }

    /// `this.trunkOffsetY` — the protected trunk-offset `IntProvider`.
    fn trunk_offset_y(&self) -> &IntProvider;

    /// `this.rootProvider` — the protected root `BlockStateProvider`.
    fn root_provider(&self) -> &Arc<dyn ErasedBlockStateProvider>;

    /// `this.aboveRootPlacement` — the protected optional above-root placement.
    fn above_root_placement(&self) -> &Option<AboveRootPlacement>;

    /// `as_any` — the downcast seam (Java's erased `RootPlacer` cast).
    fn as_any(&self) -> &dyn Any;
}

/// The object-safe carrier the codec graph stores each placer as — the
/// dispatch identity plus the `dyn`-compatible surface. Every `RootPlacer`
/// implements it via the blanket impl.
pub trait ErasedRootPlacer: Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> RootPlacerTypeId;

    /// `as_any` — the downcast seam over the erased carrier.
    fn as_any(&self) -> &dyn Any;
}

impl<P: RootPlacer + ?Sized> ErasedRootPlacer for P {
    fn type_id(&self) -> RootPlacerTypeId {
        RootPlacer::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        RootPlacer::as_any(self)
    }
}

/// `RootPlacer.CODEC` — the by-name dispatch codec, as the ops-generic
/// `root_placer_codec::<Ops>()` factory.
pub fn root_placer_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Arc<dyn ErasedRootPlacer>, Ops>> {
    // `BuiltInRegistries.ROOT_PLACER_TYPE.byNameCodec().dispatch(...)`.
    map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        RootPlacerTypeId,
        Arc<dyn ErasedRootPlacer>,
        Ops,
    >(
        "type",
        root_placer_type_by_name_codec::<Ops>(),
        Arc::new(|p: &Arc<dyn ErasedRootPlacer>| {
            DataResult::success(ErasedRootPlacer::type_id(&**p))
        }),
        codec_for_type(),
    ))
}

/// `RootPlacerType::codec` — resolve a `RootPlacerTypeId` to its
/// `MapCodec<Arc<dyn ErasedRootPlacer>>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static + RegistryOpsLookup>(
) -> key_dispatch_codec::CodecFn<RootPlacerTypeId, Arc<dyn ErasedRootPlacer>, Ops> {
    Arc::new(move |k: &RootPlacerTypeId| {
        if *k == crate::levelgen::feature::rootplacers::root_placer_type::RootPlacerTypes::MANGROVE_ROOT_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::rootplacers::mangrove_root_placer::MangroveRootPlacer, Ops>(
                crate::levelgen::feature::rootplacers::mangrove_root_placer::mangrove_root_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|m: &crate::levelgen::feature::rootplacers::mangrove_root_placer::MangroveRootPlacer| {
                    Arc::new(m.clone()) as Arc<dyn ErasedRootPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedRootPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::rootplacers::mangrove_root_placer::MangroveRootPlacer>()
                        .unwrap_or_else(|| panic!("root-placer dispatch produced a non-mangrove value"))
                        .clone()
                }),
            ))
        } else {
            DataResult::error(format!(
                "Root placer type '{}' is not ported",
                k.location
            ))
        }
    })
}

/// Lift a concrete placer's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedRootPlacer>>` — Java's
/// `MapCodec<? extends RootPlacer>` variance, via xmap (the same lift every
/// dispatch file defines).
#[allow(clippy::type_complexity)]
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> Arc<dyn ErasedRootPlacer> + Send + Sync>,
    unwrap: Arc<dyn Fn(&Arc<dyn ErasedRootPlacer>) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedRootPlacer>, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.ROOT_PLACER_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key()
/// .identifier())`, with the same unknown-key error shape every by-name codec
/// reproduces (`Registries.ROOT_PLACER_TYPE` = `createRegistryKey(
/// "worldgen/root_placer_type")`).
#[allow(clippy::doc_lazy_continuation)]
pub fn root_placer_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<RootPlacerTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, RootPlacerTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match root_placer_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/root_placer_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &RootPlacerTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::rootplacers::root_placer_type::RootPlacerTypes;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn by_name_codec_round_trips_mangrove() {
        let codec = root_placer_type_by_name_codec::<JsonOps>();
        let input = json!(RootPlacerTypes::MANGROVE_ROOT_PLACER.location);
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &input)
            .result()
            .expect("decode should succeed");
        assert_eq!(decoded, RootPlacerTypes::MANGROVE_ROOT_PLACER);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn by_name_codec_rejects_unknown_location() {
        let codec = root_placer_type_by_name_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:not_a_placer"));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/root_placer_type]: minecraft:not_a_placer"),
            "got: {msg}"
        );
    }

    /// A probe placer exposing the `get_trunk_origin` formula with a fixed
    /// offset.
    struct Probe {
        offset: IntProvider,
    }
    impl RootPlacer for Probe {
        fn type_id(&self) -> RootPlacerTypeId {
            RootPlacerTypes::MANGROVE_ROOT_PLACER
        }
        fn place_roots<R: RandomSource>(
            &self,
            _level: &dyn WorldGenLevel,
            _setter: &mut dyn FnMut(&BlockPos, BlockState),
            _random: &mut R,
            _origin: &BlockPos,
            _trunk_origin: &BlockPos,
            _config: &TreeConfiguration,
        ) -> bool {
            true
        }
        fn trunk_offset_y(&self) -> &IntProvider {
            &self.offset
        }
        fn root_provider(&self) -> &Arc<dyn ErasedBlockStateProvider> {
            panic!("unused by get_trunk_origin")
        }
        fn above_root_placement(&self) -> &Option<AboveRootPlacement> {
            &None
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn get_trunk_origin_is_origin_above_offset() {
        let origin = BlockPos::new(1, 2, 3);
        let probe = Probe {
            offset: IntProvider::Constant(rivet_util::valueproviders::constant_int::ConstantInt::of(
                4,
            )),
        };
        let mut random = rivet_util::random::LegacyRandomSource::of(0);
        assert_eq!(
            probe.get_trunk_origin(&origin, &mut random),
            BlockPos::new(1, 6, 3)
        );
    }
}
