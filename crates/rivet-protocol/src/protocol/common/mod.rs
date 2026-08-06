//! Port of `net.minecraft.network.protocol.common` — the crossover packet bodies
//! shared by play and configuration (issue #86, join-path slice).
//!
//! One module per Java class. `packet_types` holds the `CommonPacketTypes`
//! discriminators. Each body is a value type + `stream_codec()` + `Packet`
//! impl; `handle()` stays deferred with the listener hierarchy (M1.1/#148).
//!
//! The `custom` module (`...protocol.common.custom`) holds the custom-payload
//! dispatch machinery. Bodies that are not portable yet (need a registry-wired
//! codec, a `Component` stream codec, or a value type not yet ported) are
//! deferred as STUB modules with a `blocked` note in their doc — see the
//! `clientbound_disconnect`, `clientbound_resource_pack_push`,
//! `clientbound_server_links`, `clientbound_show_dialog`, `clientbound_update_tags`,
//! and `serverbound_client_information` modules.

pub mod clientbound_clear_dialog;
pub mod clientbound_custom_payload;
pub mod clientbound_custom_report_details;
pub mod clientbound_disconnect;
pub mod clientbound_keep_alive;
pub mod clientbound_ping;
pub mod clientbound_resource_pack_pop;
pub mod clientbound_resource_pack_push;
pub mod clientbound_server_links;
pub mod clientbound_show_dialog;
pub mod clientbound_store_cookie;
pub mod clientbound_transfer;
pub mod clientbound_update_tags;
pub mod custom;
pub mod packet_types;
pub mod serverbound_client_information;
pub mod serverbound_custom_click_action;
pub mod serverbound_custom_payload;
pub mod serverbound_keep_alive;
pub mod serverbound_pong;
pub mod serverbound_resource_pack;
