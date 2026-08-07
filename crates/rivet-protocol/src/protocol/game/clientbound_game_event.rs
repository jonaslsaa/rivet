//! Port of `net.minecraft.network.protocol.game.ClientboundGameEventPacket`
//! (issue #87) — `game_event` (play clientbound id 38).
//!
//! Java source: `.../network/protocol/game/ClientboundGameEventPacket.java`.
//! Wire body: `event.id` as an **unsigned byte** then a `param` float.
//!
//! The `Type` is a value whose only surface is its `id`. Java's decode does
//! `TYPES.get(readUnsignedByte())` — the `Type` constructor side-registers into
//! a static `Int2ObjectMap`, and an unregistered id yields `null` (there is no
//! fallback, unlike `GameType.ZERO`). The Rust port mirrors that exactly: the
//! packet carries `Option<Type>` (`None` for an unregistered id), and decode
//! **always succeeds** — Java stores the null `Type` and does not fail the
//! connection. The encode side writes the id of the always-present `event`:
//! the server only ever constructs the packet with a non-null `Type`, so a
//! `None` here is the Java NPE (`this.event.id` on a null reference), surfaced
//! as a panic. The captured golden body `0d00000000` is
//! `LEVEL_CHUNKS_LOAD_START` (id 13), `param 0.0`.
//!
//! The `Type` constants the join path emits (`LEVEL_CHUNKS_LOAD_START`) port
//! here; the remaining event constants are deferred with their emitting paths.

use crate::codec::{StreamCodec, codec};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::game::packet_types::clientbound_game_event;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `ClientboundGameEventPacket.Type` — the event kind, keyed by its wire id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Type {
    /// `id`.
    id: u8,
}

impl Type {
    /// The `Type(int)` constructor.
    pub const fn new(id: u8) -> Self {
        Type { id }
    }

    /// `ClientboundGameEventPacket.Type.id`.
    pub fn id(&self) -> u8 {
        self.id
    }
}

/// `ClientboundGameEventPacket.NO_RESPAWN_BLOCK_AVAILABLE` — id `0`.
pub const NO_RESPAWN_BLOCK_AVAILABLE: Type = Type::new(0);
/// `ClientboundGameEventPacket.START_RAINING` — id `1`.
pub const START_RAINING: Type = Type::new(1);
/// `ClientboundGameEventPacket.STOP_RAINING` — id `2`.
pub const STOP_RAINING: Type = Type::new(2);
/// `ClientboundGameEventPacket.CHANGE_GAME_MODE` — id `3`.
pub const CHANGE_GAME_MODE: Type = Type::new(3);
/// `ClientboundGameEventPacket.WIN_GAME` — id `4`.
pub const WIN_GAME: Type = Type::new(4);
/// `ClientboundGameEventPacket.DEMO_EVENT` — id `5`.
pub const DEMO_EVENT: Type = Type::new(5);
/// `ClientboundGameEventPacket.PLAY_ARROW_HIT_SOUND` — id `6`.
pub const PLAY_ARROW_HIT_SOUND: Type = Type::new(6);
/// `ClientboundGameEventPacket.RAIN_LEVEL_CHANGE` — id `7`.
pub const RAIN_LEVEL_CHANGE: Type = Type::new(7);
/// `ClientboundGameEventPacket.THUNDER_LEVEL_CHANGE` — id `8`.
pub const THUNDER_LEVEL_CHANGE: Type = Type::new(8);
/// `ClientboundGameEventPacket.PUFFER_FISH_STING` — id `9`.
pub const PUFFER_FISH_STING: Type = Type::new(9);
/// `ClientboundGameEventPacket.GUARDIAN_ELDER_EFFECT` — id `10`.
pub const GUARDIAN_ELDER_EFFECT: Type = Type::new(10);
/// `ClientboundGameEventPacket.IMMEDIATE_RESPAWN` — id `11`.
pub const IMMEDIATE_RESPAWN: Type = Type::new(11);
/// `ClientboundGameEventPacket.LIMITED_CRAFTING` — id `12`.
pub const LIMITED_CRAFTING: Type = Type::new(12);
/// `ClientboundGameEventPacket.LEVEL_CHUNKS_LOAD_START` — id `13`.
pub const LEVEL_CHUNKS_LOAD_START: Type = Type::new(13);

