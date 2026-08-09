//! Java-grounded tests for the issue #99 offline login packet bodies
//! (`crates/rivet-protocol/src/protocol/login/`).
//!
//! The four Java classes port here — `ServerboundHelloPacket`,
//! `ClientboundLoginCompressionPacket`, `ClientboundLoginFinishedPacket`,
//! `ServerboundLoginAcknowledgedPacket` — are the login slice of #99. They
//! register in `LoginProtocols` order; the generated table (#50) pins the
//! vanilla ids (serverbound hello 0, login_acknowledged 3; clientbound
//! login_finished 2, login_compression 3). Because this slice registers only
//! these 4 of the 11 login packets, `ProtocolInfoBuilder` assigns sequential
//! registration-order ids (0..3); the vanilla ids are pinned by the generated
//! table and asserted below.
//!
//! Three packet bodies — hello, login_finished, login_acknowledged — appear
//! byte-exactly in the canonical #153 join capture (`tools/rivet-capture/
//! fixtures/join/capture.jsonl`, Paper `26.2-DEV-main@0a99345`, offline
//! `RivetProbe`); login_compression carries the deterministic threshold 256.
//! The captured `login_finished` `sessionId` is the #194/#219 canonicalized
//! zero UUID (the raw value is per-server random and excluded from the byte
//! fixture). The golden bodies and the hostile/truncation/UTF-16 boundary tests
//! below are the fixture-derived and Java-grounded DoD for #99.
//!
//! Gated on the `packets` feature (the `login` module lives behind it).

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder, map, unit};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets;
use rivet_protocol::generated::protocol::{ConnectionProtocol, PacketFlow};
use rivet_protocol::protocol::login::clientbound_login_compression_packet::ClientboundLoginCompressionPacket;
use rivet_protocol::protocol::login::clientbound_login_finished_packet::ClientboundLoginFinishedPacket;
use rivet_protocol::protocol::login::serverbound_hello_packet::ServerboundHelloPacket;
use rivet_protocol::protocol::login::serverbound_login_acknowledged_packet::{
    self, ServerboundLoginAcknowledgedPacket,
};
use rivet_protocol::protocol::{
    Packet, PacketType, ProtocolInfoBuilder, clientbound_protocol, serverbound_protocol,
};
use rivet_registry::core::{GameProfile, Property, PropertyMap};
use rivet_util::uuid::Uuid;
use std::fmt;
use std::panic::catch_unwind;

/// The captured RivetProbe UUID: `0a9ffa92-...` (offline `nameUUIDFromBytes`).
const RIVET_PROBE_ID: Uuid = Uuid {
    most: 0x0a9f_fa92_a706_3e6f,
    least: 0x900c_f12f_869d_37eau64 as i64,
};

/// Hex body -> `Vec<u8>`.
fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

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

/// The offline `RivetProbe` profile — id + name, no properties (the capture's
/// `login_finished` property count is 0).
fn probe_profile() -> GameProfile {
    GameProfile::new_without_properties(RIVET_PROBE_ID, "RivetProbe".to_string())
}

/// The canonicalized `login_finished` sessionId (zero UUID — the raw value is
/// per-server random, excluded from the byte fixture by #194/#219).
fn zero_session_id() -> Uuid {
    Uuid { most: 0, least: 0 }
}

// ---------------------------------------------------------------------------
// The erased login/serverbound + clientbound packet values.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum LoginServerbound {
    Hello(ServerboundHelloPacket),
    LoginAcknowledged,
}

impl Packet for LoginServerbound {
    fn packet_type(&self) -> PacketType {
        match self {
            LoginServerbound::Hello(_) => {
                rivet_protocol::protocol::login::packet_types::serverbound_hello()
            }
            LoginServerbound::LoginAcknowledged => {
                rivet_protocol::protocol::login::packet_types::serverbound_login_acknowledged()
            }
        }
    }
    fn is_terminal(&self) -> bool {
        matches!(self, LoginServerbound::LoginAcknowledged)
    }
}

impl fmt::Display for LoginServerbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

#[derive(Debug, Clone, PartialEq)]
enum LoginClientbound {
    LoginFinished(ClientboundLoginFinishedPacket),
    LoginCompression(ClientboundLoginCompressionPacket),
}

