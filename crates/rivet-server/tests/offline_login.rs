//! Byte-level integration tests for the M1 offline login + configuration slice
//! (issues #96/#99): the `HELLO → VERIFYING → PROTOCOL_SWITCHING` login state
//! machine, compression activation, and the configuration listener's brand +
//! task-queue behavior, exercised over a real loopback socket.
//!
//! Every test boots a `Server` on an ephemeral port and drives it with a raw
//! `TcpStream`, so framing, compression, and disconnects are tested at the byte
//! level rather than through a library client. Wire formats are the pinned
//! Paper 26.2 protocol (776) forms verified against `working/Paper`.

use std::io::Read;
use std::time::Duration;

use flate2::read::ZlibDecoder;
use rivet_protocol::generated::packets::play::clientbound::PacketType;
use rivet_server::server::{Server, ServerConfig};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// Intent ids (ClientIntent byId).
const LOGIN_INTENT: i32 = 2;

const PROTOCOL_VERSION: i32 = 776;

/// Handshake packet id (HandshakeProtocols.SERVERBOUND index 0).
const HANDSHAKE_PACKET_ID: i32 = 0;

/// Login serverbound ids (LoginProtocols / generated table).
const HELLO_PACKET_ID: i32 = 0;
const KEY_PACKET_ID: i32 = 1;
const LOGIN_ACKNOWLEDGED_PACKET_ID: i32 = 3;

/// Login clientbound ids — `login_finished` 2, `login_compression` 3.
const CLIENTBOUND_LOGIN_FINISHED_ID: i32 = 2;
const CLIENTBOUND_LOGIN_COMPRESSION_ID: i32 = 3;

/// Configuration serverbound id for `client_information`.
const CONFIG_CLIENT_INFORMATION_ID: i32 = 0;
/// Configuration clientbound id for `custom_payload` (the brand).
const CONFIG_CLIENTBOUND_CUSTOM_PAYLOAD_ID: i32 = 1;
/// Configuration clientbound id for `finish_configuration` (0-byte body).
const CONFIG_CLIENTBOUND_FINISH_CONFIGURATION_ID: i32 = 3;

/// Play serverbound id for `keep_alive` (a long body) — the play packet this
/// slice forwards to the tick thread after the configuration→play handoff.
const PLAY_KEEP_ALIVE_PACKET_ID: i32 = 28;

/// The pinned capture fixture's offline profile for the test player
/// (`UUID.nameUUIDFromBytes("OfflinePlayer:RivetProbe")`, issue #198): most
/// `0x0a9ffa92a7063e6f`, least `0x900cf12f869d37ea`. The listener derives this
/// from the hello name and carries it in `ClientboundLoginFinishedPacket`, so
/// asserting these bytes pins the offline-UUID canonicalization to the fixture.
const RIVET_PROBE_OFFLINE_UUID_BYTES: [u8; 16] = [
    0x0a, 0x9f, 0xfa, 0x92, 0xa7, 0x06, 0x3e, 0x6f, //
    0x90, 0x0c, 0xf1, 0x2f, 0x86, 0x9d, 0x37, 0xea,
];

/// Start a server on an ephemeral port; returns the bound address and the serve task.
async fn start_server(config: ServerConfig) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let server = Server::new(config);
    let listener = server.bind().await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let handle = tokio::spawn(async move {
        let _ = server.serve(listener).await;
    });
    (addr, handle)
}

fn config_with_threshold(compression_threshold: i32) -> ServerConfig {
    ServerConfig {
        bind_host: std::net::IpAddr::from([127, 0, 0, 1]),
        port: 0,
        max_connections: 16,
        read_timeout: Duration::from_secs(30),
        compression_threshold,
        tick_interval: Duration::from_millis(50),
        catchup_ticks: 5,
        inbound_channel_capacity: 64,
        outbound_channel_capacity: 64,
        lifecycle_capacity: 64,
        enable_join: false,
    }
}

/// Protocol VarInt encode.
fn varint(value: i32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut v = value as u32;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            break;
        }
    }
    out
}

/// Decode a VarInt from a byte slice; returns the value and the bytes consumed.
fn decode_varint(bytes: &[u8]) -> (i32, usize) {
    let mut out: u32 = 0;
    for (i, &b) in bytes.iter().enumerate() {
        out |= ((b & 0x7F) as u32) << (i * 7);
        if b & 0x80 == 0 {
            return (out as i32, i + 1);
        }
    }
    panic!("incomplete varint in {:02x?}", bytes);
}

/// VarInt21-frame a raw packet body.
fn frame(body: &[u8]) -> Vec<u8> {
    let mut out = varint(body.len() as i32);
    out.extend_from_slice(body);
    out
}

/// A full `ClientIntentionPacket` frame for the LOGIN intent (protocol 776).
fn login_handshake_frame() -> Vec<u8> {
    let mut body = varint(HANDSHAKE_PACKET_ID);
    body.extend_from_slice(&varint(PROTOCOL_VERSION));
    body.extend_from_slice(&varint("localhost".len() as i32));
    body.extend_from_slice(b"localhost");
    body.extend_from_slice(&25565u16.to_be_bytes());
    body.extend_from_slice(&varint(LOGIN_INTENT));
    frame(&body)
}

/// A `ServerboundHelloPacket` frame (login serverbound id 0): `name` as
/// `Utf8String.read(16)` then a UUID. The `profile_id` the client believes it
/// is — offline clients send `UUIDUtil.createOfflinePlayerUUID(name)`; the
/// listener ignores it (M1 builds the offline profile from the name).
fn hello_frame(name: &str, profile_id: [u8; 16]) -> Vec<u8> {
    let mut body = varint(HELLO_PACKET_ID);
    body.extend_from_slice(&varint(name.len() as i32));
    body.extend_from_slice(name.as_bytes());
    body.extend_from_slice(&profile_id);
    frame(&body)
}

