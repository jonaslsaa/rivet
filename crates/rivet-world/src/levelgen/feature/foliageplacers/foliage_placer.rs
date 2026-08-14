//! Port of `net.minecraft.world.level.levelgen.feature.foliageplacers.
//! FoliagePlacer` (abstract class, 26.2) — the dispatch root of the
//! foliage-placer framework.
//!
//! Java is the abstract base of the eleven concrete foliage placers, dispatching
//! through `CODEC` (the by-name placer-type registry codec, dispatching to each
//! type's own codec).
//!
//! The port splits identity from behavior the same way `BlockStateProvider`
//! does: [`FoliagePlacer`] is the behavior contract whose methods are generic
//! over the random source (`RandomSource` is `Sized`, not object-safe), and
//! [`ErasedFoliagePlacer`] is the object-safe carrier the placer codec graph
//! stores each placer as (`Arc<dyn ErasedFoliagePlacer>`). Every concrete
//! placer implements `FoliagePlacer`, so the erased carrier is blanket-derived;
//! `as_any` is the explicit downcast seam (Java's erased `FoliagePlacer` cast)
//! the dispatch codec uses on encode, exactly like `BlockStateProvider::as_any`.
//!
//! The `CODEC` is the ops-generic [`foliage_placer_codec::<Ops>()`] factory: a
//! `key_dispatch_codec::dispatch_map` over `"type"` whose `codec` function
//! resolves each [`FoliagePlacerTypeId`] to its concrete placer's `MapCodec`
//! (lifted to the erased carrier by the local `erase_map_codec`, the same
//! per-dispatch private helper every dispatch file defines).
//! When the owning `configurations.tree` unit lands, its `TreeConfiguration.CODEC`
//! will embed this codec for its `"foliage_placer"` field and recurse back through
//! it for the placer's `foliage_provider` (`BlockStateProvider.CODEC`), the two
//! codecs cross-referencing each other through the config record, so no
//! `codec::recursive` cycle is needed. The current stub config
//! (`configurations/tree_configuration.rs`) carries only the three
//! `BlockStateProvider` fields, so this recursion is not yet wired.
//!
//! The shared leaf-placing surface is ported on this base exactly as Java
//! defines it. Java `protected` instance helpers that virtual-dispatch on
//! `this` — `offset` (private), `shouldSkipLocationSigned`,
//! `placeLeavesRow`, `placeLeavesRowWithHangingLeavesBelow` — become default
//! trait methods, so a concrete placer's virtual dispatch reaches them exactly
//! like Java's inheritance. Java `static` helpers — `tryPlaceLeaf`,
//! `tryPlaceExtension` — become free functions.
//!
//! The `WorldGenLevel` state/fluid-test seams (`is_state_at_position`,
//! `is_fluid_at_position`, both real defaults resolving through
//! `get_block_state`, RivetTodo #399) drive the leaf placement and the
//! waterlogging decision, and the tree family's `valid_tree_pos` gate
//! (`isAir() || is(REPLACEABLE_BY_TREES)`) is shared with `TreeFeature` via
//! `crate::levelgen::feature::tree_feature::valid_tree_pos`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::foliageplacers::foliage_placer_type::{
    FoliagePlacerTypeId, foliage_placer_type_by_name,
};
use crate::levelgen::feature::stateproviders::block_state_provider::block_state_provider_get_state;
use crate::levelgen::feature::tree_feature::valid_tree_pos;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::AxisDirection;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Vec3i;
use rivet_registry::core::{Direction, Plane};
use rivet_registry::fluid_id::FluidId;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::map_codec::MapCodec;
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec_with_bounds};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `FoliagePlacer.FoliageAttachment` — the per-attachment geometry record:
/// the foliage position, its radius offset from the trunk, and the
/// double-trunk flag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoliageAttachment {
    pub pos: BlockPos,
    pub radius_offset: i32,
    pub double_trunk: bool,
}

impl FoliageAttachment {
    /// `new FoliageAttachment(BlockPos, int, boolean)` — the record constructor.
    pub fn new(pos: BlockPos, radius_offset: i32, double_trunk: bool) -> FoliageAttachment {
        FoliageAttachment {
            pos,
            radius_offset,
            double_trunk,
        }
    }
}

