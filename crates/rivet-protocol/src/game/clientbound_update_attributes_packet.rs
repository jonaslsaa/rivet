//! Port of `net.minecraft.network.protocol.game.ClientboundUpdateAttributesPacket`
//! (MC 26.2) — `update_attributes` (play clientbound id 131).
//!
//! Java source: `.../network/protocol/game/ClientboundUpdateAttributesPacket.java`.
//! Wire body: VarInt entity id, then `ByteBufCodecs.list(128)` attribute
//! snapshots (VarInt count, **max 128**), each a `holderRegistry(ATTRIBUTE)`
//! VarInt holder id, a double base value, and a VarInt-counted list of
//! `AttributeModifier`s (Identifier string, double amount, VarInt Operation
//! `idMapper`). The buffer is `RegistryFriendlyByteBuf` (the holder ids resolve
//! through the buffer's `RegistryAccess`).
//!
//! `AttributeModifier`/`Operation` are ported here because this slice's only
//! consumer is this packet; the full attribute unit (M3) may move them.
//! `handle` is a documented STUB like the serverbound slice.

use crate::codec::byte_buf_codecs::{self, read_count, write_count};
use crate::codec::registry_byte_buf_codecs::{holder_registry, identifier_stream_codec, lift};
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, composite_3, of};
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::Identifier;
use rivet_registry::holder::Holder;
use rivet_registry::registries::{ATTRIBUTE, Attribute};

/// `ByteBufCodecs.list(128)` — the snapshot count ceiling.
pub const MAX_SNAPSHOTS: i32 = 128;

/// `AttributeModifier.Operation` — the wire id-mapped operation enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// `AttributeModifier.Operation.ADD_VALUE` — id 0.
    AddValue,
    /// `ADD_MULTIPLIED_BASE` — id 1.
    AddMultipliedBase,
    /// `ADD_MULTIPLIED_TOTAL` — id 2.
    AddMultipliedTotal,
}

impl Operation {
    /// `Operation.id()`.
    pub fn id(self) -> i32 {
        match self {
            Operation::AddValue => 0,
            Operation::AddMultipliedBase => 1,
            Operation::AddMultipliedTotal => 2,
        }
    }

    /// `Operation.BY_ID` — `ByIdMap.continuous(id, values(), ZERO)`. The ZERO
    /// out-of-bounds strategy maps any id outside `0..=2` to `values[0]` =
    /// `ADD_VALUE`.
    pub fn by_id(id: i32) -> Operation {
        match id {
            1 => Operation::AddMultipliedBase,
            2 => Operation::AddMultipliedTotal,
            _ => Operation::AddValue,
        }
    }
}

/// `AttributeModifier` — the `(Identifier id, double amount, Operation)` wire
/// value (record). The full attribute-unit model (CODEC/MAP_CODEC) is deferred;
/// this slice ports the `STREAM_CODEC` the update-attributes packet uses.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeModifier {
    pub id: Identifier,
    pub amount: f64,
    pub operation: Operation,
}

impl AttributeModifier {
    /// `new AttributeModifier(Identifier, double, Operation)`.
    pub fn new(id: Identifier, amount: f64, operation: Operation) -> Self {
        AttributeModifier {
            id,
            amount,
            operation,
        }
    }

    /// `AttributeModifier.STREAM_CODEC` — `composite(Identifier.STREAM_CODEC,
    /// DOUBLE, Operation.STREAM_CODEC)`. `DOUBLE` and the operation `idMapper`
    /// are `ByteBufCodecs` base-buffer primitives, lifted to
    /// `RegistryFriendlyByteBuf` (Java's `? super B` wildcard composition).
    pub fn modifier_stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, AttributeModifier> {
        composite_3(
            identifier_stream_codec(),
            |m: &AttributeModifier| m.id.clone(),
            lift(byte_buf_codecs::double()),
            |m: &AttributeModifier| m.amount,
            lift(byte_buf_codecs::id_mapper(
                Operation::by_id,
                |op: &Operation| op.id(),
            )),
            |m: &AttributeModifier| m.operation,
            AttributeModifier::new,
        )
    }
}

/// `ClientboundUpdateAttributesPacket.AttributeSnapshot` — one
/// `(Holder<Attribute> holder, double base, Collection<AttributeModifier>)`.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributeSnapshot {
    pub attribute: Holder<Attribute>,
    pub base: f64,
    pub modifiers: Vec<AttributeModifier>,
}

impl AttributeSnapshot {
    /// `new AttributeSnapshot(Holder<Attribute>, double, Collection<AttributeModifier>)`.
    pub fn new(attribute: Holder<Attribute>, base: f64, modifiers: Vec<AttributeModifier>) -> Self {
        AttributeSnapshot {
            attribute,
            base,
            modifiers,
        }
    }

