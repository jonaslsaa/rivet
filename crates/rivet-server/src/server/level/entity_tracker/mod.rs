//! Port of Paper 26.2 entity tracker (`ChunkMap.TrackedEntity`, `ServerEntity`,
//! `VecDeltaCodec`, `TrackingRange`, and `ChunkMap` tracker lifecycle).
//!
//! Issue #424 (M3): pairing/unpairing, visibility range checks, passenger /
//! leash-link / equipment / syncher / attribute dirty-state emission scheduling,
//! and remove transitions. Consumes existing packet structs from
//! `rivet-protocol` (#90) and does not own entity storage or wire codecs.

pub mod chunk_map_tracker;
pub mod packets;
pub mod server_entity;
pub mod tracked_entity;
pub mod vec_delta_codec;
pub mod visibility;

#[cfg(test)]
mod tests;

pub use chunk_map_tracker::{
    ChunkMapEntityTracker, EntityTrackRegistration, EntityTrackerError, TrackerTickContext,
};
pub use packets::{EntityTrackerPacket, EquipmentEntry, EquipmentSlot, MinecartStep};
pub use server_entity::{
    FORCED_POS_UPDATE_PERIOD, FORCED_TELEPORT_PERIOD, HARD_COLLIDING_TELEPORT_DELAY, ServerEntity,
    ServerEntitySynchronizer, TOLERANCE_LEVEL_POSITION, TOLERANCE_LEVEL_ROTATION,
    TrackedEntityState,
};
pub use tracked_entity::{EntityTrackerHost, NearbyPlayersChunk, TrackedEntity};
pub use vec_delta_codec::{TRUNCATION_STEPS, VecDeltaCodec, java_math_round};
pub use visibility::{
    ActivationType, TrackingEntityKind, TrackingRangeConfig, TrackingRangeYConfig, ViewerInfo,
    get_effective_range, get_entity_tracking_range, get_scaled_tracking_distance,
    is_visible_to_player,
};
