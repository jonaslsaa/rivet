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
//! conversion). The `mc.world.level.lighting.core` unit lands here too:
//! [`data_layer_storage_map`] (the section-node → `DataLayer` storage),
//! [`dynamic_graph_min_fixed_point`] (the min-fixed-point propagation graph +
//! its [`DynamicGraphNode`](dynamic_graph_min_fixed_point::DynamicGraphNode)
//! subclass seam), [`leveled_priority_queue`] (the graph's bucket queue),
//! [`spatial_long_set`] (the packed spatial long set), and the live
//! [`light_event_listener`]/[`layer_light_event_listener`] interfaces the
//! engines consume. The `LightEngine` propagation surface and the
//! `LayerLightSectionStorage`/`BlockLightSectionStorage`/`SkyLightSectionStorage`
//! storages defer with the `mc.world.level.lighting.engine` unit; the Starlight
//! engines that consume these nibbles defer with the
//! `ca.spottedleaf.moonrise.patches.starlight.light` unit.

pub mod data_layer_storage_map;
pub mod dynamic_graph_min_fixed_point;
pub mod layer_light_event_listener;
pub mod level_light_engine;
pub mod leveled_priority_queue;
pub mod light_event_listener;
pub mod light_update_data;
pub mod spatial_long_set;
pub mod star_light_provider;
pub mod swmr_nibble_array;
