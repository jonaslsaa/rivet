//! `net.minecraft.world.level.lighting` — light engine module. Only the light
//! payload *producer* (`light_update_data`) is ported so far (issue #100); the
//! propagation engines live under the owning manifest unit.
//!
//! #184 (M2) adds the cycle-breaking provider seam: [`star_light_provider`]
//! (the `StarLightProvider` trait that lets `rivet-server`'s Starlight impl
//! live outside `rivet-world`), [`level_light_engine`] (the
//! `LevelLightEngine` facade skeleton), and — Phase B —
//! [`swmr_nibble_array`] (the Starlight `SWMRNibbleArray` data surface: the
//! updating/visible copy-on-write section store with its save/vanilla
//! conversion). The `LightLayer` grid enum, the `LightEngine` propagation
//! surface, and the `LightEventListener`/storage layers defer with their
//! manifest units (`mc.world.level.lighting.core`/`.engine`); the Starlight
//! engines that consume these nibbles defer with the
//! `ca.spottedleaf.moonrise.patches.starlight.light` unit.

pub mod level_light_engine;
pub mod light_update_data;
pub mod save_util;
pub mod star_light_provider;
pub mod swmr_nibble_array;
