//! `ca.spottedleaf.moonrise.patches.starlight.light` — the server-side light
//! engine (`StarLightInterface` and its propagation engines, #184).
//!
//! Ported so far: the [`StarLightProvider`] seam (the concrete impl that plugs
//! into the `rivet-world` facade) and the Starlight flood-fill compute core as
//! `star_light_engine::SkyStarLightEngine`. The provider is now
//! `star_light_provider_impl::SkyLightProvider` — a real synchronous layer that
//! drives the engine on an explicitly supplied in-progress chunk and publishes
//! the computed sky nibbles + sky-emptiness map back onto it. What defers with
//! #184 is the real `StarLightInterface` queue wiring, the block engine, live
//! `blockChange`/`sectionChange`/`relightChunks`/`checkChunkEdges`, the client
//! notify path, and the final generated-serving pipeline wiring into a concrete
//! chunk storage.

pub mod star_light_engine;
pub mod star_light_provider_impl;

pub use star_light_provider_impl::SkyLightProvider;
