//! `net.minecraft.world.level.lighting` — light engine module. Only the light
//! payload *producer* (`light_update_data`) is ported so far (issue #100); the
//! propagation engines live under the owning manifest unit.

pub mod light_update_data;
