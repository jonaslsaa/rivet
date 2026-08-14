//! Port of `net.minecraft.world.level.levelgen.feature.trunkplacers.
//! TrunkPlacer` (abstract class, 26.2) — the dispatch root of the
//! trunk-placer framework.
//!
//! Java is the abstract base of the nine concrete trunk placers, with the
//! dispatch codec `CODEC`:
//!
//! ```text
//! CODEC = BuiltInRegistries.TRUNK_PLACER_TYPE.byNameCodec()
//!     .dispatch(TrunkPlacer::type, TrunkPlacerType::codec);
//! ```
//!
//! The port splits identity from behavior the same way `BlockStateProvider`
//! and `FoliagePlacer` do: [`TrunkPlacer`] is the behavior contract generic
//! over the random source, and [`ErasedTrunkPlacer`] is the object-safe carrier
//! the placer codec graph stores each placer as (`Arc<dyn ErasedTrunkPlacer>`).
//! Every concrete placer implements `TrunkPlacer`, so the erased carrier is
//! blanket-derived; `as_any` is the explicit downcast seam the dispatch codec
//! uses on encode.
//!
//! The `BiConsumer<BlockPos, BlockState>` trunk setter Java passes in is a
//! `&mut dyn FnMut(&BlockPos, BlockState)` closure (the `TreeFeature.place`
//! anonymous class captures the `trunks` set and writes through the level).
//! Java `protected` instance helpers virtual-dispatch on `this`
//! (`placeLog`, `placeLogIfFree`, `validTreePos`, `isFree`), so they are
//! default trait methods; Java `static` `placeBelowTrunkBlock` is a free
//! function.
//!
//! The `"logs"` tag read (`isFree`'s `state.is(BlockTags.LOGS)`) uses the
//! generated `BlockState::is_in_tag("minecraft:logs")` table, and the
//! below-trunk provider's optional state resolves through the
//! `block_state_provider_get_optional_state` dispatch (RivetTodo #181 codegen
//! surface, same as the `get_state` form).
//!
//! The trunk placer fields are plain ints, but the dispatch carries the
//! `RegistryOpsLookup` bound anyway: `UpwardsBranchingTrunkPlacer`'s codec
//! embeds a `RegistryCodecs.homogeneousList(Registries.BLOCK)` holder set, and
//! the `TreeConfiguration` codec that consumes this dispatch already embeds the
//! `BlockStateProvider` dispatch (registry-bound). So the trunk dispatch is
//! bound exactly like `BlockStateProvider.CODEC`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer::FoliageAttachment;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    block_state_provider_get_optional_state, block_state_provider_get_state,
};
use crate::levelgen::feature::tree_feature::valid_tree_pos;
use crate::levelgen::feature::trunkplacers::trunk_placer_type::{
    TrunkPlacerTypeId, trunk_placer_type_by_name,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::MutableBlockPos;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::RecordCodecBuilder;
use rivet_util::RandomSource;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `TrunkPlacer.MAX_HEIGHT` — the public `80` bound.
pub const MAX_HEIGHT: i32 = 80;

/// `TrunkPlacer.trunkPlacerParts(Instance)` — the shared three-field record
/// group (`Codec.intRange(0, 32).fieldOf("base_height")`,
/// `Codec.intRange(0, 24).fieldOf("height_rand_a")`, and
/// `Codec.intRange(0, 24).fieldOf("height_rand_b")`), the `P3` every concrete
/// placer codec starts from. The fixed 3-arity tuple makes the ordering part of
/// the type, mirroring Java's `P3<Mu<P>, Integer, Integer, Integer>` (the same
/// shape `foliage_placer_parts` uses for its `P2`).
#[allow(clippy::type_complexity)] // the fixed 3-tuple of record builders mirrors Java's `P3`.
pub(crate) fn trunk_placer_parts<P, Ops>(
    get_base_height: Arc<dyn Fn(&P) -> i32 + Send + Sync>,
    get_height_rand_a: Arc<dyn Fn(&P) -> i32 + Send + Sync>,
    get_height_rand_b: Arc<dyn Fn(&P) -> i32 + Send + Sync>,
) -> (
    RecordCodecBuilder<P, Ops, i32>,
    RecordCodecBuilder<P, Ops, i32>,
    RecordCodecBuilder<P, Ops, i32>,
)
where
    P: 'static,
    Ops: DynamicOps + 'static,
{
    (
        RecordCodecBuilder::of(
            get_base_height,
            codec::field_of(codec::int_range::<Ops>(0, 32), "base_height".to_string()),
        ),
        RecordCodecBuilder::of(
            get_height_rand_a,
            codec::field_of(codec::int_range::<Ops>(0, 24), "height_rand_a".to_string()),
        ),
        RecordCodecBuilder::of(
            get_height_rand_b,
            codec::field_of(codec::int_range::<Ops>(0, 24), "height_rand_b".to_string()),
        ),
    )
}

/// `net.minecraft.world.level.levelgen.feature.trunkplacers.TrunkPlacer` — the
/// behavior contract of a trunk placer (Java's abstract
/// `placeTrunk`/`getBaseHeight` + `type()`).
///
/// The Java `protected` instance helpers virtual-dispatch on `this`, so they
/// are default trait methods here exactly as Java's inheritance reaches them:
/// `getTreeHeight`, `placeLog` (both overloads), `placeLogIfFree`,
/// `validTreePos`, `isFree`.
pub trait TrunkPlacer: Any + Debug + Send + Sync + 'static {
    /// `TrunkPlacer.type()` — the registry-held `TrunkPlacerType<?>` identity.
    fn type_id(&self) -> TrunkPlacerTypeId;

    /// `TrunkPlacer.placeTrunk(...)` — the abstract per-placer trunk placement,
    /// returning the `FoliageAttachment` list `TreeFeature.doPlace` feeds to the
    /// foliage placer.
    fn place_trunk<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        tree_height: i32,
        origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> Vec<FoliageAttachment>;

    /// `TrunkPlacer.getBaseHeight()` — `this.baseHeight`.
    fn get_base_height(&self) -> i32;

    /// `TrunkPlacer.getTreeHeight(RandomSource)` —
    /// `this.baseHeight + random.nextInt(this.heightRandA + 1) +
    /// random.nextInt(this.heightRandB + 1)`.
    fn get_tree_height<R: RandomSource>(&self, random: &mut R) -> i32 {
        self.base_height()
            .wrapping_add(random.next_int_bound(self.height_rand_a().wrapping_add(1)))
            .wrapping_add(random.next_int_bound(self.height_rand_b().wrapping_add(1)))
    }

    /// `TrunkPlacer.placeLog(WorldGenLevel, BiConsumer, RandomSource, BlockPos,
    /// TreeConfiguration)` — the no-modifier overload delegating with the
    /// identity modifier.
    fn place_log<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        pos: &BlockPos,
        config: &TreeConfiguration,
    ) -> bool {
        self.place_log_with_modifier(level, trunk_setter, random, pos, config, &|s| s)
    }

    /// `TrunkPlacer.placeLog(..., Function<BlockState, BlockState>)` — place a
    /// trunk log when `validTreePos`, applying the state modifier to the
    /// provider's state.
    fn place_log_with_modifier<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        pos: &BlockPos,
        config: &TreeConfiguration,
        state_modifier: &dyn Fn(BlockState) -> BlockState,
    ) -> bool {
        if self.valid_tree_pos(level, pos) {
            let state =
                block_state_provider_get_state(config.trunk_provider.as_ref(), level, random, pos);
            trunk_setter(pos, state_modifier(state));
            true
        } else {
            false
        }
    }

    /// `TrunkPlacer.placeLogIfFree(..., BlockPos.MutableBlockPos, ...)` —
    /// `placeLog` when `isFree`.
    fn place_log_if_free<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        pos: &MutableBlockPos,
        config: &TreeConfiguration,
    ) {
        if self.is_free(level, &pos.immutable()) {
            self.place_log(level, trunk_setter, random, &pos.immutable(), config);
        }
    }

    /// `TrunkPlacer.validTreePos(WorldGenLevel, BlockPos)` —
    /// `TreeFeature.validTreePos`.
    fn valid_tree_pos(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        valid_tree_pos(level, pos)
    }

    /// `TrunkPlacer.isFree(WorldGenLevel, BlockPos)` — `validTreePos` or the
    /// position's state is in `BlockTags.LOGS`.
    fn is_free(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        self.valid_tree_pos(level, pos)
            || level
                .is_state_at_position(pos, &|state: &BlockState| state.is_in_tag("minecraft:logs"))
    }

    /// `this.baseHeight` — the protected base-height int.
    fn base_height(&self) -> i32;

    /// `this.heightRandA` — the protected height-rand-a int.
    fn height_rand_a(&self) -> i32;

    /// `this.heightRandB` — the protected height-rand-b int.
    fn height_rand_b(&self) -> i32;

    /// `as_any` — the downcast seam (Java's erased `TrunkPlacer` cast).
    fn as_any(&self) -> &dyn Any;
}

