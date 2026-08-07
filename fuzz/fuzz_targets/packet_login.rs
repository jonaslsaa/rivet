//! Fuzz target: the login-protocol serverbound packet decode paths.
//!
//! Feeds arbitrary bytes through the real registration-built dispatch codec
//! (`serverbound_protocol` + `ProtocolInfoBuilder` + `IdDispatchCodec`) for the
//! two offline login serverbound packets this crate ports: `hello` (a UTF-8
//! name bounded at 16 UTF-16 units + a profile UUID) and `login_acknowledged`
//! (unit body), registered in the target's own dispatch table as ids 0/1
//! (real protocol ids are 0 and 3 — only the ported subset is registered).
//!
//! The hello name goes through `ByteBufCodecs.string_utf8(16)`, whose
//! oversize/truncated cases return `Err`; the raw `read_uuid` short-read case
//! panics faithfully (EOF). Any panic outside the `guard` set aborts the
//! fuzzer and writes an artifact.
#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::login::packet_types::{
    serverbound_hello, serverbound_login_acknowledged,
};
use rivet_protocol::protocol::login::serverbound_hello_packet::ServerboundHelloPacket;
use rivet_protocol::protocol::login::serverbound_login_acknowledged_packet::{
    self, ServerboundLoginAcknowledgedPacket,
};
use rivet_protocol::protocol::{Packet, PacketType, ProtocolInfoBuilder, serverbound_protocol};
use std::sync::OnceLock;

mod guard;
use guard::guarded;

/// The erased login/serverbound packet value (the offline `HELLO → ACK` pair
/// registered by `LoginProtocols.SERVERBOUND_TEMPLATE`).
#[derive(Debug, Clone, PartialEq)]
enum LoginServerbound {
    Hello(ServerboundHelloPacket),
    LoginAcknowledged,
}

impl Packet for LoginServerbound {
    fn packet_type(&self) -> PacketType {
        match self {
            LoginServerbound::Hello(_) => serverbound_hello(),
            LoginServerbound::LoginAcknowledged => serverbound_login_acknowledged(),
        }
    }
}

fn login_serverbound(b: &mut ProtocolInfoBuilder<LoginServerbound, ()>) {
    b.add_packet(
        serverbound_hello(),
        map(
            ServerboundHelloPacket::stream_codec(),
            |v: &ServerboundHelloPacket| LoginServerbound::Hello(v.clone()),
            |p: &LoginServerbound| match p {
                LoginServerbound::Hello(v) => v.clone(),
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        serverbound_login_acknowledged(),
        map(
            serverbound_login_acknowledged_packet::stream_codec(),
            |_: &ServerboundLoginAcknowledgedPacket| LoginServerbound::LoginAcknowledged,
            |p: &LoginServerbound| match p {
                LoginServerbound::LoginAcknowledged => ServerboundLoginAcknowledgedPacket,
                _ => unreachable!(),
            },
        ),
    );
}

fn dispatch_codec() -> &'static StreamCodec<FriendlyByteBuf, LoginServerbound> {
    static CODEC: OnceLock<StreamCodec<FriendlyByteBuf, LoginServerbound>> = OnceLock::new();
    CODEC.get_or_init(|| {
        let template = serverbound_protocol::<LoginServerbound>(ConnectionProtocol::Login, |b| {
            login_serverbound(b);
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
