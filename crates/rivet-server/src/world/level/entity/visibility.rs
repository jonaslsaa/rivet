//! Port of `net.minecraft.world.level.entity.Visibility` (Paper 26.2).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/entity/Visibility.java`

/// Chunk entity visibility level matching Paper's `Visibility` enum.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Visibility {
    /// Entities are loaded in section storage but neither tracked/visible nor ticking.
    #[default]
    Hidden,
    /// Entities are accessible/tracked in `EntityLookup` but not ticking.
    Tracked,
    /// Entities are both accessible/tracked and ticking.
    Ticking,
}

/// Chunk status levels mapped by `Visibility::from_full_chunk_status`.
///
/// Mirrors `net.minecraft.server.level.FullChunkStatus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FullChunkStatus {
    Inaccessible,
    Full,
    BlockTicking,
    EntityTicking,
}

impl FullChunkStatus {
    /// Returns `true` if `self` is at least `other` (`FullChunkStatus.isOrAfter`).
    #[must_use]
    pub const fn is_or_after(self, other: Self) -> bool {
        (self as u8) >= (other as u8)
    }
}

impl Visibility {
    /// `Visibility.isTicking()`.
    #[must_use]
    pub const fn is_ticking(self) -> bool {
        matches!(self, Self::Ticking)
    }

    /// `Visibility.isAccessible()`.
    #[must_use]
    pub const fn is_accessible(self) -> bool {
        matches!(self, Self::Tracked | Self::Ticking)
    }

    /// `Visibility.fromFullChunkStatus(FullChunkStatus)`.
    #[must_use]
    pub const fn from_full_chunk_status(status: FullChunkStatus) -> Self {
        if status.is_or_after(FullChunkStatus::EntityTicking) {
            Self::Ticking
        } else if status.is_or_after(FullChunkStatus::Full) {
            Self::Tracked
        } else {
            Self::Hidden
        }
    }
}