/// The object-safe carrier the codec graph stores each placer as — the
/// dispatch identity plus the `dyn`-compatible surface. Every `TrunkPlacer`
/// implements it via the blanket impl.
pub trait ErasedTrunkPlacer: Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> TrunkPlacerTypeId;

    /// `as_any` — the downcast seam over the erased carrier.
    fn as_any(&self) -> &dyn Any;
}

impl<P: TrunkPlacer + ?Sized> ErasedTrunkPlacer for P {
    fn type_id(&self) -> TrunkPlacerTypeId {
        TrunkPlacer::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        TrunkPlacer::as_any(self)
    }
}

/// `TrunkPlacer.placeBelowTrunkBlock(...)` (protected static) — place the
/// `below_trunk_provider`'s optional state at `pos` via the trunk setter (only
/// when the provider yields a state).
pub fn place_below_trunk_block<R: RandomSource>(
    level: &dyn WorldGenLevel,
    trunk_setter: &mut dyn FnMut(&BlockPos, BlockState),
    random: &mut R,
    pos: &BlockPos,
    config: &TreeConfiguration,
) {
    if let Some(block_below_trunk) = block_state_provider_get_optional_state(
        config.below_trunk_provider.as_ref(),
        level,
        random,
        pos,
    ) {
        trunk_setter(pos, block_below_trunk);
    }
}

