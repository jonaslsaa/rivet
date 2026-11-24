//! `net.minecraft.world.phys.BlockHitResult`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/BlockHitResult.java`.

use std::any::Any;

use super::hit_result::{HitResult, HitResultBehavior, Type};
use super::vec3::Vec3;
use crate::core::{BlockPos, Direction};

/// `BlockHitResult extends HitResult`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockHitResult {
    hit_result: HitResult,
    direction: Direction,
    block_pos: BlockPos,
    miss: bool,
    inside: bool,
    world_border_hit: bool,
}

impl BlockHitResult {
    /// `BlockHitResult.miss(Vec3, Direction, BlockPos)`.
    pub fn miss(location: Vec3, direction: Direction, pos: BlockPos) -> BlockHitResult {
        BlockHitResult::new_full(true, location, direction, pos, false, false)
    }

    /// `BlockHitResult(Vec3, Direction, BlockPos, boolean)`.
    pub fn new(
        location: Vec3,
        direction: Direction,
        pos: BlockPos,
        inside: bool,
    ) -> BlockHitResult {
        BlockHitResult::new_full(false, location, direction, pos, inside, false)
    }

    /// `BlockHitResult(Vec3, Direction, BlockPos, boolean, boolean)`.
    pub fn new_with_world_border_hit(
        location: Vec3,
        direction: Direction,
        pos: BlockPos,
        inside: bool,
        world_border_hit: bool,
    ) -> BlockHitResult {
        BlockHitResult::new_full(false, location, direction, pos, inside, world_border_hit)
    }

    /// The private six-argument constructor.
    fn new_full(
        miss: bool,
        location: Vec3,
        direction: Direction,
        block_pos: BlockPos,
        inside: bool,
        world_border_hit: bool,
    ) -> BlockHitResult {
        BlockHitResult {
            hit_result: HitResult::new(location),
            direction,
            block_pos,
            miss,
            inside,
            world_border_hit,
        }
    }

    /// `BlockHitResult.withDirection(Direction)`.
    pub fn with_direction(&self, direction: Direction) -> BlockHitResult {
        BlockHitResult::new_full(
            self.miss,
            self.hit_result.location,
            direction,
            self.block_pos,
            self.inside,
            self.world_border_hit,
        )
    }

    /// `BlockHitResult.withPosition(BlockPos)`.
    pub fn with_position(&self, block_pos: BlockPos) -> BlockHitResult {
        BlockHitResult::new_full(
            self.miss,
            self.hit_result.location,
            self.direction,
            block_pos,
            self.inside,
            self.world_border_hit,
        )
    }

    /// `BlockHitResult.hitBorder()`.
    pub fn hit_border(&self) -> BlockHitResult {
        BlockHitResult::new_full(
            self.miss,
            self.hit_result.location,
            self.direction,
            self.block_pos,
            self.inside,
            true,
        )
    }

    /// `BlockHitResult.getBlockPos()`.
    pub fn get_block_pos(&self) -> BlockPos {
        self.block_pos
    }

    /// `BlockHitResult.getDirection()`.
    pub fn get_direction(&self) -> Direction {
        self.direction
    }

    /// `BlockHitResult.isInside()`.
    pub fn is_inside(&self) -> bool {
        self.inside
    }

    /// `BlockHitResult.isWorldBorderHit()`.
    pub fn is_world_border_hit(&self) -> bool {
        self.world_border_hit
    }
}

impl HitResultBehavior for BlockHitResult {
    fn hit_result(&self) -> &HitResult {
        &self.hit_result
    }

    /// `BlockHitResult.getType()`.
    fn get_type(&self) -> Type {
        if self.miss { Type::Miss } else { Type::Block }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
