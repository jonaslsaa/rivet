//! Port of `net.minecraft.world.level.lighting.LayerLightEventListener`
//! (MC 26.2, Paper) — a `LightEventListener` that exposes the per-layer light
//! data.
//!
//! Java: `LayerLightEventListener.java` in `working/Paper`. A single-layer
//! light engine (sky or block) implements this to let the level read the raw
//! `DataLayer` of a section and the computed light value at a block position.
//! It extends `LightEventListener`, so every layer listener is also a light
//! event listener.
//!
//! `DummyLightLayerEventListener` (the `enum` singleton `INSTANCE`) is the
//! no-op listener Java uses when a layer engine is disabled
//! (`LevelLightEngine.DUMMY`). The port mirrors that singleton with a unit
//! struct and a `pub static` instance; the `enum` with no methods is pure Java
//! idiom, not ported semantics.
//!
//! #184 Phase A: this is the live reader interface slice from
//! `mc.world.level.lighting.core`. The engines that implement it (the vanilla
//! `LightEngine` layer storages are dead jar-surface — issue #184 re-scoped to
//! Starlight) land with the `mc.world.level.lighting.engine` unit and the
//! `starlight.light` engine port.

use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};

use crate::chunk::data_layer::DataLayer;
use crate::lighting::light_event_listener::LightEventListener;

/// `net.minecraft.world.level.lighting.LayerLightEventListener`.
pub trait LayerLightEventListener: LightEventListener {
    /// `getDataLayerData(SectionPos)` — the layer's `DataLayer` at `pos`, or
    /// `None` when the engine has no data for that section (Java `null`).
    fn get_data_layer_data(&self, pos: SectionPos) -> Option<DataLayer>;

    /// `getLightValue(BlockPos)` — the light value at `pos`.
    fn get_light_value(&self, pos: BlockPos) -> i32;
}

/// `DummyLightLayerEventListener` — the no-op single-layer listener Java's
/// `LevelLightEngine` uses when a light layer is disabled.
pub struct DummyLightLayerEventListener;

/// The `INSTANCE` singleton — Java's `DummyLightLayerEventListener.INSTANCE`.
pub const DUMMY_LIGHT_LAYER_EVENT_LISTENER: DummyLightLayerEventListener =
    DummyLightLayerEventListener;

impl LayerLightEventListener for DummyLightLayerEventListener {
    fn get_data_layer_data(&self, _pos: SectionPos) -> Option<DataLayer> {
        None
    }

    fn get_light_value(&self, _pos: BlockPos) -> i32 {
        0
    }
}

impl LightEventListener for DummyLightLayerEventListener {
    fn check_block(&mut self, _pos: BlockPos) {}

    fn has_light_work(&self) -> bool {
        false
    }

    fn run_light_updates(&mut self) -> i32 {
        0
    }

    fn update_section_status(&mut self, _pos: SectionPos, _section_empty: bool) {}

    fn set_light_enabled(&mut self, _pos: ChunkPos, _enable: bool) {}

    fn propagate_light_sources(&mut self, _pos: ChunkPos) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dummy_listener_is_a_total_no_op() {
        let mut dummy = DummyLightLayerEventListener;
        dummy.check_block(BlockPos::new(1, 2, 3));
        dummy.update_section_status(SectionPos::of(4, 5, 6), true);
        dummy.set_light_enabled(ChunkPos::new(7, 8), false);
        dummy.propagate_light_sources(ChunkPos::new(9, 10));
        assert!(!dummy.has_light_work());
        assert_eq!(dummy.run_light_updates(), 0);
        assert!(dummy.get_data_layer_data(SectionPos::of(4, 5, 6)).is_none());
        assert_eq!(dummy.get_light_value(BlockPos::new(1, 2, 3)), 0);
    }

    #[test]
    fn block_pos_update_section_status_delegates_to_section_form() {
        // `updateSectionStatus(BlockPos)` must resolve to the section of the
        // block position, mirroring Java's default method.
        struct Recording;
        impl LightEventListener for Recording {
            fn check_block(&mut self, _pos: BlockPos) {}
            fn has_light_work(&self) -> bool {
                false
            }
            fn run_light_updates(&mut self) -> i32 {
                0
            }
            fn update_section_status(&mut self, pos: SectionPos, _section_empty: bool) {
                assert_eq!(pos, SectionPos::of(1, 4, 2));
            }
            fn set_light_enabled(&mut self, _pos: ChunkPos, _enable: bool) {}
            fn propagate_light_sources(&mut self, _pos: ChunkPos) {}
        }
        // Block (28, 66, 44) >> 4 = section (1, 4, 2).
        Recording.update_section_status_pos(BlockPos::new(28, 66, 44), false);
    }
}