/// `FoliagePlacer.FoliageSetter` — the leaf-set callback interface the
/// `TreeFeature.place` anonymous class implements (`foliage.add(pos.immutable());
/// level.setBlock(...)`, `isSet` = `foliage.contains(pos)`).
pub trait FoliageSetter {
    /// `FoliageSetter.set(BlockPos, BlockState)`.
    fn set(&mut self, pos: &BlockPos, state: BlockState);

    /// `FoliageSetter.isSet(BlockPos)`.
    fn is_set(&self, pos: &BlockPos) -> bool;
}

/// `net.minecraft.world.level.levelgen.feature.foliageplacers.FoliagePlacer`
/// — the behavior contract of a foliage placer (Java's abstract
/// `createFoliage`/`foliageHeight`/`shouldSkipLocation` + `type()`).
///
/// The erased carrier `Arc<dyn ErasedFoliagePlacer>` is what the dispatch codec
/// (de)serializes — the Rust analogue of Java's `Codec<FoliagePlacer>` value.
/// `Any` (supertrait) enables the dispatch codec's downcast of an erased value
/// back to its concrete type on encode, via the explicit [`FoliagePlacer::as_any`]
/// seam.
///
/// The Java `protected` instance helpers virtual-dispatch on `this`, so they
/// are default trait methods here (a concrete placer reaches them exactly as
/// Java's inheritance does): `createFoliage` (the 8-arg public resolves
/// `this.offset(random)` then delegates to the 9-arg abstract),
/// `shouldSkipLocationSigned`, `placeLeavesRow`,
/// `placeLeavesRowWithHangingLeavesBelow` (final in Java).
pub trait FoliagePlacer: Any + Debug + Send + Sync + 'static {
    /// `FoliagePlacer.type()` — the registry-held `FoliagePlacerType<?>` identity
    /// this placer dispatches on (the key `FoliagePlacer.CODEC` uses).
    fn type_id(&self) -> FoliagePlacerTypeId;

    /// `FoliagePlacer.createFoliage(..., int foliageHeight, int leafRadius)` —
    /// the public entry: resolves `this.offset(random)` and delegates to the
    /// abstract [`FoliagePlacer::create_foliage_with_offset`].
    fn create_foliage<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        foliage_setter: &mut dyn FoliageSetter,
        random: &mut R,
        config: &TreeConfiguration,
        tree_height: i32,
        foliage_attachment: &FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
    ) {
        let offset = self.offset().sample(random);
        self.create_foliage_with_offset(
            level,
            foliage_setter,
            random,
            config,
            tree_height,
            foliage_attachment,
            foliage_height,
            leaf_radius,
            offset,
        );
    }

    /// `FoliagePlacer.createFoliage(..., int foliageHeight, int leafRadius,
    /// int offset)` — the abstract per-placer foliage placement.
    fn create_foliage_with_offset<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        foliage_setter: &mut dyn FoliageSetter,
        random: &mut R,
        config: &TreeConfiguration,
        tree_height: i32,
        foliage_attachment: &FoliageAttachment,
        foliage_height: i32,
        leaf_radius: i32,
        offset: i32,
    );

    /// `FoliagePlacer.foliageHeight(RandomSource, int treeHeight,
    /// TreeConfiguration)` — the abstract foliage height.
    fn foliage_height<R: RandomSource>(
        &self,
        random: &mut R,
        tree_height: i32,
        config: &TreeConfiguration,
    ) -> i32;

    /// `FoliagePlacer.foliageRadius(RandomSource, int trunkHeight)` —
    /// `this.radius.sample(random)`.
    fn foliage_radius<R: RandomSource>(&self, random: &mut R, _trunk_height: i32) -> i32 {
        self.radius().sample(random)
    }

    /// `FoliagePlacer.shouldSkipLocation(...)` — the abstract corner-skip
    /// predicate.
    fn should_skip_location<R: RandomSource>(
        &self,
        random: &mut R,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool;

    /// `FoliagePlacer.shouldSkipLocationSigned(...)` — resolves the
    /// double-trunk distance minimums (`min(|dx|, |dx-1|)` for a double trunk)
    /// then delegates to `shouldSkipLocation`.
    fn should_skip_location_signed<R: RandomSource>(
        &self,
        random: &mut R,
        dx: i32,
        y: i32,
        dz: i32,
        current_radius: i32,
        double_trunk: bool,
    ) -> bool {
        let (min_dx, min_dz) = if double_trunk {
            (
                std::cmp::min(dx.wrapping_abs(), dx.wrapping_sub(1).wrapping_abs()),
                std::cmp::min(dz.wrapping_abs(), dz.wrapping_sub(1).wrapping_abs()),
            )
        } else {
            (dx.wrapping_abs(), dz.wrapping_abs())
        };
        self.should_skip_location(random, min_dx, y, min_dz, current_radius, double_trunk)
    }

    /// `FoliagePlacer.placeLeavesRow(...)` — place the leaves row at relative
    /// height `y` of the given radius around `origin`, skipping the
    /// `shouldSkipLocationSigned` corners. `BlockPos.setWithOffset(origin, dx,
    /// y, dz)` is the mutable cursor (`set_with_offset_xyz`).
    fn place_leaves_row<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        foliage_setter: &mut dyn FoliageSetter,
        random: &mut R,
        config: &TreeConfiguration,
        origin: &BlockPos,
        current_radius: i32,
        y: i32,
        double_trunk: bool,
    ) {
        let offset = if double_trunk { 1 } else { 0 };
        let origin_vec = Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z());
        let mut pos = origin.mutable();
        for dx in -current_radius..=current_radius.wrapping_add(offset) {
            for dz in -current_radius..=current_radius.wrapping_add(offset) {
                if !self.should_skip_location_signed(
                    random,
                    dx,
                    y,
                    dz,
                    current_radius,
                    double_trunk,
                ) {
                    pos.set_with_offset_xyz(&origin_vec, dx, y, dz);
                    try_place_leaf(level, foliage_setter, random, config, &pos.immutable());
                }
            }
        }
    }

    /// `FoliagePlacer.placeLeavesRowWithHangingLeavesBelow(...)` (final) —
    /// `placeLeavesRow` first, then hanging extensions below each horizontal
    /// edge, walked by `pos.move(...)`.
    fn place_leaves_row_with_hanging_leaves_below<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        foliage_setter: &mut dyn FoliageSetter,
        random: &mut R,
        config: &TreeConfiguration,
        origin: &BlockPos,
        current_radius: i32,
        y: i32,
        double_trunk: bool,
        hanging_leaves_chance: f32,
        hanging_leaves_extension_chance: f32,
    ) {
        self.place_leaves_row(
            level,
            foliage_setter,
            random,
            config,
            origin,
            current_radius,
            y,
            double_trunk,
        );
        let offset = if double_trunk { 1 } else { 0 };
        let log_pos = origin.below();
        let origin_vec = Vec3i::new(origin.get_x(), origin.get_y(), origin.get_z());
        let mut pos = origin.mutable();

        for along_edge in Plane::Horizontal.faces() {
            let to_edge = along_edge.get_clock_wise();
            let offset_to_edge = if to_edge.get_axis_direction() == AxisDirection::Positive {
                current_radius.wrapping_add(offset)
            } else {
                current_radius
            };
            pos.set_with_offset_xyz(&origin_vec, 0, y.wrapping_sub(1), 0);
            pos.move_dir_steps(&to_edge, offset_to_edge);
            pos.move_dir_steps(along_edge, -current_radius);
            let mut offset_along_edge = -current_radius;

            while offset_along_edge < current_radius.wrapping_add(offset) {
                let leaves_above = foliage_setter.is_set(&pos.move_dir(&Direction::Up).immutable());
                pos.move_dir(&Direction::Down);
                if leaves_above
                    && try_place_extension(
                        level,
                        foliage_setter,
                        random,
                        config,
                        hanging_leaves_chance,
                        &log_pos,
                        &pos.immutable(),
                    )
                {
                    pos.move_dir(&Direction::Down);
                    try_place_extension(
                        level,
                        foliage_setter,
                        random,
                        config,
                        hanging_leaves_extension_chance,
                        &log_pos,
                        &pos.immutable(),
                    );
                    pos.move_dir(&Direction::Up);
                }

                offset_along_edge = offset_along_edge.wrapping_add(1);
                pos.move_dir(along_edge);
            }
        }
    }

    /// `this.radius` — the protected radius provider.
    fn radius(&self) -> &IntProvider;

    /// `this.offset` — the protected offset provider.
    fn offset(&self) -> &IntProvider;

    /// `as_any` — the downcast seam (Java's erased `FoliagePlacer` cast) the
    /// dispatch codec uses on encode to recover the concrete placer type.
    fn as_any(&self) -> &dyn Any;
}