impl Packet for LoginClientbound {
    fn packet_type(&self) -> PacketType {
        match self {
            LoginClientbound::LoginFinished(_) => {
                rivet_protocol::protocol::login::packet_types::clientbound_login_finished()
            }
            LoginClientbound::LoginCompression(_) => {
                rivet_protocol::protocol::login::packet_types::clientbound_login_compression()
            }
        }
    }
    fn is_terminal(&self) -> bool {
        matches!(self, LoginClientbound::LoginFinished(_))
    }
}

impl fmt::Display for LoginClientbound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.packet_type())
    }
}

fn hello_codec() -> StreamCodec<FriendlyByteBuf, LoginServerbound> {
    map(
        ServerboundHelloPacket::stream_codec(),
        |v: &ServerboundHelloPacket| LoginServerbound::Hello(v.clone()),
        |p: &LoginServerbound| match p {
            LoginServerbound::Hello(v) => v.clone(),
            _ => unreachable!("hello_codec only handles Hello"),
        },
    )
}

fn login_acknowledged_codec() -> StreamCodec<FriendlyByteBuf, LoginServerbound> {
    map(
        serverbound_login_acknowledged_packet::stream_codec(),
        |_: &ServerboundLoginAcknowledgedPacket| LoginServerbound::LoginAcknowledged,
        |p: &LoginServerbound| match p {
            LoginServerbound::LoginAcknowledged => ServerboundLoginAcknowledgedPacket,
            _ => unreachable!("login_acknowledged_codec only handles LoginAcknowledged"),
        },
    )
}

fn login_finished_codec() -> StreamCodec<FriendlyByteBuf, LoginClientbound> {
    map(
        ClientboundLoginFinishedPacket::stream_codec(),
        |v: &ClientboundLoginFinishedPacket| LoginClientbound::LoginFinished(v.clone()),
        |p: &LoginClientbound| match p {
            LoginClientbound::LoginFinished(v) => v.clone(),
            _ => unreachable!("login_finished_codec only handles LoginFinished"),
        },
    )
}

fn login_compression_codec() -> StreamCodec<FriendlyByteBuf, LoginClientbound> {
    map(
        ClientboundLoginCompressionPacket::stream_codec(),
        |v: &ClientboundLoginCompressionPacket| LoginClientbound::LoginCompression(*v),
        |p: &LoginClientbound| match p {
            LoginClientbound::LoginCompression(v) => *v,
            _ => unreachable!("login_compression_codec only handles LoginCompression"),
        },
    )
}

// ---------------------------------------------------------------------------
// Registration order == generated vanilla ids (pinned against LoginProtocols).
// ---------------------------------------------------------------------------

#[test]
fn registration_order_matches_generated_vanilla_ids() {
    // LoginProtocols.SERVERBOUND_TEMPLATE registers hello, key, custom_query_answer,
    // login_acknowledged, cookie_response. This slice registers hello (0) and
    // login_acknowledged (3).
    let template = serverbound_protocol::<LoginServerbound>(ConnectionProtocol::Login, |b| {
        b.add_packet(
            rivet_protocol::protocol::login::packet_types::serverbound_hello(),
            hello_codec(),
        )
        .add_packet(
            rivet_protocol::protocol::login::packet_types::serverbound_login_acknowledged(),
            login_acknowledged_codec(),
        );
    });
    assert_eq!(
        template.details().list_packets(),
        &[
            (
                rivet_protocol::protocol::login::packet_types::serverbound_hello(),
                0
            ),
            (
                rivet_protocol::protocol::login::packet_types::serverbound_login_acknowledged(),
                1
            ),
        ]
    );
    // The generated table (#50) pins the vanilla addPacket-order ids.
    assert_eq!(packets::login::serverbound::PacketType::Hello.id(), 0);
    assert_eq!(
        packets::login::serverbound::PacketType::LoginAcknowledged.id(),
        3
    );
    // key (the RSA challenge) stays unported with #88, but its slot is pinned.
    assert_eq!(packets::login::serverbound::PacketType::Key.id(), 1);

    let client = clientbound_protocol::<LoginClientbound>(ConnectionProtocol::Login, |b| {
        b.add_packet(
            rivet_protocol::protocol::login::packet_types::clientbound_login_finished(),
            login_finished_codec(),
        )
        .add_packet(
            rivet_protocol::protocol::login::packet_types::clientbound_login_compression(),
            login_compression_codec(),
        );
    });
    assert_eq!(
        client.details().list_packets(),
        &[
            (
                rivet_protocol::protocol::login::packet_types::clientbound_login_finished(),
                0
            ),
            (
                rivet_protocol::protocol::login::packet_types::clientbound_login_compression(),
                1
            ),
        ]
    );
    assert_eq!(
        packets::login::clientbound::PacketType::LoginFinished.id(),
        2
    );
    assert_eq!(
        packets::login::clientbound::PacketType::LoginCompression.id(),
        3
    );
}

