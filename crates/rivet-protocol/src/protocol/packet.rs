//! Port of `net.minecraft.network.protocol.Packet` (MC 26.2).
//!
//! Java: `Packet.java` in `working/Paper` (vanilla 26.2). The interface is the
//! erased packet value type of a protocol's dispatch codec: every packet body
//! (epic #10/M1.1) implements it and returns its `PacketType` from `type()`.
//!
//! This slice ports the registration-relevant surface only — the type tag and
//! the two pipeline-visible flags. The remaining members are deferred with the
//! units that need them:
//!   - `handle(T listener)` — the dispatch target needs the `PacketListener`
//!     hierarchy (`net.minecraft.network.PacketListener`, unit `mc.network`),
//!     which lands with the server-side state machines (#96/#13). Ported when
//!     the first listener is ported, not speculatively now.
//!   - `isSkippable()` is ported (it is a plain flag the frame handler reads);
//!     the skip/decode machinery that consumes it is `PacketDecoder` (unit
//!     `mc.network`, deferred).
//!   - Paper's `hasLargePacketFallback`/`packetTooLarge`/`onPacketDispatch*`
//!     (Paper patch on `Packet.java`) are connection-level and land with the
//!     connection port.
//!
//! `Packet.codec(StreamMemberEncoder writer, StreamDecoder reader)` is Java's
//! static helper (`StreamCodec.ofMember(writer, reader)`); the Rust port is the
//! free function [`codec`], re-exported here so call sites read
//! `rivet_protocol::protocol::packet::codec(...)`.

pub use crate::codec::codec;

use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.Packet<T>` — the erased packet value.
///
/// `Send + Sync` mirrors the codec requirement (`StreamCodecDyn` is
/// `Send + Sync`; a packet value lives inside the dispatch codec's entries), so
/// every packet body is sendable onto the connection thread.
pub trait Packet: Send + Sync {
    /// `Packet.type()` — the packet's `PacketType` discriminator. Object-safe
    /// (returns the erased concrete `PacketType`), exactly like Java's
    /// `PacketType<? extends Packet<T>> type()`.
    fn packet_type(&self) -> PacketType;

    /// `Packet.isTerminal()` — a terminal packet swaps the inbound/outbound
    /// protocol handler (`ProtocolSwapHandler`; the netty pipeline swap is
    /// deferred to the connection port). Defaults to `false` like Java.
    fn is_terminal(&self) -> bool {
        false
    }

    /// `Packet.isSkippable()` — the frame handler may skip the packet body.
    /// Defaults to `false` like Java.
    fn is_skippable(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::StreamDecoder;
    use crate::codec::StreamEncoder;
    use crate::friendly_byte_buf::FriendlyByteBuf;
    use bytes::BytesMut;

    #[derive(Debug, Clone, PartialEq)]
    struct Hello(String);

    impl Packet for Hello {
        fn packet_type(&self) -> PacketType {
            PacketType::serverbound("hello")
        }
    }

    #[test]
    fn defaults_are_false() {
        assert!(!Hello("x".to_string()).is_terminal());
        assert!(!Hello("x".to_string()).is_skippable());
    }

    #[test]
    fn codec_static_builds_round_tripping_codec() {
        // `Packet.codec(writer, reader)` — the static every body uses for its
        // STREAM_CODEC.
        let codec: crate::codec::StreamCodec<FriendlyByteBuf, Hello> = codec(
            |value: &Hello, output: &mut FriendlyByteBuf| {
                output.write_utf(&value.0);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| Ok(Hello(input.read_utf())),
        );
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        codec.encode(&mut out, &Hello("hi".to_string())).unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(codec.decode(&mut input).unwrap(), Hello("hi".to_string()));
    }
}
