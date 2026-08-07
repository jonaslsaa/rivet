use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use rivet_protocol::compression_decoder::CompressionDecoder;
use rivet_protocol::compression_encoder::CompressionEncoder;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
use rivet_protocol::varint21_length_field_prepender::encode_frame;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;

use super::connection_id::ConnectionId;
use super::packet_listener::{DisconnectReason, ListenerOutcome, PacketListener};
use crate::server::ServerConfig;
use crate::server::tick::channels::{
    MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN, MAX_INBOUND_FRAMES_PER_DRAIN, ServerboundFrame,
};

/// The enabled compression handlers for one connection: the
/// `CompressionDecoder`/`CompressionEncoder` pair Paper's
/// `Connection.setupCompression` inserts between the VarInt21 frame codec and
/// the packet codec. Present only while compression is enabled
/// (`threshold >= 0`); `Connection::setup_compression` creates it on first
/// enable and reconfigures it on subsequent calls (mirrors `setThreshold` on
/// the existing handlers).
struct CompressionState {
    threshold: i32,
    validate_decompressed: bool,
    decoder: CompressionDecoder,
    encoder: CompressionEncoder,
}

impl CompressionState {
    fn new(threshold: i32, validate_decompressed: bool) -> Self {
        CompressionState {
            threshold,
            validate_decompressed,
            decoder: CompressionDecoder::new(threshold, validate_decompressed),
            encoder: CompressionEncoder::new(threshold),
        }
    }

    fn set_threshold(&mut self, threshold: i32, validate_decompressed: bool) {
        self.threshold = threshold;
        self.validate_decompressed = validate_decompressed;
        self.decoder.set_threshold(threshold, validate_decompressed);
        self.encoder.set_threshold(threshold);
    }
}

/// Outcome of [`Connection::process_inbound`]: whether the per-connection loop
/// keeps dispatching to the listener or must hand the connection to the play
/// state (forwarding decoded frames to the tick thread).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundOutcome {
    /// The listener stayed in place; the connection remains in its pre-play state.
    Keep,
    /// The listener transitioned the connection to play; subsequent frames are
    /// forwarded to the tick thread (OWNERSHIP §Network).
    Play,
}

/// The per-connection state machine on the tokio side. Mirrors the parts of
/// `net.minecraft.network.Connection` that matter before the play state: the
/// VarInt21 frame decoder, the outbound protocol, and the packet listener that
/// dispatches each fully-framed inbound packet. Play-state packets cross to the
/// tick thread over channels keyed by `ConnectionId` (OWNERSHIP §Network); the
/// per-connection task owns those channel ends (sub-issue #93). The tick thread
/// can also queue already-encoded frames here via [`Connection::queue_raw_frame`]
/// (drained to the socket by `flush_out`).
///
/// The socket write half lives here so `send_packet` can append to a pending
/// outbound buffer without holding the task's read loop; the per-connection task
/// calls `flush_out` after each inbound batch and before closing — the Rust
/// analog of netty's write-then-flush.
pub struct Connection {
    id: ConnectionId,
    remote_addr: SocketAddr,
    config: Arc<ServerConfig>,
    write: OwnedWriteHalf,
    /// Inbound bytes not yet decoded into a full frame.
    read_buf: BytesMut,
    /// Encoded frames pending on the wire (netty `channel.write` queue).
    out_buf: BytesMut,
    decoder: Varint21FrameDecoder,
    /// The compression pipeline stage, when enabled (`setupCompression`).
    compression: Option<CompressionState>,
    outbound_protocol: Option<ConnectionProtocol>,
    /// The play-state inbound drain window: cumulative decompressed bytes
    /// forwarded to the tick channel since the channel was last observed empty
    /// (the tick thread drained it). The frame/byte budget in
    /// [`Connection::forward_play`] is enforced against this window so a
    /// hostile client cannot let the bounded channel retain multi-GiB of
    /// decompressed frames. Reset when the channel is observed empty.
    inbound_window_bytes: usize,
    /// The play-state inbound drain window, frame-count half (see
    /// [`Self::inbound_window_bytes`]).
    inbound_window_frames: usize,
}

impl Connection {
    pub fn new(
        id: ConnectionId,
        remote_addr: SocketAddr,
        config: Arc<ServerConfig>,
        write: OwnedWriteHalf,
    ) -> Self {
        Connection {
            id,
            remote_addr,
            config,
            write,
            read_buf: BytesMut::new(),
            out_buf: BytesMut::new(),
            decoder: Varint21FrameDecoder::new(None),
            // Paper's pipeline has no COMPRESS/DECOMPRESS handlers until
            // `setupCompression` is called at login.
            compression: None,
            outbound_protocol: None,
            inbound_window_bytes: 0,
            inbound_window_frames: 0,
        }
    }

    pub fn id(&self) -> ConnectionId {
        self.id
    }

    pub fn remote_addr(&self) -> SocketAddr {
        self.remote_addr
    }