/// The object-safe carrier the codec graph stores each placer as — the
/// dispatch identity plus the `dyn`-compatible surface. Every `FoliagePlacer`
/// implements it via the blanket impl, so the concrete placer modules only
/// implement `FoliagePlacer`.
pub trait ErasedFoliagePlacer: Debug + Send + Sync + 'static {
    /// `type()` — the registry-held type identity.
    fn type_id(&self) -> FoliagePlacerTypeId;

    /// `as_any` — the downcast seam over the erased carrier.
    fn as_any(&self) -> &dyn Any;
}

impl<P: FoliagePlacer + ?Sized> ErasedFoliagePlacer for P {
    fn type_id(&self) -> FoliagePlacerTypeId {
        FoliagePlacer::type_id(self)
    }

    fn as_any(&self) -> &dyn Any {
        FoliagePlacer::as_any(self)
    }
}

/// `foliagePlacerParts(Instance)` — the shared two-field record group
/// (`IntProviders.codec(0, 16).fieldOf("radius")` and
/// `IntProviders.codec(0, 16).fieldOf("offset")`), the `P2` every concrete
/// placer codec starts from. The fixed 2-arity tuple (radius then offset)
/// makes the ordering part of the type, mirroring Java's
/// `P2<Mu<P>, IntProvider, IntProvider>`.
pub(crate) fn foliage_placer_parts<P, Ops>(
    get_radius: Arc<dyn Fn(&P) -> IntProvider + Send + Sync>,
    get_offset: Arc<dyn Fn(&P) -> IntProvider + Send + Sync>,
) -> (
    rivet_serialization::record_builder::RecordCodecBuilder<P, Ops, IntProvider>,
    rivet_serialization::record_builder::RecordCodecBuilder<P, Ops, IntProvider>,
)
where
    P: 'static,
    Ops: DynamicOps + 'static,
{
    (
        rivet_serialization::record_builder::RecordCodecBuilder::of(
            get_radius,
            codec::field_of(
                int_provider_codec_with_bounds::<Ops>(0, 16),
                "radius".to_string(),
            ),
        ),
        rivet_serialization::record_builder::RecordCodecBuilder::of(
            get_offset,
            codec::field_of(
                int_provider_codec_with_bounds::<Ops>(0, 16),
                "offset".to_string(),
            ),
        ),
    )
}

