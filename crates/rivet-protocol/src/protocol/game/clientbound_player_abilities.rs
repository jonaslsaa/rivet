//! Port of `net.minecraft.network.protocol.game.ClientboundPlayerAbilitiesPacket`
//! (issue #87) — `player_abilities` (play clientbound id 64).
//!
//! Java source: `.../network/protocol/game/ClientboundPlayerAbilitiesPacket.java`.
//! Wire body: a bitfield byte (`FLAG_INVULNERABLE = 1`, `FLAG_FLYING = 2`,
//! `FLAG_CAN_FLY = 4`, `FLAG_INSTABUILD = 8`), then `flyingSpeed` float and
//! `walkingSpeed` float. The captured golden body is `003d4ccccd3dcccccd` —
//! no flags, `flyingSpeed = 0.05f`, `walkingSpeed = 0.1f` (the client default
//! speeds the `Abilities` constructor seeds).

use crate::codec::{StreamCodec, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_player_abilities;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `FLAG_INVULNERABLE`.
const FLAG_INVULNERABLE: i8 = 1;
/// `FLAG_FLYING`.
const FLAG_FLYING: i8 = 2;
/// `FLAG_CAN_FLY`.
const FLAG_CAN_FLY: i8 = 4;
/// `FLAG_INSTABUILD`.
const FLAG_INSTABUILD: i8 = 8;

/// `ClientboundPlayerAbilitiesPacket` — the flags + two speeds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClientboundPlayerAbilitiesPacket {
    /// `invulnerable`.
    invulnerable: bool,
    /// `isFlying`.
    is_flying: bool,
    /// `canFly`.
    can_fly: bool,
    /// `instabuild`.
    instabuild: bool,
    /// `flyingSpeed`.
    flying_speed: f32,
    /// `walkingSpeed`.
    walking_speed: f32,
}

impl ClientboundPlayerAbilitiesPacket {
    /// The all-args constructor (the Java decode ctor / the `Abilities`
    /// snapshot ctor both collapse to this shape).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        invulnerable: bool,
        is_flying: bool,
        can_fly: bool,
        instabuild: bool,
        flying_speed: f32,
        walking_speed: f32,
    ) -> Self {
        ClientboundPlayerAbilitiesPacket {
            invulnerable,
            is_flying,
            can_fly,
            instabuild,
            flying_speed,
            walking_speed,
        }
    }

    /// `ClientboundPlayerAbilitiesPacket.isInvulnerable()`.
    pub fn is_invulnerable(&self) -> bool {
        self.invulnerable
    }

    /// `ClientboundPlayerAbilitiesPacket.isFlying()`.
    pub fn is_flying(&self) -> bool {
        self.is_flying
    }

    /// `ClientboundPlayerAbilitiesPacket.canFly()`.
    pub fn can_fly(&self) -> bool {
        self.can_fly
    }

    /// `ClientboundPlayerAbilitiesPacket.canInstabuild()`.
    pub fn can_instabuild(&self) -> bool {
        self.instabuild
    }

    /// `ClientboundPlayerAbilitiesPacket.getFlyingSpeed()`.
    pub fn get_flying_speed(&self) -> f32 {
        self.flying_speed
    }

    /// `ClientboundPlayerAbilitiesPacket.getWalkingSpeed()`.
    pub fn get_walking_speed(&self) -> f32 {
        self.walking_speed
    }

    /// `STREAM_CODEC` — `Packet.codec(write, read)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundPlayerAbilitiesPacket> {
        codec(
            |value: &ClientboundPlayerAbilitiesPacket, output: &mut FriendlyByteBuf| {
                let mut bitfield: i8 = 0;
                if value.invulnerable {
                    bitfield |= FLAG_INVULNERABLE;
                }
                if value.is_flying {
                    bitfield |= FLAG_FLYING;
                }
                if value.can_fly {
                    bitfield |= FLAG_CAN_FLY;
                }
                if value.instabuild {
                    bitfield |= FLAG_INSTABUILD;
                }
                output.write_byte(bitfield);
                output.write_float(value.flying_speed);
                output.write_float(value.walking_speed);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let bitfield = input.read_byte();
                Ok(ClientboundPlayerAbilitiesPacket {
                    invulnerable: (bitfield & FLAG_INVULNERABLE) != 0,
                    is_flying: (bitfield & FLAG_FLYING) != 0,
                    can_fly: (bitfield & FLAG_CAN_FLY) != 0,
                    instabuild: (bitfield & FLAG_INSTABUILD) != 0,
                    flying_speed: input.read_float(),
                    walking_speed: input.read_float(),
                })
            },
        )
    }
}

impl Packet for ClientboundPlayerAbilitiesPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_player_abilities()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    /// Hex string -> `Vec<u8>` (test helper, same shape as the integration tests).
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture: `003d4ccccd3dcccccd` — no flags, 0.05f / 0.1f.
        let bytes = hex("003d4ccccd3dcccccd");
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundPlayerAbilitiesPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(input.readable_bytes(), 0);
        assert!(!decoded.is_invulnerable());
        assert!(!decoded.is_flying());
        assert!(!decoded.can_fly());
        assert!(!decoded.can_instabuild());
        assert_eq!(decoded.get_flying_speed(), 0.05);
        assert_eq!(decoded.get_walking_speed(), 0.1);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundPlayerAbilitiesPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), bytes);
    }

    #[test]
    fn all_flags_bitfield_round_trips() {
        let packet = ClientboundPlayerAbilitiesPacket::new(true, true, true, true, 0.05, 0.1);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundPlayerAbilitiesPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        // All four flags -> bitfield 0x0F, then the two speeds.
        assert_eq!(out.as_slice()[0], 0x0F);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ClientboundPlayerAbilitiesPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }
}
