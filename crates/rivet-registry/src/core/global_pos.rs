//! `net.minecraft.core.GlobalPos` — a `(ResourceKey<Level>, BlockPos)` record.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/GlobalPos.java`.
//! Ported as a record-like value type: `of`, the `dimension`/`pos` record
//! accessors, `toString` (`dimension + " " + pos`) and `isCloseEnough` mirror
//! the Java record; `PartialEq`/`Eq`/`Hash`/`Clone` are derived over the two
//! components (Java record value semantics — `ResourceKey` carries value
//! semantics per the #107 binding, see `resource_key.rs`).
//!
//! The `dimension` type is `ResourceKey<Level>` using the world-unit
//! placeholder `crate::registries::Level` — the same type `Registries::DIMENSION`
//! and `Registries::level_stem_to_level` use, so a `GlobalPos` can be built
//! against a registry key directly.
//!
//! RivetTodo(#126): `MAP_CODEC`/`CODEC` (need `Level.RESOURCE_KEY_CODEC` + the
//! DFU map-codec surface) and `STREAM_CODEC` (`ResourceKey.streamCodec(
//! Registries.DIMENSION)` + `BlockPos.STREAM_CODEC`) defer with the protocol
//! codec surface (#126). The value type itself needs nothing else from the
//! #124 registry SCC beyond `ResourceKey`, which has landed.

use super::block_pos::BlockPos;
use crate::ResourceKey;
use crate::registries::Level;

/// `net.minecraft.core.GlobalPos` — a `(ResourceKey<Level>, BlockPos)` record.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlobalPos {
    dimension: ResourceKey<Level>,
    pos: BlockPos,
}

impl GlobalPos {
    /// `GlobalPos.of(ResourceKey<Level>, BlockPos)`.
    pub fn of(dimension: ResourceKey<Level>, pos: BlockPos) -> Self {
        Self { dimension, pos }
    }

    /// `GlobalPos.dimension()`.
    pub fn dimension(&self) -> &ResourceKey<Level> {
        &self.dimension
    }

    /// `GlobalPos.pos()`.
    pub fn pos(&self) -> BlockPos {
        self.pos
    }

    /// `GlobalPos.isCloseEnough(dimension, pos, maxDistance)` — the dimensions
    /// match and `pos.distChessboard(pos) <= maxDistance`.
    pub fn is_close_enough(
        &self,
        dimension: &ResourceKey<Level>,
        pos: &BlockPos,
        max_distance: i32,
    ) -> bool {
        self.dimension == *dimension && self.pos.dist_chessboard(pos) <= max_distance
    }
}

impl std::fmt::Display for GlobalPos {
    /// `GlobalPos.toString()` — `dimension + " " + pos`. `ResourceKey`'s
    /// `Display` is `"ResourceKey[registry / identifier]"` and `BlockPos`'s is
    /// `"BlockPos{x=…, y=…, z=…}"`, matching Java's string concatenation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.dimension, self.pos)
    }
}