/// `FoliagePlacer.tryPlaceLeaf(...)` (protected static) — place one leaf at
/// `pos`: skip when the existing state is persistent or the position is not a
/// valid tree position, then set the provider's state (waterlogged when the
/// position is a water source and the state supports it).
///
/// `pub(crate)` because Java's `protected static` is inherited by the concrete
/// leaves: `RandomSpreadFoliagePlacer.createFoliage` calls `tryPlaceLeaf`
/// directly (the leaf modules reach it through the base's dispatch).
pub(crate) fn try_place_leaf<R: RandomSource>(
    level: &dyn WorldGenLevel,
    foliage_setter: &mut dyn FoliageSetter,
    random: &mut R,
    config: &TreeConfiguration,
    pos: &BlockPos,
) -> bool {
    let is_persistent = level.is_state_at_position(pos, &|state: &BlockState| {
        state.get_value(BlockStateProperties::PERSISTENT) == Some(PropertyValue::Bool(true))
    });
    if !is_persistent && valid_tree_pos(level, pos) {
        let mut foliage_state =
            block_state_provider_get_state(config.foliage_provider.as_ref(), level, random, pos);
        if foliage_state.has_property(BlockStateProperties::WATERLOGGED) {
            // `isFluidAtPosition(pos, fluidState -> fluidState.isSourceOfType(Fluids.WATER))`,
            // where `FluidState.isSourceOfType` is `this.isSource && this.owner == fluidType`.
            //
            // `fluid_id() == FluidId::WATER` is exactly `isSourceOfType(Fluids.WATER)`: source
            // vs flowing is encoded by the fluid *type*, not a separate flag — the level-0
            // source belongs to the `minecraft:water` type (id 2, `FluidId::WATER`), levels
            // 1-15 to `minecraft:flowing_water` (id 1), and the WATER type's only state is a
            // source (`WaterFluid.Source.isSource` is always true), so `owner == WATER` holds
            // exactly when `isSource`. No `isSource` conjunct is dropped.
            let waterlogged =
                level.is_fluid_at_position(pos, &|fluid: &FluidId| *fluid == FluidId::WATER);
            foliage_state = foliage_state
                .set_value(BlockStateProperties::WATERLOGGED, waterlogged)
                .expect("FoliagePlacer waterlogged a state that has the property");
        }
        foliage_setter.set(pos, foliage_state);
        true
    } else {
        false
    }
}

