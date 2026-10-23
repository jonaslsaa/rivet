//! Live block and level-event packet bodies from pinned Paper 26.2.
//! Section entries retain the source collection's order; this codec does not
//! invent a sorting or coalescing policy for the chunk holder.
use crate::{
    codec::{CodecError, StreamCodec, codec},
    friendly_byte_buf::FriendlyByteBuf,
    protocol::{packet::Packet, packet_type::PacketType},
    registry_friendly_byte_buf::RegistryFriendlyByteBuf,
};
use rivet_registry::{
    block_state::BlockState,
    core::{BlockPos, SectionPos},
    generated::block_states::{BLOCK_STATE_COUNT, StateId},
};

fn decode_state(id: i32) -> Result<BlockState, CodecError> {
    if id < 0 || id >= i32::from(BLOCK_STATE_COUNT) {
        return Err(CodecError::new(format!("Unknown block state id {id}")));
    }
    Ok(BlockState::new(StateId(id as u16)))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundBlockUpdatePacket {
    pub pos: BlockPos,
    pub state: BlockState,
}
impl ClientboundBlockUpdatePacket {
    pub fn stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, Self> {
        codec(
            |value: &Self, output: &mut RegistryFriendlyByteBuf| {
                output.write_block_pos(&value.pos);
                output.write_var_int(i32::from(value.state.id().0));
                Ok(())
            },
            |input: &mut RegistryFriendlyByteBuf| {
                Ok(Self {
                    pos: input.read_block_pos(),
                    state: decode_state(input.read_var_int())?,
                })
            },
        )
    }
}
impl Packet for ClientboundBlockUpdatePacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("block_update")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundSectionBlocksUpdatePacket {
    pub section: SectionPos,
    /// Source short positions (x << 8 | z << 4 | y) paired with final states.
    pub changes: Vec<(i16, BlockState)>,
}
impl ClientboundSectionBlocksUpdatePacket {
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, Self> {
        codec(
            |value: &Self, output: &mut FriendlyByteBuf| {
                output.write_long(value.section.as_long_self());
                output.write_var_int(value.changes.len() as i32);
                for (pos, state) in &value.changes {
                    output.write_var_long((i64::from(state.id().0) << 12) | i64::from(*pos));
                }
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let section = SectionPos::of_long(input.read_long());
                let count = input.read_var_int();
                if count < 0 {
                    return Err(CodecError::new("negative block-change array size"));
                }
                let mut changes = Vec::new();
                for _ in 0..count {
                    let packed = input.read_var_long();
                    changes.push((
                        (packed & 4095) as i16,
                        decode_state(((packed as u64) >> 12) as i32)?,
                    ));
                }
                Ok(Self { section, changes })
            },
        )
    }

    pub fn run_updates(&self, mut update: impl FnMut(BlockPos, BlockState)) {
        for (pos, state) in &self.changes {
            update(
                BlockPos::new(
                    (self.section.x() << 4) + (i32::from(*pos) >> 8 & 15),
                    (self.section.y() << 4) + (i32::from(*pos) & 15),
                    (self.section.z() << 4) + (i32::from(*pos) >> 4 & 15),
                ),
                *state,
            );
        }
    }
}
impl Packet for ClientboundSectionBlocksUpdatePacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("section_blocks_update")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundLevelEventPacket {
    pub event: i32,
    pub pos: BlockPos,
    pub data: i32,
    pub global: bool,
}
impl ClientboundLevelEventPacket {
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, Self> {
        codec(
            |value: &Self, output: &mut FriendlyByteBuf| {
                output.write_int(value.event);
                output.write_long(value.pos.as_long());
                output.write_int(value.data);
                output.write_boolean(value.global);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                Ok(Self {
                    event: input.read_int(),
                    pos: BlockPos::of_long(input.read_long()),
                    data: input.read_int(),
                    global: input.read_boolean(),
                })
            },
        )
    }
}
impl Packet for ClientboundLevelEventPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("level_event")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;
    use rivet_registry::RegistryAccess;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn live_block_packet_bodies_match_actual_paper() {
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/live-block-packets.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let coords = row["pos"].as_array().unwrap();
            let pos = BlockPos::new(
                coords[0].as_i64().unwrap() as i32,
                coords[1].as_i64().unwrap() as i32,
                coords[2].as_i64().unwrap() as i32,
            );
            let state = decode_state(row["state"].as_i64().unwrap() as i32).unwrap();
            let packet = ClientboundBlockUpdatePacket { pos, state };
            let mut buffer = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
            ClientboundBlockUpdatePacket::stream_codec()
                .encode(&mut buffer, &packet)
                .unwrap();
            assert_eq!(hex(buffer.as_slice()), row["block"].as_str().unwrap());
            assert_eq!(
                ClientboundBlockUpdatePacket::stream_codec()
                    .decode(&mut buffer)
                    .unwrap(),
                packet
            );
            let packet = ClientboundSectionBlocksUpdatePacket {
                section: SectionPos::of_block_pos(&pos),
                changes: row["changes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| {
                        (
                            entry[0].as_i64().unwrap() as i16,
                            decode_state(entry[1].as_i64().unwrap() as i32).unwrap(),
                        )
                    })
                    .collect(),
            };
            let mut buffer = FriendlyByteBuf::new(BytesMut::new());
            ClientboundSectionBlocksUpdatePacket::stream_codec()
                .encode(&mut buffer, &packet)
                .unwrap();
            assert_eq!(hex(buffer.as_slice()), row["section"].as_str().unwrap());
            assert_eq!(
                ClientboundSectionBlocksUpdatePacket::stream_codec()
                    .decode(&mut buffer)
                    .unwrap(),
                packet
            );
            for (index, global) in [false, true].into_iter().enumerate() {
                let packet = ClientboundLevelEventPacket {
                    event: 1501,
                    pos,
                    data: i32::from(state.id().0),
                    global,
                };
                let mut buffer = FriendlyByteBuf::new(BytesMut::new());
                ClientboundLevelEventPacket::stream_codec()
                    .encode(&mut buffer, &packet)
                    .unwrap();
                assert_eq!(
                    hex(buffer.as_slice()),
                    row["events"][index].as_str().unwrap()
                );
                assert_eq!(
                    ClientboundLevelEventPacket::stream_codec()
                        .decode(&mut buffer)
                        .unwrap(),
                    packet
                );
            }
        }
    }
}
