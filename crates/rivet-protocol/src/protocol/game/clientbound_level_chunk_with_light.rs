//! Port of `net.minecraft.network.protocol.game.ClientboundLevelChunkWithLightPacket`
//! (issue #94).
//!
//! Java: `ClientboundLevelChunkWithLightPacket.java` in `working/Paper`. The
//! join-path chunk packet: `int x, int z` as **big-endian i32** (`writeInt`, NOT
//! VarInt — the `ClientboundLightUpdatePacket` twin uses VarInt), then the chunk
//! payload, then the light payload. The codec runs over [`RegistryFriendlyByteBuf`]
//! because the chunk payload's block-entity list decodes its `type` through the
//! `BLOCK_ENTITY_TYPE` registry.
//!
//! Wire form (verified byte-for-byte against the PR #194 capture fixture):
//! `[x i32 BE][z i32 BE][LevelChunkPacketData][LightUpdatePacketData]`.

use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::protocol::game::level_chunk_packet_data::LevelChunkPacketData;
use crate::protocol::game::light_update_packet_data::LightUpdatePacketData;
use crate::protocol::game::packet_types::clientbound_level_chunk_with_light;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;

/// `ClientboundLevelChunkWithLightPacket` — `x`/`z` (BE i32), then the chunk
/// and light payloads.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundLevelChunkWithLightPacket {
    x: i32,
    z: i32,
    chunk_data: LevelChunkPacketData,
    light_data: LightUpdatePacketData,
}

impl ClientboundLevelChunkWithLightPacket {
    /// `new ClientboundLevelChunkWithLightPacket(int x, int z, ...)`.
    pub fn new(
        x: i32,
        z: i32,
        chunk_data: LevelChunkPacketData,
        light_data: LightUpdatePacketData,
    ) -> Self {
        ClientboundLevelChunkWithLightPacket {
            x,
            z,
            chunk_data,
            light_data,
        }
    }

    /// `getX()`.
    pub fn x(&self) -> i32 {
        self.x
    }

    /// `getZ()`.
    pub fn z(&self) -> i32 {
        self.z
    }

    /// `getChunkData()`.
    pub fn chunk_data(&self) -> &LevelChunkPacketData {
        &self.chunk_data
    }

    /// `getLightData()`.
    pub fn light_data(&self) -> &LightUpdatePacketData {
        &self.light_data
    }

    /// `STREAM_CODEC`.
    pub fn stream_codec()
    -> StreamCodec<RegistryFriendlyByteBuf, ClientboundLevelChunkWithLightPacket> {
        let chunk_codec = LevelChunkPacketData::stream_codec();
        let chunk_codec_decode = chunk_codec.clone();
        let light_codec = LightUpdatePacketData::stream_codec();
        let light_codec_decode = light_codec.clone();
        codec(
            move |value: &ClientboundLevelChunkWithLightPacket,
                  output: &mut RegistryFriendlyByteBuf| {
                output.inner_mut().write_int(value.x);
                output.inner_mut().write_int(value.z);
                chunk_codec.encode(output, &value.chunk_data)?;
                light_codec.encode(output.inner_mut(), &value.light_data)?;
                Ok(())
            },
            move |input: &mut RegistryFriendlyByteBuf| {
                let x = input.inner_mut().read_int();
                let z = input.inner_mut().read_int();
                let chunk_data = chunk_codec_decode.decode(input)?;
                let light_data = light_codec_decode.decode(input.inner_mut())?;
                Ok(ClientboundLevelChunkWithLightPacket {
                    x,
                    z,
                    chunk_data,
                    light_data,
                })
            },
        )
    }
}

impl Packet for ClientboundLevelChunkWithLightPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_level_chunk_with_light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::game::heightmap_types::HeightmapType;
    use bytes::BytesMut;
    use rivet_registry::RegistryAccess;

    /// The PR #194 capture fixture's first `level_chunk_with_light` body
    /// (play clientbound id 45, coords -5/-4). All 117 chunk bodies in the
    /// capture are byte-identical apart from this 8-byte coordinate header.
    const GOLDEN_FULL: &str = include_str!("../../../tests/fixtures/chunk_golden_full.hex");
    /// `body[8:]` — the truly canonical chunk (heightmaps + palettes sorted).
    const GOLDEN_BODY: &str = include_str!("../../../tests/fixtures/chunk_golden_body.hex");
    const GOLDEN_BUFFER: &str = include_str!("../../../tests/fixtures/chunk_golden_buffer.hex");
    const GOLDEN_LIGHT: &str = include_str!("../../../tests/fixtures/chunk_golden_light.hex");
    /// `body[0..895]` — the three heightmaps (types 1/4/5, 37 longs each). Pins
    /// the heightmap region against an independent slice of the capture, so a
    /// copy-corruption of the heightmap bytes cannot drift silently.
    const GOLDEN_HEIGHTMAPS: &str =
        include_str!("../../../tests/fixtures/chunk_golden_heightmaps.hex");

    fn hex(s: &str) -> Vec<u8> {
        let trimmed: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
        (0..trimmed.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).unwrap())
            .collect()
    }

    fn decode_full() -> ClientboundLevelChunkWithLightPacket {
        let mut input = RegistryFriendlyByteBuf::new(
            BytesMut::from(hex(GOLDEN_FULL).as_slice()),
            RegistryAccess::empty(),
        );
        let packet = ClientboundLevelChunkWithLightPacket::stream_codec()
            .decode(&mut input)
            .expect("golden chunk decodes");
        assert_eq!(input.readable_bytes(), 0, "no trailing bytes");
        packet
    }

    #[test]
    fn golden_chunk_decodes_fields() {
        let packet = decode_full();
        assert_eq!(packet.x(), -5);
        assert_eq!(packet.z(), -4);
        // The heightmap region (the first 895 bytes of the canonical body,
        // which sits at byte 8 of the full packet) is pinned against the
        // independent `chunk_golden_heightmaps` fixture.
        let mut header_only =
            RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        ClientboundLevelChunkWithLightPacket::stream_codec()
            .encode(&mut header_only, &packet)
            .unwrap();
        let full_bytes = header_only.into_inner().to_vec();
        assert_eq!(
            full_bytes[8..8 + 895].to_vec(),
            hex(GOLDEN_HEIGHTMAPS),
            "heightmap region must match the independent fixture"
        );
        let chunk = packet.chunk_data();
        // 3 heightmaps (types 1/4/5), each 37 longs: 36 copies of the 9-bit
        // packed value 1 across the first 8 slots, then the 37th slot falling
        // in the last byte. A flat superflat chunk: the single stone layer at
        // y=0 gives every column height 1.
        let mut expected_heightmap = vec![0x40201008040201i64; 36];
        expected_heightmap.push(0x0000000008040201i64);
        // The three client heightmap types in EnumMap (ascending id) order,
        // each carrying the same flat-world height data.
        let expected_types = [
            HeightmapType::WorldSurface,
            HeightmapType::MotionBlocking,
            HeightmapType::MotionBlockingNoLeaves,
        ];
        assert_eq!(
            chunk
                .heightmaps()
                .iter()
                .map(|(ty, _)| *ty)
                .collect::<Vec<_>>(),
            expected_types.to_vec()
        );
        for (_, raw) in chunk.heightmaps() {
            assert_eq!(raw, &expected_heightmap);
        }
        // 2242-byte opaque sections buffer; zero block entities (fresh superflat).
        assert_eq!(chunk.buffer(), &hex(GOLDEN_BUFFER));
        assert!(chunk.block_entities().is_empty());
        // Light: skyYMask=0x06, blockYMask empty, emptySkyYMask=0x01,
        // emptyBlockYMask=0x07, 2 sky updates of 2048 bytes, 0 block updates.
        let light = packet.light_data();
        assert_eq!(light.sky_y_mask(), vec![0x06]);
        assert!(light.block_y_mask().is_empty());
        assert_eq!(light.empty_sky_y_mask(), vec![0x01]);
        assert_eq!(light.empty_block_y_mask(), vec![0x07]);
        assert_eq!(light.sky_updates().len(), 2);
        assert_eq!(light.sky_updates()[0].len(), 2048);
        assert_eq!(light.sky_updates()[1].len(), 2048);
        assert!(light.block_updates().is_empty());
    }

    #[test]
    fn golden_chunk_reencodes_byte_identical() {
        let packet = decode_full();
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        ClientboundLevelChunkWithLightPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), hex(GOLDEN_FULL));
    }

    #[test]
    fn canonical_body_and_light_match_fixtures() {
        // The body is the whole packet minus the 8-byte coordinate header; the
        // light region is the last 4130 bytes of that body. Guards the
        // sub-vector split above and pins the exact fixture bytes.
        let packet = decode_full();
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        ClientboundLevelChunkWithLightPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        assert_eq!(bytes[8..].to_vec(), hex(GOLDEN_BODY));
        // body offset 3140: after heightmaps (895) + buffer varint (2) +
        // buffer (2242) + block-entity count byte (1). The light region is the
        // last 4130 bytes of the canonical body.
        assert_eq!(
            bytes[8 + 3140..].to_vec(),
            hex(GOLDEN_LIGHT),
            "light region starts at body offset 3140 (block-entity count byte at 3139)"
        );
    }

    #[test]
    fn heightmap_wire_order_preserved_on_round_trip() {
        // The decode normalizes heightmaps to ascending type id (EnumMap order);
        // a decode -> encode round trip is byte-identical with the fixture.
        let packet = decode_full();
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        ClientboundLevelChunkWithLightPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let bytes = out.into_inner().to_vec();
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), RegistryAccess::empty());
        let again = ClientboundLevelChunkWithLightPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(
            again.chunk_data().heightmaps(),
            packet.chunk_data().heightmaps()
        );
    }
}