    /// `AttributeSnapshot.STREAM_CODEC` — `composite(Attribute.STREAM_CODEC,
    /// DOUBLE, MODIFIER_STREAM_CODEC.apply(collection(ArrayList::new)))`.
    ///
    /// `Attribute.STREAM_CODEC` is `ByteBufCodecs.holderRegistry(Registries.
    /// ATTRIBUTE)`; the modifiers are varint-counted (Java's unbounded
    /// `collection(ArrayList::new)` — no `maxSize`, so `Integer.MAX_VALUE`).
    pub fn snapshot_stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, AttributeSnapshot> {
        composite_3(
            holder_registry(&*ATTRIBUTE),
            |s: &AttributeSnapshot| s.attribute.clone(),
            lift(byte_buf_codecs::double()),
            |s: &AttributeSnapshot| s.base,
            modifier_list_codec(),
            |s: &AttributeSnapshot| s.modifiers.clone(),
            AttributeSnapshot::new,
        )
    }
}

/// `MODIFIER_STREAM_CODEC.apply(ByteBufCodecs.collection(ArrayList::new))` — the
/// varint-counted modifiers list, lifted to `RegistryFriendlyByteBuf` (Java's
/// unbounded `collection`, `Integer.MAX_VALUE` maxSize).
fn modifier_list_codec() -> StreamCodec<RegistryFriendlyByteBuf, Vec<AttributeModifier>> {
    let element = AttributeModifier::modifier_stream_codec();
    let encoder = element.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, value: &Vec<AttributeModifier>| {
            write_count(output.inner_mut(), value.len() as i32, i32::MAX)?;
            for m in value {
                encoder.encode(output, m)?;
            }
            Ok(())
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            let count = read_count(input.inner_mut(), i32::MAX)?;
            // Java `collection(ArrayList::new)` -> `new ArrayList<>(Math.min(count,
            // 65536))`; a negative count passes `readCount` (it only upper-bounds)
            // and then throws `IllegalArgumentException("Illegal Capacity: -n")`
            // — a catchable error, never a capacity overflow.
            let capacity = count.min(byte_buf_codecs::MAX_INITIAL_COLLECTION_SIZE);
            if capacity < 0 {
                panic!("Illegal Capacity: {capacity}");
            }
            let mut out = Vec::with_capacity(capacity as usize);
            for _ in 0..count {
                out.push(element.decode(input)?);
            }
            Ok(out)
        },
    )
}

/// `ClientboundUpdateAttributesPacket` — `(entityId VarInt, snapshots list ≤128)`.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundUpdateAttributesPacket {
    pub entity_id: i32,
    pub attributes: Vec<AttributeSnapshot>,
}

impl ClientboundUpdateAttributesPacket {
    /// `new ClientboundUpdateAttributesPacket(int entityId, List<AttributeSnapshot>)`.
    pub fn new(entity_id: i32, attributes: Vec<AttributeSnapshot>) -> Self {
        ClientboundUpdateAttributesPacket {
            entity_id,
            attributes,
        }
    }

    /// `getEntityId()`.
    pub fn get_entity_id(&self) -> i32 {
        self.entity_id
    }

    /// `getValues()`.
    pub fn get_values(&self) -> &[AttributeSnapshot] {
        &self.attributes
    }
}

impl Packet for ClientboundUpdateAttributesPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::clientbound("update_attributes")
    }
}

/// `STREAM_CODEC` — `StreamCodec.composite(VAR_INT,
/// AttributeSnapshot.STREAM_CODEC.apply(ByteBufCodecs.list(128)), new)`.
pub fn update_attributes_codec()
-> StreamCodec<RegistryFriendlyByteBuf, ClientboundUpdateAttributesPacket> {
    codec(
        |value: &ClientboundUpdateAttributesPacket, output: &mut RegistryFriendlyByteBuf| {
            output.inner_mut().write_var_int(value.entity_id);
            let snapshot_codec = AttributeSnapshot::snapshot_stream_codec();
            write_count(
                output.inner_mut(),
                value.attributes.len() as i32,
                MAX_SNAPSHOTS,
            )?;
            for snapshot in &value.attributes {
                snapshot_codec.encode(output, snapshot)?;
            }
            Ok(())
        },
        |input: &mut RegistryFriendlyByteBuf| {
            let entity_id = input.inner_mut().read_var_int();
            let count = read_count(input.inner_mut(), MAX_SNAPSHOTS)?;
            let snapshot_codec = AttributeSnapshot::snapshot_stream_codec();
            // Java `list(128)` -> `collection(ArrayList::new)` -> `new
            // ArrayList<>(Math.min(count, 65536))`: a negative count that passes
            // `readCount` throws `IllegalArgumentException("Illegal Capacity:
            // -n")`, catchable — not a capacity overflow.
            let capacity = count.min(byte_buf_codecs::MAX_INITIAL_COLLECTION_SIZE);
            if capacity < 0 {
                panic!("Illegal Capacity: {capacity}");
            }
            let mut attributes = Vec::with_capacity(capacity as usize);
            for _ in 0..count {
                attributes.push(snapshot_codec.decode(input)?);
            }
            Ok(ClientboundUpdateAttributesPacket {
                entity_id,
                attributes,
            })
        },
    )
}
