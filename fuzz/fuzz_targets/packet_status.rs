//! Fuzz target: the status-protocol serverbound packet decode paths.
//!
//! Feeds arbitrary bytes through the real registration-built dispatch codec
//! (`serverbound_protocol` + `ProtocolInfoBuilder` + `IdDispatchCodec`) for the
//! two status serverbound packets: `status_request` (unit body, id 0) and
//! `ping_request` (a `long`, id 1). The varint packet id selects the body
//! codec, mirroring the protocol-layer `IdDispatchCodec` / Java `PacketDecoder`
//! path.
//!
//! Decoding must never *genuinely* panic: hostile input either returns `Err`
//! from the codec boundary, or panics with a faithful message (EOF on a short
//! read, an over-length varint) that `guard` recognizes. Anything else aborts
//! the fuzzer and writes an artifact.
#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::ping::packet_types::serverbound_ping_request;
use rivet_protocol::protocol::ping::serverbound_ping_request::ServerboundPingRequestPacket;
use rivet_protocol::protocol::status::packet_types::serverbound_status_request;
use rivet_protocol::protocol::status::serverbound_status_request_packet::{
    INSTANCE as STATUS_REQUEST_INSTANCE, ServerboundStatusRequestPacket,
};
use rivet_protocol::protocol::{Packet, PacketType, ProtocolInfoBuilder, serverbound_protocol};
use std::sync::OnceLock;

mod guard;
use guard::guarded;

/// The erased status/serverbound packet value (status + ping share the
/// serverbound template, mirroring `StatusProtocols.SERVERBOUND_TEMPLATE`).
#[derive(Debug, Clone, PartialEq)]
enum StatusServerbound {
    StatusRequest,
    PingRequest(i64),
}

impl Packet for StatusServerbound {
    fn packet_type(&self) -> PacketType {
        match self {
            StatusServerbound::StatusRequest => serverbound_status_request(),
            StatusServerbound::PingRequest(_) => serverbound_ping_request(),
        }
    }
}

fn status_serverbound(b: &mut ProtocolInfoBuilder<StatusServerbound, ()>) {
    b.add_packet(
        serverbound_status_request(),
        map(
            ServerboundStatusRequestPacket::stream_codec(),
            |_: &ServerboundStatusRequestPacket| StatusServerbound::StatusRequest,
            |p: &StatusServerbound| match p {
                StatusServerbound::StatusRequest => STATUS_REQUEST_INSTANCE,
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        serverbound_ping_request(),
        map(
            ServerboundPingRequestPacket::stream_codec(),
            |v: &ServerboundPingRequestPacket| StatusServerbound::PingRequest(v.time()),
            |p: &StatusServerbound| match p {
                StatusServerbound::PingRequest(t) => ServerboundPingRequestPacket::new(*t),
                _ => unreachable!(),
            },
        ),
    );
}

fn dispatch_codec() -> &'static StreamCodec<FriendlyByteBuf, StatusServerbound> {
    static CODEC: OnceLock<StreamCodec<FriendlyByteBuf, StatusServerbound>> = OnceLock::new();
    CODEC.get_or_init(|| {
        let template = serverbound_protocol::<StatusServerbound>(ConnectionProtocol::Status, |b| {
            status_serverbound(b);
        });
        template.bind().codec().clone()
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() > guard::MAX_INPUT_LEN {
        return;
    }
    guarded(|| {
        let mut input = FriendlyByteBuf::new(BytesMut::from(data));
        let _ = dispatch_codec().decode(&mut input);
    });
});