/// A `ServerboundLoginAcknowledgedPacket` frame (id 3, 0-byte body). `compressed`
/// selects the post-`setupCompression` wire form: `varint(0) ++ body` inside the
/// VarInt21 frame when the packet is under the threshold (which the ack always
/// is — a single id varint).
fn login_acknowledged_frame(compressed: bool) -> Vec<u8> {
    let body = varint(LOGIN_ACKNOWLEDGED_PACKET_ID);
    if compressed {
        let mut wire = varint(0); // declaredLength 0: payload is raw
        wire.extend_from_slice(&body);
        frame(&wire)
    } else {
        frame(&body)
    }
}

/// A configuration `client_information` packet (id 0) carrying the default
/// `ClientInformation` value with a non-default `viewDistance` (byte 4), so a
/// decode that merely passed the body through would be indistinguishable — the
/// store path is the load-bearing assertion (the connection stays open).
fn client_information_frame(view_distance: u8) -> Vec<u8> {
    let mut body = varint(CONFIG_CLIENT_INFORMATION_ID);
    body.extend_from_slice(&varint(5));
    body.extend_from_slice(b"en_us");
    body.push(view_distance);
    body.push(0x00); // ChatVisiblity.FULL
    body.push(0x01); // chatColors true
    body.push(0x00); // modelCustomisation 0
    body.push(0x01); // HumanoidArm.RIGHT
    body.push(0x00); // textFilteringEnabled false
    body.push(0x00); // allowsListing false
    body.push(0x00); // ParticleStatus.ALL
    let mut wire = varint(0); // under threshold → uncompressed
    wire.extend_from_slice(&body);
    frame(&wire)
}

/// Read exactly `n` bytes with a deadline; panics on EOF or timeout.
async fn read_bytes(stream: &mut TcpStream, n: usize) -> Vec<u8> {
    let mut out = vec![0u8; n];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    tokio::time::timeout_at(deadline, stream.read_exact(&mut out))
        .await
        .expect("timeout reading from server")
        .expect("read_exact");
    out
}

/// Read one VarInt21 frame; returns the frame payload (the bytes after the
/// length header).
async fn read_frame_payload(stream: &mut TcpStream) -> Vec<u8> {
    let mut header = Vec::new();
    loop {
        let b = read_bytes(stream, 1).await[0];
        header.push(b);
        if b & 0x80 == 0 {
            break;
        }
    }
    let (len, _) = decode_varint(&header);
    read_bytes(stream, len as usize).await
}

/// Assert nothing arrives and the connection stays open (a short timeout with
/// no data and no EOF).
async fn expect_silent_open(stream: &mut TcpStream) {
    let mut buf = [0u8; 16];
    match tokio::time::timeout(Duration::from_millis(200), stream.read(&mut buf)).await {
        Ok(Ok(0)) => panic!("connection closed; expected it to stay open"),
        Ok(Ok(n)) => panic!("unexpected data: {n} bytes"),
        Ok(Err(_)) => panic!("read error"),
        Err(_) => {} // timeout: still open, nothing sent
    }
}

/// Read until EOF; panics if the server writes data instead of closing (the
/// deterministic-close paths here close without a formatted disconnect body).
///
/// A single read round suffices: the close paths this helper asserts produce
/// EOF or a read error with no data, so a `loop` would never repeat — the
/// deadline (a fixed future instant) is the timeout.
async fn expect_eof(stream: &mut TcpStream) {
    let mut buf = [0u8; 128];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    match tokio::time::timeout_at(deadline, stream.read(&mut buf)).await {
        Ok(Ok(0)) => {}
        Ok(Ok(n)) => panic!("server sent {n} bytes before closing; expected EOF"),
        Ok(Err(_)) => {}
        Err(_) => panic!("timed out waiting for EOF"),
    }
}

/// The client's view of one compressed outbound packet: returns the declared
/// uncompressed length and the decompressed packet bytes (`varint(declaredLen)
/// ++ zlib(payload)`, or the raw payload when `declaredLen == 0`).
async fn read_compressed_packet(stream: &mut TcpStream) -> (i32, Vec<u8>) {
    let wire = read_frame_payload(stream).await;
    let (declared, used) = decode_varint(&wire);
    let payload = &wire[used..];
    if declared == 0 {
        (0, payload.to_vec())
    } else {
        let mut decoder = ZlibDecoder::new(payload);
        let mut out = Vec::new();
        decoder.read_to_end(&mut out).expect("zlib inflate");
        (declared, out)
    }
}

/// The full `ClientboundLoginFinishedPacket` payload for `name`: packet id 2,
/// the offline profile `(uuid, name, 0 properties)`, then the zero session id.
/// `GameProfile` writes UUID first then name (`ByteBufCodecs.GAME_PROFILE`),
/// and the composite appends `UUIDUtil.STREAM_CODEC` — 16 zero bytes for the
/// session id (RivetTodo #96). Total 45 bytes for "RivetProbe".
fn finished_payload(name: &str) -> Vec<u8> {
    let mut payload = varint(CLIENTBOUND_LOGIN_FINISHED_ID);
    payload.extend_from_slice(&RIVET_PROBE_OFFLINE_UUID_BYTES);
    payload.extend_from_slice(&varint(name.len() as i32));
    payload.extend_from_slice(name.as_bytes());
    payload.push(0x00); // PropertyMap size 0
    payload.extend_from_slice(&[0u8; 16]); // zero session id
    payload
}

