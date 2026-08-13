//! Port of `net.minecraft.server.level.FullChunkStatus` (MC 26.2, Paper) — the
//! status ladder a full chunk can hold.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/FullChunkStatus.java`.
//!
//! Owned by the `mc.server.level.pipeline.level` manifest unit (#185). The
//! declaration order IS the Java ordinal ladder (INACCESSIBLE=0, FULL=1,
//! BLOCK_TICKING=2, ENTITY_TICKING=3), so `self as usize` is the exact
//! `Enum.ordinal()`; the ladder is pinned against the Paper golden fixture
//! (`tools/rivet-oracle/fixtures/chunk-level/`, ChunkLevelProbe).

/// `FullChunkStatus` — Java's enum in declaration order. The discriminants are
/// the Java ordinals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FullChunkStatus {
    Inaccessible,
    Full,
    BlockTicking,
    EntityTicking,
}

impl FullChunkStatus {
    /// The ladder in Java declaration order (indices are the ordinals).
    pub const ALL: [Self; 4] = [
        Self::Inaccessible,
        Self::Full,
        Self::BlockTicking,
        Self::EntityTicking,
    ];

    /// `FullChunkStatus.isOrAfter(step)` — `this.ordinal() >= step.ordinal()`.
    pub fn is_or_after(self, step: Self) -> bool {
        self as usize >= step as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_ordinal_ladder() {
        // Java ordinals: INACCESSIBLE=0, FULL=1, BLOCK_TICKING=2,
        // ENTITY_TICKING=3. `ALL` is the declaration order, so the enum
        // discriminants must be these exact values.
        assert_eq!(FullChunkStatus::ALL.len(), 4);
        assert_eq!(FullChunkStatus::Inaccessible as usize, 0);
        assert_eq!(FullChunkStatus::Full as usize, 1);
        assert_eq!(FullChunkStatus::BlockTicking as usize, 2);
        assert_eq!(FullChunkStatus::EntityTicking as usize, 3);
        for (ordinal, status) in FullChunkStatus::ALL.into_iter().enumerate() {
            assert_eq!(status as usize, ordinal);
        }
    }

    #[test]
    fn is_or_after_is_the_ordinal_comparison() {
        // `this.ordinal() >= step.ordinal()`, so each ladder rung is
        // is-or-after itself and everything before it.
        for a in FullChunkStatus::ALL {
            for b in FullChunkStatus::ALL {
                assert_eq!(a.is_or_after(b), a as usize >= b as usize, "{a:?} >= {b:?}");
            }
        }
        // Spot-pin the extremes the paper oracle also pins.
        assert!(FullChunkStatus::EntityTicking.is_or_after(FullChunkStatus::Inaccessible));
        assert!(FullChunkStatus::EntityTicking.is_or_after(FullChunkStatus::EntityTicking));
        assert!(!FullChunkStatus::Inaccessible.is_or_after(FullChunkStatus::Full));
        assert!(!FullChunkStatus::Full.is_or_after(FullChunkStatus::BlockTicking));
    }
}