#[test]
fn login_direction_flow_mismatch_panics_with_java_message() {
    // A clientbound type in a serverbound login protocol is rejected at build
    // with ProtocolCodecBuilder.add's message (LoginProtocols.SERVERBOUND is
    // the serverbound flow).
    let msg = panic_message(|| {
        let mut b = ProtocolInfoBuilder::<LoginServerbound, ()>::new(
            ConnectionProtocol::Login,
            PacketFlow::Serverbound,
        );
        b.add_packet(
            rivet_protocol::protocol::login::packet_types::clientbound_login_finished(),
            unit(LoginServerbound::LoginAcknowledged),
        );
        b.build_unbound(());
    });
    assert_eq!(
        msg,
        "Invalid packet flow for packet clientbound/minecraft:login_finished, expected SERVERBOUND"
    );
}

// ---------------------------------------------------------------------------
// Captured golden bodies + decode->encode identity.
// ---------------------------------------------------------------------------

/// Encode `value`, assert the exact golden body bytes, decode back, and assert
/// equality (decode→encode identity for the round-trip).
fn golden(
    codec: &StreamCodec<FriendlyByteBuf, LoginServerbound>,
    value: &LoginServerbound,
    body: &str,
) {
    let mut out = buf();
    codec.encode(&mut out, value).unwrap();
    assert_eq!(written(out), hex(body));
    let mut input = FriendlyByteBuf::new(BytesMut::from(hex(body).as_slice()));
    assert_eq!(&codec.decode(&mut input).unwrap(), value);
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn serverbound_hello_golden_capture_body() {
    // capture.jsonl login/serverbound id 0: `0a` name length 10 + "RivetProbe"
    // + the 16-byte offline UUID.
    golden(
        &hello_codec(),
        &LoginServerbound::Hello(ServerboundHelloPacket::new(
            "RivetProbe".to_string(),
            RIVET_PROBE_ID,
        )),
        "0a526976657450726f62650a9ffa92a7063e6f900cf12f869d37ea",
    );
}

#[test]
fn serverbound_login_acknowledged_encodes_zero_body() {
    // capture.jsonl login/serverbound id 3 has an empty body.
    let mut out = buf();
    login_acknowledged_codec()
        .encode(&mut out, &LoginServerbound::LoginAcknowledged)
        .unwrap();
    assert!(written(out).is_empty());

    let mut input = buf();
    assert_eq!(
        login_acknowledged_codec().decode(&mut input).unwrap(),
        LoginServerbound::LoginAcknowledged
    );
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn clientbound_login_finished_golden_capture_body() {
    // capture.jsonl login/clientbound id 2: the 16-byte RivetProbe UUID + name
    // + 0 property count + the canonicalized zero sessionId (44 bytes). This
    // exercises `ByteBufCodecs.GAME_PROFILE` + `UUIDUtil.STREAM_CODEC` in the
    // composite's exact wire order.
    let packet = LoginClientbound::LoginFinished(ClientboundLoginFinishedPacket::new(
        probe_profile(),
        zero_session_id(),
    ));
    let codec = login_finished_codec();
    let mut out = buf();
    codec.encode(&mut out, &packet).unwrap();
    assert_eq!(
        written(out),
        hex(
            "0a9ffa92a7063e6f900cf12f869d37ea0a526976657450726f62650000000000000000000000000000000000"
        )
    );
    let mut input = FriendlyByteBuf::new(BytesMut::from(
        hex("0a9ffa92a7063e6f900cf12f869d37ea0a526976657450726f62650000000000000000000000000000000000")
            .as_slice(),
    ));
    assert_eq!(codec.decode(&mut input).unwrap(), packet);
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn clientbound_login_compression_golden_capture_body() {
    // capture.jsonl login/clientbound id 3: `80 02` — VarInt 256
    // (MinecraftServer.getCompressionThreshold()). Also round-trips a negative
    // threshold (compression off) and the max positive VarInt.
    for threshold in [256i32, -1, 0, 2_147_483_647] {
        let packet =
            LoginClientbound::LoginCompression(ClientboundLoginCompressionPacket::new(threshold));
        let codec = login_compression_codec();
        let mut out = buf();
        codec.encode(&mut out, &packet).unwrap();
        let bytes = written(out);
        let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(codec.decode(&mut input).unwrap(), packet);
        assert_eq!(input.readable_bytes(), 0);
    }
    // The capture's 256 is exactly `80 02`.
    let mut out = buf();
    login_compression_codec()
        .encode(
            &mut out,
            &LoginClientbound::LoginCompression(ClientboundLoginCompressionPacket::new(256)),
        )
        .unwrap();
    assert_eq!(written(out), hex("8002"));
}

#[test]
fn terminal_flags_match_paper() {
    // login_finished isTerminal: the client swaps to configuration after it.
    assert!(ClientboundLoginFinishedPacket::new(probe_profile(), zero_session_id()).is_terminal());
    // login_acknowledged isTerminal: the server swaps to configuration after it.
    assert!(ServerboundLoginAcknowledgedPacket.is_terminal());
    assert!(!ServerboundHelloPacket::new("x".to_string(), RIVET_PROBE_ID).is_terminal());
    assert!(!ClientboundLoginCompressionPacket::new(256).is_terminal());
    for p in [
        LoginServerbound::Hello(ServerboundHelloPacket::new("x".to_string(), RIVET_PROBE_ID)),
        LoginServerbound::LoginAcknowledged,
    ] {
        assert!(!p.is_skippable());
    }
    for p in [
        LoginClientbound::LoginFinished(ClientboundLoginFinishedPacket::new(
            probe_profile(),
            zero_session_id(),
        )),
        LoginClientbound::LoginCompression(ClientboundLoginCompressionPacket::new(256)),
    ] {
        assert!(!p.is_skippable());
    }
}

// ---------------------------------------------------------------------------
// Bound codec round-trip through the id-dispatch machinery.
// ---------------------------------------------------------------------------

#[test]
fn bound_login_serverbound_round_trips_bodies_byte_identically() {
    let template = serverbound_protocol::<LoginServerbound>(ConnectionProtocol::Login, |b| {
        b.add_packet(
            rivet_protocol::protocol::login::packet_types::serverbound_hello(),
            hello_codec(),
        )
        .add_packet(
            rivet_protocol::protocol::login::packet_types::serverbound_login_acknowledged(),
            login_acknowledged_codec(),
        );
    });
    let info = template.bind();
    let codec = info.codec();

    // hello id 0 -> wire [0x00, varint 10, "RivetProbe", 16-byte uuid].
    let mut out = buf();
    codec
        .encode(
            &mut out,
            &LoginServerbound::Hello(ServerboundHelloPacket::new(
                "RivetProbe".to_string(),
                RIVET_PROBE_ID,
            )),
        )
        .unwrap();
    assert_eq!(
        written(out),
        hex("000a526976657450726f62650a9ffa92a7063e6f900cf12f869d37ea")
    );
    let mut input = FriendlyByteBuf::new(BytesMut::from(
        hex("000a526976657450726f62650a9ffa92a7063e6f900cf12f869d37ea").as_slice(),
    ));
    assert_eq!(
        codec.decode(&mut input).unwrap(),
        LoginServerbound::Hello(ServerboundHelloPacket::new(
            "RivetProbe".to_string(),
            RIVET_PROBE_ID,
        ))
    );
    assert_eq!(input.readable_bytes(), 0);

    // login_acknowledged id 1 (this slice registers it second; the vanilla id
    // 3 is pinned by the generated table, but registration-order ids are
    // protocol-local — see `registration_order_matches_generated_vanilla_ids`).
    let mut out = buf();
    codec
        .encode(&mut out, &LoginServerbound::LoginAcknowledged)
        .unwrap();
    assert_eq!(written(out), vec![0x01]);
    let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x01].as_slice()));
    assert_eq!(
        codec.decode(&mut input).unwrap(),
        LoginServerbound::LoginAcknowledged
    );
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn unknown_login_id_errors_with_java_message() {
    let template = serverbound_protocol::<LoginServerbound>(ConnectionProtocol::Login, |b| {
        b.add_packet(
            rivet_protocol::protocol::login::packet_types::serverbound_hello(),
            hello_codec(),
        );
    });
    let info = template.bind();
    let mut input = buf();
    input.write_var_int(5); // unregistered login id
    let err = info.codec().decode(&mut input).unwrap_err();
    assert_eq!(err.message, "Received unknown packet id 5");
}

// ---------------------------------------------------------------------------
// Hostile wire: name limit, truncation, UTF-16 boundaries, malformed profiles.
// ---------------------------------------------------------------------------

#[test]
fn hello_name_over_16_utf16_units_errors_on_decode_and_encode() {
    // PLAYER_NAME = stringUtf8(16): 17 ASCII chars error on decode; 16
    // round-trips. Wire: length prefix, then the name, then a full uuid.
    let uuid = RIVET_PROBE_ID;
    let ok_bytes = {
        let mut out = buf();
        out.write_var_int(16);
        out.write_bytes(b"abcdefghijklmnop");
        out.write_uuid(uuid);
        out.into_inner().to_vec()
    };
    let decoded = ServerboundHelloPacket::stream_codec()
        .decode(&mut FriendlyByteBuf::new(BytesMut::from(
            ok_bytes.as_slice(),
        )))
        .unwrap();
    assert_eq!(decoded.name(), "abcdefghijklmnop");

    let over_bytes = {
        let mut out = buf();
        out.write_var_int(17);
        out.write_bytes(b"abcdefghijklmnopq");
        out.write_uuid(uuid);
        out.into_inner().to_vec()
    };
    let err = ServerboundHelloPacket::stream_codec()
        .decode(&mut FriendlyByteBuf::new(BytesMut::from(
            over_bytes.as_slice(),
        )))
        .unwrap_err();
    assert_eq!(
        err.message,
        "The received string length is longer than maximum allowed (17 > 16)"
    );

    // Encode side: `writeUtf(name, 16)` throws the same way in Java.
    let err = ServerboundHelloPacket::stream_codec()
        .encode(
            &mut buf(),
            &ServerboundHelloPacket::new("abcdefghijklmnopq".to_string(), uuid),
        )
        .unwrap_err();
    assert_eq!(err.message, "String too big (was 17 characters, max 16)");
}

#[test]
fn hello_utf16_name_bound_counts_code_units_not_bytes() {
    // "😀" is one char but 2 UTF-16 units / 4 UTF-8 bytes. 8 emoji = 16 units
    // / 32 bytes: exactly at the bound, so it decodes; 9 emoji = 18 units /
    // 36 bytes is over.
    let uuid = RIVET_PROBE_ID;
    let at_max = {
        let mut out = buf();
        out.write_var_int(32); // 8 emoji, 32 bytes
        out.write_bytes("😀".repeat(8).as_bytes());
        out.write_uuid(uuid);
        out.into_inner().to_vec()
    };
    let decoded = ServerboundHelloPacket::stream_codec()
        .decode(&mut FriendlyByteBuf::new(BytesMut::from(at_max.as_slice())))
        .unwrap();
    assert_eq!(decoded.name(), &"😀".repeat(8));

    // 9 emoji is 36 bytes, still under the 48-byte encoded bound, but over the
    // 16-unit decode limit because each emoji is a UTF-16 surrogate pair.
    let over = {
        let mut out = buf();
        out.write_var_int(36);
        out.write_bytes("😀".repeat(9).as_bytes());
        out.write_uuid(uuid);
        out.into_inner().to_vec()
    };
    let err = ServerboundHelloPacket::stream_codec()
        .decode(&mut FriendlyByteBuf::new(BytesMut::from(over.as_slice())))
        .unwrap_err();
    assert_eq!(
        err.message,
        "The received string length is longer than maximum allowed (18 > 16)"
    );
}

#[test]
fn hello_truncated_name_errors_and_truncated_uuid_panics() {
    // A length prefix that runs off the buffer end: the codec boundary returns
    // `Err` with Java's "Not enough bytes in buffer..." text (a hostile wire
    // value closes the connection) — it does not panic.
    let mut input = buf();
    input.write_var_int(10);
    input.write_bytes(b"short"); // 5 bytes for a declared 10
    let err = ServerboundHelloPacket::stream_codec()
        .decode(&mut input)
        .unwrap_err();
    assert_eq!(
        err.message,
        "Not enough bytes in buffer, expected 10, but got 5"
    );

    // A valid name but a truncated 16-byte UUID: `read_long` hits EOF, which
    // panics like netty's `IndexOutOfBoundsException` (the raw scalar path has
    // no codec boundary).
    let mut input = buf();
    input.write_utf_max("RivetProbe", 16);
    input.write_long(1); // 8 of the 16 uuid bytes, then EOF
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ServerboundHelloPacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );
}