/// The `ClientboundLoginFinishedPacket` frame payload under compression (the
/// `varint(0)` declaredLength prefix — sub-threshold, so not zlib-compressed).
/// Returns the bytes after the VarInt21 length header (what `read_frame_payload`
/// yields).
fn expected_finished_frame(name: &str) -> Vec<u8> {
    let mut wire = varint(0); // declaredLength 0 → raw payload
    wire.extend_from_slice(&finished_payload(name));
    wire
}

/// The expected `ClientboundCustomPayloadPacket` (brand) frame payload: config
/// clientbound id 1, `minecraft:brand` then "Rivet", with the `varint(0)`
/// declaredLength prefix (sub-threshold, uncompressed). Returns the bytes after
/// the VarInt21 length header (what `read_frame_payload` yields).
fn expected_brand_frame() -> Vec<u8> {
    let mut payload = varint(CONFIG_CLIENTBOUND_CUSTOM_PAYLOAD_ID);
    payload.extend_from_slice(b"\x0Fminecraft:brand\x05Rivet");
    let mut wire = varint(0); // declaredLength 0 → raw payload
    wire.extend_from_slice(&payload);
    wire
}

/// The expected `ClientboundUpdateEnabledFeaturesPacket` frame payload: config
/// clientbound id 12, the `{minecraft:vanilla}` set (the M1 offline world's
/// enabled features). `varint(0)` declaredLength prefix (sub-threshold).
fn expected_enabled_features_frame() -> Vec<u8> {
    let mut payload = varint(12);
    payload.extend_from_slice(b"\x01\x11minecraft:vanilla");
    let mut wire = varint(0);
    wire.extend_from_slice(&payload);
    wire
}

/// The expected `ClientboundSelectKnownPacks` frame payload: config clientbound
/// id 14, the `[minecraft:core:26.2]` list (the capture's advertised pack).
/// `varint(0)` declaredLength prefix.
fn expected_select_known_packs_frame() -> Vec<u8> {
    let mut payload = varint(14);
    payload.extend_from_slice(b"\x01\x09minecraft\x04core\x0426.2");
    let mut wire = varint(0);
    wire.extend_from_slice(&payload);
    wire
}

/// The expected `ClientboundFinishConfigurationPacket` frame payload: config
/// clientbound id 3, 0-byte body (`StreamCodec.unit(INSTANCE)`), with the
/// `varint(0)` declaredLength prefix (sub-threshold, uncompressed).
fn expected_finish_configuration_frame() -> Vec<u8> {
    let mut wire = varint(0); // declaredLength 0 → raw payload
    wire.extend_from_slice(&varint(CONFIG_CLIENTBOUND_FINISH_CONFIGURATION_ID));
    wire
}

/// Consume the two configuration packets the server sends right after the
/// brand: `update_enabled_features` (id 12) then `select_known_packs` (id 14).
/// Returns the two frame payloads (what `read_frame_payload` yields).
async fn consume_config_sync_opening(stream: &mut TcpStream) -> (Vec<u8>, Vec<u8>) {
    let enabled = read_frame_payload(stream).await;
    let select = read_frame_payload(stream).await;
    (enabled, select)
}

/// The serverbound `select_known_packs` reply for the given pack triplets,
/// compressed (varint(0) declaredLength prefix — sub-threshold). The id is
/// configuration serverbound 7.
fn serverbound_select_known_packs_reply(packs: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut body = varint(7);
    body.extend_from_slice(&varint(packs.len() as i32));
    for (ns, id, version) in packs {
        body.extend_from_slice(&varint(ns.len() as i32));
        body.extend_from_slice(ns.as_bytes());
        body.extend_from_slice(&varint(id.len() as i32));
        body.extend_from_slice(id.as_bytes());
        body.extend_from_slice(&varint(version.len() as i32));
        body.extend_from_slice(version.as_bytes());
    }
    let mut wire = varint(0);
    wire.extend_from_slice(&body);
    frame(&wire)
}

/// The 29 `RegistryDataLoader.SYNCHRONIZED_REGISTRIES` keys in wire order (the
/// `ClientboundRegistryDataPacket` stream order).
const SYNCHRONIZED_KEYS: &[&str] = &[
    "minecraft:worldgen/biome",
    "minecraft:chat_type",
    "minecraft:trim_pattern",
    "minecraft:trim_material",
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_sound_variant",
    "minecraft:cow_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
    "minecraft:sulfur_cube_archetype",
    "minecraft:dimension_type",
    "minecraft:damage_type",
    "minecraft:banner_pattern",
    "minecraft:enchantment",
    "minecraft:jukebox_song",
    "minecraft:instrument",
    "minecraft:test_environment",
    "minecraft:test_instance",
    "minecraft:dialog",
    "minecraft:world_clock",
    "minecraft:timeline",
];

/// Decode a `registry_data` packet's registry key from a decompressed payload
/// (`[id varint] ++ [len varint] ++ key`).
fn decode_registry_key(payload: &[u8]) -> String {
    let (_, used) = decode_varint(payload); // packet id
    let rest = &payload[used..];
    let (len, used) = decode_varint(rest);
    std::str::from_utf8(&rest[used..used + len as usize])
        .expect("registry key utf8")
        .to_string()
}

// The `registry_data_capture.json` hex decoder, shared with the `registry_sync`
// unit tests via a single `include!` (see the helper's header comment).
include!("support/registry_data_capture.rs");