    /// `Connection.setupOutboundProtocol(ProtocolInfo)` — records which state the
    /// outbound direction is in. Sends are validated against it.
    pub fn set_outbound_protocol(&mut self, protocol: ConnectionProtocol) {
        self.outbound_protocol = Some(protocol);
    }

    /// The protocol the outbound direction is currently in (`None` before the
    /// first `set_outbound_protocol`). The configuration→play handoff flips it
    /// to [`ConnectionProtocol::Play`] before handing the connection off.
    pub fn outbound_protocol(&self) -> Option<ConnectionProtocol> {
        self.outbound_protocol
    }

    /// The encoded frames queued for the wire (netty `channel.write` queue).
    /// `flush_out` writes them to the socket; unit tests inspect the queue to
    /// assert what a listener queued without a live socket.
    #[cfg(test)]
    pub(crate) fn outbound_bytes(&self) -> &[u8] {
        &self.out_buf
    }

    /// Append inbound bytes, decode every complete frame, and dispatch it to the
    /// current listener. Returns `Err(reason)` on the first corrupted frame or
    /// listener rejection; the per-connection task then closes the socket.
    /// A `ListenerOutcome::Switch` replaces the listener (the handshake→
    /// status/login boundary); a `ListenerOutcome::Play` ends the pre-play
    /// dispatch and reports [`InboundOutcome::Play`] so the caller hands the
    /// connection to the play state (forwarding to the tick thread).
    ///
    /// When compression is enabled, each VarInt21 frame payload is run through
    /// the compression decoder before dispatch — the netty pipeline order
    /// SPLITTER → DECOMPRESS → DECODER.
    ///
    /// The inbound drain budget ([`MAX_INBOUND_FRAMES_PER_DRAIN`] /
    /// [`MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN`]) is enforced per call (one
    /// TCP read): a hostile client coalescing thousands of max-size compressed
    /// frames into a single read is disconnected instead of letting the
    /// dispatch loop decode and allocate multi-GiB.
    pub fn process_inbound(
        &mut self,
        data: &[u8],
        listener: &mut Box<dyn PacketListener>,
    ) -> Result<InboundOutcome, DisconnectReason> {
        self.read_buf.extend_from_slice(data);
        let mut drained_bytes = 0usize;
        let mut drained_frames = 0usize;
        loop {
            let Some(packet) = self.next_frame()? else {
                break; // need more bytes
            };
            drained_frames += 1;
            drained_bytes += packet.len();
            if drained_frames > MAX_INBOUND_FRAMES_PER_DRAIN
                || drained_bytes > MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN
            {
                return Err(inbound_overflow(drained_frames, drained_bytes));
            }
            let config = Arc::clone(&self.config);
            match listener.handle_frame(packet, self, &config) {
                Ok(ListenerOutcome::Keep) => {}
                Ok(ListenerOutcome::Switch(next)) => *listener = next,
                Ok(ListenerOutcome::Play) => return Ok(InboundOutcome::Play),
                Err(reason) => return Err(reason),
            }
        }
        Ok(InboundOutcome::Keep)
    }

    /// Decode the next complete frame from `read_buf`, decompressing it when the
    /// compression stage is enabled. `Ok(None)` means the buffer does not yet
    /// hold a full frame. Shared by the pre-play dispatch ([`Self::process_inbound`])
    /// and the play-state forwarding ([`Self::forward_play`]); mirrors the netty
    /// pipeline order SPLITTER → DECOMPRESS → DECODER.
    fn next_frame(&mut self) -> Result<Option<Bytes>, DisconnectReason> {
        let frame = match self.decoder.decode(&mut self.read_buf) {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(None),
            Err(e) => {
                return Err(DisconnectReason::Malformed(format!(
                    "corrupted frame: {}",
                    e.message
                )));
            }
        };
        let packet = match &mut self.compression {
            Some(compression) => compression
                .decoder
                .decode(&frame)
                .map_err(|e| DisconnectReason::Malformed(format!("compression: {}", e.message)))?,
            None => frame,
        };
        Ok(Some(packet))
    }

