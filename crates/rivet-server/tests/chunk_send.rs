//! End-to-end tests for the Moonrise direct chunk-send path (issue #100): the
//! ordered 117-chunk send-set framed and delivered over a real TCP socket, and
//! the bounded tick→network backpressure it respects.
//!
//! The byte ground truth is the #194 superflat chunk capture fixture
//! (`rivet-protocol/tests/fixtures/chunk_golden_full.hex` — the first of the 117
//! bodies, coords -5/-4). All 117 bodies differ only in the 8-byte BE
//! coordinate header. The send order is the deterministic X-major/Z-minor
//! raster `rivet-capture` canonicalizes the fixture to (`normalize.rs` sorts
//! chunk packets by coordinate) — Paper's raw wire order is timing-dependent and
//! outside the parity contract.

use std::sync::Arc;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use rivet_protocol::compression_decoder::CompressionDecoder;
use rivet_protocol::var_int;
use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
use rivet_server::server::ServerConfig;
use rivet_server::server::level::{
    PlayerChunkLoader, ServerLevel, ServerLevelConfig, encode_play_frame,
};
use rivet_server::server::network::connection::Connection;
use rivet_server::server::network::connection_id::ConnectionId;
use rivet_server::server::tick::channels::{InboundDrained, LifecycleEvent, OutboundEvent};
use rivet_server::server::tick::registry::{ConnectionRegistry, OutboundError};
use rivet_server::server::tick::shutdown::Shutdown;
use tokio::io::AsyncReadExt;

/// The #194 capture fixture's first `level_chunk_with_light` body (coords
/// -5/-4). All 117 bodies in the capture are byte-identical apart from the
/// 8-byte BE coordinate header.
const GOLDEN_FULL: &str = include_str!("../../rivet-protocol/tests/fixtures/chunk_golden_full.hex");

fn hex(s: &str) -> Vec<u8> {
    let trimmed: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
    (0..trimmed.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).unwrap())
        .collect()
}

fn test_config() -> ServerConfig {
    ServerConfig {
        compression_threshold: 256,
        outbound_channel_capacity: 1024,
        ..ServerConfig::default()
    }
}

/// Build the ordered M1 send-set frames at the fixture's compression threshold.
fn send_set_frames() -> Vec<Bytes> {
    let mut world = ServerLevel::new(ServerLevelConfig::default());
    let mut loader = PlayerChunkLoader::new(world.view().center());
    let packets = loader
        .add_and_send_chunks(&mut world, None)
        .expect("build the M1 send set");
    packets
        .iter()
        .map(|p| encode_play_frame(p, 256).expect("frame a play packet"))
        .collect()
}

/// A minimal client-side decoder: split a raw byte stream back into
/// `(packet_id, body)` pairs (VarInt21 framing + decompression).
fn decode_wire(bytes: &[u8]) -> Vec<(i32, Vec<u8>)> {
    let mut buf = BytesMut::from(bytes);
    let framer = Varint21FrameDecoder::new(None);
    let mut decompressor = CompressionDecoder::new(256, true);
    let mut out = Vec::new();
    while let Some(frame) = framer.decode(&mut buf).expect("well-formed frame") {
        let mut packet = decompressor.decode(&frame).expect("decompressible frame");
        let id = var_int::read(&mut packet);
        out.push((id, packet.to_vec()));
    }
    out
}

/// The full M1 send-set (3 cache packets + 117 chunks), framed at the fixture
/// threshold 256, reaches a real TCP client in order and decodes byte-identically
/// to the #194 capture: the 117 chunk bodies match the fixture apart from the
/// 8-byte BE coordinate header, and the cache packets carry radius 4 /
/// simulation 4 / center (0,0).
#[tokio::test]
async fn chunk_send_set_reaches_a_real_tcp_client_in_order() {
    let frames = send_set_frames();
    assert_eq!(frames.len(), 3 + 117);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    // Client: connect, read until EOF, decode every frame.
    let client = tokio::spawn(async move {
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut all = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = tokio::time::timeout(Duration::from_secs(5), sock.read(&mut buf))
                .await
                .expect("read timeout")
                .expect("read");
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        decode_wire(&all)
    });

    // Server: accept, queue every frame, flush to the socket, close.
    let (sock, _remote) = listener.accept().await.unwrap();
    let (_read, write) = sock.into_split();
    let mut conn = Connection::new(
        ConnectionId(1),
        addr,
        Arc::new(test_config()),
        Arc::new(Shutdown::new()),
        write,
        InboundDrained::new(),
    );
    for frame in &frames {
        conn.queue_raw_frame(frame.clone());
    }
    conn.flush_out().await.expect("flush");
    conn.close().await;

    let decoded = client.await.expect("client decode task");

    // The 3 cache packets come first (radius → simulation → center).
    assert_eq!(decoded[0].0, 95, "set_chunk_cache_radius");
    assert_eq!(decoded[0].1, vec![0x04], "radius 4");
    assert_eq!(decoded[1].0, 111, "set_simulation_distance");
    assert_eq!(decoded[1].1, vec![0x04], "simulation distance 4");
    assert_eq!(decoded[2].0, 94, "set_chunk_cache_center");
    assert_eq!(decoded[2].1, vec![0x00, 0x00], "center (0, 0)");

    // Then exactly 117 chunk packets, in the deterministic X-major/Z-minor
    // raster the fixture canonicalizes to.
    let chunk_packets = &decoded[3..];
    assert_eq!(chunk_packets.len(), 117);
    let golden = hex(GOLDEN_FULL);
    let mut seen = std::collections::HashSet::new();
    // The raster: X-major, Z-minor. The ±5 columns start at z=-4 (the z=±5
    // corners are cut); every other column spans -5..=5.
    let mut expected_x = -5i32;
    let mut expected_z = -4i32;
    for (i, (id, body)) in chunk_packets.iter().enumerate() {
        assert_eq!(*id, 45, "level_chunk_with_light at {i}");
        // Body matches the fixture apart from the 8-byte BE coordinate header.
        assert_eq!(
            &body[8..],
            &golden[8..],
            "chunk body {i} matches the fixture"
        );
        let x = i32::from_be_bytes([body[0], body[1], body[2], body[3]]);
        let z = i32::from_be_bytes([body[4], body[5], body[6], body[7]]);
        assert_eq!(&body[..8], &[x.to_be_bytes(), z.to_be_bytes()].concat());
        assert!(seen.insert((x, z)), "no duplicate chunk ({x},{z})");
        if x != expected_x {
            assert_eq!(x, expected_x + 1, "column advances by one at {i}");
            expected_x = x;
            expected_z = if x.abs() == 5 { -4 } else { -5 };
            assert_eq!(z, expected_z, "new column starts at its first z at {i}");
        } else {
            assert_eq!(z, expected_z, "z runs sequentially within a column at {i}");
        }
        // Advance the expected z for the next chunk in this column.
        expected_z += 1;
    }
    assert_eq!(
        chunk_packets[0].1, golden,
        "first chunk is exactly the fixture"
    );
}

