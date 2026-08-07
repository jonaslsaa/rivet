//! Byte-level integration tests for the M1 server skeleton (issue #145): the
//! TCP listener + VarInt21 framing + handshake/next-state boundary, exercised
//! over a real loopback socket.
//!
//! Every test boots a `Server` on an ephemeral port and drives it with a raw
//! `TcpStream`, so framing, partial reads, and disconnects are tested at the
//! byte level rather than through a library client.

use std::time::Duration;

use rivet_server::server::{Server, ServerConfig};
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// Intent ids (ClientIntent byId).
const STATUS_INTENT: i32 = 1;
const LOGIN_INTENT: i32 = 2;
const TRANSFER_INTENT: i32 = 3;

const PROTOCOL_VERSION: i32 = 776;

/// Handshake packet id (HandshakeProtocols.SERVERBOUND index 0).
const HANDSHAKE_PACKET_ID: i32 = 0;

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

fn default_config() -> ServerConfig {
    ServerConfig {
        bind_host: std::net::IpAddr::from([127, 0, 0, 1]),
        port: 0,
        max_connections: 16,
        read_timeout: Duration::from_secs(30),
        compression_threshold: 256,
        tick_interval: Duration::from_millis(50),
        catchup_ticks: 5,
        inbound_channel_capacity: 64,
        outbound_channel_capacity: 64,
        lifecycle_capacity: 64,
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

/// VarInt21-frame a raw packet body.
fn frame(body: &[u8]) -> Vec<u8> {
    let mut out = varint(body.len() as i32);
    out.extend_from_slice(body);
    out
}

/// A full `ClientIntentionPacket` frame: `(id, protocolVersion, hostName, port,
/// intention)` — the wire form ported from Paper's `ClientIntentionPacket`.
fn handshake_frame(protocol_version: i32, hostname: &str, port: u16, intention: i32) -> Vec<u8> {
    let mut body = varint(HANDSHAKE_PACKET_ID);
    body.extend_from_slice(&varint(protocol_version));
    body.extend_from_slice(&varint(hostname.len() as i32));
    body.extend_from_slice(hostname.as_bytes());
    body.extend_from_slice(&port.to_be_bytes());
    body.extend_from_slice(&varint(intention));
    frame(&body)
}

/// A `ServerboundStatusRequestPacket` frame (id 0, empty body).
fn status_request_frame() -> Vec<u8> {
    frame(&varint(0))
}

/// A `ServerboundPingRequestPacket` frame (id 1, one long).
fn ping_frame(time: i64) -> Vec<u8> {
    let mut body = varint(1);
    body.extend_from_slice(&time.to_be_bytes());
    frame(&body)
}

/// Read with a deadline; returns `None` on timeout (nothing arrived, still open).
async fn read_with_deadline(stream: &mut TcpStream, buf: &mut [u8]) -> Option<usize> {
    tokio::time::timeout(Duration::from_millis(200), stream.read(buf))
        .await
        .ok()
        .map(|r| r.expect("read"))
}

/// Read until EOF (0) or a full buffer, for the "server closed" assertions.
async fn expect_eof(stream: &mut TcpStream) {
    let mut buf = [0u8; 128];
    let mut total = 0usize;
    loop {
        match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(0)) => return,
            Ok(Ok(n)) => {
                total += n;
                assert!(
                    total < 1024,
                    "expected EOF but server is sending data: {total} bytes"
                );
            }
            Ok(Err(_)) => return,
            Err(_) => panic!("timed out waiting for EOF"),
        }
    }
}

/// Read until EOF asserting the server wrote nothing before closing. The
/// handshake-rejection paths close without a disconnect packet body (the formatted
/// body is deferred to #96/epic #10), so a silent EOF is the load-bearing assertion.
async fn expect_eof_silent(stream: &mut TcpStream) {
    let mut buf = [0u8; 128];
    // A single read round: EOF (0) and read-error both mean closed; data means the
    // server wrote a disconnect body this slice must not invent.
    match tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
        Ok(Ok(0)) => {}
        Ok(Ok(n)) => panic!("server sent {n} bytes before closing; expected a silent EOF"),
        Ok(Err(_)) => {}
        Err(_) => panic!("timed out waiting for EOF"),
    }
}

#[tokio::test]
async fn fragmented_handshake_decodes_and_status_request_stays_open() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Split the handshake across several writes (partial headers + partial body).
    let full = handshake_frame(PROTOCOL_VERSION, "localhost", 25565, STATUS_INTENT);
    for chunk in full.chunks(3) {
        client.write_all(chunk).await.expect("write");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // Status request in a separate write, then confirm the connection stayed open.
    client
        .write_all(&status_request_frame())
        .await
        .expect("write");

    // No data and no EOF → the status listener accepted both frames.
    let mut buf = [0u8; 64];
    assert!(
        read_with_deadline(&mut client, &mut buf).await.is_none(),
        "connection should remain open after status_request"
    );

    server_task.abort();
}