    /// Forward decoded play-state frames to the tick thread — the OWNERSHIP
    /// §Network play boundary. The per-connection task stops parsing packets
    /// into a listener and sends the raw decoded frames over the connection's
    /// bounded inbound channel; the tick thread owns play-state dispatch.
    ///
    /// `data` is appended to the inbound buffer first; `None` drains frames
    /// already buffered (a play packet coalesced with the `finish_configuration`
    /// that triggered the handoff — the same TCP chunk can carry both).
    ///
    /// Backpressure is the bounded channel: a full channel blocks until the tick
    /// thread drains it (each tick), and a closed channel (the tick thread is
    /// gone) is a server stop, reported as [`DisconnectReason::ServerShutdown`].
    ///
    /// The inbound budget ([`MAX_INBOUND_FRAMES_PER_DRAIN`] /
    /// [`MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN`]) is enforced here as an
    /// *admission* cap against a sliding window (`inbound_window_*`) that resets
    /// whenever the channel is observed empty (the tick thread drained it). Each
    /// compressed frame can decompress to 8 MiB, so without the cap a hostile
    /// client flooding 8 MiB frames faster than the tick drains could fill the
    /// 1024-deep channel with multi-GiB of live frames; the admission cap
    /// disconnects exactly such a client, while a client the tick keeps up with
    /// is never affected (its window resets each drain). Exceeding the budget
    /// reports [`DisconnectReason::InboundOverflow`].
    ///
    /// The authoritative per-tick bound is the tick side: [`ConnectionRegistry::drain_one`]
    /// stops draining at the same budget, so even a sender that races the window
    /// reset (observing the channel empty mid-drain and refilling) cannot make
    /// one tick deliver more than the budget. This admission cap is the
    /// memory-retention backstop that keeps the bounded channel from holding
    /// multi-GiB before that tick-side bound is even reached.
    pub async fn forward_play(
        &mut self,
        data: Option<&[u8]>,
        in_tx: &tokio::sync::mpsc::Sender<ServerboundFrame>,
    ) -> Result<(), DisconnectReason> {
        if let Some(data) = data {
            self.read_buf.extend_from_slice(data);
        }
        loop {
            let Some(packet) = self.next_frame()? else {
                break;
            };
            // The tick thread fully drained the channel since the last push:
            // the client is keeping up, so the window restarts. `capacity()`
            // is the number of free slots; at the max the channel is empty.
            if in_tx.capacity() == in_tx.max_capacity() {
                self.inbound_window_bytes = 0;
                self.inbound_window_frames = 0;
            }
            self.inbound_window_bytes += packet.len();
            self.inbound_window_frames += 1;
            if self.inbound_window_bytes > MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN
                || self.inbound_window_frames > MAX_INBOUND_FRAMES_PER_DRAIN
            {
                return Err(inbound_overflow(
                    self.inbound_window_frames,
                    self.inbound_window_bytes,
                ));
            }
            if in_tx
                .send(ServerboundFrame { bytes: packet })
                .await
                .is_err()
            {
                return Err(DisconnectReason::ServerShutdown);
            }
        }
        Ok(())
    }

    /// `Connection.setupCompression(int, boolean)` — enables, reconfigures, or
    /// disables the compression pipeline stage.
    ///
    /// Mirrors Paper: `threshold >= 0` inserts (on first call) or reconfigures
    /// (on later calls) the COMPRESS/DECOMPRESS handlers; `threshold < 0`
    /// removes them. The login flow calls this *after* queuing the
    /// `ClientboundLoginCompressionPacket`, so that packet itself goes out
    /// uncompressed and the client learns the threshold before the encoder
    /// starts compressing.
    pub fn setup_compression(&mut self, threshold: i32, validate_decompressed: bool) {
        if threshold >= 0 {
            match &mut self.compression {
                Some(compression) => {
                    compression.set_threshold(threshold, validate_decompressed);
                }
                None => {
                    self.compression =
                        Some(CompressionState::new(threshold, validate_decompressed));
                }
            }
        } else {
            self.compression = None;
        }
    }

    /// Whether the compression pipeline stage is currently enabled.
    pub fn compression_enabled(&self) -> bool {
        self.compression.is_some()
    }

    /// The current compression threshold (the value `ClientboundLoginCompressionPacket`
    /// carries), or `-1` when compression is disabled.
    pub fn compression_threshold(&self) -> i32 {
        self.compression
            .as_ref()
            .map(|compression| compression.threshold)
            .unwrap_or(-1)
    }

    /// `connection.send(Packet)` — encodes `packet_id ++ body` as one VarInt21
    /// frame and queues it. The write half is only touched by `flush_out`, so the
    /// encode/queue step is synchronous (mirrors netty queueing a write on the
    /// event loop).
    ///
    /// When compression is enabled, the packet is run through the compression
    /// encoder first (netty outbound order COMPRESS → PREPENDER), so the wire
    /// form is `varint21(varint(declaredLen) ++ payload)`.
    pub fn send_packet(
        &mut self,
        protocol: ConnectionProtocol,
        packet_id: u32,
        body: &[u8],
    ) -> Result<(), String> {
        if self.outbound_protocol != Some(protocol) {
            return Err(format!(
                "cannot send {protocol:?} packet: outbound protocol is {:?}",
                self.outbound_protocol
            ));
        }
        // Varint21 frames cap the length header at 3 bytes; a packet id itself
        // is a single VarInt of up to 5 bytes (`VarInt.MAX_VARINT_SIZE`).
        let mut payload = Vec::with_capacity(5 + body.len());
        rivet_protocol::var_int::write(&mut payload, packet_id as i32);
        payload.extend_from_slice(body);
        let wire = match &mut self.compression {
            Some(compression) => compression
                .encoder
                .encode(&payload)
                .map_err(|e| e.message)?,
            None => BytesMut::from(&payload[..]),
        };
        let frame = encode_frame(&wire).map_err(|e| e.message)?;
        self.out_buf.extend_from_slice(&frame);
        Ok(())
    }

