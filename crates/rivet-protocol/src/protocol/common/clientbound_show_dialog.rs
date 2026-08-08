//! STUB(mc.network.protocol.common) — `ClientboundShowDialogPacket` body not
//! ported.
//!
//! Java: `ClientboundShowDialogPacket.java` in `working/Paper`. The packet
//! carries a single `Holder<Dialog>` field — nothing else. The `Dialog`
//! interface (`net.minecraft.server.dialog`) exposes a `CommonDialogData` whose
//! `title` field is a `Component` serialized with the pre-existing
//! `ComponentSerialization.CODEC` over `NbtOps` (issue #89); the packet itself
//! never carries a `Component`.
//!
//! BLOCKED on the `Dialog` value type (`net.minecraft.server.dialog`).
//! RivetTodo(#207): `Dialog` needs the `ItemStackTemplate`/`Item`/
//! `DataComponentPatch` value types and `ClickEvent.ShowDialog` (the
//! `Holder<Dialog>` action), which has a Java-level cycle with `Dialog` and so
//! cannot be ported as a crate-cycle-free leaf. The `trusted_context_free_component`
//! stream codec ported in this issue (`crate::chat::trusted_context_free_component`,
//! `ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC`) does NOT serve
//! this packet — it serves `ServerLinks` custom display names — so the `Dialog`
//! tree is the only remaining blocker. Registered in play and configuration
//! clientbound but never sent on a vanilla join.
//! Discriminator: `packet_types::clientbound_show_dialog`.
