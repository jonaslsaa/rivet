//! Authoritative tick-thread world-interaction service (Paper 26.2, issue #448 / B13).
//!
//! Source of truth: Paper 26.2 (`0a993450f129c4942c2a9ed45ba047412b4667cf`):
//! - `net.minecraft.network.protocol.game.ServerboundUseItemOnPacket`
//! - `net.minecraft.network.protocol.game.ServerboundUseItemPacket`
//! - `net.minecraft.network.protocol.game.ClientboundBlockChangedAckPacket`
//! - `net.minecraft.network.protocol.game.ClientboundBlockDestructionPacket`
//! - `net.minecraft.server.level.ServerPlayerGameMode`
//! - `net.minecraft.server.network.ServerGamePacketListenerImpl` (interaction ingress
//!   validation, sequence acknowledgement, and correction dispatch)
//! - `net.minecraft.world.InteractionResult`
//! - `net.minecraft.world.entity.player.Player` (block interaction reach and adventure
//!   restriction checks)
//!
//! Ownership boundary (E9 / B13):
//! - E9 exclusively owns `ServerGamePacketListenerImpl` and network frame routing.
//! - This module publishes the typed tick-thread [`WorldInteractionService`] and
//!   [`ServerPlayerGameMode`] state machine consumed by E9, returning ordered
//!   [`CorrectionPacket`] outputs for E9 to dispatch on the player's outbound channel.
//! - Target-entity attacks belong to #23 and container clicks belong to #22.

pub mod game_mode;
pub mod packets;
pub mod service;
pub mod validation;

pub use game_mode::{
    CorrectionPacket, InteractionPlayer, InteractionResult, InteractionWorld, PaperSuccessContext,
    ServerPlayerGameMode, SwingSource,
};
pub use packets::{
    BlockHitResult, ClientboundBlockChangedAckPacket, ClientboundBlockDestructionPacket,
    InteractionHand, InteractionPacketError, ServerboundUseItemOnPacket, ServerboundUseItemPacket,
    decode_player_action, decode_use_item, decode_use_item_on,
};
pub use service::WorldInteractionService;
pub use validation::{
    DEFAULT_BLOCK_INTERACTION_RANGE, HitLocationError, INTERACTION_RANGE_BUFFER,
    InvalidSequenceError, MAX_HIT_LOCATION_OFFSET, SequenceAckTracker, aabb_block_distance_to_sqr,
    is_within_block_interaction_range, validate_hit_location,
};
