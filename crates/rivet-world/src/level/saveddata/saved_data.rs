//! Port of `net.minecraft.world.level.saveddata.SavedData` (MC 26.2).
//!
//! The base class every world persisted-data blob extends. Paper's 26.2
//! version carries a single `boolean dirty` flag with `setDirty()` /
//! `setDirty(boolean)` / `isDirty()`. There is no serialization surface here —
//! the per-type `CODEC`/`SavedDataType` wiring lives in the subclass
//! (`WanderingTraderData`/`WeatherData`), and the load/save/disk lifecycle
//! belongs to the `ServerLevel` storage runtime (see `level/storage`).

/// `net.minecraft.world.level.saveddata.SavedData`.
///
/// The `dirty` flag is tick-thread-confined game state (D5) — a plain `bool`,
/// no lock.
#[derive(Debug, Clone, Default)]
pub struct SavedData {
    /// Java `private boolean dirty`.
    dirty: bool,
}

impl SavedData {
    /// `setDirty()` — mark dirty.
    pub fn set_dirty(&mut self) {
        self.set_dirty_flag(true);
    }

    /// `setDirty(boolean dirty)` (overload — suffix by the `boolean` arg per
    /// PORTING.md).
    pub fn set_dirty_flag(&mut self, dirty: bool) {
        self.dirty = dirty;
    }

    /// `isDirty()`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_flag_can_be_cleared_and_redirtied() {
        let mut data = SavedData::default();
        assert!(!data.is_dirty());
        data.set_dirty();
        assert!(data.is_dirty());
        // Java `setDirty(boolean false)` clears the flag.
        data.set_dirty_flag(false);
        assert!(!data.is_dirty());
        data.set_dirty_flag(true);
        assert!(data.is_dirty());
    }
}