/// Reply to `select_known_packs` with the accepting `minecraft:core:26.2`
/// pack, consume the 29 `registry_data` packets + the `update_tags` trailer,
/// and assert the `finish_configuration` frame that `JoinWorldTask` then sends
/// (the task starts as soon as the sync finishes). Returns the finish frame
/// payload.
async fn complete_sync_and_consume_finish(stream: &mut TcpStream) -> Vec<u8> {
    stream
        .write_all(&serverbound_select_known_packs_reply(&[(
            "minecraft",
            "core",
            "26.2",
        )]))
        .await
        .expect("write accepting select_known_packs reply");

    for (i, expected_key) in SYNCHRONIZED_KEYS.iter().enumerate() {
        let (_, payload) = read_compressed_packet(stream).await;
        let (id, _) = decode_varint(&payload);
        assert_eq!(id, 7, "registry_data id");
        let key = decode_registry_key(&payload);
        assert_eq!(key, *expected_key, "registry_data {i} key");
    }
    let (_, payload) = read_compressed_packet(stream).await;
    let (id, _) = decode_varint(&payload);
    assert_eq!(id, 13, "update_tags id");

    // `JoinWorldTask` starts the moment the sync finishes, sending the
    // fieldless `ClientboundFinishConfigurationPacket` (id 3).
    let finish = read_frame_payload(stream).await;
    assert_eq!(
        finish,
        expected_finish_configuration_frame(),
        "finish_configuration frame"
    );
    finish
}

/// Drive handshake + hello, and assert the login response: the compression
/// packet (uncompressed, carrying `threshold`) followed by the finished packet
/// (wire order: compression before finished — the compression packet is queued
/// before `setupCompression` runs).
async fn login_and_assert_response(stream: &mut TcpStream, threshold: i32) -> Vec<u8> {
    stream
        .write_all(&login_handshake_frame())
        .await
        .expect("write handshake");
    stream
        .write_all(&hello_frame("RivetProbe", [0u8; 16]))
        .await
        .expect("write hello");

    // login_compression: `[id 3][varint(threshold)]`, uncompressed.
    let compression = read_frame_payload(stream).await;
    let mut expected_compression = varint(CLIENTBOUND_LOGIN_COMPRESSION_ID);
    expected_compression.extend_from_slice(&varint(threshold));
    assert_eq!(compression, expected_compression, "login_compression frame");

    // login_finished: uncompressed under the default threshold.
    let finished = read_frame_payload(stream).await;
    assert_eq!(
        finished,
        expected_finished_frame("RivetProbe"),
        "login_finished frame"
    );

    // The client acks (compressed form now that compression is enabled).
    stream
        .write_all(&login_acknowledged_frame(true))
        .await
        .expect("write ack");
    // The brand is the first configuration packet.
    let brand = read_frame_payload(stream).await;
    assert_eq!(brand, expected_brand_frame(), "config brand frame");
    brand
}

#[tokio::test]
async fn full_offline_login_compression_ack_and_config_brand() {
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    let brand = login_and_assert_response(&mut client, 256).await;
    // The brand is 23 bytes of payload: `[id 1][0x0F"minecraft:brand"][0x05"Rivet"]`,
    // so it stayed uncompressed (under 256). Its declaredLength prefix is the
    // `varint(0)` the compression encoder writes for sub-threshold packets.
    let (declared, used) = decode_varint(&brand);
    assert_eq!(declared, 0, "sub-threshold brand is not zlib-compressed");
    assert_eq!(
        &brand[used..],
        b"\x01\x0Fminecraft:brand\x05Rivet",
        "brand payload"
    );

    server_task.abort();
}

