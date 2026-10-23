//! `ca.spottedleaf.moonrise.patches.starlight.light` — the server-side light
//! engine (`StarLightInterface` and its propagation engines, #184).
//!
//! Ported so far: the [`StarLightProvider`] seam (the concrete impl that plugs
//! into the `rivet-world` facade) and the Starlight flood-fill compute core as
//! `star_light_engine::SkyStarLightEngine`. The provider is now
//! `star_light_provider_impl::SkyLightProvider` — a real synchronous layer that
//! drives the engine on an explicitly supplied in-progress chunk and publishes
//! the computed sky/block nibbles and emptiness maps back onto it. Generation
//! includes block-source scanning and block edge reconciliation; conditional
//! block-light occlusion faces remain explicit capability refusals. What defers
//! with #184 is the live `StarLightInterface` queue wiring,
//! `blockChange`/`sectionChange`/`relightChunks`/`checkChunkEdges`, the client
//! notify path, and the final generated-serving pipeline wiring into a concrete
//! chunk storage.

#[cfg(test)]
mod generated_block_light_fixture;
pub mod generated_light_bridge;
#[cfg(test)]
mod incremental_light_fixture;
pub mod live_light_runtime;
pub use live_light_runtime::{LiveLightError, LiveLightRuntime};
pub mod star_light_engine;
pub mod star_light_provider_impl;

pub use generated_light_bridge::{
    GENERATED_LIGHT_NEIGHBOR_RADIUS, GeneratedLightBridge, GeneratedLightBridgeError,
    GeneratedLightStorage, GeneratedLightWorkspace, GeneratedLightWorkspaceError,
};
pub use star_light_provider_impl::{ChunkAccessFn, LightChunk, SkyLightProvider};