#[test]
fn hello_negative_declared_name_length_errors() {
    // `Utf8String.read` rejects a negative buffer length with Java's text.
    let mut input = buf();
    input.write_var_int(-1);
    input.write_uuid(RIVET_PROBE_ID);
    let err = ServerboundHelloPacket::stream_codec()
        .decode(&mut input)
        .unwrap_err();
    assert_eq!(
        err.message,
        "The received encoded string buffer length is less than zero! Weird string!"
    );
}

#[test]
fn login_finished_profile_name_bound_and_order() {
    // The profile name shares PLAYER_NAME(16): over-limit errors, and the wire
    // order is profile (uuid, name, count) then sessionId.
    let over = {
        let mut out = buf();
        out.write_uuid(RIVET_PROBE_ID);
        out.write_var_int(17);
        out.write_bytes(b"abcdefghijklmnopq");
        out.write_var_int(0); // property count
        out.write_uuid(zero_session_id());
        out.into_inner().to_vec()
    };
    let err = ClientboundLoginFinishedPacket::stream_codec()
        .decode(&mut FriendlyByteBuf::new(BytesMut::from(over.as_slice())))
        .unwrap_err();
    assert_eq!(
        err.message,
        "The received string length is longer than maximum allowed (17 > 16)"
    );

    // Negative property count passes readCount and the loop never runs (Java
    // behavior preserved, PORTING.md): the profile still decodes, then sessionId.
    let mut out = buf();
    out.write_uuid(RIVET_PROBE_ID);
    out.write_var_int(10);
    out.write_bytes(b"RivetProbe");
    out.write_var_int(-1); // negative property count
    out.write_uuid(zero_session_id());
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let decoded = ClientboundLoginFinishedPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(decoded.game_profile().name(), "RivetProbe");
    assert_eq!(decoded.game_profile().properties().len(), 0);
    assert_eq!(decoded.session_id(), zero_session_id());
}

