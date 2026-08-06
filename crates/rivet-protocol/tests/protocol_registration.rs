//! Java-grounded tests for the #84 packet-registration surface
//! (`ProtocolInfoBuilder`/`ProtocolCodecBuilder`/`Packet`/`PacketType`).
//!
//! Every test uses synthetic packet value enums (real packet bodies are
//! M1.1/#148 — deferred, not speculative here) and asserts Paper-observable
//! facts:
//!   - network ids are `addPacket` registration order and match the generated
//!     tables (#50, which encode `ProtocolInfoBuilder.addPacket` order);
//!   - a duplicate registration panics with Java's `IllegalStateException`
//!     message (`IdDispatchCodec.Builder.build`);
//!   - an unregistered packet encode / unknown id decode error with Java's
//!     `EncoderException`/`DecoderException` messages;
//!   - state/direction separation: a clientbound type in a serverbound
//!     protocol panics with `ProtocolCodecBuilder.add`'s message, and the same
//!     canonical packet name maps to protocol-local ids;
//!   - `Packet.codec` (the static) builds a usable `StreamCodec`;
//!   - `withBundlePacket` puts the delimiter at network id 0 and records the
//!     `BundlerInfo`.
//!
//! The crate's integration tests are gated on the `packets` feature (the
//! registration machinery lives on the generated state/direction enums); see
//! `Cargo.toml`'s `required-features`.

use bytes::BytesMut;
use rivet_protocol::codec::{
    StreamCodec, StreamDecoder, StreamEncoder, byte_buf_codecs, map, unit,
};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets;
use rivet_protocol::generated::protocol::{ConnectionProtocol, PacketFlow};
use rivet_protocol::protocol::packet as packet_module;
use rivet_protocol::protocol::{
    Packet, PacketType, ProtocolInfoBuilder, clientbound_protocol, serverbound_protocol,
};
use std::fmt;
use std::panic::catch_unwind;

fn buf() -> FriendlyByteBuf {
    FriendlyByteBuf::new(BytesMut::new())
}

