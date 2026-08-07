//! STUB(mc.network.protocol.common) — `ClientboundShowDialogPacket` body not
//! ported.
//!
//! Java: `ClientboundShowDialogPacket.java` in `working/Paper`. Carries a
//! `Dialog` value (server-level dialog tree; needs `net.minecraft.server.dialog`
//! not yet ported) and a `Component`.
//!
//! BLOCKED on the `Dialog` value type (`server.dialog`).
//! RivetTodo(#207): the `Dialog` value type (server.dialog) is not ported;
//! the `Component` stream codec (#89) is also required. Registered in
//! play and configuration clientbound but never sent on a vanilla join.
//! Discriminator: `packet_types::clientbound_show_dialog`.
