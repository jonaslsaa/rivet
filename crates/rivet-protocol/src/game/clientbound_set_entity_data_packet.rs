//! Port of `net.minecraft.network.protocol.game.ClientboundSetEntityDataPacket`
//! (MC 26.2) — `set_entity_data` (play clientbound id 99).
//!
//! Java source: `.../network/protocol/game/ClientboundSetEntityDataPacket.java`.
//! Wire body: VarInt entity id, then the packed `DataValue` items terminated by
//! the `0xFF` EOF marker. There is **no length prefix and no count prefix** —
//! the list is sentinel-terminated, and the marker is written even when the list
//! is empty.
//!
//! The buffer is `RegistryFriendlyByteBuf` in Java (the item value codecs are
//! registry-aware; `DataValue.write`/`read` port that surface). The packet
//! `handle` is a documented STUB like the serverbound slice.

use crate::codec::StreamCodec;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use crate::syncher::DataValue;

/// `ClientboundSetEntityDataPacket.EOF_MARKER` — the terminator byte.
pub const EOF_MARKER: u8 = 255;

/// `ClientboundSetEntityDataPacket` — `(id VarInt, packedItems sentinel list)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundSetEntityDataPacket {
    pub id: i32,
    pub packed_items: Vec<DataValue>,
}

impl ClientboundSetEntityDataPacket {
    /// `new ClientboundSetEntityDataPacket(int id, List<DataValue>)`.
    pub fn new(id: i32, packed_items: Vec<DataValue>) -> Self {
        ClientboundSetEntityDataPacket { id, packed_items }
    }
}

impl Packet for ClientboundSetEntityDataPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("set_entity_data")
    }
}

/// `STREAM_CODEC` — `Packet.codec(ClientboundSetEntityDataPacket::write,
/// ClientboundSetEntityDataPacket::new)`.
///
/// `pack` writes every item then the `0xFF` terminator (written even when empty);
/// `unpack` reads `id` bytes until one equals `0xFF`. Accessor ids `0..=254` are
/// legal item ids — only `255` terminates (an item with id `255` would be
/// consumed as the terminator and its serializer id + payload left trailing,
/// exactly as Java's `readUnsignedByte() != 255` loop behaves).
pub fn set_entity_data_codec()
-> StreamCodec<RegistryFriendlyByteBuf, ClientboundSetEntityDataPacket> {
    codec(
        |value: &ClientboundSetEntityDataPacket, output: &mut RegistryFriendlyByteBuf| {
            output.inner_mut().write_var_int(value.id);
            for item in &value.packed_items {
                item.write(output);
            }
            output.inner_mut().write_byte(EOF_MARKER as i8);
            Ok(())
        },
        |input: &mut RegistryFriendlyByteBuf| {
            let id = input.inner_mut().read_var_int();
            let mut result = Vec::new();
            loop {
                let item_id = input.inner_mut().read_unsigned_byte();
                if item_id == EOF_MARKER {
                    break;
                }
                result.push(DataValue::read(input, item_id));
            }
            Ok(ClientboundSetEntityDataPacket {
                id,
                packed_items: result,
            })
        },
    )
}
