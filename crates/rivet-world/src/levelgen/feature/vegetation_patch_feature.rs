//! Paper VegetationPatchFeature and WaterloggedVegetationPatchFeature.
//! Surface sets preserve Java HashSet iteration, including the subclass's
//! second insertion/rehash pass before water writes and nested vegetation RNG.

use super::configurations::VegetationPatchConfiguration;
use super::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use super::stateproviders::block_state_provider::block_state_provider_get_state;
use super::{FeatureBehavior, FeaturePlaceContext};
use crate::{block::blocks::Blocks, chunk::chunk_generator::ChunkGenerator, level::WorldGenLevel};
use rivet_registry::{
    block_state_properties::BlockStateProperties,
    block_state_property::PropertyValue,
    core::{BlockPos, Direction, JavaBlockPosSet},
};
use rivet_util::RandomSource;

#[derive(Debug)]
pub struct VegetationPatchFeature;
pub const VEGETATION_PATCH: VegetationPatchFeature = VegetationPatchFeature;
#[derive(Debug)]
pub struct WaterloggedVegetationPatchFeature;
pub const WATERLOGGED_VEGETATION_PATCH: WaterloggedVegetationPatchFeature =
    WaterloggedVegetationPatchFeature;

fn insert(surface: &mut JavaBlockPosSet, pos: BlockPos) {
    surface
        .try_insert(pos)
        .unwrap_or_else(|error| std::panic::panic_any(error));
}

fn place_ground<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    config: &VegetationPatchConfiguration,
    random: &mut R,
    mut pos: BlockPos,
    depth: i32,
) -> bool {
    for i in 0..depth {
        let state =
            block_state_provider_get_state(config.ground_state.as_ref(), level, random, &pos);
        let below = level.get_block_state(&pos);
        if state.block() != below.block() {
            if !config.replaceable.contains_id(below.block().id() as u32) {
                return i != 0;
            }
            level.set_block(&pos, state, 2);
            pos = pos.relative(&config.surface.get_direction());
        }
        // Deliberately do not move when the provider chose the existing block:
        // Java re-samples that SAME position for the next depth iteration.
    }
    true
}

fn place_ground_patch<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    config: &VegetationPatchConfiguration,
    random: &mut R,
    origin: &BlockPos,
    xr: i32,
    zr: i32,
) -> JavaBlockPosSet {
    let mut surface = JavaBlockPosSet::new();
    let inwards = config.surface.get_direction();
    let outwards = inwards.get_opposite();
    for dx in xr.wrapping_neg()..=xr {
        let x_edge = dx == xr.wrapping_neg() || dx == xr;
        for dz in zr.wrapping_neg()..=zr {
            let z_edge = dz == zr.wrapping_neg() || dz == zr;
            let corner = x_edge && z_edge;
            let edge = x_edge || z_edge;
            if corner
                || (edge
                    && (config.extra_edge_column_chance == 0.0
                        || random.next_float() > config.extra_edge_column_chance))
            {
                continue;
            }
            let mut pos = origin.offset(dx, 0, dz);
            let mut offset = 0;
            while level.is_state_at_position(&pos, &|state| state.is_air())
                && offset < config.vertical_range
            {
                pos = pos.relative(&inwards);
                offset += 1;
            }
            offset = 0;
            while level.is_state_at_position(&pos, &|state| !state.is_air())
                && offset < config.vertical_range
            {
                pos = pos.relative(&outwards);
                offset += 1;
            }
            let below_pos = pos.relative(&inwards);
            let below = level.get_block_state(&below_pos);
            if level.is_empty_block(&pos) && level.is_face_sturdy(&below_pos, &below, &outwards) {
                let depth = config.depth.sample(random).wrapping_add(i32::from(
                    config.extra_bottom_block_chance > 0.0
                        && random.next_float() < config.extra_bottom_block_chance,
                ));
                if place_ground(level, config, random, below_pos, depth) {
                    insert(&mut surface, below_pos);
                }
            }
        }
    }
    surface
}

fn place_vegetation<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    config: &VegetationPatchConfiguration,
    generator: &dyn ChunkGenerator,
    random: &mut R,
    pos: &BlockPos,
) -> bool {
    let access = level.registry_access();
    let placed = config.vegetation_feature.value(
        access
            .lookup(&*PLACED_FEATURE)
            .expect("vegetation patch needs the placed-feature registry"),
    );
    placed.place(
        access
            .lookup(&*CONFIGURED_FEATURE)
            .expect("vegetation patch needs the configured-feature registry"),
        level,
        generator,
        random,
        &pos.relative(&config.surface.get_direction().get_opposite()),
    )
}

fn place_patch<R: RandomSource>(
    context: &mut FeaturePlaceContext<'_, VegetationPatchConfiguration, R>,
    waterlogged: bool,
) -> bool {
    let config = context.config;
    let level = &mut *context.level;
    let random = &mut *context.random;
    let xr = config.xz_radius.sample(random).wrapping_add(1);
    let zr = config.xz_radius.sample(random).wrapping_add(1);
    let mut surface = place_ground_patch(level, config, random, context.origin, xr, zr);
    if waterlogged {
        let mut water_surface = JavaBlockPosSet::new();
        for pos in surface.iter() {
            let exposed = [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
                Direction::Down,
            ]
            .into_iter()
            .any(|direction| {
                let neighbor_pos = pos.relative(&direction);
                !level.is_face_sturdy(
                    &neighbor_pos,
                    &level.get_block_state(&neighbor_pos),
                    &direction.get_opposite(),
                )
            });
            if !exposed {
                insert(&mut water_surface, *pos);
            }
        }
        for pos in water_surface.iter() {
            level.set_block(pos, Blocks::WATER.default_block_state(), 2);
        }
        surface = water_surface;
    }
    for pos in surface.iter() {
        if config.vegetation_chance > 0.0 && random.next_float() < config.vegetation_chance {
            if waterlogged {
                if place_vegetation(level, config, context.chunk_generator, random, &pos.below()) {
                    let state = level.get_block_state(pos);
                    if state.get_value(BlockStateProperties::WATERLOGGED)
                        == Some(PropertyValue::Bool(false))
                    {
                        level.set_block(
                            pos,
                            state
                                .set_value(BlockStateProperties::WATERLOGGED, true)
                                .unwrap(),
                            2,
                        );
                    }
                }
            } else {
                place_vegetation(level, config, context.chunk_generator, random, pos);
            }
        }
    }
    !surface.is_empty()
}

