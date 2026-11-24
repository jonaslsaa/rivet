//! `net.minecraft.world.phys.BlockHitResult` — a ray hit on a block face (#448).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/phys/BlockHitResult.java` (and the `location` field of its
//! superclass `HitResult`, embedded here as a plain field).
//!
//! Placement follows the [`Vec3`] precedent (`net.minecraft.world.phys.Vec3`
//! lives in `rivet-registry::core` so `rivet-protocol` can hold it below
//! itself): `FriendlyByteBuf.readBlockHitResult`/`writeBlockHitResult` carry
//! this value on the `ServerboundUseItemOnPacket` wire, and a
//! `rivet-protocol → rivet-world` edge would be a cycle.
//!
//! Deferred to the `mc.world.phys` unit: the rest of the `HitResult`
//! hierarchy (`distanceTo`, `EntityHitResult`); only `HitResult.Type` is
//! ported here because `getType()` is derived from the `miss` field.

use super::{BlockPos, Direction, Vec3};

/// `HitResult.Type` — declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HitResultType {
    /// `MISS`.
    Miss,
    /// `BLOCK`.
    Block,
    /// `ENTITY`.
    Entity,
}

/// `BlockHitResult`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockHitResult {
    location: Vec3,
    direction: Direction,
    block_pos: BlockPos,
    miss: bool,
    inside: bool,
    world_border_hit: bool,
}

impl BlockHitResult {
    /// `BlockHitResult.miss(Vec3 location, Direction direction, BlockPos pos)`.
    pub fn miss(location: Vec3, direction: Direction, pos: BlockPos) -> Self {
        BlockHitResult::new_full(true, location, direction, pos, false, false)
    }

    /// `new BlockHitResult(Vec3 location, Direction direction, BlockPos pos, boolean inside)`.
    pub fn new(location: Vec3, direction: Direction, pos: BlockPos, inside: bool) -> Self {
        BlockHitResult::new_full(false, location, direction, pos, inside, false)
    }

    /// `new BlockHitResult(Vec3, Direction, BlockPos, boolean inside, boolean worldBorderHit)`.
    pub fn new_with_world_border_hit(
        location: Vec3,
        direction: Direction,
        pos: BlockPos,
        inside: bool,
        world_border_hit: bool,
    ) -> Self {
        BlockHitResult::new_full(false, location, direction, pos, inside, world_border_hit)
    }

    /// The private all-fields constructor.
    fn new_full(
        miss: bool,
        location: Vec3,
        direction: Direction,
        block_pos: BlockPos,
        inside: bool,
        world_border_hit: bool,
    ) -> Self {
        BlockHitResult {
            location,
            direction,
            block_pos,
            miss,
            inside,
            world_border_hit,
        }
    }

    /// `withDirection(Direction)`.
    pub fn with_direction(&self, direction: Direction) -> Self {
        BlockHitResult::new_full(
            self.miss,
            self.location,
            direction,
            self.block_pos,
            self.inside,
            self.world_border_hit,
        )
    }

    /// `withPosition(BlockPos)`.
    pub fn with_position(&self, block_pos: BlockPos) -> Self {
        BlockHitResult::new_full(
            self.miss,
            self.location,
            self.direction,
            block_pos,
            self.inside,
            self.world_border_hit,
        )
    }

    /// `hitBorder()`.
    pub fn hit_border(&self) -> Self {
        BlockHitResult::new_full(
            self.miss,
            self.location,
            self.direction,
            self.block_pos,
            self.inside,
            true,
        )
    }

    /// `HitResult.getLocation()`.
    pub fn get_location(&self) -> Vec3 {
        self.location
    }

    /// `getBlockPos()`.
    pub fn get_block_pos(&self) -> BlockPos {
        self.block_pos
    }

    /// `getDirection()`.
    pub fn get_direction(&self) -> Direction {
        self.direction
    }

    /// `getType()` — `MISS` when built by [`BlockHitResult::miss`], else `BLOCK`.
    pub fn get_type(&self) -> HitResultType {
        if self.miss {
            HitResultType::Miss
        } else {
            HitResultType::Block
        }
    }

    /// `isInside()`.
    pub fn is_inside(&self) -> bool {
        self.inside
    }

    /// `isWorldBorderHit()`.
    pub fn is_world_border_hit(&self) -> bool {
        self.world_border_hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_miss_inside_and_border_like_java() {
        let location = Vec3::new(1.5, 2.0, 3.25);
        let pos = BlockPos::new(1, 2, 3);
        let hit = BlockHitResult::new(location, Direction::Up, pos, true);
        assert_eq!(hit.get_type(), HitResultType::Block);
        assert!(hit.is_inside());
        assert!(!hit.is_world_border_hit());
        assert_eq!(hit.get_location(), location);
        assert_eq!(hit.get_block_pos(), pos);
        assert_eq!(hit.get_direction(), Direction::Up);

        let missed = BlockHitResult::miss(location, Direction::Down, pos);
        assert_eq!(missed.get_type(), HitResultType::Miss);
        assert!(!missed.is_inside());

        let border = hit.hit_border();
        assert!(border.is_world_border_hit());
        assert!(border.is_inside());
        assert_eq!(
            hit.with_direction(Direction::East).get_direction(),
            Direction::East
        );
        assert_eq!(
            hit.with_position(BlockPos::ZERO).get_block_pos(),
            BlockPos::ZERO
        );
    }
}