/// `FoliagePlacer.tryPlaceExtension(...)` (private static) — extend a hanging
/// vine when within 7 blocks (Manhattan) of the log position and the chance
/// passes.
fn try_place_extension<R: RandomSource>(
    level: &dyn WorldGenLevel,
    foliage_setter: &mut dyn FoliageSetter,
    random: &mut R,
    config: &TreeConfiguration,
    chance: f32,
    log_pos: &BlockPos,
    pos: &BlockPos,
) -> bool {
    pos.dist_manhattan(log_pos) < 7
        && !(random.next_float() > chance)
        && try_place_leaf(level, foliage_setter, random, config, pos)
}

/// `FoliagePlacer.CODEC` — the by-name dispatch codec, as the ops-generic
/// `foliage_placer_codec::<Ops>()` factory. Every placer's fields are
/// `IntProvider`s (no `RegistryOpsLookup` requirement), so unlike the
/// `BlockStateProvider` dispatch this one is plain `DynamicOps`.
pub fn foliage_placer_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<Arc<dyn ErasedFoliagePlacer>, Ops>> {
    // `BuiltInRegistries.FOLIAGE_PLACER_TYPE.byNameCodec().dispatch(...)`.
    map_codec::codec_of(key_dispatch_codec::dispatch_map::<
        FoliagePlacerTypeId,
        Arc<dyn ErasedFoliagePlacer>,
        Ops,
    >(
        "type",
        foliage_placer_type_by_name_codec::<Ops>(),
        Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
            DataResult::success(ErasedFoliagePlacer::type_id(&**p))
        }),
        codec_for_type(),
    ))
}