    /// Append an already-encoded VarInt21 frame produced by the tick thread to
    /// the outbound buffer; `flush_out` writes it to the socket. Queue order is
    /// preserved for frames from the tick thread's per-connection channel.
    ///
    /// The frame is passed through opaque: the tick thread owns the play-state
    /// wire format, so when compression is enabled it must produce
    /// `varint21(varint(declaredLen) ++ payload)` frames itself (the play-state
    /// outbound path, sub-issue #96, learns the threshold from
    /// [`Connection::compression_threshold`]). Handshake/status/login packets,
    /// which this task encodes directly, go through [`Connection::send_packet`]
    /// and are compressed here.
    pub fn queue_raw_frame(&mut self, frame: Bytes) {
        self.out_buf.extend_from_slice(&frame);
    }

    /// Flush pending outbound frames to the socket.
    ///
    /// The write is bounded by the connection's read-timeout liveness window
    /// (`config.read_timeout`), so a client that stops reading cannot wedge the
    /// per-connection task — or `serve()`'s shutdown drain — in `write_all`
    /// forever once the socket send buffer fills. On timeout the flush is
    /// aborted and the caller closes the socket (Paper's read-timeout handler
    /// disconnects the same client).
    pub async fn flush_out(&mut self) -> std::io::Result<()> {
        let pending = std::mem::take(&mut self.out_buf);
        if !pending.is_empty() {
            tokio::time::timeout(self.config.read_timeout, self.write.write_all(&pending))
                .await
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "outbound flush timed out (peer not reading)",
                    )
                })??;
        }
        Ok(())
    }

    /// `connection.disconnect(...)` — flush any queued outbound (netty flushes
    /// pending writes before closing) then close the socket.
    pub async fn close(&mut self) {
        let _ = self.flush_out().await;
        let _ = self.write.shutdown().await;
    }
}

