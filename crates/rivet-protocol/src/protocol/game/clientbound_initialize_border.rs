//! Port of `net.minecraft.network.protocol.game.ClientboundInitializeBorderPacket`
//! (issue #87) — `initialize_border` (play clientbound id 43).
//!
//! Java source: `.../network/protocol/game/ClientboundInitializeBorderPacket.java`.
//! Wire body: `newCenterX`, `newCenterZ`, `oldSize`, `newSize` doubles, then a
//! `lerpTime` VarLong, then `newAbsoluteMaxSize`, `warningBlocks`, `warningTime`
//! VarInts — the `WorldBorder` snapshot the server sends on join. The `WorldBorder`
//! constructor (from a `WorldBorder` value) is the server-side half and is
//! deferred with the world-border unit (M2); the capture-proven wire shape is
//! fully ported here.

use crate::codec::{StreamCodec, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_initialize_border;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundInitializeBorderPacket` — the border snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClientboundInitializeBorderPacket {
    /// `newCenterX`.
    new_center_x: f64,
    /// `newCenterZ`.
    new_center_z: f64,
    /// `oldSize`.
    old_size: f64,
    /// `newSize`.
    new_size: f64,
    /// `lerpTime`.
    lerp_time: i64,
    /// `newAbsoluteMaxSize`.
    new_absolute_max_size: i32,
    /// `warningBlocks`.
    warning_blocks: i32,
    /// `warningTime`.
    warning_time: i32,
}

impl ClientboundInitializeBorderPacket {
    /// The all-args constructor (the decode ctor / the `WorldBorder` snapshot
    /// ctor both collapse to this shape).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        new_center_x: f64,
        new_center_z: f64,
        old_size: f64,
        new_size: f64,
        lerp_time: i64,
        new_absolute_max_size: i32,
        warning_blocks: i32,
        warning_time: i32,
    ) -> Self {
        ClientboundInitializeBorderPacket {
            new_center_x,
            new_center_z,
            old_size,
            new_size,
            lerp_time,
            new_absolute_max_size,
            warning_blocks,
            warning_time,
        }
    }

    /// `ClientboundInitializeBorderPacket.getNewCenterX()`.
    pub fn get_new_center_x(&self) -> f64 {
        self.new_center_x
    }

    /// `ClientboundInitializeBorderPacket.getNewCenterZ()`.
    pub fn get_new_center_z(&self) -> f64 {
        self.new_center_z
    }

    /// `ClientboundInitializeBorderPacket.getOldSize()`.
    pub fn get_old_size(&self) -> f64 {
        self.old_size
    }

    /// `ClientboundInitializeBorderPacket.getNewSize()`.
    pub fn get_new_size(&self) -> f64 {
        self.new_size
    }

    /// `ClientboundInitializeBorderPacket.getLerpTime()`.
    pub fn get_lerp_time(&self) -> i64 {
        self.lerp_time
    }

    /// `ClientboundInitializeBorderPacket.getNewAbsoluteMaxSize()`.
    pub fn get_new_absolute_max_size(&self) -> i32 {
        self.new_absolute_max_size
    }

    /// `ClientboundInitializeBorderPacket.getWarningBlocks()`.
    pub fn get_warning_blocks(&self) -> i32 {
        self.warning_blocks
    }

    /// `ClientboundInitializeBorderPacket.getWarningTime()`.
    pub fn get_warning_time(&self) -> i32 {
        self.warning_time
    }

    /// `STREAM_CODEC` — `Packet.codec(write, read)`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundInitializeBorderPacket> {
        codec(
            |value: &ClientboundInitializeBorderPacket, output: &mut FriendlyByteBuf| {
                output.write_double(value.new_center_x);
                output.write_double(value.new_center_z);
                output.write_double(value.old_size);
                output.write_double(value.new_size);
                output.write_var_long(value.lerp_time);
                output.write_var_int(value.new_absolute_max_size);
                output.write_var_int(value.warning_blocks);
                output.write_var_int(value.warning_time);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                Ok(ClientboundInitializeBorderPacket {
                    new_center_x: input.read_double(),
                    new_center_z: input.read_double(),
                    old_size: input.read_double(),
                    new_size: input.read_double(),
                    lerp_time: input.read_var_long(),
                    new_absolute_max_size: input.read_var_int(),
                    warning_blocks: input.read_var_int(),
                    warning_time: input.read_var_int(),
                })
            },
        )
    }
}

impl Packet for ClientboundInitializeBorderPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_initialize_border()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    /// Hex string -> `Vec<u8>` (test helper).
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture (40 bytes):
        // newCenterX/Z 0.0, oldSize/newSize 59999968.0 (0x418C9C3700000000),
        // lerpTime 0 (VarLong), absMax 29999984 (0xF086A70E), warningBlocks 5,
        // warningTime 300 (0xAC02).
        let bytes =
            hex("00000000000000000000000000000000418c9c3700000000418c9c370000000000f086a70e05ac02");
        assert_eq!(bytes.len(), 40);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundInitializeBorderPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(input.readable_bytes(), 0);
        assert_eq!(decoded.get_new_center_x(), 0.0);
        assert_eq!(decoded.get_new_center_z(), 0.0);
        assert_eq!(decoded.get_old_size(), 59_999_968.0);
        assert_eq!(decoded.get_new_size(), 59_999_968.0);
        assert_eq!(decoded.get_lerp_time(), 0);
        assert_eq!(decoded.get_new_absolute_max_size(), 29_999_984);
        assert_eq!(decoded.get_warning_blocks(), 5);
        assert_eq!(decoded.get_warning_time(), 300);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundInitializeBorderPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), bytes);
    }
}
