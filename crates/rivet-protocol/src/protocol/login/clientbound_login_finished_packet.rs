//! Port of `net.minecraft.network.protocol.login.ClientboundLoginFinishedPacket`
//! (issue #99).
//!
//! Java: `ClientboundLoginFinishedPacket.java` in `working/Paper`. The record
//! `(GameProfile gameProfile, UUID sessionId)` — sent by
//! `ServerLoginPacketListenerImpl.finishLoginAndWaitForClient` once offline
//! login is verified. `sessionId` is the per-server lazy
//! `ServerConnectionListener.getSessionId()` (`UUID.randomUUID()` on first use;
//! the non-deterministic value is canonicalized to the zero UUID in the
//! #194/#219 join fixture). Registered at login clientbound id 2; `isTerminal()`
//! is true — the packet swaps the inbound protocol to configuration.
//!
//! The wire order is profile first, then sessionId (`StreamCodec.composite` of
//! `ByteBufCodecs.GAME_PROFILE` and `UUIDUtil.STREAM_CODEC`); the client-side
//! `handleLoginFinished` is deferred with the listener hierarchy.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, composite_2};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::login::packet_types::clientbound_login_finished;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::protocol::stream_codecs::uuid_codec;
use rivet_registry::core::GameProfile;
use rivet_util::mth::Uuid;

/// `net.minecraft.network.protocol.login.ClientboundLoginFinishedPacket` — the
/// record `(GameProfile gameProfile, UUID sessionId)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundLoginFinishedPacket {
    game_profile: GameProfile,
    session_id: Uuid,
}

impl ClientboundLoginFinishedPacket {
    /// `new ClientboundLoginFinishedPacket(GameProfile gameProfile, UUID
    /// sessionId)`.
    pub fn new(game_profile: GameProfile, session_id: Uuid) -> Self {
        ClientboundLoginFinishedPacket {
            game_profile,
            session_id,
        }
    }

    /// `ClientboundLoginFinishedPacket.gameProfile()`.
    pub fn game_profile(&self) -> &GameProfile {
        &self.game_profile
    }

    /// `ClientboundLoginFinishedPacket.sessionId()`.
    pub fn session_id(&self) -> Uuid {
        self.session_id
    }

    /// `ClientboundLoginFinishedPacket.STREAM_CODEC` — `StreamCodec.composite`:
    /// `ByteBufCodecs.GAME_PROFILE` then `UUIDUtil.STREAM_CODEC`, in that wire
    /// order.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundLoginFinishedPacket> {
        composite_2(
            byte_buf_codecs::game_profile(),
            |p: &ClientboundLoginFinishedPacket| p.game_profile.clone(),
            uuid_codec(),
            |p: &ClientboundLoginFinishedPacket| p.session_id,
            ClientboundLoginFinishedPacket::new,
        )
    }
}

impl Packet for ClientboundLoginFinishedPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_login_finished()
    }

    fn is_terminal(&self) -> bool {
        true
    }
}