#[test]
fn login_finished_with_properties_round_trips_in_wire_order() {
    // A signed profile with properties exercises GAME_PROFILE_PROPERTIES inside
    // the composite: each property is (name ≤64, value ≤32767, nullable
    // signature), key-grouped in insertion order.
    let props = PropertyMap::new(vec![
        Property::new("textures".to_string(), "abc".to_string()),
        Property::new_with_signature(
            "textures".to_string(),
            "def".to_string(),
            Some("sig".to_string()),
        ),
        Property::new("skin".to_string(), "gh".to_string()),
    ]);
    let profile = GameProfile::new(RIVET_PROBE_ID, "RivetProbe".to_string(), props);
    let packet = ClientboundLoginFinishedPacket::new(profile, zero_session_id());
    let codec = ClientboundLoginFinishedPacket::stream_codec();
    let mut out = buf();
    codec.encode(&mut out, &packet).unwrap();
    let bytes = written(out);
    let mut input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    assert_eq!(codec.decode(&mut input).unwrap(), packet);
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn login_finished_truncated_session_and_partial_profile() {
    // Profile decodes but the sessionId UUID is truncated -> panic (EOF).
    let mut input = buf();
    input.write_uuid(RIVET_PROBE_ID);
    input.write_utf_max("RivetProbe", 16);
    input.write_var_int(0);
    input.write_long(1); // 8 of the 16 session bytes, then EOF
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundLoginFinishedPacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );

    // A partially-written profile UUID -> panic (EOF), exactly like the
    // byte_buf_codecs::game_profile truncation test.
    let mut input = buf();
    input.write_long(1);
    assert!(
        catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ClientboundLoginFinishedPacket::stream_codec().decode(&mut input);
        }))
        .is_err()
    );
}

