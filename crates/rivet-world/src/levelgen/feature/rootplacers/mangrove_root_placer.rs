//! Port of `net.minecraft.world.level.levelgen.feature.rootplacers.
//! MangroveRootPlacer` (26.2) — the single concrete root placer.
//!
//! Java extends `RootPlacer` with a trailing `MangroveRootPlacement` field:
//! its `CODEC` is `rootPlacerParts(instance).and(
//! MangroveRootPlacement.CODEC.fieldOf("mangrove_root_placement"))`, a
//! four-field record codec (`trunk_offset_y`, `root_provider`,
//! `above_root_placement`, `mangrove_root_placement`) built on the dispatch
//! root's shared `rootPlacerParts` group.
//!
//! `placeRoots` grows the root network from `trunkOrigin.below()` outward along
//! each horizontal axis: every column between `origin` and `trunkOrigin` must be
//! placeable (else the whole system fails), then `simulateRoots` recursively
//! walks candidate positions while the spread stays within
//! `ROOT_LENGTH_LIMIT`. `potentialRootPositions` biases the walk by the current
//! Manhattan distance to `rootOrigin` against `maxRootWidth` and the random
//! skew chance; `canPlaceRoot` extends the base check with `canGrowThrough`,
//! and `placeRoot` swaps in `muddyRootsProvider`'s state over `muddyRootsIn`
//! positions before falling back to the base `placeRoot`.
//!
//! Java `List.of(...)` reads become `vec![...]`, the `Direction.Plane.HORIZONTAL`
//! loop becomes `Plane::Horizontal.faces()`, and the `MutableBlockPos` column
//! walk uses `move_dir(&Direction::Up)`.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::configurations::TreeConfiguration;
use crate::levelgen::feature::rootplacers::above_root_placement::AboveRootPlacement;
use crate::levelgen::feature::rootplacers::mangrove_root_placement::{
    MangroveRootPlacement, mangrove_root_placement_map_codec,
};
use crate::levelgen::feature::rootplacers::root_placer::{
    RootPlacer, root_placer_parts,
};
use crate::levelgen::feature::rootplacers::root_placer_type::{
    RootPlacerTypeId, RootPlacerTypes,
};
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_get_state,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::core::direction::{Direction, Plane};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::IntProvider;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `MangroveRootPlacer.ROOT_WIDTH_LIMIT`.
pub const ROOT_WIDTH_LIMIT: i32 = 8;
/// `MangroveRootPlacer.ROOT_LENGTH_LIMIT`.
pub const ROOT_LENGTH_LIMIT: i32 = 15;

/// `net.minecraft.world.level.levelgen.feature.rootplacers.MangroveRootPlacer`.
#[derive(Debug, Clone)]
pub struct MangroveRootPlacer {
    trunk_offset_y: IntProvider,
    root_provider: Arc<dyn ErasedBlockStateProvider>,
    above_root_placement: Option<AboveRootPlacement>,
    mangrove_root_placement: MangroveRootPlacement,
}

impl MangroveRootPlacer {
    /// `new MangroveRootPlacer(IntProvider, BlockStateProvider, Optional,
    /// MangroveRootPlacement)` — the constructor (fields 1–3 forwarded to
    /// `super`).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trunk_offset_y: IntProvider,
        root_provider: Arc<dyn ErasedBlockStateProvider>,
        above_root_placement: Option<AboveRootPlacement>,
        mangrove_root_placement: MangroveRootPlacement,
    ) -> MangroveRootPlacer {
        MangroveRootPlacer {
            trunk_offset_y,
            root_provider,
            above_root_placement,
            mangrove_root_placement,
        }
    }

    /// `MangroveRootPlacer.mangroveRootPlacement()` — the accessor.
    pub fn mangrove_root_placement(&self) -> &MangroveRootPlacement {
        &self.mangrove_root_placement
    }
}

impl RootPlacer for MangroveRootPlacer {
    fn type_id(&self) -> RootPlacerTypeId {
        RootPlacerTypes::MANGROVE_ROOT_PLACER
    }

