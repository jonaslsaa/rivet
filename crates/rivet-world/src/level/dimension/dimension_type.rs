//! Port of the `net.minecraft.world.level.dimension.DimensionType` height
//! constants (class, 26.2) — the minimal slice issue #388 needs.
//!
//! Java: the constants derive from `BlockPos.PACKED_Y_LENGTH`:
//!
//! ```java
//! public static final int BITS_FOR_Y = BlockPos.PACKED_Y_LENGTH;   // 12
//! public static final int Y_SIZE = (1 << BITS_FOR_Y) - 32;          // 4064
//! public static final int MAX_Y = (Y_SIZE >> 1) - 1;                // 2031
//! public static final int MIN_Y = MAX_Y - Y_SIZE + 1;               // -2032
//! ```
//!
//! `VerticalAnchor`'s per-variant codecs bound their int fields with
//! `Codec.intRange(MIN_Y, MAX_Y)`, so this slice is exactly the constants it
//! references. RivetTodo(#388): the full `DimensionType` record/codec (the
//! 16-field record, the constructor validation, `MonsterSettings`/`Skybox`,
//! `DIRECT_CODEC`/`NETWORK_CODEC`, and the remaining constants
//! `MIN_HEIGHT`/`WAY_ABOVE_MAX_Y`/`WAY_BELOW_MIN_Y` that no current consumer
//! needs) defers with the owning `mc.world.level.dimension` manifest unit; when
//! it lands, the record folds these constants in and this shell is replaced.

use rivet_registry::core::BlockPos;

/// `DimensionType.BITS_FOR_Y` — `BlockPos.PACKED_Y_LENGTH`.
pub const BITS_FOR_Y: i32 = BlockPos::PACKED_Y_LENGTH;

/// `DimensionType.Y_SIZE` — `(1 << BITS_FOR_Y) - 32`.
pub const Y_SIZE: i32 = (1 << BITS_FOR_Y) - 32;

/// `DimensionType.MAX_Y` — `(Y_SIZE >> 1) - 1`.
pub const MAX_Y: i32 = (Y_SIZE >> 1) - 1;

/// `DimensionType.MIN_Y` — `MAX_Y - Y_SIZE + 1`.
pub const MIN_Y: i32 = MAX_Y - Y_SIZE + 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_constants_match_paper_26_2() {
        // Derived from BlockPos.PACKED_Y_LENGTH = 12. The literals are pinned
        // from the Paper 26.2 source (DimensionType.java/BlockPos.java).
        assert_eq!(BITS_FOR_Y, 12);
        assert_eq!(Y_SIZE, 4064);
        assert_eq!(MAX_Y, 2031);
        assert_eq!(MIN_Y, -2032);
        // The derived relations hold.
        assert_eq!(MAX_Y, MIN_Y + Y_SIZE - 1);
        assert_eq!(Y_SIZE, (1 << BITS_FOR_Y) - 32);
    }
}