#[tokio::test]
async fn coalesced_frames_decode_in_one_batch() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Handshake + status_request in a single write (two VarInt21 frames back to back).
    let mut coalesced = handshake_frame(PROTOCOL_VERSION, "localhost", 25565, STATUS_INTENT);
    coalesced.extend_from_slice(&status_request_frame());
    client.write_all(&coalesced).await.expect("write");

    let mut buf = [0u8; 64];
    assert!(
        read_with_deadline(&mut client, &mut buf).await.is_none(),
        "connection should remain open after coalesced frames"
    );

    server_task.abort();
}

#[tokio::test]
async fn login_intention_alone_stays_open_until_hello() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Login handshake alone (no hello yet) stays open — the state transition to
    // the login listener is accepted; only a frame is refused.
    client
        .write_all(&handshake_frame(
            PROTOCOL_VERSION,
            "localhost",
            25565,
            LOGIN_INTENT,
        ))
        .await
        .expect("write");
    let mut buf = [0u8; 16];
    assert!(
        read_with_deadline(&mut client, &mut buf).await.is_none(),
        "login state should not close before any frame arrives"
    );

    // The login listener is reached, so a handshake-shaped frame (id 0) is now
    // the `ServerboundHelloPacket` dispatch — a hello with an over-length name
    // is malformed (`Utf8String.read(16)`), closing deterministically.
    let mut body = varint(0);
    body.extend_from_slice(&varint(17)); // declared name length > 16 units
    body.extend_from_slice(&[b'a'; 17]);
    body.extend_from_slice(&[0u8; 16]); // profile uuid
    client.write_all(&frame(&body)).await.expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn zero_length_frame_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // `\x00` — VarInt21 length zero → `CorruptedFrameException("Frame length cannot be zero")`.
    client.write_all(&[0x00]).await.expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn header_wider_than_21_bits_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Three continuation bytes → length varint wider than 21 bits.
    client.write_all(&[0x80, 0x80, 0x80]).await.expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn unknown_intention_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    client
        .write_all(&handshake_frame(PROTOCOL_VERSION, "localhost", 25565, 5))
        .await
        .expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn wrong_protocol_version_login_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    client
        .write_all(&handshake_frame(100, "localhost", 25565, LOGIN_INTENT))
        .await
        .expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn wrong_protocol_version_status_stays_open() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Vanilla skips the protocol-version gate for STATUS — a wrong-version
    // client may still ping.
    client
        .write_all(&handshake_frame(100, "localhost", 25565, STATUS_INTENT))
        .await
        .expect("write");
    let time = 7i64;
    client.write_all(&ping_frame(time)).await.expect("write");

    let mut buf = [0u8; 16];
    let n = read_with_deadline(&mut client, &mut buf)
        .await
        .expect("pong");
    let mut expected = varint(9);
    expected.push(0x01);
    expected.extend_from_slice(&time.to_be_bytes());
    assert_eq!(&buf[..n], &expected[..]);

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn oversized_hostname_byte_length_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Check 1 of `readUtf(Short.MAX_VALUE)`: a length varint over
    // Short.MAX_VALUE * 3 = 98301 fires before the payload is touched, so the
    // frame can be short.
    let mut body = varint(HANDSHAKE_PACKET_ID);
    body.extend_from_slice(&varint(PROTOCOL_VERSION));
    body.extend_from_slice(&varint(98_302));
    client.write_all(&frame(&body)).await.expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn oversized_hostname_decoded_length_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Check 4 of `readUtf(Short.MAX_VALUE)`: 32768 NUL bytes decode to 32768
    // UTF-16 code units, over the 32767 char cap. The byte length (32768) is
    // under the 98301 byte cap, so only the decoded-length check catches it.
    let mut body = varint(HANDSHAKE_PACKET_ID);
    body.extend_from_slice(&varint(PROTOCOL_VERSION));
    body.extend_from_slice(&varint(32_768));
    body.extend_from_slice(&[0u8; 32_768]);
    body.extend_from_slice(&25565u16.to_be_bytes());
    body.extend_from_slice(&varint(STATUS_INTENT));
    client.write_all(&frame(&body)).await.expect("write");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn read_utf_declared_payload_longer_than_frame_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // A complete frame whose hostname length varint declares 100 bytes but the
    // body only holds a 3-byte hostname + port + intention (6 bytes left after the
    // length varint). Java `Utf8String.read` check 3 fires — DecoderException
    // "Not enough bytes in buffer, expected 100, but got 6" — and `PacketDecoder`
    // closes the connection. The close is silent: no disconnect body is invented.
    let mut body = varint(HANDSHAKE_PACKET_ID);
    body.extend_from_slice(&varint(PROTOCOL_VERSION));
    body.extend_from_slice(&varint(100));
    body.extend_from_slice(b"abc");
    body.extend_from_slice(&25565u16.to_be_bytes());
    body.extend_from_slice(&varint(STATUS_INTENT));
    client.write_all(&frame(&body)).await.expect("write");

    expect_eof_silent(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn trailing_bytes_after_well_formed_handshake_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // A well-formed handshake frame with 3 extra body bytes. Java `PacketDecoder`
    // throws IOException "was larger than I expected, found 3 bytes extra" after
    // decoding the packet, closing the connection instead of leaking the bytes
    // into the next protocol state.
    let mut body = varint(HANDSHAKE_PACKET_ID);
    body.extend_from_slice(&varint(PROTOCOL_VERSION));
    body.extend_from_slice(&varint("localhost".len() as i32));
    body.extend_from_slice(b"localhost");
    body.extend_from_slice(&25565u16.to_be_bytes());
    body.extend_from_slice(&varint(STATUS_INTENT));
    body.extend_from_slice(&[0xAA, 0xBB, 0xCC]);
    client.write_all(&frame(&body)).await.expect("write");

    expect_eof_silent(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn trailing_bytes_after_status_request_closes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    client
        .write_all(&handshake_frame(
            PROTOCOL_VERSION,
            "localhost",
            25565,
            STATUS_INTENT,
        ))
        .await
        .expect("write");
    // A `ServerboundStatusRequestPacket` (id 0) with 1 extra body byte. Java
    // `PacketDecoder` throws IOException "was larger than I expected, found 1
    // bytes extra" after decoding the packet, closing the connection instead of
    // leaking the byte into the ping path.
    let mut body = varint(0);
    body.push(0xAA);
    client.write_all(&frame(&body)).await.expect("write");

    expect_eof_silent(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn transfer_intention_closes_immediately_without_body() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // Paper `acceptsTransfers()` is false by default: TRANSFER sets up the login
    // CLIENTBOUND, sends `ClientboundLoginDisconnectPacket` (transfers_disabled),
    // then disconnects — the connection never enters the login listener and the
    // protocol-version gate never runs. This slice closes at the handshake boundary
    // and defers the formatted disconnect body to #96, so the client sees a silent
    // EOF, matching Paper's observable close.
    client
        .write_all(&handshake_frame(
            PROTOCOL_VERSION,
            "localhost",
            25565,
            TRANSFER_INTENT,
        ))
        .await
        .expect("write");

    expect_eof_silent(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn ping_echo_then_close() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    client
        .write_all(&handshake_frame(
            PROTOCOL_VERSION,
            "localhost",
            25565,
            STATUS_INTENT,
        ))
        .await
        .expect("write");
    // Vanilla answers a ping_request at any point in the status state, with or
    // without a preceding status_request.
    let time = 0x1122_3344_5566_7788i64;
    client.write_all(&ping_frame(time)).await.expect("write");

    // Expect `[length 0x09][id 0x01][8-byte long]` (Varint21-framed pong).
    let mut buf = [0u8; 16];
    let n = read_with_deadline(&mut client, &mut buf)
        .await
        .expect("pong");
    let expected = {
        let mut e = Vec::new();
        e.extend_from_slice(&varint(9));
        e.push(0x01); // StatusProtocols.CLIENTBOUND pong_response id
        e.extend_from_slice(&time.to_be_bytes());
        e
    };
    assert_eq!(&buf[..n], &expected[..], "pong frame bytes");

    // Server disconnects after the ping (multiplayer.status.request_handled).
    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn status_request_then_ping_echoes() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    client
        .write_all(&handshake_frame(
            PROTOCOL_VERSION,
            "localhost",
            25565,
            STATUS_INTENT,
        ))
        .await
        .expect("write");
    client
        .write_all(&status_request_frame())
        .await
        .expect("write");
    let time = 42i64;
    client.write_all(&ping_frame(time)).await.expect("write");

    let mut buf = [0u8; 16];
    let n = read_with_deadline(&mut client, &mut buf)
        .await
        .expect("pong");
    let mut expected = varint(9);
    expected.push(0x01);
    expected.extend_from_slice(&time.to_be_bytes());
    assert_eq!(&buf[..n], &expected[..]);

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn eof_closes_connection() {
    let (addr, server_task) = start_server(default_config()).await;
    let mut client = TcpStream::connect(addr).await.expect("connect");

    // A half-close (FIN) with no data: the server sees EOF and closes.
    client.shutdown().await.expect("shutdown");

    expect_eof(&mut client).await;
    server_task.abort();
}

#[tokio::test]
async fn connection_limit_closes_excess() {
    let mut config = default_config();
    config.max_connections = 1;
    let (addr, server_task) = start_server(config).await;

    // First client stays active (status handshake + request, then parked open).
    let mut first = TcpStream::connect(addr).await.expect("connect first");
    first
        .write_all(&handshake_frame(
            PROTOCOL_VERSION,
            "localhost",
            25565,
            STATUS_INTENT,
        ))
        .await
        .expect("write");
    first
        .write_all(&status_request_frame())
        .await
        .expect("write");
    let mut buf = [0u8; 16];
    assert!(
        read_with_deadline(&mut first, &mut buf).await.is_none(),
        "first connection should stay open"
    );

    // Second connection is over the cap → deterministically closed.
    let mut second = TcpStream::connect(addr).await.expect("connect second");
    expect_eof(&mut second).await;

    server_task.abort();
}
