//! Port of `net.minecraft.network.protocol.game.ServerboundClientCommandPacket`
//! (MC 26.2).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/network/
//! protocol/game/ServerboundClientCommandPacket.java`. Wire body is a single
//! VarInt enum ordinal. `readEnum(Action.class)` is
//! `clazz.getEnumConstants()[readVarInt()]` — an out-of-range ordinal throws
//! Java's `ArrayIndexOutOfBoundsException`. The Rust port mirrors that with a
//! panic whose message is exactly the JVM's text (`Index <n> out of bounds for
//! length <m>`), matching the `FriendlyByteBuf` convention that Java's
//! unchecked `RuntimeException`s map to panics (see its module doc).

use crate::codec::StreamCodec;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::packet::{Packet, codec};
use crate::protocol::packet_type::PacketType;

/// `ServerboundClientCommandPacket.Action` — the three client commands, in Java
/// declaration (ordinal) order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    PerformRespawn,
    RequestStats,
    RequestGameruleValues,
}

impl Action {
    /// `Action.values()` — declaration order, `ordinal() == index`.
    pub const VALUES: [Action; 3] = [
        Action::PerformRespawn,
        Action::RequestStats,
        Action::RequestGameruleValues,
    ];

    /// `Action.values()[ordinal]` — panics with the JVM's
    /// `ArrayIndexOutOfBoundsException` message on an out-of-range ordinal,
    /// exactly as `FriendlyByteBuf.readEnum(Action.class)` does.
    pub fn from_ordinal(ordinal: i32) -> Action {
        match ordinal {
            0 => Action::PerformRespawn,
            1 => Action::RequestStats,
            2 => Action::RequestGameruleValues,
            _ => panic!(
                "Index {ordinal} out of bounds for length {}",
                Action::VALUES.len()
            ),
        }
    }
}

/// `ServerboundClientCommandPacket` — a client command action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServerboundClientCommandPacket {
    pub action: Action,
}

impl ServerboundClientCommandPacket {
    /// `new ServerboundClientCommandPacket(Action action)`.
    pub fn new(action: Action) -> Self {
        ServerboundClientCommandPacket { action }
    }

    /// `getAction()`.
    pub fn get_action(&self) -> Action {
        self.action
    }
}

impl Packet for ServerboundClientCommandPacket {
    fn packet_type(&self) -> PacketType {
        PacketType::serverbound("client_command")
    }
}

/// `STREAM_CODEC` — the enum ordinal as a VarInt.
pub fn client_command_codec() -> StreamCodec<FriendlyByteBuf, ServerboundClientCommandPacket> {
    codec(
        |value: &ServerboundClientCommandPacket, output: &mut FriendlyByteBuf| {
            output.write_var_int(value.action as i32);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| {
            Ok(ServerboundClientCommandPacket {
                action: Action::from_ordinal(input.read_var_int()),
            })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;
    use std::panic::catch_unwind;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    #[test]
    fn round_trips_enum_ordinals() {
        let codec = client_command_codec();
        for (ordinal, action) in Action::VALUES.iter().enumerate() {
            let mut out = buf();
            codec
                .encode(&mut out, &ServerboundClientCommandPacket::new(*action))
                .unwrap();
            assert_eq!(out.into_inner().to_vec(), vec![ordinal as u8]);
            let mut out = buf();
            codec
                .encode(&mut out, &ServerboundClientCommandPacket::new(*action))
                .unwrap();
            let mut input = FriendlyByteBuf::new(out.into_inner());
            let decoded = codec.decode(&mut input).unwrap();
            assert_eq!(decoded, ServerboundClientCommandPacket::new(*action));
            assert_eq!(input.readable_bytes(), 0);
        }
    }

    #[test]
    fn out_of_range_ordinal_panics_with_java_aioobe_message() {
        // Java `readEnum`: `clazz.getEnumConstants()[this.readVarInt()]` with a
        // varint of 3 -> `ArrayIndexOutOfBoundsException: Index 3 out of bounds
        // for length 3`.
        let codec = client_command_codec();
        let mut input = buf();
        input.write_var_int(3);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "Index 3 out of bounds for length 3");

        let mut input = buf();
        input.write_var_int(-1);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "Index -1 out of bounds for length 3");

        let mut input = buf();
        input.write_var_int(i32::MAX);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(
            msg,
            format!("Index {} out of bounds for length 3", i32::MAX)
        );
    }

    #[test]
    fn packet_type_is_client_command() {
        assert_eq!(
            ServerboundClientCommandPacket::new(Action::PerformRespawn).packet_type(),
            PacketType::serverbound("client_command")
        );
    }
}
