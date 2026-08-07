//! Port of `net.minecraft.network.protocol.game.ClientboundLightUpdatePacketData`
//! (MC 26.2) — the *producer* half (issue #100).
//!
//! The packet-body value type + codec live in
//! `rivet-protocol::protocol::game::light_update_packet_data` (#94). This
//! module ports the `ClientboundLightUpdatePacketData` constructor's
//! `prepareSectionData` loop that turns a `LevelLightEngine`'s `DataLayer`s
//! into the four masks plus the two layer lists the packet carries.
//!
//! Java's loop: for each light section `sectionIndex` in `0..getLightSectionCount`
//! (which is `getSectionsCount() + 2`), look up
//! `getDataLayerData(SectionPos.of(pos, minLightSection + sectionIndex))`; a
//! `null` layer contributes nothing, an empty layer sets `emptyMask`, and any
//! other layer sets `mask` and appends `data.copy().getData()` (2048 bytes).
//!
//! The Rust port resolves the layer lookup via a per-section `layer_at`
//! closure so it stays a pure value (no `LevelLightEngine` back-reference).
//! The superflat chunk's deterministic sky layers are produced by the content
//! builder in `crate::superflat`; this module only encodes the masks/lists.
//!
//! RivetTodo(#184): the light propagation engines (`LightEngine` + the
//! `LayerLightSectionStorage`/`BlockLightSectionStorage`/`SkyLightSectionStorage`
//! producers of the `DataLayer`s) are not ported (owned by the
//! `mc.world.level.lighting.engine` unit); this module only folds
//! caller-supplied layers into the packet payload.

use rivet_protocol::protocol::game::light_update_packet_data::LightUpdatePacketData;

/// `prepareSectionData` / the `ClientboundLightUpdatePacketData` constructor —
/// folds `sky_layers`/`block_layers` (each an `Option<DataLayer>` per light
/// section, `None` when the engine has no layer there) into the packet payload.
///
/// Mask bits are set at the section *index* (not the section y), so the layer
/// slice is indexed `0..light_section_count`.
pub fn build_light_update_data(
    sky_layers: &[Option<crate::chunk::data_layer::DataLayer>],
    block_layers: &[Option<crate::chunk::data_layer::DataLayer>],
) -> LightUpdatePacketData {
    let mut sky_y_mask = 0u64;
    let mut block_y_mask = 0u64;
    let mut empty_sky_y_mask = 0u64;
    let mut empty_block_y_mask = 0u64;
    let mut sky_updates: Vec<Vec<u8>> = Vec::new();
    let mut block_updates: Vec<Vec<u8>> = Vec::new();

    for (section_index, layer) in sky_layers.iter().enumerate() {
        if let Some(data) = layer {
            if data.is_empty() {
                empty_sky_y_mask |= 1u64 << section_index;
            } else {
                sky_y_mask |= 1u64 << section_index;
                sky_updates.push(data.copy().get_data());
            }
        }
    }
    for (section_index, layer) in block_layers.iter().enumerate() {
        if let Some(data) = layer {
            if data.is_empty() {
                empty_block_y_mask |= 1u64 << section_index;
            } else {
                block_y_mask |= 1u64 << section_index;
                block_updates.push(data.copy().get_data());
            }
        }
    }

    LightUpdatePacketData::new(
        word_vec(sky_y_mask),
        word_vec(block_y_mask),
        word_vec(empty_sky_y_mask),
        word_vec(empty_block_y_mask),
        sky_updates,
        block_updates,
    )
}

/// A 64-bit mask to the `BitSet.toLongArray()` form the wire carries (trailing
/// zero words stripped by `writeBitSet` anyway; kept as a single word).
fn word_vec(mask: u64) -> Vec<u64> {
    if mask == 0 { Vec::new() } else { vec![mask] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::data_layer::{DataLayer, SIZE};

    #[test]
    fn empty_layers_set_empty_masks_only() {
        let empty = DataLayer::new(0);
        let layers = [Some(empty), Some(DataLayer::new(0))];
        let data = build_light_update_data(&layers, &[None, None]);
        // Both layers are empty (uniform 0), so they set the *empty* sky mask
        // at section indices 0 and 1, contributing nothing to the update masks.
        assert!(data.sky_y_mask().is_empty());
        assert!(data.block_y_mask().is_empty());
        assert_eq!(data.empty_sky_y_mask(), &[0x3]);
        assert!(data.empty_block_y_mask().is_empty());
        assert!(data.sky_updates().is_empty());
        assert!(data.block_updates().is_empty());
    }

    #[test]
    fn filled_layers_push_updates() {
        let full = DataLayer::new(15);
        let layers = [Some(full), Some(DataLayer::new(0))];
        let block_layers = vec![None; 2];
        let data = build_light_update_data(&layers, &block_layers);
        assert_eq!(data.sky_y_mask(), &[0x1]);
        assert_eq!(data.empty_sky_y_mask(), &[0x2]);
        assert_eq!(data.sky_updates().len(), 1);
        assert_eq!(data.sky_updates()[0], vec![0xFF; SIZE as usize]);
    }
}
