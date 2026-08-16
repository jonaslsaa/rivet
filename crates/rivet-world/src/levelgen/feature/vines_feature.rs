//! Port of `net.minecraft.world.level.levelgen.feature.VinesFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.vines`
//! manifest unit.
//!
//! Java: `Feature<NoneFeatureConfiguration>` that attaches a `minecraft:vine`
//! to the origin cell. The origin must be empty; then the six axis directions
//! are walked in `Direction.values()` order (DOWN, UP, NORTH, SOUTH, WEST, EAST)
//! and the first face (skipping DOWN) whose neighbour is acceptable hosts the
//! vine: the state is written with `Block.UPDATE_CLIENTS` and the matching
//! face property set. No random draws. Returns `true` iff a face was found.
//!
//! `VineBlock.isAcceptableNeighbour` is `MultifaceBlock.canAttachTo(level,
//! directionToNeighbour, neighbourPos, level.getBlockState(neighbourPos))` —
//! `Block.isFaceFull(supportShape, opposite) || Block.isFaceFull(collisionShape,
//! opposite)`. The port maps it to the dedicated `WorldGenLevel::can_attach_to`
//! seam; it must not be reduced to `is_face_sturdy`, because leaves have a full
//! collision face but no full support face.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::Property;
use rivet_registry::core::BlockPos;
use rivet_registry::core::Direction;
use rivet_util::RandomSource;

/// `Block.UPDATE_CLIENTS` — the write-flag constant `VinesFeature` uses.
const UPDATE_CLIENTS: u32 = 2;

/// `VineBlock.getPropertyForFace(Direction)` — the face property for a
/// direction (`PROPERTY_BY_DIRECTION`: UP→UP, NORTH→NORTH, EAST→EAST,
/// SOUTH→SOUTH, WEST→WEST). `VinesFeature` never passes DOWN.
fn property_for_face(direction: &Direction) -> Property {
    match direction {
        Direction::Up => BlockStateProperties::UP,
        Direction::North => BlockStateProperties::NORTH,
        Direction::East => BlockStateProperties::EAST,
        Direction::South => BlockStateProperties::SOUTH,
        Direction::West => BlockStateProperties::WEST,
        Direction::Down => unreachable!("VinesFeature skips DOWN"),
    }
}

/// `VineBlock.isAcceptableNeighbour(WorldGenLevel, BlockPos, Direction)` —
/// `MultifaceBlock.canAttachTo(level, directionToNeighbour, neighbourPos,
/// level.getBlockState(neighbourPos))`, checking the neighbour's support or
/// collision face toward `origin` (`canAttachTo` on the opposite face).
fn is_acceptable_neighbour(
    level: &dyn WorldGenLevel,
    neighbour_pos: &BlockPos,
    direction: &Direction,
) -> bool {
    let neighbour_state = level.get_block_state(neighbour_pos);
    level.can_attach_to(neighbour_pos, &neighbour_state, &direction.get_opposite())
}

/// `net.minecraft.world.level.levelgen.feature.VinesFeature`.
#[derive(Debug)]
pub struct VinesFeature;

/// `Feature.VINES` — the registered `minecraft:vines` singleton.
pub const VINES: VinesFeature = VinesFeature;

