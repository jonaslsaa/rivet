//! `net.minecraft.world.level.lighting` — light engine module. Only the light
//! payload *producer* (`light_update_data`) is ported so far (issue #100); the
//! propagation engines live under the owning manifest unit.
//!
//! #184 (M2) adds the cycle-breaking provider seam: [`light_layer`] (the
//! two-light-grid enum), [`star_light_provider`] (the `StarLightProvider` trait
//! that lets `rivet-server`'s Starlight impl live outside `rivet-world`), and
//! [`level_light_engine`] (the `LevelLightEngine` facade skeleton). The
//! `LightEngine` propagation surface and the `LightEventListener`/storage
//! layers defer with their manifest units (`mc.world.level.lighting.core`/
//! `.engine`).

pub mod level_light_engine;
pub mod light_layer;
pub mod light_update_data;
pub mod star_light_provider;
