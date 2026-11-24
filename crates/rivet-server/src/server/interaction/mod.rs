//! The typed tick-thread world-interaction service E9 consumes (issue #448).
//!
//! This module is the cross-issue seam, not a Java class: it holds the
//! interaction slice of `ServerGamePacketListenerImpl` that #448 owns —
//! the digging arms of `handlePlayerAction`, `handleUseItemOn`,
//! `handleUseItem`, `ackBlockChangesUpTo`, the `ackBlockChangesUpTo` flush at
//! the head of `tick()`, and Spigot's `checkLimit` — so E9, which exclusively
//! owns the listener, can delegate those packets to one typed entry point.
//!
//! The Java classes the slice calls live in their mirrored homes:
//! - `ServerPlayerGameMode` → `crate::server::level::server_player_game_mode`
//! - the packets → `rivet_protocol::game::{serverbound_use_item_on_packet,
//!   serverbound_use_item_packet, clientbound_block_changed_ack_packet,
//!   clientbound_block_destruction_packet}` (`ServerboundPlayerActionPacket`
//!   and `ClientboundBlockUpdatePacket` were already there)
//! - `InteractionHand` / `BlockHitResult` → `rivet_registry::core` (below
//!   `rivet-protocol`, like `Vec3`/`Difficulty`)
//! - `InteractionResult` → `rivet_world::interaction_result`
//!
//! Malformed frames are rejected by the play decode boundary (the protocol
//! codecs plus E9's trailing-byte check), as netty's `DecoderException` path
//! does in Paper; this service only sees decoded packets.
//!
//! Target-entity attacks belong to #23 and container clicks to #22.

pub mod service;

pub use service::{
    INVALID_SEQUENCE_DISCONNECT_MESSAGE, PAPER_DEFAULT_INCOMING_PACKET_THRESHOLD,
    WorldInteractionService,
};