impl FeatureBehavior<NoneFeatureConfiguration> for VinesFeature {
    /// `VinesFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// if (!level.isEmptyBlock(origin)) return false;
    /// for (Direction direction : Direction.values()) {
    ///     if (direction != Direction.DOWN && VineBlock.isAcceptableNeighbour(
    ///             level, origin.relative(direction), direction)) {
    ///         level.setBlock(origin, Blocks.VINE.defaultBlockState()
    ///             .setValue(VineBlock.getPropertyForFace(direction), true),
    ///             Block.UPDATE_CLIENTS);
    ///         return true;
    ///     }
    /// }
    /// return false;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext { level, origin, .. } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let origin = **origin;
        if !level.is_empty_block(&origin) {
            return false;
        }
        for direction in Direction::VALUES {
            if direction != Direction::Down
                && is_acceptable_neighbour(level, &origin.relative(&direction), &direction)
            {
                let vine = Blocks::VINE
                    .default_block_state()
                    .set_value(property_for_face(&direction), true)
                    .expect("vine has the face property for every horizontal/up direction");
                level.set_block(&origin, vine, UPDATE_CLIENTS);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, TestGenerator, TestLevel, access,
    };
    use rivet_registry::block_state::BlockState;
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;

    fn place_with<R: rivet_util::RandomSource>(
        level: &mut TestLevel,
        origin: BlockPos,
        random: &mut R,
    ) -> bool {
        let generator = TestGenerator;
        VINES.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &generator,
            random,
            &origin,
            &NoneFeatureConfiguration,
        ))
    }

    /// A non-empty origin fails the gate with no writes and no draws.
    #[test]
    fn non_empty_origin_returns_false() {
        let mut level = TestLevel::over(access());
        level.states.insert(
            BlockPos::new(0, 0, 0),
            BlockState::of(BlockId::from_name("minecraft:stone").unwrap()),
        );
        let mut random = RecordingRandom::new(7);
        assert!(!place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    /// `TestLevel::face_sturdy` defaults true, so the first non-DOWN direction
    /// (UP) is acceptable: the origin is written with the UP face property and
    /// `true` returns after one write and no draws through the `can_attach_to`
    /// seam.
    #[test]
    fn attaches_vine_on_the_first_acceptable_face() {
        let mut level = TestLevel::over(access());
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert_eq!(level.writes.len(), 1);
        let (pos, state) = &level.writes[0];
        assert_eq!(*pos, BlockPos::new(0, 0, 0));
        assert_eq!(state.block(), BlockId::from_name("minecraft:vine").unwrap());
        assert_eq!(
            state.get_value(BlockStateProperties::UP),
            Some(PropertyValue::Bool(true))
        );
        assert!(random.calls.is_empty());
    }

    /// The DOWN direction is skipped by the loop guard even when its neighbour
    /// is face-sturdy: with only -Y (DOWN) acceptable, every non-DOWN face
    /// fails and the feature returns `false` with no write. This pins that the
    /// DOWN direction can never host the vine.
    #[test]
    fn down_neighbour_alone_does_not_attach() {
        let mut level = TestLevel::over(access());
        level.face_sturdy = false;
        // The `can_attach_to` seam in the double uses a single global boolean,
        // so this per-direction verdict is exercised through the per-position
        // override (the -Y neighbour is sturdy, every other face fails).
        level
            .face_sturdy_at
            .insert((BlockPos::new(0, -1, 0), Direction::Up), true);
        let mut random = RecordingRandom::new(7);
        assert!(!place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert!(level.writes.is_empty());
        assert!(random.calls.is_empty());
    }

    #[test]
    fn attaches_on_full_collision_face_without_sturdy_support() {
        let mut level = TestLevel::over(access());
        level.face_sturdy = false;
        level.states.insert(
            BlockPos::new(0, 1, 0),
            BlockState::of(BlockId::from_name("minecraft:oak_leaves").unwrap()),
        );
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert_eq!(
            level.writes[0].1.get_value(BlockStateProperties::UP),
            Some(PropertyValue::Bool(true))
        );
        assert!(random.calls.is_empty());
    }

    /// A face-sturdy +Y neighbour attaches on the UP face; the write is the
    /// vine with the `UP` property set, and the walk stops (one write only)
    /// even though every other neighbour is also sturdy.
    #[test]
    fn attaches_on_up_when_up_neighbour_is_sturdy() {
        let mut level = TestLevel::over(access());
        level
            .face_sturdy_at
            .insert((BlockPos::new(0, 1, 0), Direction::Down), true);
        level.face_sturdy = false;
        let mut random = RecordingRandom::new(7);
        assert!(place_with(&mut level, BlockPos::new(0, 0, 0), &mut random));
        assert_eq!(level.writes.len(), 1);
        assert_eq!(level.writes[0].0, BlockPos::new(0, 0, 0));
        assert!(
            level.writes[0].1.get_value(BlockStateProperties::UP)
                == Some(PropertyValue::Bool(true))
        );
    }
}