/// `TrunkPlacer.CODEC` — the by-name dispatch codec, as the ops-generic
/// `trunk_placer_codec::<Ops>()` factory.
pub fn trunk_placer_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Arc<dyn ErasedTrunkPlacer>, Ops>> {
    // `BuiltInRegistries.TRUNK_PLACER_TYPE.byNameCodec().dispatch(...)`.
    map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        TrunkPlacerTypeId,
        Arc<dyn ErasedTrunkPlacer>,
        Ops,
    >(
        "type",
        trunk_placer_type_by_name_codec::<Ops>(),
        Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
            DataResult::success(ErasedTrunkPlacer::type_id(&**p))
        }),
        codec_for_type(),
    ))
}

/// `TrunkPlacerType::codec` — resolve a `TrunkPlacerTypeId` to its
/// `MapCodec<Arc<dyn ErasedTrunkPlacer>>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> key_dispatch_codec::CodecFn<TrunkPlacerTypeId, Arc<dyn ErasedTrunkPlacer>, Ops> {
    use crate::levelgen::feature::trunkplacers::trunk_placer_type::TrunkPlacerTypes;
    Arc::new(move |k: &TrunkPlacerTypeId| {
        if *k == TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::straight_trunk_placer::StraightTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::straight_trunk_placer::straight_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|s: &crate::levelgen::feature::trunkplacers::straight_trunk_placer::StraightTrunkPlacer| {
                    Arc::new(s.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::straight_trunk_placer::StraightTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-straight value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::FORKING_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::forking_trunk_placer::ForkingTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::forking_trunk_placer::forking_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|f: &crate::levelgen::feature::trunkplacers::forking_trunk_placer::ForkingTrunkPlacer| {
                    Arc::new(f.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::forking_trunk_placer::ForkingTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-forking value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::GIANT_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::giant_trunk_placer::GiantTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::giant_trunk_placer::giant_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|g: &crate::levelgen::feature::trunkplacers::giant_trunk_placer::GiantTrunkPlacer| {
                    Arc::new(g.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::giant_trunk_placer::GiantTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-giant value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::mega_jungle_trunk_placer::MegaJungleTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::mega_jungle_trunk_placer::mega_jungle_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|m: &crate::levelgen::feature::trunkplacers::mega_jungle_trunk_placer::MegaJungleTrunkPlacer| {
                    Arc::new(m.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::mega_jungle_trunk_placer::MegaJungleTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-mega-jungle value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::dark_oak_trunk_placer::DarkOakTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::dark_oak_trunk_placer::dark_oak_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|d: &crate::levelgen::feature::trunkplacers::dark_oak_trunk_placer::DarkOakTrunkPlacer| {
                    Arc::new(d.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::dark_oak_trunk_placer::DarkOakTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-dark-oak value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::FANCY_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::fancy_trunk_placer::FancyTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::fancy_trunk_placer::fancy_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|f: &crate::levelgen::feature::trunkplacers::fancy_trunk_placer::FancyTrunkPlacer| {
                    Arc::new(f.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::fancy_trunk_placer::FancyTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-fancy value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::BENDING_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::bending_trunk_placer::BendingTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::bending_trunk_placer::bending_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|b: &crate::levelgen::feature::trunkplacers::bending_trunk_placer::BendingTrunkPlacer| {
                    Arc::new(b.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::bending_trunk_placer::BendingTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-bending value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::upwards_branching_trunk_placer::UpwardsBranchingTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::upwards_branching_trunk_placer::upwards_branching_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|u: &crate::levelgen::feature::trunkplacers::upwards_branching_trunk_placer::UpwardsBranchingTrunkPlacer| {
                    Arc::new(u.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::upwards_branching_trunk_placer::UpwardsBranchingTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-upwards-branching value"))
                        .clone()
                }),
            ))
        } else if *k == TrunkPlacerTypes::CHERRY_TRUNK_PLACER {
            DataResult::success(erase_map_codec::<
                crate::levelgen::feature::trunkplacers::cherry_trunk_placer::CherryTrunkPlacer,
                Ops,
            >(
                crate::levelgen::feature::trunkplacers::cherry_trunk_placer::cherry_trunk_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|c: &crate::levelgen::feature::trunkplacers::cherry_trunk_placer::CherryTrunkPlacer| {
                    Arc::new(c.clone()) as Arc<dyn ErasedTrunkPlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedTrunkPlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::trunkplacers::cherry_trunk_placer::CherryTrunkPlacer>()
                        .unwrap_or_else(|| panic!("trunk-placer dispatch produced a non-cherry value"))
                        .clone()
                }),
            ))
        } else {
            DataResult::error(format!("Trunk placer type '{}' is not ported", k.location))
        }
    })
}

/// Lift a concrete placer's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedTrunkPlacer>>` — Java's
/// `MapCodec<? extends TrunkPlacer>` variance, via xmap (the same lift every
/// dispatch file defines).
#[allow(clippy::type_complexity)]
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> Arc<dyn ErasedTrunkPlacer> + Send + Sync>,
    unwrap: Arc<dyn Fn(&Arc<dyn ErasedTrunkPlacer>) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedTrunkPlacer>, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.TRUNK_PLACER_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key()
/// .identifier())`, with the same unknown-key error shape every by-name codec
/// reproduces (`Registries.TRUNK_PLACER_TYPE` = `createRegistryKey(
/// "worldgen/trunk_placer_type")`).
#[allow(clippy::doc_lazy_continuation)]
pub fn trunk_placer_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<TrunkPlacerTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, TrunkPlacerTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match trunk_placer_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/trunk_placer_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &TrunkPlacerTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::feature::trunkplacers::trunk_placer_type::TrunkPlacerTypes;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::{
        Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey,
    };
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn by_name_codec_round_trips_each_registered_location() {
        let codec = trunk_placer_type_by_name_codec::<JsonOps>();
        for id in [
            TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER,
            TrunkPlacerTypes::FORKING_TRUNK_PLACER,
            TrunkPlacerTypes::GIANT_TRUNK_PLACER,
            TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER,
            TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER,
            TrunkPlacerTypes::FANCY_TRUNK_PLACER,
            TrunkPlacerTypes::BENDING_TRUNK_PLACER,
            TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER,
            TrunkPlacerTypes::CHERRY_TRUNK_PLACER,
        ] {
            let input = json!(id.location);
            let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
            let decoded = decoded_result.result().expect("decode should succeed");
            assert_eq!(*decoded, id);
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, decoded)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, input);
        }
    }

    #[test]
    fn by_name_codec_rejects_unknown_location() {
        let codec = trunk_placer_type_by_name_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:not_a_placer"));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/trunk_placer_type]: minecraft:not_a_placer"),
            "got: {msg}"
        );
    }

    #[test]
    fn get_tree_height_is_base_plus_rand_bounds() {
        // A probe placer exposing the base-height formula with fixed bounds.
        #[derive(Debug)]
        struct Probe;
        impl TrunkPlacer for Probe {
            fn type_id(&self) -> TrunkPlacerTypeId {
                TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER
            }
            fn place_trunk<R: RandomSource>(
                &self,
                _level: &dyn WorldGenLevel,
                _setter: &mut dyn FnMut(&BlockPos, BlockState),
                _random: &mut R,
                _tree_height: i32,
                _origin: &BlockPos,
                _config: &TreeConfiguration,
            ) -> Vec<FoliageAttachment> {
                Vec::new()
            }
            fn get_base_height(&self) -> i32 {
                3
            }
            fn base_height(&self) -> i32 {
                3
            }
            fn height_rand_a(&self) -> i32 {
                2
            }
            fn height_rand_b(&self) -> i32 {
                1
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let probe = Probe;
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // `3 + nextInt(3) + nextInt(2)`.
        let height = probe.get_tree_height(&mut random);
        assert!((3..=7).contains(&height));
        assert!(probe.get_base_height() == 3);
    }

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `minecraft:stone` registered at element id 0, so
    /// `UpwardsBranchingTrunkPlacer`'s `can_grow_through` holder set resolves
    /// (and re-encodes against the same registry instance).
    fn block_ops() -> TestOps {
        let key = rivet_registry::registries::BLOCK.clone();
        let mut builder = RegistryBuilder::<rivet_registry::registries::BlockType>::new(&key);
        builder.register(
            &ResourceKey::create(&key, Identifier::with_default_namespace("stone")),
            Arc::new(rivet_registry::registries::BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let access = RegistryAccess::from_single_registry(key, registry);
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    #[test]
    fn trunk_placer_codec_dispatch_round_trips_each_type() {
        // The by-name dispatch (`trunk_placer_codec`) end-to-end, one input per
        // registered type: decode dispatches on `"type"` to the concrete xmap
        // arm, and encode must reproduce the input byte-for-byte.
        let codec = trunk_placer_codec::<TestOps>();
        let ops = block_ops();
        let cases: Vec<(serde_json::Value, TrunkPlacerTypeId)> = vec![
            (
                json!({"type": "minecraft:straight_trunk_placer", "base_height": 7, "height_rand_a": 2, "height_rand_b": 3}),
                TrunkPlacerTypes::STRAIGHT_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:forking_trunk_placer", "base_height": 6, "height_rand_a": 4, "height_rand_b": 2}),
                TrunkPlacerTypes::FORKING_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:giant_trunk_placer", "base_height": 11, "height_rand_a": 3, "height_rand_b": 0}),
                TrunkPlacerTypes::GIANT_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:mega_jungle_trunk_placer", "base_height": 14, "height_rand_a": 4, "height_rand_b": 5}),
                TrunkPlacerTypes::MEGA_JUNGLE_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:dark_oak_trunk_placer", "base_height": 6, "height_rand_a": 2, "height_rand_b": 1}),
                TrunkPlacerTypes::DARK_OAK_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:fancy_trunk_placer", "base_height": 12, "height_rand_a": 5, "height_rand_b": 4}),
                TrunkPlacerTypes::FANCY_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:bending_trunk_placer", "base_height": 7, "height_rand_a": 3, "height_rand_b": 2, "min_height_for_leaves": 4, "bend_length": {"min_inclusive": 2, "max_inclusive": 8, "type": "minecraft:uniform"}}),
                TrunkPlacerTypes::BENDING_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:upwards_branching_trunk_placer", "base_height": 8, "height_rand_a": 3, "height_rand_b": 2, "extra_branch_steps": {"min_inclusive": 1, "max_inclusive": 4, "type": "minecraft:uniform"}, "place_branch_per_log_probability": 0.5, "extra_branch_length": {"min_inclusive": 0, "max_inclusive": 3, "type": "minecraft:uniform"}, "can_grow_through": "minecraft:stone"}),
                TrunkPlacerTypes::UPWARDS_BRANCHING_TRUNK_PLACER,
            ),
            (
                json!({"type": "minecraft:cherry_trunk_placer", "base_height": 8, "height_rand_a": 3, "height_rand_b": 2, "branch_count": {"min_inclusive": 2, "max_inclusive": 3, "type": "minecraft:uniform"}, "branch_horizontal_length": {"min_inclusive": 3, "max_inclusive": 8, "type": "minecraft:uniform"}, "branch_start_offset_from_top": {"min_inclusive": -5, "max_inclusive": -1}, "branch_end_offset_from_top": {"min_inclusive": -3, "max_inclusive": 4, "type": "minecraft:uniform"}}),
                TrunkPlacerTypes::CHERRY_TRUNK_PLACER,
            ),
        ];
        for (input, expected) in cases {
            let decoded_result = codec.parse(&ops, &input);
            let decoded = decoded_result.result().expect("decode should succeed");
            assert_eq!(ErasedTrunkPlacer::type_id(&**decoded), expected);
            let encoded = codec
                .encode_start(&ops, decoded)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, input);
        }
    }

    #[test]
    fn trunk_placer_codec_rejects_unknown_type() {
        // The hostile dispatch input: an unknown `"type"` must surface the same
        // by-name error the type-id codec produces, not silently default.
        let codec = trunk_placer_codec::<TestOps>();
        let ops = block_ops();
        let result = codec.parse(&ops, &json!({"type": "minecraft:not_a_trunk_placer"}));
        assert!(result.is_error(), "got: {:?}", result);
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/trunk_placer_type]: minecraft:not_a_trunk_placer"),
            "got: {msg}"
        );
    }

    #[test]
    fn place_below_trunk_block_is_a_no_op_when_the_provider_has_no_state() {
        // `TreeConfiguration::stub()`'s `SimpleStateProvider` always yields a
        // state, so the `placeBelowTrunkBlock` no-op branch (Java `null`) is
        // dead in the geometry tests. A `RuleBasedStateProvider` with no rules
        // and no fallback returns `None` from `getOptionalState`, exercising it.
        use crate::levelgen::feature::stateproviders::block_state_provider::ErasedBlockStateProvider;
        use crate::levelgen::feature::stateproviders::rule_based_state_provider::RuleBasedStateProvider;
        let empty: Arc<dyn ErasedBlockStateProvider> =
            Arc::new(RuleBasedStateProvider::new(None, Vec::new()));
        let stub = TreeConfiguration::stub();
        let config = TreeConfiguration {
            foliage_provider: stub.foliage_provider.clone(),
            trunk_provider: stub.trunk_provider.clone(),
            below_trunk_provider: empty,
        };
        let mut calls = 0;
        let mut setter = |_pos: &BlockPos, _state: BlockState| {
            calls += 1;
        };
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        place_below_trunk_block(
            &TestLevel,
            &mut setter,
            &mut random,
            &BlockPos::new(0, 0, 0),
            &config,
        );
        assert_eq!(calls, 0, "no optional state means nothing is placed");
    }

    struct TestLevel;

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            crate::block::blocks::Blocks::AIR.default_block_state()
        }
    }
}
