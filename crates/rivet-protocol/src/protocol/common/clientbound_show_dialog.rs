//! STUB — `net.minecraft.network.protocol.common.ClientboundShowDialogPacket`.
//!
//! Java: `ClientboundShowDialogPacket.java` in `working/Paper`. Carries a
//! `Dialog` value (server-level dialog tree; needs `net.minecraft.server.dialog`
//! not yet ported) and a `Component`.
//!
//! BLOCKED on the `Dialog` value type (`server.dialog`) plus the `Component`
//! stream codec (epic #12/#98). Registered in play and configuration
//! clientbound but never sent on a vanilla join. Discriminator:
//! `packet_types::clientbound_show_dialog`.