fn written(b: FriendlyByteBuf) -> Vec<u8> {
    b.into_inner().to_vec()
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

/// A synthetic status-packet value type. Real bodies are M1.1; this carries
/// just enough payload to prove the dispatch codec round-trips body bytes.
#[derive(Debug, Clone, PartialEq)]
enum StatusPacket {
    StatusRequest,
    PingRequest(i32),
    StatusResponse(String),
}

impl Packet for StatusPacket {
    fn packet_type(&self) -> PacketType {
        match self {
            StatusPacket::StatusRequest => PacketType::serverbound("status_request"),
            StatusPacket::PingRequest(_) => PacketType::serverbound("ping_request"),
            StatusPacket::StatusResponse(_) => PacketType::clientbound("status_response"),
        }
    }
}

impl fmt::Display for StatusPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

/// A `unit` codec — encodes nothing (the wire is just the id varint).
fn status_request_codec() -> StreamCodec<FriendlyByteBuf, StatusPacket> {
    unit(StatusPacket::StatusRequest)
}

/// A varint-payload codec for `PingRequest(v)` (wire: id varint, then v varint).
fn ping_request_codec() -> StreamCodec<FriendlyByteBuf, StatusPacket> {
    map(
        byte_buf_codecs::var_int(),
        |v: &i32| StatusPacket::PingRequest(*v),
        |p: &StatusPacket| match p {
            StatusPacket::PingRequest(v) => *v,
            _ => unreachable!("ping_request_codec only handles PingRequest"),
        },
    )
}

fn status_response_codec() -> StreamCodec<FriendlyByteBuf, StatusPacket> {
    map(
        byte_buf_codecs::string(),
        |s: &String| StatusPacket::StatusResponse(s.clone()),
        |p: &StatusPacket| match p {
            StatusPacket::StatusResponse(s) => s.clone(),
            _ => unreachable!("status_response_codec only handles StatusResponse"),
        },
    )
}

// ---------------------------------------------------------------------------
// ID lookup: addPacket order == network id, matching the generated tables.
// ---------------------------------------------------------------------------

#[test]
fn status_serverbound_registration_matches_generated_ids_and_round_trips() {
    let template = serverbound_protocol::<StatusPacket>(ConnectionProtocol::Status, |b| {
        b.add_packet(
            PacketType::serverbound("status_request"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("ping_request"),
            ping_request_codec(),
        );
    });

    assert_eq!(template.details().id(), ConnectionProtocol::Status);
    assert_eq!(template.details().flow(), PacketFlow::Serverbound);
    // addPacket order -> network ids.
    assert_eq!(
        template.details().list_packets(),
        &[
            (PacketType::serverbound("status_request"), 0),
            (PacketType::serverbound("ping_request"), 1),
        ]
    );
    // The generated table (#50) pins the same addPacket order facts.
    assert_eq!(
        packets::status::serverbound::PacketType::StatusRequest.id(),
        0
    );
    assert_eq!(
        packets::status::serverbound::PacketType::PingRequest.id(),
        1
    );

    // The bound codec dispatches on the varint id: PingRequest(5) -> [id 1, 5].
    let info = template.bind();
    assert_eq!(info.id(), ConnectionProtocol::Status);
    assert_eq!(info.flow(), PacketFlow::Serverbound);
    assert!(info.bundler_info().is_none());

    let mut out = buf();
    info.codec()
        .encode(&mut out, &StatusPacket::PingRequest(5))
        .unwrap();
    let bytes = written(out);
    assert_eq!(bytes, vec![1, 5]);

    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    assert_eq!(
        info.codec().decode(&mut input).unwrap(),
        StatusPacket::PingRequest(5)
    );
    assert_eq!(input.readable_bytes(), 0);

    // Unit-codec packet: id 0, no body.
    let mut out = buf();
    info.codec()
        .encode(&mut out, &StatusPacket::StatusRequest)
        .unwrap();
    assert_eq!(written(out), vec![0]);
}

#[test]
fn configuration_serverbound_registration_matches_generated_ids() {
    // The join path reaches configuration; pin the first five serverbound ids
    // against the generated table (addPacket order in ConfigurationProtocols).
    let template = serverbound_protocol::<StatusPacket>(ConnectionProtocol::Configuration, |b| {
        b.add_packet(
            PacketType::serverbound("client_information"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("cookie_response"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("custom_payload"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("finish_configuration"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("keep_alive"),
            status_request_codec(),
        );
    });
    assert_eq!(
        template.details().list_packets(),
        &[
            (PacketType::serverbound("client_information"), 0),
            (PacketType::serverbound("cookie_response"), 1),
            (PacketType::serverbound("custom_payload"), 2),
            (PacketType::serverbound("finish_configuration"), 3),
            (PacketType::serverbound("keep_alive"), 4),
        ]
    );
    assert_eq!(
        packets::configuration::serverbound::PacketType::ClientInformation.id(),
        0
    );
    assert_eq!(
        packets::configuration::serverbound::PacketType::KeepAlive.id(),
        4
    );
}

// ---------------------------------------------------------------------------
// Duplicate registration.
// ---------------------------------------------------------------------------

#[test]
fn duplicate_registration_panics_with_java_message() {
    let msg = panic_message(|| {
        let mut b = ProtocolInfoBuilder::<StatusPacket, ()>::new(
            ConnectionProtocol::Status,
            PacketFlow::Serverbound,
        );
        b.add_packet(
            PacketType::serverbound("status_request"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("status_request"),
            status_request_codec(),
        );
        b.build_unbound(());
    });
    assert_eq!(
        msg,
        "Duplicate registration for type serverbound/minecraft:status_request"
    );
}

// ---------------------------------------------------------------------------
// Unknown registration / unknown id errors.
// ---------------------------------------------------------------------------

#[test]
fn encode_unregistered_packet_errors_with_java_message() {
    let template = serverbound_protocol::<StatusPacket>(ConnectionProtocol::Status, |b| {
        b.add_packet(
            PacketType::serverbound("status_request"),
            status_request_codec(),
        );
    });
    let info = template.bind();
    let mut out = buf();
    // PingRequest is not registered in this protocol.
    let err = info
        .codec()
        .encode(&mut out, &StatusPacket::PingRequest(5))
        .unwrap_err();
    assert_eq!(
        err.message,
        "Sending unknown packet 'serverbound/minecraft:ping_request'"
    );
}

#[test]
fn decode_unknown_id_errors_with_java_message() {
    let template = serverbound_protocol::<StatusPacket>(ConnectionProtocol::Status, |b| {
        b.add_packet(
            PacketType::serverbound("status_request"),
            status_request_codec(),
        );
    });
    let info = template.bind();
    let mut input = buf();
    input.write_var_int(5);
    let err = info.codec().decode(&mut input).unwrap_err();
    assert_eq!(err.message, "Received unknown packet id 5");
}

// ---------------------------------------------------------------------------
// State/direction separation.
// ---------------------------------------------------------------------------

#[test]
fn flow_mismatch_panics_with_java_message() {
    // A clientbound type registered into a serverbound protocol is rejected at
    // build with ProtocolCodecBuilder.add's message (flow.name() == SERVERBOUND).
    let msg = panic_message(|| {
        let mut b = ProtocolInfoBuilder::<StatusPacket, ()>::new(
            ConnectionProtocol::Status,
            PacketFlow::Serverbound,
        );
        b.add_packet(
            PacketType::clientbound("status_response"),
            status_response_codec(),
        );
        b.build_unbound(());
    });
    assert_eq!(
        msg,
        "Invalid packet flow for packet clientbound/minecraft:status_response, expected SERVERBOUND"
    );
}

#[test]
fn same_packet_name_is_protocol_local_id() {
    // `minecraft:keep_alive` (serverbound) is id 4 in configuration and id 28 in
    // play: the same canonical PacketType maps to a protocol-local network id.
    assert_eq!(
        packets::configuration::serverbound::PacketType::KeepAlive.id(),
        4
    );
    assert_eq!(packets::play::serverbound::PacketType::KeepAlive.id(), 28);

    // Registering the same PacketType value at different add positions in two
    // protocols yields different network ids — ids are protocol-local, assigned
    // by registration order, never shared across states.
    let config = serverbound_protocol::<StatusPacket>(ConnectionProtocol::Configuration, |b| {
        b.add_packet(
            PacketType::serverbound("client_information"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("cookie_response"),
            status_request_codec(),
        )
        .add_packet(
            PacketType::serverbound("keep_alive"),
            status_request_codec(),
        );
    });
    let play = serverbound_protocol::<StatusPacket>(ConnectionProtocol::Play, |b| {
        b.add_packet(
            PacketType::serverbound("keep_alive"),
            status_request_codec(),
        );
    });
    let keep_alive = PacketType::serverbound("keep_alive");
    // Same PacketType value, but config registered it third and play first.
    assert_eq!(config.details().list_packets()[2], (keep_alive.clone(), 2));
    assert_eq!(play.details().list_packets()[0], (keep_alive, 0));
}

// ---------------------------------------------------------------------------
// Packet.codec usability.
// ---------------------------------------------------------------------------

#[test]
fn packet_codec_static_builds_a_usable_codec() {
    // `Packet.codec(writer, reader)` — the static every body uses for its
    // STREAM_CODEC. Re-exported at `protocol::packet::codec` and
    // `protocol::codec`.
    #[derive(Debug, Clone, PartialEq)]
    struct Hello(String);
    impl Packet for Hello {
        fn packet_type(&self) -> PacketType {
            PacketType::serverbound("hello")
        }
    }
    impl fmt::Display for Hello {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.packet_type())
        }
    }

    let hello_codec: StreamCodec<FriendlyByteBuf, Hello> = packet_module::codec(
        |value: &Hello, output: &mut FriendlyByteBuf| {
            output.write_utf(&value.0);
            Ok(())
        },
        |input: &mut FriendlyByteBuf| Ok(Hello(input.read_utf())),
    );
    let mut out = buf();
    hello_codec
        .encode(&mut out, &Hello("hi".to_string()))
        .unwrap();
    let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
    assert_eq!(
        hello_codec.decode(&mut input).unwrap(),
        Hello("hi".to_string())
    );
}

// ---------------------------------------------------------------------------
// withBundlePacket.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum PlayPacket {
    BundleDelimiter,
    Login,
}

impl Packet for PlayPacket {
    fn packet_type(&self) -> PacketType {
        match self {
            PlayPacket::BundleDelimiter => PacketType::clientbound("bundle_delimiter"),
            PlayPacket::Login => PacketType::clientbound("login"),
        }
    }
}

impl fmt::Display for PlayPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

#[test]
fn with_bundle_packet_puts_delimiter_at_network_id_0() {
    let template = clientbound_protocol::<PlayPacket>(ConnectionProtocol::Play, |b| {
        b.with_bundle_packet(
            PacketType::clientbound("bundle"),
            PlayPacket::BundleDelimiter,
        );
        b.add_packet(PacketType::clientbound("login"), unit(PlayPacket::Login));
    });
    // The delimiter is the first registered packet -> id 0 (Paper's play/clientbound
    // `bundle_delimiter`), and bundler info is recorded.
    assert_eq!(
        template.details().list_packets(),
        &[
            (PacketType::clientbound("bundle_delimiter"), 0),
            (PacketType::clientbound("login"), 1),
        ]
    );
    assert_eq!(
        packets::play::clientbound::PacketType::BundleDelimiter.id(),
        0
    );
    assert_eq!(packets::play::clientbound::PacketType::Login.id(), 49);

    let info = template.bind();
    let bundler = info
        .bundler_info()
        .expect("withBundlePacket records bundler info");
    assert_eq!(
        *bundler.bundle_packet_type(),
        PacketType::clientbound("bundle")
    );
    assert_eq!(
        *bundler.delimiter_packet_type(),
        PacketType::clientbound("bundle_delimiter")
    );

    // The delimiter wire-encodes as just the id varint (unit codec, no body).
    let mut out = buf();
    info.codec()
        .encode(&mut out, &PlayPacket::BundleDelimiter)
        .unwrap();
    assert_eq!(written(out), vec![0]);
}

// ---------------------------------------------------------------------------
// Packet flags.
// ---------------------------------------------------------------------------

#[test]
fn terminal_packet_flag_is_observable() {
    #[derive(Debug, Clone, PartialEq)]
    struct Terminal;
    impl Packet for Terminal {
        fn packet_type(&self) -> PacketType {
            PacketType::serverbound("login_acknowledged")
        }
        fn is_terminal(&self) -> bool {
            true
        }
    }
    impl fmt::Display for Terminal {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.packet_type())
        }
    }
    assert!(Terminal.is_terminal());
    assert!(!Terminal.is_skippable());
}
