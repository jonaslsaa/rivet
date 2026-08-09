//! Port of `net.minecraft.network.protocol.game.ClientboundPlayerInfoRemovePacket`
//! (issue #87) — `player_info_remove` (play clientbound id 69).
//!
//! Java source: `.../network/protocol/game/ClientboundPlayerInfoRemovePacket.java`.
//! Wire body: a varint-counted list of UUIDs (`writeCollection(profileIds,
//! UUIDUtil.STREAM_CODEC)`). It is the counterpart to
//! [`clientbound_player_info_update`]; the #153 join capture never emits it (a
//! join adds players but does not remove them), so it has no golden fixture —
//! the codec is exercised by the hostile/truncation/mutation tests and the
//! registration test instead.
//!
//! [`clientbound_player_info_update`]: crate::protocol::game::clientbound_player_info_update

use crate::codec::{StreamCodec, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_player_info_remove;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_util::uuid::Uuid;

/// `ClientboundPlayerInfoRemovePacket` — the record `(List<UUID> profileIds)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundPlayerInfoRemovePacket {
    /// `profileIds`.
    profile_ids: Vec<Uuid>,
}

impl ClientboundPlayerInfoRemovePacket {
    /// The record's canonical constructor.
    pub fn new(profile_ids: Vec<Uuid>) -> Self {
        ClientboundPlayerInfoRemovePacket { profile_ids }
    }

    /// `ClientboundPlayerInfoRemovePacket.profileIds()`.
    pub fn profile_ids(&self) -> &[Uuid] {
        &self.profile_ids
    }

    /// `STREAM_CODEC` — `Packet.codec(write, new(FriendlyByteBuf))`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundPlayerInfoRemovePacket> {
        codec(
            |packet: &ClientboundPlayerInfoRemovePacket, output: &mut FriendlyByteBuf| {
                output.write_var_int(packet.profile_ids.len() as i32);
                for id in &packet.profile_ids {
                    output.write_uuid(*id);
                }
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let count = input.read_var_int();
                let mut profile_ids = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    profile_ids.push(input.read_uuid());
                }
                Ok(ClientboundPlayerInfoRemovePacket { profile_ids })
            },
        )
    }
}

impl Packet for ClientboundPlayerInfoRemovePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_player_info_remove()
    }
}