#[tokio::test]
async fn compression_activates_above_threshold() {
    // A threshold below the finished (45-byte) and brand (23-byte) packets but
    // above the ack (1-byte): the finished and brand must be zlib-compressed on
    // the wire; the ack passes through with declaredLength 0.
    let (addr, server_task) = start_server(config_with_threshold(16)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    stream_login_hello(&mut client).await;

    // login_compression, uncompressed, carries the threshold.
    let compression = read_frame_payload(&mut client).await;
    assert_eq!(
        compression,
        vec![0x03, 0x10],
        "login_compression threshold 16"
    );

    // login_finished, 45 bytes > 16 → compressed.
    let (declared, packet) = read_compressed_packet(&mut client).await;
    assert_eq!(declared, 45, "finished declared length");
    assert_eq!(
        packet,
        finished_payload("RivetProbe"),
        "compressed finished payload"
    );

    // Ack, 1 byte < 16 → declaredLength 0 pass-through.
    client
        .write_all(&login_acknowledged_frame(true))
        .await
        .expect("write ack");
    // Brand, 23 bytes > 16 → compressed.
    let (declared, packet) = read_compressed_packet(&mut client).await;
    assert_eq!(declared, 23, "brand declared length");
    assert_eq!(
        packet, b"\x01\x0Fminecraft:brand\x05Rivet",
        "compressed brand payload"
    );

    server_task.abort();
}

/// Send the handshake + hello (used by tests that drive the response manually).
async fn stream_login_hello(stream: &mut TcpStream) {
    stream
        .write_all(&login_handshake_frame())
        .await
        .expect("write handshake");
    stream
        .write_all(&hello_frame("RivetProbe", [0u8; 16]))
        .await
        .expect("write hello");
}

#[tokio::test]
async fn disabled_compression_sends_no_compression_packet() {
    // `compression_threshold = -1`: Paper skips the compression packet and
    // `setupCompression` entirely; the finished packet and the config brand go
    // out uncompressed, and the ack is the plain 0-byte body frame.
    let (addr, server_task) = start_server(config_with_threshold(-1)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    stream_login_hello(&mut client).await;

    // No login_compression frame — the first response is the finished packet,
    // with NO `varint(0)` declaredLength prefix (compression never enabled).
    let finished = read_frame_payload(&mut client).await;
    assert_eq!(
        finished,
        finished_payload("RivetProbe"),
        "uncompressed finished"
    );

    // Ack in the uncompressed form (no declaredLength prefix).
    client
        .write_all(&login_acknowledged_frame(false))
        .await
        .expect("write ack");
    let brand = read_frame_payload(&mut client).await;
    let mut expected_brand = varint(CONFIG_CLIENTBOUND_CUSTOM_PAYLOAD_ID);
    expected_brand.extend_from_slice(b"\x0Fminecraft:brand\x05Rivet");
    assert_eq!(brand, expected_brand, "uncompressed brand");

    server_task.abort();
}

#[tokio::test]
async fn fragmented_login_frames_accumulate() {
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Split handshake + hello into 3-byte chunks with pauses; the response must
    // still be the complete compression + finished sequence.
    let mut full = login_handshake_frame();
    full.extend_from_slice(&hello_frame("RivetProbe", [0u8; 16]));
    for chunk in full.chunks(3) {
        client.write_all(chunk).await.expect("write");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }

    let compression = read_frame_payload(&mut client).await;
    assert_eq!(compression, vec![0x03, 0x80, 0x02], "login_compression 256");
    let finished = read_frame_payload(&mut client).await;
    assert_eq!(finished, expected_finished_frame("RivetProbe"));

    server_task.abort();
}

#[tokio::test]
async fn coalesced_login_frames_decode_in_one_batch() {
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Handshake + hello in a single write (two VarInt21 frames back to back).
    let mut coalesced = login_handshake_frame();
    coalesced.extend_from_slice(&hello_frame("RivetProbe", [0u8; 16]));
    client.write_all(&coalesced).await.expect("write");

    let compression = read_frame_payload(&mut client).await;
    assert_eq!(compression, vec![0x03, 0x80, 0x02], "login_compression 256");
    let finished = read_frame_payload(&mut client).await;
    assert_eq!(finished, expected_finished_frame("RivetProbe"));

    server_task.abort();
}

#[tokio::test]
async fn out_of_order_login_acknowledgement_closes() {
    // An ack in the HELLO state is `Validate.validState(PROTOCOL_SWITCHING,
    // "Unexpected login acknowledgement packet")` → deterministic close.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    client
        .write_all(&login_handshake_frame())
        .await
        .expect("write handshake");
    client
        .write_all(&login_acknowledged_frame(false))
        .await
        .expect("write ack");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn duplicate_hello_closes() {
    // A second hello in the VERIFYING state is `Validate.validState(HELLO,
    // "Unexpected hello packet")` → deterministic close. The second hello must
    // be compressed (compression is enabled after the first).
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    stream_login_hello(&mut client).await;
    // Consume the login response so the listener has processed the first hello.
    let _ = read_frame_payload(&mut client).await;
    let _ = read_frame_payload(&mut client).await;

    let hello = hello_frame("RivetProbe", [0u8; 16]);
    let mut wire = varint(0);
    wire.extend_from_slice(&hello);
    client.write_all(&frame(&wire)).await.expect("write hello");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn key_packet_unsupported_closes() {
    // `handleKey` is the RSA online-auth path (#88); M1 runs offline, so a key
    // is rejected as unsupported with the DISCONNECT_UNEXPECTED_QUERY key.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    stream_login_hello(&mut client).await;
    let _ = read_frame_payload(&mut client).await;
    let _ = read_frame_payload(&mut client).await;

    let mut wire = varint(0); // under threshold → uncompressed
    wire.extend_from_slice(&varint(KEY_PACKET_ID));
    client.write_all(&frame(&wire)).await.expect("write key");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn unknown_login_packet_id_closes() {
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    stream_login_hello(&mut client).await;
    let _ = read_frame_payload(&mut client).await;
    let _ = read_frame_payload(&mut client).await;

    let mut wire = varint(0);
    wire.extend_from_slice(&varint(99));
    client.write_all(&frame(&wire)).await.expect("write id 99");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn select_known_packs_empty_reply_serves_full_content() {
    // A client that does NOT accept `minecraft:core:26.2` (here: an empty pack
    // list, the RivetProbe capture's reply) forces Paper's full-content path —
    // every element NBT-encoded. Rivet serves the pre-baked capture payloads
    // (`registry_sync::pack_registries`); each `ClientboundRegistryDataPacket`
    // body must match the committed capture fixture byte-for-byte, and the
    // `update_tags` trailer follows.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    let (enabled, select) = consume_config_sync_opening(&mut client).await;
    assert_eq!(
        enabled,
        expected_enabled_features_frame(),
        "update_enabled_features"
    );
    assert_eq!(
        select,
        expected_select_known_packs_frame(),
        "select_known_packs"
    );

    client
        .write_all(&serverbound_select_known_packs_reply(&[]))
        .await
        .expect("write empty select_known_packs reply");

    let expected_bodies = registry_data_capture_bodies();
    let mut seen_keys = Vec::new();
    for (i, expected_key) in SYNCHRONIZED_KEYS.iter().enumerate() {
        let (_, payload) = read_compressed_packet(&mut client).await;
        let (id, used) = decode_varint(&payload);
        assert_eq!(id, 7, "registry_data id");
        let body = &payload[used..];
        let key = decode_registry_key(&payload);
        assert_eq!(key, *expected_key, "registry_data {i} key");
        assert_eq!(
            body,
            expected_bodies
                .get(&key)
                .expect("fixture covers every registry"),
            "registry_data {key} body must match the capture byte-for-byte"
        );
        seen_keys.push(key);
    }
    assert_eq!(seen_keys, SYNCHRONIZED_KEYS.to_vec());

    // The update_tags trailer (id 13), exactly as in the accepting-client path.
    let (_, payload) = read_compressed_packet(&mut client).await;
    let (id, _) = decode_varint(&payload);
    assert_eq!(id, 13, "update_tags id");

    server_task.abort();
}

#[tokio::test]
async fn finish_configuration_before_sync_finishes_closes() {
    // `handleConfigurationFinished` -> `finishCurrentTask(JoinWorldTask.TYPE)`.
    // The current task is the registry sync (not JoinWorldTask — the finish→play
    // handoff is #100/#101), so Java throws `IllegalStateException` — a
    // deterministic Malformed close.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;

    let mut wire = varint(0);
    wire.extend_from_slice(&varint(3)); // finish_configuration
    client
        .write_all(&frame(&wire))
        .await
        .expect("write finish_configuration");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn config_client_information_valid_keeps_open() {
    // `handleClientInformation` stores the value; a valid body decodes and the
    // connection stays open (the sync opening frames are consumed, then silence).
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    let (enabled, select) = consume_config_sync_opening(&mut client).await;
    assert_eq!(enabled, expected_enabled_features_frame());
    assert_eq!(select, expected_select_known_packs_frame());

    client
        .write_all(&client_information_frame(4))
        .await
        .expect("write client_information");

    expect_silent_open(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn config_client_information_malformed_enum_closes() {
    // A ChatVisiblity ordinal of 99 (COUNT is 3) is a hostile enum value:
    // `Index 99 out of bounds for length 3` — a decode error, so the connection
    // closes deterministically instead of accepting a bogus value.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;

    let mut body = varint(CONFIG_CLIENT_INFORMATION_ID);
    body.extend_from_slice(&varint(5));
    body.extend_from_slice(b"en_us");
    body.push(2);
    body.push(99); // ChatVisiblity ordinal out of range
    let mut wire = varint(0);
    wire.extend_from_slice(&body);
    client
        .write_all(&frame(&wire))
        .await
        .expect("write malformed");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn play_packet_before_finish_configuration_closes() {
    // Hostile ordering: a play keep_alive (id 28) sent while the configuration
    // listener is still current is an unknown configuration packet id — Java's
    // `PacketDecoder` finds no handler and closes deterministically. Only after
    // `finish_configuration` does the connection hand off and forward play
    // frames (see `play_frame_after_finish_is_forwarded_to_tick_thread`).
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;

    let mut body = varint(PLAY_KEEP_ALIVE_PACKET_ID);
    body.extend_from_slice(&7i64.to_be_bytes());
    let mut wire = varint(0);
    wire.extend_from_slice(&body);
    client
        .write_all(&frame(&wire))
        .await
        .expect("write play keep_alive during configuration");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn config_registry_sync_opening_frames() {
    // The configuration sync opening: brand, then update_enabled_features
    // (`{minecraft:vanilla}`), then select_known_packs (`[minecraft:core:26.2]`).
    // The latter two pin the capture's wire bytes (`config_update_enabled_features.hex`
    // and `config_clientbound_select_known_packs.hex`).
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    let (enabled, select) = consume_config_sync_opening(&mut client).await;

    assert_eq!(enabled, expected_enabled_features_frame());
    assert_eq!(select, expected_select_known_packs_frame());

    server_task.abort();
}

#[tokio::test]
async fn select_known_packs_accepting_core_sends_registries_and_tags() {
    // The M1 vanilla client accepts `minecraft:core:26.2`. The server then sends
    // the 29 `ClientboundRegistryDataPacket`s (each element `data` skipped), the
    // `ClientboundUpdateTagsPacket`, and — once the sync task finishes — the
    // `JoinWorldTask`'s `ClientboundFinishConfigurationPacket`, in
    // `SYNCHRONIZED_REGISTRIES` order.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;

    let finish = complete_sync_and_consume_finish(&mut client).await;
    // The finish frame carries the fieldless config packet (id 3).
    let (declared, used) = decode_varint(&finish);
    assert_eq!(declared, 0, "sub-threshold finish is not zlib-compressed");
    assert_eq!(
        decode_varint(&finish[used..]),
        (CONFIG_CLIENTBOUND_FINISH_CONFIGURATION_ID, 1),
        "finish_configuration id"
    );

    server_task.abort();
}

/// A `ServerboundFinishConfigurationPacket` frame (config serverbound id 3,
/// 0-byte body) compressed (varint(0) declaredLength prefix — sub-threshold).
fn finish_configuration_reply_frame() -> Vec<u8> {
    let mut wire = varint(0);
    wire.extend_from_slice(&varint(3)); // finish_configuration
    frame(&wire)
}

#[tokio::test]
async fn finish_configuration_after_sync_reaches_play() {
    // Issue #96: after the registry sync finishes and `JoinWorldTask` sends
    // `ClientboundFinishConfigurationPacket`, the client's `finish_configuration`
    // reply finishes the task, the outbound protocol flips to play, and the
    // connection reaches the play state — it no longer mismatches and closes.
    let (addr, server_task) = start_server(config_with_threshold(256)).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;
    complete_sync_and_consume_finish(&mut client).await;

    client
        .write_all(&finish_configuration_reply_frame())
        .await
        .expect("write finish_configuration");

    // The handoff is silent here because this test config leaves `enable_join`
    // off (issue #101 Slice B: the join burst runs only when the play listener
    // is wired). Nothing is sent in play yet, but the connection stays open (no
    // deterministic close). The burst itself is covered by `join_burst.rs`.
    expect_silent_open(&mut client).await;
    server_task.abort();
}

/// Drive handshake → hello → registry sync → `JoinWorldTask` → handoff, write
/// `post_sync_writes` to the client socket, and return every decoded frame the
/// tick thread observed in `ctx.inbound` (a recording tickable on the real tick
/// thread). This is the load-bearing assertion for the play handoff: the frame
/// must cross the OWNERSHIP §Network boundary and land in the tick context, not
/// merely "keep the connection open".
async fn handoff_and_collect_play_frames(post_sync_writes: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
    let seen_clone = std::sync::Arc::clone(&seen);
    let server = Server::new(config_with_threshold(256)).with_tickable(move |ctx| {
        for (_id, frame) in &ctx.inbound {
            seen_clone.lock().unwrap().push(frame.bytes.to_vec());
        }
    });
    let listener = server.bind().await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let serve_task = tokio::spawn(async move { server.serve(listener).await });

    let mut client = TcpStream::connect(addr).await.expect("connect");
    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;
    complete_sync_and_consume_finish(&mut client).await;
    for w in post_sync_writes {
        client.write_all(w).await.expect("write post-sync frames");
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let got = seen.lock().unwrap().len();
        if got >= 1 {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for play frames on the tick thread"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let result = seen.lock().unwrap().clone();
    serve_task.abort();
    result
}

#[tokio::test]
async fn play_frame_after_finish_is_forwarded_to_tick_thread() {
    // The seam: once in play, decoded frames are forwarded to the tick thread
    // over the connection's bounded inbound channel instead of being parsed by
    // a listener. The tickable observes the decoded `keep_alive` frame directly
    // in `ctx.inbound` — the forwarding/buffer-drain is load-bearing, not just
    // "the connection stays open".
    let mut body = varint(PLAY_KEEP_ALIVE_PACKET_ID);
    body.extend_from_slice(&7i64.to_be_bytes());
    let mut wire = varint(0);
    wire.extend_from_slice(&body);

    // finish_configuration (the handoff) then the play keep_alive, in separate
    // writes — the play frame crosses to the tick thread after the handoff.
    let observed =
        handoff_and_collect_play_frames(&[finish_configuration_reply_frame(), frame(&wire)]).await;
    assert_eq!(
        observed,
        vec![body],
        "the play frame crosses to the tick thread after the handoff"
    );
}

#[tokio::test]
async fn finish_and_play_coalesced_in_one_write_reaches_tick() {
    // A client that coalesces `finish_configuration` with its first play packet
    // into one TCP chunk: the finish frame triggers the handoff and the already
    // buffered play frame is drained into the tick channel — no frame is lost at
    // the seam. The tickable observes the coalesced play frame directly.
    let mut body = varint(PLAY_KEEP_ALIVE_PACKET_ID);
    body.extend_from_slice(&42i64.to_be_bytes());
    let mut wire = varint(0);
    wire.extend_from_slice(&body);
    let mut coalesced = finish_configuration_reply_frame();
    coalesced.extend_from_slice(&frame(&wire));

    let observed = handoff_and_collect_play_frames(&[coalesced]).await;
    assert_eq!(
        observed,
        vec![body],
        "the coalesced play frame reaches the tick thread (no loss at the seam)"
    );
}

#[tokio::test]
async fn play_frame_crosses_to_tick_thread_tickable() {
    // The full OWNERSHIP §Network seam: after `finish_configuration` hands the
    // connection to the play state, decoded frames cross to the tick thread over
    // the connection's bounded inbound channel and land in the tick context's
    // `inbound` — where #101's play listener will consume them. A tickable
    // observes the `(ConnectionId, ServerboundFrame)` pair.
    use std::sync::Arc;
    use std::sync::Mutex;

    /// A tick's `(tick number, forwarded frame bytes)` pair.
    type TickSeen = Vec<(u64, Vec<u8>)>;
    let seen: Arc<Mutex<TickSeen>> = Arc::new(Mutex::new(Vec::new()));
    let seen_clone = Arc::clone(&seen);
    let server = Server::new(config_with_threshold(256)).with_tickable(move |ctx| {
        for (_id, frame) in &ctx.inbound {
            seen_clone
                .lock()
                .unwrap()
                .push((ctx.tick, frame.bytes.to_vec()));
        }
    });
    let listener = server.bind().await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let serve_task = tokio::spawn(async move { server.serve(listener).await });

    let mut client = TcpStream::connect(addr).await.expect("connect");
    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;
    complete_sync_and_consume_finish(&mut client).await;
    client
        .write_all(&finish_configuration_reply_frame())
        .await
        .expect("write finish_configuration");

    // Play keep_alive (id 28, long 1234567890), sub-threshold.
    let mut body = varint(PLAY_KEEP_ALIVE_PACKET_ID);
    body.extend_from_slice(&1_234_567_890i64.to_be_bytes());
    let mut wire = varint(0);
    wire.extend_from_slice(&body);
    client
        .write_all(&frame(&wire))
        .await
        .expect("write play keep_alive");

    // The tick thread's next tick drains the channel and delivers the frame.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let got = seen.lock().unwrap().clone();
        if got.iter().any(|(_, f)| f == &body) {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for the play frame on the tick thread, got {got:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    serve_task.abort();
}

#[tokio::test]
async fn play_flood_over_inbound_budget_closes_with_eof() {
    // The compressed-frame memory-amplification bound at the TCP level: a
    // hostile client flooding play-state frames whose decompressed size exceeds
    // the inbound drain byte budget (16 MiB) is disconnected with EOF. A long
    // tick interval keeps the tick thread from draining mid-flood, so the
    // byte budget trips deterministically in `forward_play`.
    let mut config = config_with_threshold(256);
    config.tick_interval = Duration::from_secs(60);
    let (addr, server_task) = start_server(config).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;
    complete_sync_and_consume_finish(&mut client).await;
    client
        .write_all(&finish_configuration_reply_frame())
        .await
        .expect("write finish_configuration");

    // Three compressed frames each decompressing to 6 MiB (18 MiB total > 16 MiB
    // budget), framed as the play-state wire (`varint21(len) ++ varint(6MiB) ++
    // zlib`). The packet content is irrelevant — forward_play forwards the
    // decoded frames without parsing them.
    let mut encoder = rivet_protocol::compression_encoder::CompressionEncoder::new(256);
    let wire = encoder.encode(&vec![0x42u8; 6 * 1024 * 1024]).unwrap();
    let framed = frame(&wire);
    let mut flood = Vec::new();
    for _ in 0..3 {
        flood.extend_from_slice(&framed);
    }
    client
        .write_all(&flood)
        .await
        .expect("write compressed flood");

    expect_eof(&mut client).await;
    server_task.abort();
}

/// The live-join config: `enable_join` on (issue #101 Slice B) with channel
/// capacities large enough for the whole 135-frame join burst — the tick fires
/// the burst into the connection's outbound channel in one synchronous pass, so
/// the channel must hold it before the per-connection task drains it to the
/// socket (the 64 default would overflow and disconnect the client mid-join).
fn config_with_join() -> ServerConfig {
    ServerConfig {
        bind_host: std::net::IpAddr::from([127, 0, 0, 1]),
        port: 0,
        max_connections: 16,
        read_timeout: Duration::from_secs(30),
        compression_threshold: 256,
        tick_interval: Duration::from_millis(50),
        catchup_ticks: 5,
        inbound_channel_capacity: 1024,
        outbound_channel_capacity: 1024,
        lifecycle_capacity: 1024,
        enable_join: true,
    }
}

/// The `place_new_player` join burst in Paper's send order — the same sequence
/// `join_burst.rs::burst_order` asserts at the unit level, mirrored here so the
/// live wire path is covered end to end: the 11 join members, the 3 #100 cache
/// packets, the 117 superflat chunks, then the second `sendLevelInfo` block.
fn join_burst_ids() -> Vec<u32> {
    let mut ids = vec![
        PacketType::Login.id(),
        PacketType::ChangeDifficulty.id(),
        PacketType::PlayerAbilities.id(),
        PacketType::SetHeldSlot.id(),
        PacketType::EntityEvent.id(),
        PacketType::PlayerPosition.id(),
        PacketType::InitializeBorder.id(),
        PacketType::SetTime.id(),
        PacketType::SetDefaultSpawnPosition.id(),
        PacketType::GameEvent.id(),
        PacketType::PlayerInfoUpdate.id(),
        PacketType::SetChunkCacheRadius.id(),
        PacketType::SetSimulationDistance.id(),
        PacketType::SetChunkCacheCenter.id(),
    ];
    ids.extend(std::iter::repeat_n(
        PacketType::LevelChunkWithLight.id(),
        117,
    ));
    ids.extend([
        PacketType::InitializeBorder.id(),
        PacketType::SetTime.id(),
        PacketType::SetDefaultSpawnPosition.id(),
        PacketType::GameEvent.id(),
    ]);
    ids
}

/// The end-to-end live join (issue #101 Slice B): boot the server with
/// `enable_join` on, drive the full handshake → login → registry sync →
/// `client_information` → `finish_configuration` over a real socket, then
/// assert the tick-owned play listener fires the entire 135-frame
/// `place_new_player` burst on the wire in Paper's order.
///
/// The client reports view distance 8 (the #153 capture client's value); the
/// Moonrise ladder caps the send distance at `load - 1` (4), so the send-set is
/// the capture's 117-chunk square — not the 77 chunks a `createDefault` view
/// distance 2 would resolve. `join_burst.rs` covers the byte-exact bodies; this
/// test proves the burst is live end to end (login → configuration → play over
/// TCP, framed + compressed as the connection task emits it).
#[tokio::test]
async fn live_join_burst_over_tcp_after_finish_configuration() {
    let (addr, server_task) = start_server(config_with_join()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    login_and_assert_response(&mut client, 256).await;
    consume_config_sync_opening(&mut client).await;
    complete_sync_and_consume_finish(&mut client).await;
    client
        .write_all(&client_information_frame(8))
        .await
        .expect("write client_information");
    client
        .write_all(&finish_configuration_reply_frame())
        .await
        .expect("write finish_configuration");

    // The join burst: read every frame the tick fired, decompressing as needed
    // (below-threshold frames pass through with declaredLength 0; the ~7 KB
    // chunks exceed the 256 threshold and are zlib-compressed).
    let mut seen = Vec::with_capacity(135);
    for _ in 0..135 {
        let (_, payload) = read_compressed_packet(&mut client).await;
        let (id, _) = decode_varint(&payload);
        seen.push(id as u32);
    }

    assert_eq!(seen, join_burst_ids(), "the 135-frame live join burst");

    // The connection stays open in play (a live session, not a silent close).
    expect_silent_open(&mut client).await;
    server_task.abort();
}