#[test]
fn login_finished_invalid_utf8_profile_name_decodes_to_replacement() {
    // `ED A0 80` is a lone 3-byte surrogate: the WHATWG decoder yields one
    // U+FFFD (matching `new String(bytes, UTF_8)`), and the name becomes a
    // single replacement char — 1 UTF-16 unit, within the 16-unit bound.
    let mut out = buf();
    out.write_uuid(RIVET_PROBE_ID);
    out.write_var_int(3);
    out.write_bytes(&[0xED, 0xA0, 0x80]);
    out.write_var_int(0);
    out.write_uuid(zero_session_id());
    let mut input = FriendlyByteBuf::new(out.into_inner());
    let decoded = ClientboundLoginFinishedPacket::stream_codec()
        .decode(&mut input)
        .unwrap();
    assert_eq!(decoded.game_profile().name(), "\u{FFFD}");
    assert_eq!(input.readable_bytes(), 0);
}

// ---------------------------------------------------------------------------
// Controlled mutations (the #99 do-not-weaken counterfactual checks).
// ---------------------------------------------------------------------------

#[test]
fn mutation_flips_direction_of_login_finished() {
    // login_finished is clientbound; registering it serverbound is rejected
    // with the flow-mismatch panic (a mutation of the direction would be
    // caught at registration, not silently accepted).
    assert_eq!(
        ClientboundLoginFinishedPacket::new(probe_profile(), zero_session_id())
            .packet_type()
            .flow(),
        PacketFlow::Clientbound
    );
    assert_eq!(
        ServerboundHelloPacket::new("x".to_string(), RIVET_PROBE_ID)
            .packet_type()
            .flow(),
        PacketFlow::Serverbound
    );
}

