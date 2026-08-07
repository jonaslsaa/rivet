//! Port of `net.minecraft.network.protocol.game.ClientboundLightUpdatePacket`
//! (issue #94).
//!
//! Java: `ClientboundLightUpdatePacket.java` in `working/Paper`. The standalone
//! light-change packet: `x`/`z` as **VarInt** (NOT big-endian i32 — the chunk
//! packet twin uses `writeInt`), then the shared light payload. The whole codec
//! is registry-independent (`FriendlyByteBuf`).

use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::light_update_packet_data::LightUpdatePacketData;
use crate::protocol::game::packet_types::clientbound_light_update;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundLightUpdatePacket` — `x`/`z` (VarInt), then the light payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundLightUpdatePacket {
    x: i32,
    z: i32,
    light_data: LightUpdatePacketData,
}

impl ClientboundLightUpdatePacket {
    /// `new ClientboundLightUpdatePacket(ChunkPos, ...)` — the packet-body value
    /// constructor takes the two chunk coordinates plus the shared light payload.
    pub fn new(x: i32, z: i32, light_data: LightUpdatePacketData) -> Self {
        ClientboundLightUpdatePacket { x, z, light_data }
    }

    /// `getX()`.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `getZ()`.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// `getLightData()`.
    pub fn light_data(&self) -> &LightUpdatePacketData {
        &self.light_data
    }

    /// `STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundLightUpdatePacket> {
        let light_codec = LightUpdatePacketData::stream_codec();
        let light_codec_decode = light_codec.clone();
        codec(
            move |value: &ClientboundLightUpdatePacket, output: &mut FriendlyByteBuf| {
                output.write_var_int(value.x);
                output.write_var_int(value.z);
                light_codec.encode(output, &value.light_data)?;
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let x = input.read_var_int();
                let z = input.read_var_int();
                let light_data = light_codec_decode.decode(input)?;
                Ok(ClientboundLightUpdatePacket { x, z, light_data })
            },
        )
    }
}

impl Packet for ClientboundLightUpdatePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_light_update()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;

    #[test]
    fn var_int_coords_then_payload_round_trips() {
        // VarInt x/z, then the light payload (4 masks + 2 layer lists).
        let data = LightUpdatePacketData::new(
            vec![0x06],
            vec![],
            vec![0x01],
            vec![0x07],
            vec![vec![0xAB; 2048]],
            vec![],
        );
        let packet = ClientboundLightUpdatePacket::new(-12, 300, data.clone());
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundLightUpdatePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // x = -12 -> 5-byte varint 0xF4 0xFF 0xFF 0xFF 0x0F (two's-complement
        // 32-bit), then z = 300 -> 0xAC 0x02.
        assert_eq!(&bytes[0..5], &[0xF4, 0xFF, 0xFF, 0xFF, 0x0F]);
        assert_eq!(&bytes[5..7], &[0xAC, 0x02]);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundLightUpdatePacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded, packet);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn multi_word_sky_mask_wire_form_matches_bit_set_to_long_array() {
        // A 2-word `skyYMask` (bits 0 and 64 set) pins the cross-word wire
        // order: `BitSet.toLongArray()` -> `writeLongArray` writes varint count
        // 2, then each word big-endian. A word-order swap inside the port would
        // fail this byte-for-byte assert even though it would round-trip.
        let data = LightUpdatePacketData::new(
            vec![0x0000_0000_0000_0001u64, 0x0000_0000_0000_0001u64],
            vec![],
            vec![],
            vec![],
            vec![],
            vec![],
        );
        let packet = ClientboundLightUpdatePacket::new(0, 0, data);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundLightUpdatePacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        // VarInt x/z (both 0), then skyYMask: count 2 + two BE longs, then the
        // remaining three empty masks (count 0 each) and two empty lists.
        assert_eq!(&bytes[0..2], &[0x00, 0x00], "x and z are VarInt 0");
        assert_eq!(bytes[2], 2, "skyYMask word count");
        assert_eq!(
            &bytes[3..11],
            &0x0000_0000_0000_0001u64.to_be_bytes(),
            "word 0 big-endian"
        );
        assert_eq!(
            &bytes[11..19],
            &0x0000_0000_0000_0001u64.to_be_bytes(),
            "word 1 big-endian"
        );
        // Three empty masks + two empty layer lists: five 0 bytes.
        assert_eq!(&bytes[19..24], &[0, 0, 0, 0, 0]);
        assert_eq!(bytes.len(), 24);

        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        let decoded = ClientboundLightUpdatePacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(
            decoded.light_data().sky_y_mask(),
            vec![0x0000_0000_0000_0001u64, 0x0000_0000_0000_0001u64]
        );
        assert_eq!(input.readable_bytes(), 0);
    }
}