/// `FoliagePlacerType::codec` — resolve a `FoliagePlacerTypeId` to its
/// `MapCodec<Arc<dyn ErasedFoliagePlacer>>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static>()
-> key_dispatch_codec::CodecFn<FoliagePlacerTypeId, Arc<dyn ErasedFoliagePlacer>, Ops> {
    Arc::new(move |k: &FoliagePlacerTypeId| {
        if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::BLOB_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::blob_foliage_placer::BlobFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::blob_foliage_placer::blob_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|b: &crate::levelgen::feature::foliageplacers::blob_foliage_placer::BlobFoliagePlacer| {
                    Arc::new(b.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::blob_foliage_placer::BlobFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-blob value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::spruce_foliage_placer::SpruceFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::spruce_foliage_placer::spruce_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|s: &crate::levelgen::feature::foliageplacers::spruce_foliage_placer::SpruceFoliagePlacer| {
                    Arc::new(s.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::spruce_foliage_placer::SpruceFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-spruce value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::PINE_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::pine_foliage_placer::PineFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::pine_foliage_placer::pine_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|p: &crate::levelgen::feature::foliageplacers::pine_foliage_placer::PineFoliagePlacer| {
                    Arc::new(p.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::pine_foliage_placer::PineFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-pine value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::acacia_foliage_placer::AcaciaFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::acacia_foliage_placer::acacia_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|a: &crate::levelgen::feature::foliageplacers::acacia_foliage_placer::AcaciaFoliagePlacer| {
                    Arc::new(a.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::acacia_foliage_placer::AcaciaFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-acacia value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::BUSH_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::bush_foliage_placer::BushFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::bush_foliage_placer::bush_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|b: &crate::levelgen::feature::foliageplacers::bush_foliage_placer::BushFoliagePlacer| {
                    Arc::new(b.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::bush_foliage_placer::BushFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-bush value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::FANCY_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::fancy_foliage_placer::FancyFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::fancy_foliage_placer::fancy_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|f: &crate::levelgen::feature::foliageplacers::fancy_foliage_placer::FancyFoliagePlacer| {
                    Arc::new(f.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::fancy_foliage_placer::FancyFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-fancy value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::mega_jungle_foliage_placer::MegaJungleFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::mega_jungle_foliage_placer::mega_jungle_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|m: &crate::levelgen::feature::foliageplacers::mega_jungle_foliage_placer::MegaJungleFoliagePlacer| {
                    Arc::new(m.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::mega_jungle_foliage_placer::MegaJungleFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-mega-jungle value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::mega_pine_foliage_placer::MegaPineFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::mega_pine_foliage_placer::mega_pine_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|m: &crate::levelgen::feature::foliageplacers::mega_pine_foliage_placer::MegaPineFoliagePlacer| {
                    Arc::new(m.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::mega_pine_foliage_placer::MegaPineFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-mega-pine value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::dark_oak_foliage_placer::DarkOakFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::dark_oak_foliage_placer::dark_oak_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|d: &crate::levelgen::feature::foliageplacers::dark_oak_foliage_placer::DarkOakFoliagePlacer| {
                    Arc::new(d.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::dark_oak_foliage_placer::DarkOakFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-dark-oak value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::random_spread_foliage_placer::RandomSpreadFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::random_spread_foliage_placer::random_spread_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|r: &crate::levelgen::feature::foliageplacers::random_spread_foliage_placer::RandomSpreadFoliagePlacer| {
                    Arc::new(r.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::random_spread_foliage_placer::RandomSpreadFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-random-spread value"))
                        .clone()
                }),
            ))
        } else if *k == crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER {
            DataResult::success(erase_map_codec::<crate::levelgen::feature::foliageplacers::cherry_foliage_placer::CherryFoliagePlacer, Ops>(
                crate::levelgen::feature::foliageplacers::cherry_foliage_placer::cherry_foliage_placer_map_codec::<
                    Ops,
                >(),
                Arc::new(|c: &crate::levelgen::feature::foliageplacers::cherry_foliage_placer::CherryFoliagePlacer| {
                    Arc::new(c.clone()) as Arc<dyn ErasedFoliagePlacer>
                }),
                Arc::new(|p: &Arc<dyn ErasedFoliagePlacer>| {
                    p.as_any()
                        .downcast_ref::<crate::levelgen::feature::foliageplacers::cherry_foliage_placer::CherryFoliagePlacer>()
                        .unwrap_or_else(|| panic!("foliage-placer dispatch produced a non-cherry value"))
                        .clone()
                }),
            ))
        } else {
            DataResult::error(format!(
                "Foliage placer type '{}' is not ported",
                k.location
            ))
        }
    })
}

/// Lift a concrete placer's `MapCodec<C>` to
/// `MapCodec<Arc<dyn ErasedFoliagePlacer>>` — Java's
/// `MapCodec<? extends FoliagePlacer>` variance, via xmap (the same lift
/// `BlockStateProvider`'s `erase_map_codec` performs). The wrap side boxes a
/// clone of the concrete placer; the unwrap side downcasts the erased placer
/// through `as_any`.
#[allow(clippy::type_complexity)]
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
    wrap: Arc<dyn Fn(&C) -> Arc<dyn ErasedFoliagePlacer> + Send + Sync>,
    unwrap: Arc<dyn Fn(&Arc<dyn ErasedFoliagePlacer>) -> C + Send + Sync>,
) -> Arc<dyn MapCodec<Arc<dyn ErasedFoliagePlacer>, Ops>>
where
    C: 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(inner, wrap, unwrap)
}

/// `BuiltInRegistries.FOLIAGE_PLACER_TYPE.byNameCodec()` over the type id —
/// `Identifier.CODEC.comapFlatMap(name -> this.get(name) ..., id -> id.key()
/// .identifier())`, with the same unknown-key error shape every by-name codec
/// reproduces (`Registries.FOLIAGE_PLACER_TYPE` = `createRegistryKey(
/// "worldgen/foliage_placer_type")`).
#[allow(clippy::doc_lazy_continuation)]
pub fn foliage_placer_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<FoliagePlacerTypeId, Ops>> {
    codec::comap_flat_map::<rivet_registry::Identifier, FoliagePlacerTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &rivet_registry::Identifier| {
            match foliage_placer_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/foliage_placer_type]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &FoliagePlacerTypeId| rivet_registry::Identifier::parse(id.location)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::foliageplacers::blob_foliage_placer::BlobFoliagePlacer;
    use crate::levelgen::feature::foliageplacers::cherry_foliage_placer::CherryFoliagePlacer;
    use crate::levelgen::feature::foliageplacers::foliage_placer_type::FoliagePlacerTypes;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::uniform_int::UniformInt;
    use serde_json::json;

    fn provider(min: i32, max: i32) -> IntProvider {
        IntProvider::Uniform(UniformInt::of(min, max))
    }

    #[test]
    fn by_name_codec_round_trips_each_registered_location() {
        let codec = foliage_placer_type_by_name_codec::<JsonOps>();
        for id in [
            FoliagePlacerTypes::BLOB_FOLIAGE_PLACER,
            FoliagePlacerTypes::SPRUCE_FOLIAGE_PLACER,
            FoliagePlacerTypes::PINE_FOLIAGE_PLACER,
            FoliagePlacerTypes::ACACIA_FOLIAGE_PLACER,
            FoliagePlacerTypes::BUSH_FOLIAGE_PLACER,
            FoliagePlacerTypes::FANCY_FOLIAGE_PLACER,
            FoliagePlacerTypes::JUNGLE_FOLIAGE_PLACER,
            FoliagePlacerTypes::MEGA_PINE_FOLIAGE_PLACER,
            FoliagePlacerTypes::DARK_OAK_FOLIAGE_PLACER,
            FoliagePlacerTypes::RANDOM_SPREAD_FOLIAGE_PLACER,
            FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER,
        ] {
            let input = json!(id.location);
            let decoded_result = codec.parse(&JsonOps::INSTANCE, &input);
            let decoded = decoded_result
                .result()
                .expect("decode should succeed")
                .clone();
            assert_eq!(decoded, id);
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &decoded)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, input);
        }
    }

    #[test]
    fn by_name_codec_rejects_unknown_location() {
        let codec = foliage_placer_type_by_name_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("minecraft:not_a_placer"));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/foliage_placer_type]: minecraft:not_a_placer"),
            "got: {msg}"
        );
    }

    #[test]
    fn foliage_attachment_constructor() {
        let pos = BlockPos::new(1, 2, 3);
        let attachment = FoliageAttachment::new(pos, 4, true);
        assert_eq!(attachment.pos, pos);
        assert_eq!(attachment.radius_offset, 4);
        assert!(attachment.double_trunk);
    }

    #[test]
    fn should_skip_location_signed_double_trunk_minimums() {
        // A probe placer whose `shouldSkipLocation` reports the resolved
        // minimums, so the signed wrapper's double-trunk folding is observable.
        // (`Mutex` keeps the probe `Sync` — `FoliagePlacer` requires
        // `Send + Sync`.)
        #[derive(Debug)]
        struct Probe {
            resolved: std::sync::Mutex<(i32, i32)>,
        }
        impl FoliagePlacer for Probe {
            fn type_id(&self) -> FoliagePlacerTypeId {
                FoliagePlacerTypes::BLOB_FOLIAGE_PLACER
            }
            fn create_foliage_with_offset<R: RandomSource>(
                &self,
                _level: &dyn WorldGenLevel,
                _setter: &mut dyn FoliageSetter,
                _random: &mut R,
                _config: &TreeConfiguration,
                _tree_height: i32,
                _attachment: &FoliageAttachment,
                _foliage_height: i32,
                _leaf_radius: i32,
                _offset: i32,
            ) {
            }
            fn foliage_height<R: RandomSource>(
                &self,
                _random: &mut R,
                _tree_height: i32,
                _config: &TreeConfiguration,
            ) -> i32 {
                0
            }
            fn should_skip_location<R: RandomSource>(
                &self,
                _random: &mut R,
                dx: i32,
                _y: i32,
                dz: i32,
                _current_radius: i32,
                _double_trunk: bool,
            ) -> bool {
                *self.resolved.lock().unwrap() = (dx, dz);
                false
            }
            fn radius(&self) -> &IntProvider {
                unreachable!("probe")
            }
            fn offset(&self) -> &IntProvider {
                unreachable!("probe")
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let probe = Probe {
            resolved: std::sync::Mutex::new((0, 0)),
        };
        let mut random = rivet_util::random::LegacyRandomSource::new(0);
        // `doubleTrunk` folds `min(|dx|, |dx-1|)`: dx=0 -> 0; dz=-2 ->
        // min(2, 3) = 2.
        probe.should_skip_location_signed(&mut random, 0, 5, -2, 3, true);
        assert_eq!(*probe.resolved.lock().unwrap(), (0, 2));
        // Non-double passes absolute values through unchanged.
        probe.should_skip_location_signed(&mut random, -2, 5, 3, 3, false);
        assert_eq!(*probe.resolved.lock().unwrap(), (2, 3));
    }

    #[test]
    fn foliage_placer_codec_round_trips_through_the_type_dispatch() {
        // The full `foliage_placer_codec::<JsonOps>()` path: the by-name
        // `"type"` dispatch resolving to `codec_for_type`'s per-placer entry,
        // through `erase_map_codec`'s wrap/unwrap and the `Arc<dyn
        // ErasedFoliagePlacer>` downcast — the seam `TreeConfiguration.CODEC`
        // will embed. Cover one erasing placer (blob) and one nested-record
        // placer (cherry).
        let codec = foliage_placer_codec::<JsonOps>();

        // Blob — the shared blob record lifted through the erase seam.
        let blob_input = json!({
            "type": "minecraft:blob_foliage_placer",
            "radius": {"min_inclusive": 2, "max_inclusive": 3, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": 3
        });
        let blob = codec
            .parse(&JsonOps::INSTANCE, &blob_input)
            .result()
            .expect("blob dispatch decode should succeed")
            .clone();
        assert_eq!(
            ErasedFoliagePlacer::type_id(&*blob),
            FoliagePlacerTypes::BLOB_FOLIAGE_PLACER
        );
        let blob_concrete = blob
            .as_any()
            .downcast_ref::<BlobFoliagePlacer>()
            .expect("dispatch should produce a blob placer");
        assert_eq!(blob_concrete.height(), 3);
        assert_eq!(blob_concrete.radius(), &provider(2, 3));
        assert_eq!(blob_concrete.offset(), &provider(0, 0));
        let blob_encoded = codec
            .encode_start(&JsonOps::INSTANCE, &blob)
            .result()
            .expect("blob dispatch encode should succeed")
            .clone();
        assert_eq!(blob_encoded, blob_input);

        // Cherry — the nested five-field group (`corner_hole_chance` encodes
        // the wide-layer chance, so the input must equalize the two).
        let cherry_input = json!({
            "type": "minecraft:cherry_foliage_placer",
            "radius": {"min_inclusive": 2, "max_inclusive": 2, "type": "minecraft:uniform"},
            "offset": {"min_inclusive": 0, "max_inclusive": 0, "type": "minecraft:uniform"},
            "height": {"min_inclusive": 6, "max_inclusive": 8, "type": "minecraft:uniform"},
            "wide_bottom_layer_hole_chance": 0.7,
            "corner_hole_chance": 0.7,
            "hanging_leaves_chance": 0.6,
            "hanging_leaves_extension_chance": 0.1
        });
        let cherry = codec
            .parse(&JsonOps::INSTANCE, &cherry_input)
            .result()
            .expect("cherry dispatch decode should succeed")
            .clone();
        assert_eq!(
            ErasedFoliagePlacer::type_id(&*cherry),
            FoliagePlacerTypes::CHERRY_FOLIAGE_PLACER
        );
        let cherry_concrete = cherry
            .as_any()
            .downcast_ref::<CherryFoliagePlacer>()
            .expect("dispatch should produce a cherry placer");
        assert_eq!(cherry_concrete.height(), &provider(6, 8));
        assert_eq!(cherry_concrete.radius(), &provider(2, 2));
        assert_eq!(cherry_concrete.offset(), &provider(0, 0));
        let cherry_encoded = codec
            .encode_start(&JsonOps::INSTANCE, &cherry)
            .result()
            .expect("cherry dispatch encode should succeed")
            .clone();
        assert_eq!(cherry_encoded, cherry_input);
    }
}
