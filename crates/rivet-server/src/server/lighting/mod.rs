//! `ca.spottedleaf.moonrise.patches.starlight.light` — the server-side light
//! engine (`StarLightInterface` and its propagation engines, #184).
//!
//! Phase A ports only the seam: the concrete [`StarLightProvider`] impl that
//! plugs into the `rivet-world` facade. The propagation engines defer with the
//! Starlight unit.

pub mod star_light_provider_impl;

pub use star_light_provider_impl::StubStarLightProvider;