/// The [`DisconnectReason::InboundOverflow`] reason for an inbound drain that
/// exceeded the frame/byte budget, carrying the counts for the log line.
fn inbound_overflow(frames: usize, bytes: usize) -> DisconnectReason {
    DisconnectReason::InboundOverflow(format!(
        "drained {frames} frames / {bytes} bytes in one inbound window (limit {} frames / {} bytes)",
        MAX_INBOUND_FRAMES_PER_DRAIN, MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_protocol::compression_encoder::CompressionEncoder;
    use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
    use rivet_protocol::varint21_length_field_prepender::encode_frame;

    fn test_config() -> Arc<ServerConfig> {
        Arc::new(ServerConfig::default())
    }

    /// A throwaway connected `Connection` (the write half never actually writes
    /// unless `flush_out` is called, which these tests do not).
    async fn new_connection() -> Connection {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { listener.accept().await.unwrap() });
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (server_sock, _) = server.await.unwrap();
        let (_read, write) = server_sock.into_split();
        let mut conn = Connection::new(ConnectionId(1), addr, test_config(), write);
        conn.set_outbound_protocol(ConnectionProtocol::Login);
        conn
    }

    /// A listener that records every decoded packet frame, so tests can assert
    /// what `process_inbound` delivered after decompression. The frames land in a
    /// shared log the test reads directly (a `Box<dyn PacketListener>` is not
    /// `Any`, so the log is shared by handle instead of downcast).
    struct RecordingListener {
        frames: Arc<std::sync::Mutex<Vec<Bytes>>>,
    }

    impl PacketListener for RecordingListener {
        fn protocol(&self) -> ConnectionProtocol {
            ConnectionProtocol::Login
        }

        fn handle_frame(
            &mut self,
            frame: Bytes,
            _conn: &mut Connection,
            _config: &ServerConfig,
        ) -> Result<ListenerOutcome, DisconnectReason> {
            self.frames.lock().unwrap().push(frame);
            Ok(ListenerOutcome::Keep)
        }
    }

    fn recording() -> (Box<dyn PacketListener>, Arc<std::sync::Mutex<Vec<Bytes>>>) {
        let frames = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = Arc::clone(&frames);
        (Box::new(RecordingListener { frames }), log)
    }

    /// Encode a packet the way the outbound path does and frame it (the helper
    /// for building inbound wire bytes).
    fn wire_frame(encoder: &mut CompressionEncoder, packet: &[u8]) -> Bytes {
        let payload = encoder.encode(packet).unwrap();
        Bytes::from(encode_frame(&payload).unwrap().to_vec())
    }

    #[tokio::test]
    async fn compression_disabled_by_default() {
        let conn = new_connection().await;
        assert!(!conn.compression_enabled());
        assert_eq!(conn.compression_threshold(), -1);
    }

    #[tokio::test]
    async fn setup_compression_enables_and_reports_threshold() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        assert!(conn.compression_enabled());
        assert_eq!(conn.compression_threshold(), 256);
    }

    #[tokio::test]
    async fn setup_compression_negative_disables() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        assert!(conn.compression_enabled());
        conn.setup_compression(-1, true);
        assert!(!conn.compression_enabled());
        assert_eq!(conn.compression_threshold(), -1);
    }

    #[tokio::test]
    async fn setup_compression_reconfigures_existing() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        conn.setup_compression(0, true); // re-run with a new threshold
        assert_eq!(conn.compression_threshold(), 0);
        // The reconfiguration is applied to the live encoder: a 4-byte payload
        // is now above the 0 threshold and compresses.
        conn.send_packet(ConnectionProtocol::Login, 1, b"aaaa")
            .unwrap();
        assert_ne!(&conn.out_buf[0..1], &[0x00]);
    }

    #[tokio::test]
    async fn send_packet_below_threshold_is_uncompressed_on_the_wire() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        conn.send_packet(ConnectionProtocol::Login, 1, b"0123456789")
            .unwrap();
        // varint21(12) ++ varint(0) ++ packetId(1) ++ body — the outer length is
        // 1 (header) + 1 (packet id) + 10 (body).
        assert_eq!(
            &conn.out_buf[..],
            &[
                0x0C, 0x00, 0x01, b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9'
            ]
        );
    }

    #[tokio::test]
    async fn send_packet_at_threshold_is_compressed() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        // packet id (1 byte) + 255-byte body = 256 bytes, exactly at threshold.
        let body = vec![0x42u8; 255];
        conn.send_packet(ConnectionProtocol::Login, 1, &body)
            .unwrap();
        // The outer frame's payload starts with varint(256) then a zlib stream
        // (the exact outer length depends on the compressed size, which is not
        // asserted; the compression header is).
        assert_eq!(&conn.out_buf[1..3], &[0x80, 0x02]); // varint(256)
        assert_eq!(conn.out_buf[3], 0x78); // zlib CMF byte
        // And it round-trips through the protocol decoder.
        let frame_decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&conn.out_buf[..]);
        let frame = frame_decoder.decode(&mut buf).unwrap().unwrap();
        let mut decoder = rivet_protocol::compression_decoder::CompressionDecoder::new(256, true);
        let packet = decoder.decode(&frame).unwrap();
        let mut expected = Vec::with_capacity(1 + body.len());
        rivet_protocol::var_int::write(&mut expected, 1);
        expected.extend_from_slice(&body);
        assert_eq!(&packet[..], &expected[..]);
    }

    #[tokio::test]
    async fn login_packet_sent_before_setup_is_not_compressed() {
        // The login flow's ordering: `ClientboundLoginCompressionPacket` is
        // queued *before* `setupCompression`, so it must go out as a plain
        // VarInt21 frame (the client cannot decompress yet).
        let mut conn = new_connection().await;
        conn.send_packet(ConnectionProtocol::Login, 3, &[0x00, 0x01])
            .unwrap();
        // Plain varint21 frame, no inner compression header.
        assert_eq!(&conn.out_buf[..], &[0x03, 0x03, 0x00, 0x01]);
        // After setup, the same send is compressed.
        conn.setup_compression(256, true);
        conn.out_buf.clear();
        conn.send_packet(ConnectionProtocol::Login, 3, &[0x00, 0x01])
            .unwrap();
        // varint(0) ++ packet — uncompressed because 3 bytes < 256. The wire is
        // varint21(4) ++ varint(0) ++ varint(3) ++ body.
        assert_eq!(&conn.out_buf[..], &[0x04, 0x00, 0x03, 0x00, 0x01]);
    }

    #[tokio::test]
    async fn process_inbound_decompresses_compressed_frame() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();

        // The client compresses its packet (packet id + body) and frames it.
        let packet: Vec<u8> = vec![0x02, 0xAB, 0xCD, 0xEF]; // varint(2) ++ 3-byte body
        let mut encoder = CompressionEncoder::new(256);
        let wire = wire_frame(&mut encoder, &packet);

        assert_eq!(
            conn.process_inbound(&wire, &mut listener).unwrap(),
            InboundOutcome::Keep
        );
        assert_eq!(&*log.lock().unwrap(), &[Bytes::from(packet)]);
    }

    #[tokio::test]
    async fn process_inbound_below_threshold_compressed_is_malformed() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();

        // A client sending a compressed frame with declared length 10 (below the
        // 256 threshold) is protocol-nonconforming; Paper's validateDecompressed
        // rejects it and closes the connection. Wire: varint21(11) frame payload
        // = varint(10) declared + 10 payload bytes.
        let mut wire = vec![0x0B];
        wire.push(0x0A); // varint(10) declared
        wire.extend_from_slice(&[0x78, 0x9C, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
        let err = conn.process_inbound(&wire, &mut listener).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::Malformed(ref m) if m.contains("below server threshold of 256"))
        );
        assert!(log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn process_inbound_uncompressed_frame_passes_through() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();

        // varint(0) ++ packet — the client sends a below-threshold packet
        // uncompressed, which the decoder passes through verbatim.
        let packet: Vec<u8> = vec![0x05, 0x01, 0x02, 0x03, 0x04];
        let mut wire = vec![0x06]; // varint21(5) ++ varint(0)
        wire.push(0x00);
        wire.extend_from_slice(&packet);

        assert_eq!(
            conn.process_inbound(&wire, &mut listener).unwrap(),
            InboundOutcome::Keep
        );
        assert_eq!(&*log.lock().unwrap(), &[Bytes::from(packet)]);
    }

    #[tokio::test]
    async fn process_inbound_negative_declared_length_closes_malformed() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();
        // varint21(7) ++ varint(-1) ++ body — a hostile but well-formed frame.
        // With validation on the threshold check fires first (as in Java), and
        // the close must be a clean Malformed, never a panic on the `usize` wrap.
        let wire = [0x07, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x01, 0x02];
        let err = conn.process_inbound(&wire, &mut listener).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::Malformed(ref m) if m.contains("below server threshold")),
            "unexpected error: {err:?}"
        );
        assert!(log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn process_inbound_negative_declared_length_validation_off_closes_malformed() {
        // Finding-1 regression: with `validate=false` the threshold check is
        // skipped, and a negative-wrapped declared length previously hit a
        // capacity-overflow panic. It must close as a clean Malformed instead.
        let mut conn = new_connection().await;
        conn.setup_compression(256, false);
        let (mut listener, log) = recording();
        let wire = [0x07, 0xFF, 0xFF, 0xFF, 0xFF, 0x7F, 0x01, 0x02];
        let err = conn.process_inbound(&wire, &mut listener).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::Malformed(ref m) if m.contains("negative declared length")),
            "unexpected error: {err:?}"
        );
        assert!(log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn process_inbound_huge_declared_length_closes_malformed() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();
        // varint21(6) ++ varint(2^31-1) ++ body.
        let wire = [0x06, 0xFF, 0xFF, 0xFF, 0xFF, 0x07, 0x01];
        let err = conn.process_inbound(&wire, &mut listener).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::Malformed(ref m) if m.contains("larger than protocol maximum")),
            "unexpected error: {err:?}"
        );
        assert!(log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn process_inbound_compression_disabled_passes_frames_through() {
        let mut conn = new_connection().await;
        let (mut listener, log) = recording();

        // Without setup_compression, inbound frames are not decompressed: a
        // VarInt21 frame's payload passes through to the listener verbatim.
        let wire = Bytes::from_static(&[0x02, 0x02, 0xAB]); // varint21(2) ++ [0x02, 0xAB]
        assert_eq!(
            conn.process_inbound(&wire, &mut listener).unwrap(),
            InboundOutcome::Keep
        );
        assert_eq!(&*log.lock().unwrap(), &[Bytes::from_static(&[0x02, 0xAB])]);
    }

    #[tokio::test]
    async fn process_inbound_fragmented_compressed_wire() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();

        let packet: Vec<u8> = vec![0x02, 0xAB, 0xCD, 0xEF];
        let mut encoder = CompressionEncoder::new(256);
        let wire = wire_frame(&mut encoder, &packet);

        // Feed one byte at a time; only the final byte completes a frame.
        for (i, b) in wire.iter().enumerate() {
            assert_eq!(
                conn.process_inbound(&[*b], &mut listener).unwrap(),
                InboundOutcome::Keep
            );
            let n = log.lock().unwrap().len();
            let complete = i + 1 == wire.len();
            assert_eq!(n, complete as usize, "frame delivered at byte {i}");
        }
        assert_eq!(&*log.lock().unwrap(), &[Bytes::from(packet)]);
    }

    /// A listener that immediately transitions the connection to the play state.
    struct PlayListener;

    impl PacketListener for PlayListener {
        fn protocol(&self) -> ConnectionProtocol {
            ConnectionProtocol::Play
        }

        fn handle_frame(
            &mut self,
            _frame: Bytes,
            _conn: &mut Connection,
            _config: &ServerConfig,
        ) -> Result<ListenerOutcome, DisconnectReason> {
            Ok(ListenerOutcome::Play)
        }
    }

    #[tokio::test]
    async fn process_inbound_play_outcome_hands_off_to_the_caller() {
        let mut conn = new_connection().await;
        let mut listener: Box<dyn PacketListener> = Box::new(PlayListener);

        // A listener returning `Play` propagates the handoff so the connection
        // loop stops dispatching and forwards frames to the tick thread.
        let wire = Bytes::from(encode_frame(&[0x00]).unwrap().to_vec());
        assert_eq!(
            conn.process_inbound(&wire, &mut listener).unwrap(),
            InboundOutcome::Play
        );
    }

    #[tokio::test]
    async fn forward_play_sends_decoded_frames_to_the_channel() {
        let mut conn = new_connection().await;
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4);

        // A complete uncompressed VarInt21 frame: `[id 0][0xDE 0xAD]`.
        let packet: Vec<u8> = vec![0x00, 0xDE, 0xAD];
        let wire = Bytes::from(encode_frame(&packet).unwrap().to_vec());
        conn.forward_play(Some(&wire), &in_tx).await.unwrap();

        let got = in_rx.recv().await.expect("forwarded play frame");
        assert_eq!(got.bytes, Bytes::from(packet));
        assert!(in_rx.try_recv().is_err(), "no extra frames");
    }

    #[tokio::test]
    async fn forward_play_drains_frames_buffered_at_the_handoff() {
        let mut conn = new_connection().await;
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4);

        // Two play frames already buffered by `process_inbound` (a client that
        // coalesced `finish_configuration` with the first play packet in one TCP
        // chunk): the `None` data drains them so no frame is lost at the seam.
        let f1 = Bytes::from(encode_frame(&[0x01]).unwrap().to_vec());
        let f2 = Bytes::from(encode_frame(&[0x02]).unwrap().to_vec());
        conn.read_buf.extend_from_slice(&f1);
        conn.read_buf.extend_from_slice(&f2);

        conn.forward_play(None, &in_tx).await.unwrap();
        assert_eq!(
            in_rx.recv().await.unwrap().bytes,
            Bytes::from_static(&[0x01])
        );
        assert_eq!(
            in_rx.recv().await.unwrap().bytes,
            Bytes::from_static(&[0x02])
        );
        assert!(in_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn forward_play_closed_channel_reports_server_shutdown() {
        let mut conn = new_connection().await;
        let (in_tx, in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4);
        drop(in_rx); // the tick thread is gone

        let wire = Bytes::from(encode_frame(&[0x01]).unwrap().to_vec());
        let err = conn.forward_play(Some(&wire), &in_tx).await.unwrap_err();
        assert_eq!(err, DisconnectReason::ServerShutdown);
    }

    /// A peer that stops reading cannot wedge `flush_out`: the write is bounded
    /// by `config.read_timeout` and aborts with a timeout error, so the
    /// per-connection task (and `serve()`'s shutdown drain) always proceeds to
    /// close. Deterministic: a 64 MiB pending frame far exceeds the maximum
    /// kernel send/receive window on any supported OS (Linux autotunes to a few
    /// MiB at most), so `write_all` cannot complete against a non-reading peer
    /// and the timeout fires.
    #[tokio::test]
    async fn flush_out_times_out_when_peer_stops_reading() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.set_nodelay(true).unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (_read, write) = server_sock.into_split();

        let config = ServerConfig {
            read_timeout: std::time::Duration::from_millis(100),
            ..ServerConfig::default()
        };
        let mut conn = Connection::new(ConnectionId(1), addr, Arc::new(config), write);
        conn.set_outbound_protocol(ConnectionProtocol::Login);
        // 64 MiB >> any kernel socket buffer, so write_all must block.
        conn.queue_raw_frame(Bytes::from(vec![0x42u8; 64 * 1024 * 1024]));

        let start = std::time::Instant::now();
        let result = conn.flush_out().await;
        assert!(
            result.is_err(),
            "flush must abort on a non-reading peer, got {result:?}"
        );
        assert!(
            start.elapsed() < std::time::Duration::from_secs(2),
            "flush must be bounded by the read timeout"
        );
        // Keep the client socket alive (dropping it would close the peer and
        // make write_all error instead of block).
        assert!(client.local_addr().is_ok());
    }

    /// `MAX_INBOUND_FRAMES_PER_DRAIN + 1` tiny frames in one wire buffer: the
    /// frame-count budget trips and disconnects the hostile client instead of
    /// forwarding thousands of frames into the channel.
    #[tokio::test]
    async fn forward_play_exceeding_frame_budget_closes() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        // A channel the flood cannot fill (no receiver polling, so `send` must
        // never block): the budget, not backpressure, must end the drain.
        let (in_tx, _in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4096);

        let mut encoder = CompressionEncoder::new(256);
        let mut wire = BytesMut::new();
        for _ in 0..=MAX_INBOUND_FRAMES_PER_DRAIN {
            wire.extend_from_slice(&wire_frame(&mut encoder, &[0x00]));
        }
        let err = conn.forward_play(Some(&wire), &in_tx).await.unwrap_err();
        assert!(
            matches!(err, DisconnectReason::InboundOverflow(_)),
            "expected InboundOverflow, got {err:?}"
        );
    }

    /// Three max-size compressed frames (each decompressing to 6 MiB, above the
    /// 16 MiB byte budget in total): the byte half of the budget trips, so a
    /// hostile client cannot make one drain decode multi-GiB into the channel.
    #[tokio::test]
    async fn forward_play_exceeding_byte_budget_closes() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (in_tx, _in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4096);

        let mut encoder = CompressionEncoder::new(256);
        // 6 MiB each; 3 frames = 18 MiB > MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN.
        let big = vec![0xABu8; 6 * 1024 * 1024];
        let mut wire = BytesMut::new();
        for _ in 0..3 {
            wire.extend_from_slice(&wire_frame(&mut encoder, &big));
        }
        let err = conn.forward_play(Some(&wire), &in_tx).await.unwrap_err();
        assert!(
            matches!(err, DisconnectReason::InboundOverflow(_)),
            "expected InboundOverflow, got {err:?}"
        );
    }

    /// A legitimate multi-read burst stays under the budget: the window persists
    /// across `forward_play` calls (no channel drain between them), but a client
    /// that stays under the limit is never disconnected.
    #[tokio::test]
    async fn forward_play_under_budget_succeeds_across_calls() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4096);

        let mut encoder = CompressionEncoder::new(256);
        let frame = wire_frame(&mut encoder, &[0x00]);
        // Two calls, no drain between them: the window accumulates (2 × 100
        // frames = 200 < 1024) and both succeed.
        for _ in 0..2 {
            let mut wire = BytesMut::new();
            for _ in 0..100 {
                wire.extend_from_slice(&frame);
            }
            conn.forward_play(Some(&wire), &in_tx).await.unwrap();
        }
        let mut got = 0;
        while in_rx.try_recv().is_ok() {
            got += 1;
        }
        assert_eq!(got, 200, "all frames forwarded");
    }

    /// The sliding-window reset: a client the tick thread keeps up with (the
    /// channel is drained between reads) never trips the budget even when it
    /// sends more frames than the limit over time — only a client that outpaces
    /// the drain is disconnected.
    #[tokio::test]
    async fn forward_play_window_resets_when_channel_drained() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(4096);
        // The tick analog: a receiver that drains the channel continuously.
        let drained = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let drained_clone = std::sync::Arc::clone(&drained);
        let drainer = tokio::spawn(async move {
            let mut n = 0;
            while in_rx.recv().await.is_some() {
                n += 1;
                drained_clone.store(n, std::sync::atomic::Ordering::Relaxed);
            }
        });

        let mut encoder = CompressionEncoder::new(256);
        let frame = wire_frame(&mut encoder, &[0x00]);
        let mut wire = BytesMut::new();
        for _ in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
            wire.extend_from_slice(&frame);
        }
        // 1024 frames in the first call: exactly the budget, succeeds.
        conn.forward_play(Some(&wire), &in_tx).await.unwrap();
        // Wait for the drainer to empty the channel, then send 1024 more: the
        // window resets (channel observed empty) so the cumulative total far
        // exceeds the frame limit without tripping.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while drained.load(std::sync::atomic::Ordering::Relaxed) < MAX_INBOUND_FRAMES_PER_DRAIN {
            assert!(std::time::Instant::now() < deadline, "drainer stalled");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        conn.forward_play(Some(&wire), &in_tx).await.unwrap();
        // The drainer keeps draining; it should see 2048 total.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while drained.load(std::sync::atomic::Ordering::Relaxed) < 2 * MAX_INBOUND_FRAMES_PER_DRAIN
        {
            assert!(
                std::time::Instant::now() < deadline,
                "drainer never saw all frames"
            );
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        drop(in_tx);
        drainer.await.unwrap();
    }

    /// The pre-play dispatch has the same per-call frame-count budget: a hostile
    /// coalesced read with more frames than the limit closes instead of running
    /// the listener loop over an unbounded count.
    #[tokio::test]
    async fn process_inbound_exceeding_frame_budget_closes() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, log) = recording();

        let mut encoder = CompressionEncoder::new(256);
        let mut wire = BytesMut::new();
        for _ in 0..=MAX_INBOUND_FRAMES_PER_DRAIN {
            wire.extend_from_slice(&wire_frame(&mut encoder, &[0x00]));
        }
        let err = conn.process_inbound(&wire, &mut listener).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::InboundOverflow(_)),
            "expected InboundOverflow, got {err:?}"
        );
        // The frames before the trip were dispatched; nothing after.
        assert_eq!(log.lock().unwrap().len(), MAX_INBOUND_FRAMES_PER_DRAIN);
    }

    /// The pre-play dispatch byte budget: max-size frames in one read trip it
    /// the same way the play-state window does.
    #[tokio::test]
    async fn process_inbound_exceeding_byte_budget_closes() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (mut listener, _log) = recording();

        let mut encoder = CompressionEncoder::new(256);
        let big = vec![0xABu8; 6 * 1024 * 1024];
        let mut wire = BytesMut::new();
        for _ in 0..3 {
            wire.extend_from_slice(&wire_frame(&mut encoder, &big));
        }
        let err = conn.process_inbound(&wire, &mut listener).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::InboundOverflow(_)),
            "expected InboundOverflow, got {err:?}"
        );
    }
}