#[test]
fn mutation_reordered_login_finished_fields_changes_wire_bytes() {
    // sessionId-first would put the zero UUID before the profile; the composite
    // order is profile-first (Java evaluation order). The byte-exact capture
    // test above pins the real order; a reordered codec would fail it.
    let mut swapped = buf();
    swapped.write_uuid(zero_session_id());
    swapped.write_uuid(RIVET_PROBE_ID);
    swapped.write_utf_max("RivetProbe", 16);
    swapped.write_var_int(0);
    let swapped_bytes = swapped.into_inner().to_vec();
    let mut real = buf();
    ClientboundLoginFinishedPacket::stream_codec()
        .encode(
            &mut real,
            &ClientboundLoginFinishedPacket::new(probe_profile(), zero_session_id()),
        )
        .unwrap();
    assert_ne!(swapped_bytes, real.into_inner().to_vec());
}

#[test]
fn unit_codec_encode_mismatch_panics_with_java_message() {
    // `StreamCodec.unit` panics on a mismatched encode with Java's
    // IllegalStateException text — only the INSTANCE value can be encoded.
    let msg = panic_message(|| {
        let _ = rivet_protocol::codec::unit::<FriendlyByteBuf, LoginServerbound>(
            LoginServerbound::LoginAcknowledged,
        )
        .encode(
            &mut buf(),
            &LoginServerbound::Hello(ServerboundHelloPacket::new("x".to_string(), RIVET_PROBE_ID)),
        );
    });
    assert_eq!(
        msg,
        "Can't encode 'serverbound/minecraft:hello', expected 'serverbound/minecraft:login_acknowledged'"
    );
}

// ---------------------------------------------------------------------------
// The uuid_codec helper itself.
// ---------------------------------------------------------------------------

#[test]
fn uuid_codec_round_trips_16_bytes() {
    let codec = rivet_protocol::protocol::stream_codecs::uuid_codec();
    let mut out = buf();
    codec.encode(&mut out, &RIVET_PROBE_ID).unwrap();
    assert_eq!(written(out), hex("0a9ffa92a7063e6f900cf12f869d37ea"));
    let mut input = FriendlyByteBuf::new(BytesMut::from(
        hex("0a9ffa92a7063e6f900cf12f869d37ea").as_slice(),
    ));
    assert_eq!(codec.decode(&mut input).unwrap(), RIVET_PROBE_ID);
    assert_eq!(input.readable_bytes(), 0);
}
