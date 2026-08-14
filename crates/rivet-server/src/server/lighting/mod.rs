//! `ca.spottedleaf.moonrise.patches.starlight.light` — the server-side light
//! engine (`StarLightInterface` and its propagation engines, #184).
//!
//! Ported so far: the [`StarLightProvider`] seam (the concrete impl that plugs
//! into the `rivet-world` facade) and the Starlight flood-fill compute core as
//! `star_light_engine::SkyStarLightEngine` (exercised through the light-chunk
//! path; the provider is not yet wired to call it). What defers with #184 is
//! the real `StarLightInterface`, the block engine, the generated-serving
//! wiring into the provider, and the `blockChange`/`sectionChange`/
//! `relightChunks`/`checkChunkEdges`/client-notify paths.

pub mod star_light_engine;
pub mod star_light_provider_impl;

pub use star_light_provider_impl::StubStarLightProvider;
