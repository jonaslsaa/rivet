//! Paper 26.2 `net.minecraft.world.phys.BlockHitResult` wire projection.

use rivet_registry::core::{BlockPos, Direction, Vec3};

/// `net.minecraft.world.phys.BlockHitResult` wire projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHitResult {
    pub location: Vec3,
    pub direction: Direction,
    pub block_pos: BlockPos,
    pub inside: bool,
    pub world_border_hit: bool,
}

impl BlockHitResult {
    /// `new BlockHitResult(Vec3 location, Direction direction, BlockPos pos, boolean inside, boolean worldBorderHit)`.
    pub fn new(
        location: Vec3,
        direction: Direction,
        block_pos: BlockPos,
        inside: bool,
        world_border_hit: bool,
    ) -> Self {
        Self {
            location,
            direction,
            block_pos,
            inside,
            world_border_hit,
        }
    }
}