impl FeatureBehavior<VegetationPatchConfiguration> for VegetationPatchFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, VegetationPatchConfiguration, R>,
    ) -> bool {
        place_patch(context, false)
    }
}
impl FeatureBehavior<VegetationPatchConfiguration> for WaterloggedVegetationPatchFeature {
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, VegetationPatchConfiguration, R>,
    ) -> bool {
        place_patch(context, true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::stateproviders::simple_state_provider::SimpleStateProvider;
    use super::super::test_support::{
        RecordingRandom, RngCall, TestGenerator, TestLevel, access, no_op_placed,
    };
    use super::*;
    use crate::levelgen::placement::CaveSurface;
    use rivet_registry::{
        Holder, block_state::BlockState, generated::blocks::BlockId, holder::RegistryId,
        holder_set::HolderSet,
    };
    use rivet_util::valueproviders::{constant_int::ConstantInt, int_provider::IntProvider};
    use std::sync::Arc;

    fn state(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).expect("registered fixture block"))
    }

    fn config() -> VegetationPatchConfiguration {
        VegetationPatchConfiguration::new(
            HolderSet::direct(vec![Holder::reference(
                RegistryId(0),
                Blocks::STONE.id().0 as u32,
            )]),
            Arc::new(SimpleStateProvider::new(state("minecraft:moss_block"))),
            no_op_placed(),
            CaveSurface::Floor,
            IntProvider::Constant(ConstantInt::of(3)),
            0.0,
            3,
            0.0,
            IntProvider::Constant(ConstantInt::of(0)),
            0.0,
        )
    }

    #[test]
    fn existing_ground_block_resamples_same_position_instead_of_moving_down() {
        let mut level = TestLevel::over(access());
        let pos = BlockPos::new(0, -1, 0);
        level.states.insert(pos, state("minecraft:moss_block"));
        level
            .states
            .insert(pos.below(), Blocks::STONE.default_block_state());
        assert!(place_ground(
            &mut level,
            &config(),
            &mut RecordingRandom::new(0),
            pos,
            3
        ));
        assert!(level.writes.is_empty());
        assert_eq!(*level.reads.borrow(), [pos, pos, pos]);
    }

    #[test]
    fn ground_depth_succeeds_after_partial_column_but_refuses_first_unreplaceable_cell() {
        let mut level = TestLevel::over(access());
        let pos = BlockPos::new(0, -1, 0);
        level
            .states
            .insert(pos, Blocks::STONE.default_block_state());
        level
            .states
            .insert(pos.below(), Blocks::STONE.default_block_state());
        level
            .states
            .insert(pos.below().below(), Blocks::DIRT.default_block_state());
        assert!(place_ground(
            &mut level,
            &config(),
            &mut RecordingRandom::new(0),
            pos,
            3
        ));
        assert_eq!(
            level.writes,
            [
                (pos, state("minecraft:moss_block")),
                (pos.below(), state("minecraft:moss_block"))
            ]
        );
        assert!(!place_ground(
            &mut level,
            &config(),
            &mut RecordingRandom::new(0),
            pos.below().below(),
            3
        ));
        assert_eq!(level.writes.len(), 2);
    }

    #[test]
    fn waterlogged_patch_filters_exposed_surface_before_water_writes_and_vegetation_draws() {
        for exposed in [false, true] {
            let mut level = TestLevel::over(access());
            level.face_sturdy = false;
            let ground = BlockPos::new(0, -1, 0);
            level
                .states
                .insert(ground, Blocks::STONE.default_block_state());
            level.face_sturdy_at.insert((ground, Direction::Up), true);
            for direction in [
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
                Direction::Down,
            ] {
                let neighbor = ground.relative(&direction);
                level
                    .states
                    .insert(neighbor, Blocks::STONE.default_block_state());
                level.face_sturdy_at.insert(
                    (neighbor, direction.get_opposite()),
                    !exposed || direction != Direction::North,
                );
            }
            let mut config = config();
            config.depth = IntProvider::Constant(ConstantInt::of(1));
            config.vegetation_chance = 1.0;
            let mut random = RecordingRandom::new(0);
            let result = WATERLOGGED_VEGETATION_PATCH.place(&mut FeaturePlaceContext::new(
                None,
                &mut level,
                &TestGenerator,
                &mut random,
                &BlockPos::new(0, 0, 0),
                &config,
            ));
            assert_eq!(result, !exposed);
            if exposed {
                assert_eq!(level.writes, [(ground, state("minecraft:moss_block"))]);
                assert!(
                    random.calls.is_empty(),
                    "empty filtered surface must consume no vegetation RNG"
                );
            } else {
                assert_eq!(
                    level.writes,
                    [
                        (ground, state("minecraft:moss_block")),
                        (ground, Blocks::WATER.default_block_state())
                    ]
                );
                assert_eq!(random.calls, [RngCall::Float]);
            }
        }
    }
}