/// The send-set is delivered over TCP even with compression disabled: the frames
/// are the plain `varint21(varint(id) ++ body)` form (no inner `varint(0)`), and
/// a client with no compression stage decodes them.
#[tokio::test]
async fn chunk_send_set_tcp_without_compression_uses_plain_frames() {
    let mut world = ServerLevel::new(ServerLevelConfig::default());
    let mut loader = PlayerChunkLoader::new(world.view().center());
    let packets = loader.add_and_send_chunks(&mut world, None).unwrap();
    let frames: Vec<Bytes> = packets
        .iter()
        .map(|p| encode_play_frame(p, -1).unwrap())
        .collect();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let client = tokio::spawn(async move {
        let mut sock = tokio::net::TcpStream::connect(addr).await.unwrap();
        let mut all = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        // No compression stage: every frame payload is `varint(id) ++ body`.
        let mut buf = BytesMut::from(&all[..]);
        let framer = Varint21FrameDecoder::new(None);
        let mut out = Vec::new();
        while let Some(frame) = framer.decode(&mut buf).unwrap() {
            let mut packet = frame;
            let id = var_int::read(&mut packet);
            out.push((id, packet.to_vec()));
        }
        out
    });

    let (sock, _remote) = listener.accept().await.unwrap();
    let (_read, write) = sock.into_split();
    let mut conn = Connection::new(
        ConnectionId(1),
        addr,
        Arc::new(test_config()),
        Arc::new(Shutdown::new()),
        write,
        InboundDrained::new(),
    );
    for frame in &frames {
        conn.queue_raw_frame(frame.clone());
    }
    conn.flush_out().await.unwrap();
    conn.close().await;

    let decoded = client.await.unwrap();
    assert_eq!(decoded.len(), 3 + 117);
    // First cache packet: radius 4.
    assert_eq!(decoded[0].0, 95);
    assert_eq!(decoded[0].1, vec![0x04]);
    // A chunk: id 45, body byte-identical to the fixture.
    let golden = hex(GOLDEN_FULL);
    assert_eq!(decoded[3].0, 45);
    assert_eq!(decoded[3].1, golden);
}

/// Bounded tick→network backpressure: the 120-frame send-set is larger than a
/// small outbound channel, so enqueueing it fires the overflow policy — the
/// offending connection is pruned (disconnected) rather than the server
/// stopping, and the overflow names the connection.
#[test]
fn chunk_send_set_respects_bounded_outbound_channel() {
    let frames = send_set_frames();
    assert_eq!(frames.len(), 3 + 117);

    let mut reg = ConnectionRegistry::new();
    let id = ConnectionId(7);
    let (_in_tx, in_rx) = tokio::sync::mpsc::channel(4);
    let (out_tx, _out_rx) = tokio::sync::mpsc::channel(8); // far smaller than 120
    reg.apply(LifecycleEvent::Connect {
        id,
        remote: std::net::SocketAddr::from(([127, 0, 0, 1], 25565)),
        in_rx,
        out_tx,
        drained: InboundDrained::new(),
    });

    let mut queued = 0usize;
    let mut overflow = None;
    for frame in &frames {
        match reg.send(
            id,
            OutboundEvent::Packet {
                frame: frame.clone(),
            },
        ) {
            Ok(()) => queued += 1,
            Err(e) => {
                overflow = Some(e);
                break;
            }
        }
    }
    let overflow = overflow.expect("a 120-frame set overflows an 8-capacity channel");
    assert_eq!(overflow, OutboundError::Overflow(id));
    // The overflow policy pruned the connection (Paper disconnects on outbound
    // overflow); the server keeps running (the registry is not the server).
    assert!(!reg.contains(id));
    assert!(
        queued <= 8,
        "only the bounded capacity was queued ({queued})"
    );
    assert!(queued > 0, "the cache packets queued before the overflow");
}