/// `ClientboundGameEventPacket` — the record `(Type event, float param)`.
///
/// `event` is `Option<Type>` because Java stores `null` for an unregistered
/// id (no fallback); the packet itself is still decoded successfully.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClientboundGameEventPacket {
    /// `event` — Java `@Nullable` (null for an unregistered id).
    event: Option<Type>,
    /// `param`.
    param: f32,
}

impl ClientboundGameEventPacket {
    /// The public constructor — `ClientboundGameEventPacket(Type, float)`. The
    /// server never constructs a null event, so the argument is a plain `Type`.
    pub fn new(event: Type, param: f32) -> Self {
        ClientboundGameEventPacket {
            event: Some(event),
            param,
        }
    }

    /// `ClientboundGameEventPacket.getEvent()` — Java may return null; the
    /// port surfaces that as `None`.
    pub fn get_event(&self) -> Option<Type> {
        self.event
    }

    /// `ClientboundGameEventPacket.getParam()`.
    pub fn get_param(&self) -> f32 {
        self.param
    }

    /// `STREAM_CODEC` — `Packet.codec(write, read)`.
    ///
    /// Decode reads the event id as an **unsigned** byte (never a second byte:
    /// the id is exactly the byte before the float), then the `param` float,
    /// and **always succeeds** — Java's `TYPES.get(...)` returning null does not
    /// fail the connection. Encode writes the id of the always-present `event`;
    /// a `None` mirrors Java's NPE on `this.event.id`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundGameEventPacket> {
        codec(
            |value: &ClientboundGameEventPacket, output: &mut FriendlyByteBuf| {
                let event = value.event.unwrap_or_else(|| {
                    panic!("Cannot invoke \"ClientboundGameEventPacket.Type.id()\" because \"this.event\" is null")
                });
                output.write_byte(event.id as i8);
                output.write_float(value.param);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let event = read_type(input.read_byte() as u8);
                let param = input.read_float();
                Ok(ClientboundGameEventPacket { event, param })
            },
        )
    }
}

/// `TYPES.get(readUnsignedByte())` — the registered `Type` for `id`, or `None`
/// when no event with that id was constructed.
fn read_type(id: u8) -> Option<Type> {
    match id {
        0 => Some(NO_RESPAWN_BLOCK_AVAILABLE),
        1 => Some(START_RAINING),
        2 => Some(STOP_RAINING),
        3 => Some(CHANGE_GAME_MODE),
        4 => Some(WIN_GAME),
        5 => Some(DEMO_EVENT),
        6 => Some(PLAY_ARROW_HIT_SOUND),
        7 => Some(RAIN_LEVEL_CHANGE),
        8 => Some(THUNDER_LEVEL_CHANGE),
        9 => Some(PUFFER_FISH_STING),
        10 => Some(GUARDIAN_ELDER_EFFECT),
        11 => Some(IMMEDIATE_RESPAWN),
        12 => Some(LIMITED_CRAFTING),
        13 => Some(LEVEL_CHUNKS_LOAD_START),
        _ => None,
    }
}

impl Packet for ClientboundGameEventPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_game_event()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn captured_golden_body_round_trips() {
        // Capture: `0d00000000` — LEVEL_CHUNKS_LOAD_START (13), param 0.0.
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            vec![0x0d, 0x00, 0x00, 0x00, 0x00].as_slice(),
        ));
        let decoded = ClientboundGameEventPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded.get_event(), Some(LEVEL_CHUNKS_LOAD_START));
        assert_eq!(decoded.get_param(), 0.0);
        assert_eq!(input.readable_bytes(), 0);

        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundGameEventPacket::stream_codec()
            .encode(&mut out, &decoded)
            .unwrap();
        assert_eq!(out.as_slice().to_vec(), vec![0x0d, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn unregistered_event_id_decodes_to_none_without_failing() {
        // Java `TYPES.get(200)` is null (no fallback) and the packet is still
        // built — decode must succeed, carrying `None`; it must not read a
        // second byte (the id is the byte before the float).
        let mut input = FriendlyByteBuf::new(BytesMut::from(
            vec![200u8, 0x00, 0x00, 0x00, 0x00].as_slice(),
        ));
        let decoded = ClientboundGameEventPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded.get_event(), None);
        assert_eq!(decoded.get_param(), 0.0);
        assert_eq!(input.readable_bytes(), 0);
    }
}
