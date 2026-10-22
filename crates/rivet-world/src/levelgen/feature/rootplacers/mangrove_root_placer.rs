//! Mangrove roots: simulate every horizontal direction before writing any root.
//! The base RootPlacer helpers are folded into the only registered subtype;
//! notably their canPlaceRoot calls use the mangrove override.
use super::above_root_placement::above_root_placement_codec;
use super::mangrove_root_placement::mangrove_root_placement_codec;
use super::{AboveRootPlacement, MangroveRootPlacement};
use crate::level::WorldGenLevel;
use crate::levelgen::feature::{
    stateproviders::block_state_provider::{
        ErasedBlockStateProvider, block_state_provider_codec, block_state_provider_get_state,
    },
    tree_feature::valid_tree_pos,
};
use rivet_registry::{
    block_state::BlockState,
    block_state_properties::BlockStateProperties,
    core::{BlockPos, Direction, Plane},
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec,
    dynamic_ops::DynamicOps,
    map_codec::MapCodec,
    record_builder::{self, RecordCodecBuilder},
};
use rivet_util::{
    RandomSource,
    valueproviders::int_provider::{IntProvider, int_provider_codec},
};
use std::sync::Arc;

pub const ROOT_WIDTH_LIMIT: i32 = 8;
pub const ROOT_LENGTH_LIMIT: i32 = 15;

#[derive(Debug, Clone)]
pub struct MangroveRootPlacer {
    pub trunk_offset_y: IntProvider,
    pub root_provider: Arc<dyn ErasedBlockStateProvider>,
    pub above_root_placement: Option<AboveRootPlacement>,
    pub mangrove_root_placement: MangroveRootPlacement,
}
impl MangroveRootPlacer {
    pub fn place_roots<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        origin: &BlockPos,
        trunk_origin: &BlockPos,
    ) -> bool {
        let mut root_positions = Vec::new();
        let mut column = *origin;
        while column.get_y() < trunk_origin.get_y() {
            if !self.can_place_root(level, &column) {
                return false;
            }
            column = column.above();
        }
        root_positions.push(trunk_origin.below());
        for direction in Plane::Horizontal.faces() {
            let next = trunk_origin.relative(direction);
            let mut positions_in_direction = Vec::new();
            if !self.simulate_roots(
                level,
                random,
                &next,
                direction,
                trunk_origin,
                &mut positions_in_direction,
                0,
            ) {
                return false;
            }
            root_positions.extend(positions_in_direction);
            root_positions.push(trunk_origin.relative(direction));
        }
        for pos in root_positions {
            self.place_root(level, setter, random, &pos);
        }
        true
    }
    #[allow(clippy::too_many_arguments)]
    fn simulate_roots<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
        direction: &Direction,
        root_origin: &BlockPos,
        positions: &mut Vec<BlockPos>,
        layer: i32,
    ) -> bool {
        let max_length = self.mangrove_root_placement.max_root_length;
        if layer == max_length || positions.len() as i32 > max_length {
            return false;
        }
        for next in self.potential_root_positions(pos, direction, random, root_origin) {
            if self.can_place_root(level, &next) {
                positions.push(next);
                if !self.simulate_roots(
                    level,
                    random,
                    &next,
                    direction,
                    root_origin,
                    positions,
                    layer.wrapping_add(1),
                ) {
                    return false;
                }
            }
        }
        true
    }
    pub fn potential_root_positions<R: RandomSource>(
        &self,
        pos: &BlockPos,
        direction: &Direction,
        random: &mut R,
        root_origin: &BlockPos,
    ) -> Vec<BlockPos> {
        let below = pos.below();
        let next_to = pos.relative(direction);
        let width = pos.dist_manhattan(root_origin);
        let max_width = self.mangrove_root_placement.max_root_width;
        let chance = self.mangrove_root_placement.random_skew_chance;
        if width > max_width.wrapping_sub(3) && width <= max_width {
            if random.next_float() < chance {
                vec![below, next_to.below()]
            } else {
                vec![below]
            }
        } else if width > max_width || random.next_float() < chance {
            // Short-circuiting intentionally omits the float draw when already
            // outside the max width.
            vec![below]
        } else if random.next_boolean() {
            vec![next_to]
        } else {
            vec![below]
        }
    }
    fn can_place_root(&self, level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
        valid_tree_pos(level, pos)
            || level.is_state_at_position(pos, &|state| {
                self.mangrove_root_placement
                    .can_grow_through
                    .contains_id(state.block().id() as u32)
            })
    }
    fn place_root<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        pos: &BlockPos,
    ) {
        if level.is_state_at_position(pos, &|state| {
            self.mangrove_root_placement
                .muddy_roots_in
                .contains_id(state.block().id() as u32)
        }) {
            let muddy = block_state_provider_get_state(
                self.mangrove_root_placement.muddy_roots_provider.as_ref(),
                level,
                random,
                pos,
            );
            setter(pos, self.potentially_waterlogged_state(level, pos, muddy));
        } else if self.can_place_root(level, pos) {
            let root =
                block_state_provider_get_state(self.root_provider.as_ref(), level, random, pos);
            setter(pos, self.potentially_waterlogged_state(level, pos, root));
            if let Some(placement) = &self.above_root_placement {
                let above = pos.above();
                if random.next_float() < placement.above_root_placement_chance
                    && level.is_state_at_position(&above, &|state| state.is_air())
                {
                    let state = block_state_provider_get_state(
                        placement.above_root_provider.as_ref(),
                        level,
                        random,
                        &above,
                    );
                    setter(
                        &above,
                        self.potentially_waterlogged_state(level, &above, state),
                    );
                }
            }
        }
    }
    fn potentially_waterlogged_state(
        &self,
        level: &dyn WorldGenLevel,
        pos: &BlockPos,
        state: BlockState,
    ) -> BlockState {
        if !state.has_property(BlockStateProperties::WATERLOGGED) {
            return state;
        }
        let waterlogged = level.is_fluid_at_position(pos, &|fluid| {
            rivet_registry::generated::tags::FLUID_TAG_BY_NAME
                .get("minecraft:water")
                .expect("vanilla water tag")
                .contains(&fluid.name())
        });
        state
            .set_value(BlockStateProperties::WATERLOGGED, waterlogged)
            .expect("waterlogged property")
    }
}
pub fn mangrove_root_placer_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<MangroveRootPlacer, Ops>> {
    record_builder::map_codec(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|p: &MangroveRootPlacer| p.trunk_offset_y.clone()),
            codec::field_of(int_provider_codec::<Ops>(), "trunk_offset_y".to_string()),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &MangroveRootPlacer| p.root_provider.clone()),
            codec::field_of(
                block_state_provider_codec::<Ops>(),
                "root_provider".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &MangroveRootPlacer| p.above_root_placement.clone()),
            codec::optional_field(
                "above_root_placement".to_string(),
                above_root_placement_codec::<Ops>(),
                false,
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|p: &MangroveRootPlacer| p.mangrove_root_placement.clone()),
            codec::field_of(
                mangrove_root_placement_codec::<Ops>(),
                "mangrove_root_placement".to_string(),
            ),
        ))
        .apply(
            i,
            Arc::new(
                |trunk_offset_y, root_provider, above_root_placement, mangrove_root_placement| {
                    MangroveRootPlacer {
                        trunk_offset_y,
                        root_provider,
                        above_root_placement,
                        mangrove_root_placement,
                    }
                },
            ),
        )
    })
}
