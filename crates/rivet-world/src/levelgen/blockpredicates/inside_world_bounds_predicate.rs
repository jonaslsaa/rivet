//! Port of `net.minecraft.world.level.levelgen.blockpredicates.InsideWorldBoundsPredicate`
//! (class, 26.2).
//!
//! Java: a predicate whose `test` is
//! `worldGenLevel.isInsideBuildHeight(blockPos.offset(this.offset))` and whose
//! `type()` is `BlockPredicateType.INSIDE_WORLD_BOUNDS`. Its `CODEC` is a
//! record codec over the `"offset"` optional field (`Vec3i.offsetCodec(16)`,
//! default `BlockPos.ZERO` — the offset codec handles both `BlockPos.ZERO` and
//! `Vec3i.ZERO` since they are the same `(0,0,0)`). The codec is ported here
//! (as the ops-generic `inside_world_bounds_predicate_map_codec::<Ops>()`
//! factory) and lifted to the erased carrier in `block_predicate`.

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use crate::levelgen::blockpredicates::state_testing_predicate::offset_field;
use rivet_registry::core::{BlockPos, Vec3i};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.InsideWorldBoundsPredicate`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InsideWorldBoundsPredicate {
    /// `this.offset` — the offset applied to the tested position.
    offset: Vec3i,
}

impl InsideWorldBoundsPredicate {
    /// `new InsideWorldBoundsPredicate(Vec3i)`.
    pub fn new(offset: Vec3i) -> Self {
        InsideWorldBoundsPredicate { offset }
    }

    /// `this.offset`.
    pub fn offset(&self) -> &Vec3i {
        &self.offset
    }
}

impl BlockPredicate for InsideWorldBoundsPredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        level.is_inside_build_height_pos(&origin.offset_vec(&self.offset))
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::INSIDE_WORLD_BOUNDS
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `InsideWorldBoundsPredicate.CODEC` — the record codec over the `"offset"`
/// optional field (`Vec3i.offsetCodec(16)`, default `Vec3i.ZERO`), as the
/// ops-generic `inside_world_bounds_predicate_map_codec::<Ops>()` factory.
///
/// Java uses `BlockPos.ZERO` as the default; `BlockPos`/`Vec3i` are the same
/// `(0,0,0)` value, and the state-testing base codec defaults to `Vec3i.ZERO`
/// — identical decode/encode.
pub fn inside_world_bounds_predicate_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<InsideWorldBoundsPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(offset_field::<InsideWorldBoundsPredicate, Ops>(Arc::new(
                |p: &InsideWorldBoundsPredicate| p.offset,
            )))
            .apply(
                instance,
                Arc::new(|offset: Vec3i| InsideWorldBoundsPredicate { offset }),
            )
    })
}