    /// `MangroveRootPlacer.placeRoots(...)`.
    fn place_roots<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        root_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        origin: &BlockPos,
        trunk_origin: &BlockPos,
        config: &TreeConfiguration,
    ) -> bool {
        let mut root_positions: Vec<BlockPos> = Vec::new();
        let mut column_pos = origin.mutable();

        while column_pos.get_y() < trunk_origin.get_y() {
            if !self.can_place_root(level, &column_pos.immutable()) {
                return false;
            }
            column_pos.move_dir(&Direction::Up);
        }

        root_positions.push(trunk_origin.below());

        for dir in Plane::Horizontal.faces() {
            let pos = trunk_origin.relative(dir);
            let mut positions_in_direction: Vec<BlockPos> = Vec::new();
            if !self.simulate_roots(
                level,
                random,
                &pos,
                dir,
                trunk_origin,
                &mut positions_in_direction,
                0,
            ) {
                return false;
            }
            root_positions.extend(positions_in_direction);
            root_positions.push(trunk_origin.relative(dir));
        }

        for root_pos in &root_positions {
            self.place_root(level, root_setter, random, root_pos, config);
        }

        true
    }

    /// `MangroveRootPlacer.canPlaceRoot(...)` — base check or grow-through.
    fn can_place_root(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        RootPlacer::can_place_root(self, level, pos)
            || level.is_state_at_position(pos, &|state: &BlockState| {
                self.mangrove_root_placement
                    .can_grow_through
                    .contains_id(state.block().id() as u32)
            })
    }

    /// `MangroveRootPlacer.placeRoot(...)` — muddy-roots override then base.
    fn place_root<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        root_setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        pos: &BlockPos,
        config: &TreeConfiguration,
    ) {
        if level.is_state_at_position(pos, &|state: &BlockState| {
            self.mangrove_root_placement
                .muddy_roots_in
                .contains_id(state.block().id() as u32)
        }) {
            let muddy_roots = block_state_provider_get_state(
                &self.mangrove_root_placement.muddy_roots_provider,
                level,
                random,
                pos,
            );
            root_setter(
                pos,
                self.get_potentially_waterlogged_state(level, pos, muddy_roots),
            );
        } else {
            RootPlacer::place_root(self, level, root_setter, random, pos, config);
        }
    }

    fn trunk_offset_y(&self) -> &IntProvider {
        &self.trunk_offset_y
    }

    fn root_provider(&self) -> &Arc<dyn ErasedBlockStateProvider> {
        &self.root_provider
    }

    fn above_root_placement(&self) -> &Option<AboveRootPlacement> {
        &self.above_root_placement
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl MangroveRootPlacer {
    /// `MangroveRootPlacer.simulateRoots(...)` — recursively grow the root
    /// branch along `dir`, bounded by `maxRootLength` and the accumulated
    /// position count.
    fn simulate_roots<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        root_pos: &BlockPos,
        dir: &Direction,
        root_origin: &BlockPos,
        root_positions: &mut Vec<BlockPos>,
        layer: i32,
    ) -> bool {
        let max_root_length = self.mangrove_root_placement.max_root_length;
        if layer != max_root_length && root_positions.len() <= max_root_length as usize {
            for pos in self.potential_root_positions(root_pos, dir, random, root_origin) {
                if self.can_place_root(level, &pos) {
                    root_positions.push(pos);
                    if !self.simulate_roots(
                        level,
                        random,
                        &pos,
                        dir,
                        root_origin,
                        root_positions,
                        layer + 1,
                    ) {
                        return false;
                    }
                }
            }
            true
        } else {
            false
        }
    }

    /// `MangroveRootPlacer.potentialRootPositions(...)` — the width-bounded
    /// candidate set for the next root step.
    fn potential_root_positions<R: RandomSource>(
        &self,
        pos: &BlockPos,
        prev_dir: &Direction,
        random: &mut R,
        root_origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let below = pos.below();
        let next_to = pos.relative(prev_dir);
        let width = pos.dist_manhattan(root_origin);
        let max_root_width = self.mangrove_root_placement.max_root_width;
        let random_skew_chance = self.mangrove_root_placement.random_skew_chance;
        if width > max_root_width - 3 && width <= max_root_width {
            if random.next_float() < random_skew_chance {
                vec![below, next_to.below()]
            } else {
                vec![below]
            }
        } else if width > max_root_width {
            vec![below]
        } else if random.next_float() < random_skew_chance {
            vec![below]
        } else if random.next_boolean() {
            vec![next_to]
        } else {
            vec![below]
        }
    }
}

/// `MangroveRootPlacer.CODEC` — the four-field record codec
/// (`rootPlacerParts` + `mangrove_root_placement`), as the ops-generic
/// `mangrove_root_placer_map_codec::<Ops>()` factory. The shared group carries
/// the `RegistryOpsLookup` ops surface of `BlockStateProvider.CODEC`.
pub fn mangrove_root_placer_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>(
) -> Arc<dyn MapCodec<MangroveRootPlacer, Ops>> {
    record_builder::map_codec(|instance| {
        root_placer_parts(
            instance,
            Arc::new(|p: &MangroveRootPlacer| p.trunk_offset_y.clone()),
            Arc::new(|p: &MangroveRootPlacer| p.root_provider.clone()),
            Arc::new(|p: &MangroveRootPlacer| p.above_root_placement.clone()),
        )
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &MangroveRootPlacer| p.mangrove_root_placement.clone()),
            codec::field_of(
                map_codec::codec_of(mangrove_root_placement_map_codec::<Ops>()),
                "mangrove_root_placement".to_string(),
            ),
        ))
        .apply(instance, Arc::new(MangroveRootPlacer::new))
    })
}
