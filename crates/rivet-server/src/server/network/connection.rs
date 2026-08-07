use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Buf, Bytes, BytesMut};
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
use crate::server::tick::shutdown::Shutdown;

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
    /// forwarded to the tick channel since the last observed tick-drain
    /// progress. The frame/byte budget in [`Connection::forward_play`] is
    /// enforced against this window so a hostile client cannot let the bounded
    /// channel retain multi-GiB of decompressed frames. Reset whenever the tick
    /// thread makes drain progress (see [`Self::inbound_window_last_capacity`]).
    inbound_window_bytes: usize,
    /// The play-state inbound drain window, frame-count half (see
    /// [`Self::inbound_window_bytes`]).
    inbound_window_frames: usize,
    /// The channel capacity (free slots) observed right after the previous
    /// forward to the tick channel. [`Connection::forward_play`] resets the
    /// admission window when the current capacity is above this — proof the tick
    /// thread drained at least one frame since the last push, so the client is
    /// keeping up. The fully empty channel (capacity == max) is the extreme
    /// case.
    inbound_window_last_capacity: usize,
    /// The server-stop signal. `flush_out` races it so an outbound write that
    /// backpressures (a slow or non-reading peer) is aborted on shutdown instead
    /// of wedging the per-connection task and `serve()`'s shutdown drain.
    shutdown: Arc<Shutdown>,
}

impl Connection {
    pub fn new(
        id: ConnectionId,
        remote_addr: SocketAddr,
        config: Arc<ServerConfig>,
        shutdown: Arc<Shutdown>,
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
            inbound_window_last_capacity: 0,
            shutdown,
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

    /// Whether the server-stop signal has fired. Callers map a failed flush to
    /// [`DisconnectReason::ServerShutdown`] (not `EndOfStream`) when this is
    /// set: a flush aborted by shutdown is a stop, not the peer going away.
    pub fn shutdown_requested(&self) -> bool {
        self.shutdown.is_requested()
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
    /// on observed tick-drain progress: whenever the channel has more free slots
    /// than it did after the previous push, the tick thread drained some, so the
    /// client is keeping up and the window restarts. A client the tick observably
    /// keeps up with is never affected, even when the channel is never fully
    /// empty.
    ///
    /// The anti-flood boundary is full channel saturation: a sender that refills
    /// every drained slot before its next check (so the capacity observed at each
    /// push never exceeds the previous push's) shows no drain progress and its
    /// window accumulates to the budget, disconnecting it. This is deliberate:
    /// a persistently saturated channel is exactly the memory-retention condition
    /// the admission cap exists to bound. A no-progress flood (the tick never
    /// drains) is the extreme of the same boundary. Exceeding the budget reports
    /// [`DisconnectReason::InboundOverflow`].
    ///
    /// The authoritative per-tick bound is the tick side: [`ConnectionRegistry::drain_one`]
    /// stops draining at the same budget, so even a sender that races the window
    /// reset (observing the channel mid-drain and refilling) cannot make one tick
    /// deliver more than the budget. This admission cap is the memory-retention
    /// backstop that keeps the bounded channel from holding multi-GiB before that
    /// tick-side bound is even reached.
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
            // Observe the tick thread's drain progress since the previous push:
            // if the channel has more free slots now than it did right after
            // that push, the tick drained some — the client is keeping up, so
            // the window restarts. A saturated sender (every drained slot
            // refilled before the next check) never satisfies this, so its
            // window accumulates to the budget and trips.
            let (window_bytes, window_frames) = admission_step(
                self.inbound_window_last_capacity,
                in_tx.capacity(),
                self.inbound_window_bytes,
                self.inbound_window_frames,
                packet.len(),
            )?;
            self.inbound_window_bytes = window_bytes;
            self.inbound_window_frames = window_frames;
            if in_tx
                .send(ServerboundFrame { bytes: packet })
                .await
                .is_err()
            {
                return Err(DisconnectReason::ServerShutdown);
            }
            self.inbound_window_last_capacity = in_tx.capacity();
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
    /// Ordinary outbound writes backpressure: a slow-but-live peer is never
    /// disconnected for taking longer than the read timeout to drain — the write
    /// simply blocks until the peer reads (the socket send window refills).
    ///
    /// The only bound is shutdown. A flush reached after shutdown is already
    /// requested routes through the bounded shutdown flush
    /// ([`Self::flush_out_bounded`]): racing an already-fired signal against an
    /// immediately-writable socket could drop the queued frames, so the
    /// wall-clock bound is used instead (preserving and attempting them). A
    /// shutdown that fires *mid-write* aborts the write leaving exactly the
    /// unwritten suffix in `out_buf`, so a subsequent bounded flush can attempt
    /// only the suffix — never a duplicated prefix. Either way the connection
    /// cannot wedge the per-connection task (or `serve()`'s shutdown drain)
    /// forever.
    pub async fn flush_out(&mut self) -> std::io::Result<()> {
        loop {
            if self.out_buf.is_empty() {
                return Ok(());
            }
            if self.shutdown.is_requested() {
                return self.flush_out_bounded(self.config.read_timeout).await;
            }
            // Single-shot write: each future reports exactly how many bytes the
            // socket accepted (tokio's TcpStream poll_write returns Pending only
            // when no progress is possible, so a future dropped while blocked
            // wrote nothing). Advance the authoritative `out_buf` by that count,
            // so on a shutdown interrupt it holds exactly the unwritten suffix.
            let n = {
                let write = self.write.write(&self.out_buf);
                tokio::pin!(write);
                tokio::select! {
                    n = write.as_mut() => n?,
                    _ = self.shutdown.wait_async() => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "outbound flush aborted: server shutting down",
                        ));
                    }
                }
            };
            if n == 0 {
                // A ready socket that accepted nothing is the peer's write side
                // closed; anything queued cannot be delivered.
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "outbound write made no progress",
                ));
            }
            self.out_buf.advance(n);
        }
    }

    /// Flush pending outbound frames, bounding the write with a wall-clock
    /// timeout. Used where shutdown is already requested — the shutdown drain
    /// ([`drain_to_close`](super::server_connection_listener::drain_to_close))
    /// and any normal-path flush reached after shutdown ([`Self::flush_out`]) —
    /// where the shutdown-signal race of [`Self::flush_out`] would abort
    /// immediately. The timeout is what prevents a non-reading peer from
    /// stalling the drain; on timeout the frames are abandoned (boundedly
    /// attempted, the peer is not reading).
    pub async fn flush_out_bounded(&mut self, timeout: std::time::Duration) -> std::io::Result<()> {
        let pending = std::mem::take(&mut self.out_buf);
        if !pending.is_empty() {
            tokio::time::timeout(timeout, self.write.write_all(&pending))
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

/// One admission-window step for [`Connection::forward_play`]: apply the
/// drain-progress reset (when the channel now holds more free slots than after
/// the previous push, the tick drained some, so the window restarts) and add the
/// next frame, returning `Err(InboundOverflow)` when the budget would be
/// exceeded. Pure — factored out so the saturation boundary is deterministically
/// testable without a live channel.
///
/// A saturated sender (the observed capacity never exceeds the previous push's —
/// every drained slot is refilled before the next check) never resets and trips
/// at the budget; see [`Connection::forward_play`].
fn admission_step(
    last_capacity: usize,
    current_capacity: usize,
    window_bytes: usize,
    window_frames: usize,
    packet_len: usize,
) -> Result<(usize, usize), DisconnectReason> {
    let (window_bytes, window_frames) = if current_capacity > last_capacity {
        (packet_len, 1)
    } else {
        (window_bytes + packet_len, window_frames + 1)
    };
    if window_bytes > MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN
        || window_frames > MAX_INBOUND_FRAMES_PER_DRAIN
    {
        return Err(inbound_overflow(window_frames, window_bytes));
    }
    Ok((window_bytes, window_frames))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_protocol::compression_encoder::CompressionEncoder;
    use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
    use rivet_protocol::varint21_length_field_prepender::encode_frame;
    use tokio::io::AsyncReadExt;

    fn test_config() -> Arc<ServerConfig> {
        Arc::new(ServerConfig::default())
    }

    fn test_shutdown() -> Arc<Shutdown> {
        Arc::new(Shutdown::new())
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
        let mut conn =
            Connection::new(ConnectionId(1), addr, test_config(), test_shutdown(), write);
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

    /// Ordinary outbound backpressure never disconnects a slow-but-live peer:
    /// `flush_out` blocks until the peer drains, even when that takes far longer
    /// than the read timeout. The client here reads 1 MiB every 5 ms while the
    /// frame is 64 MiB, so the flush necessarily takes well over
    /// `read_timeout` (50 ms) yet must succeed (not time out).
    #[tokio::test]
    async fn flush_out_backpressures_without_timing_out_on_slow_reader() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (_read, write) = server_sock.into_split();

        let config = ServerConfig {
            read_timeout: std::time::Duration::from_millis(50),
            ..ServerConfig::default()
        };
        let mut conn = Connection::new(
            ConnectionId(1),
            addr,
            Arc::new(config),
            test_shutdown(),
            write,
        );
        conn.set_outbound_protocol(ConnectionProtocol::Login);
        // 64 MiB >> any kernel socket buffer, so the write can only make
        // progress as the client drains.
        let frame_len = 64 * 1024 * 1024;
        conn.queue_raw_frame(Bytes::from(vec![0x42u8; frame_len]));

        // The slow-but-live reader: drain the full frame in 1 MiB chunks with a
        // 5 ms pause between chunks (~320 ms total, far above the 50 ms
        // read_timeout). Dropping the client early would close the peer and make
        // write_all error, so it must read to the end.
        let reader = tokio::spawn(async move {
            let mut total = 0usize;
            let mut buf = vec![0u8; 1024 * 1024];
            while total < frame_len {
                let n = client.read(&mut buf).await.unwrap();
                assert!(n > 0, "peer closed early");
                total += n;
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
            total
        });

        let start = std::time::Instant::now();
        let result = conn.flush_out().await;
        assert!(
            result.is_ok(),
            "a slow-but-live reader must not be disconnected, got {result:?}"
        );
        assert!(
            start.elapsed() > std::time::Duration::from_millis(50),
            "flush should have taken longer than read_timeout under backpressure"
        );
        assert_eq!(
            reader.await.unwrap(),
            frame_len,
            "client read the full frame"
        );
    }

    /// A non-reading peer cannot wedge `flush_out` once shutdown is requested:
    /// the write races the shutdown signal and aborts. The aborted write must
    /// preserve the queued frames (not drop them) so a subsequent bounded
    /// shutdown flush can still attempt delivery.
    #[tokio::test]
    async fn flush_out_aborts_on_shutdown_with_stuck_write() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.set_nodelay(true).unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (_read, write) = server_sock.into_split();

        let shutdown = test_shutdown();
        let mut conn = Connection::new(
            ConnectionId(1),
            addr,
            test_config(),
            Arc::clone(&shutdown),
            write,
        );
        conn.set_outbound_protocol(ConnectionProtocol::Login);
        // 64 MiB >> any kernel socket buffer, and the client never reads, so the
        // write can only complete via the shutdown abort.
        conn.queue_raw_frame(Bytes::from(vec![0x42u8; 64 * 1024 * 1024]));

        let task = tokio::spawn(async move {
            let result = conn.flush_out().await;
            (result, conn)
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        shutdown.request();

        let (result, mut conn) = tokio::time::timeout(std::time::Duration::from_secs(2), task)
            .await
            .expect("flush did not abort on shutdown")
            .unwrap();
        assert!(
            result.is_err(),
            "flush must abort when shutdown fires during a stuck write"
        );
        // The frames survive the abort (mem::take put them back ahead of
        // anything new), so a bounded shutdown flush still attempts them.
        assert!(
            !conn.out_buf.is_empty(),
            "aborted flush must preserve the queued frames"
        );
        let start = std::time::Instant::now();
        let bounded = conn
            .flush_out_bounded(std::time::Duration::from_millis(50))
            .await;
        assert!(
            bounded.is_err(),
            "the bounded retry still times out on the non-reading peer"
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        // Keep the client socket alive (dropping it would close the peer and
        // make write_all error instead of block).
        assert!(client.local_addr().is_ok());
    }

    /// `flush_out_bounded` bounds a stuck write with a wall-clock timeout even
    /// when shutdown has already fired (the shutdown-race flush would abort
    /// immediately, so the shutdown drain uses the bounded variant).
    #[tokio::test]
    async fn flush_out_bounded_times_out_a_stuck_write() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.set_nodelay(true).unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (_read, write) = server_sock.into_split();

        let mut conn =
            Connection::new(ConnectionId(1), addr, test_config(), test_shutdown(), write);
        conn.set_outbound_protocol(ConnectionProtocol::Login);
        conn.queue_raw_frame(Bytes::from(vec![0x42u8; 64 * 1024 * 1024]));

        let start = std::time::Instant::now();
        let result = conn
            .flush_out_bounded(std::time::Duration::from_millis(50))
            .await;
        assert!(
            result.is_err(),
            "bounded flush must time out on a non-reading peer"
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
        assert!(client.local_addr().is_ok());
    }

    /// The partial-write corruption regression: `flush_out` interrupted by
    /// shutdown must leave exactly the unwritten suffix in the authoritative
    /// `out_buf` (never restore the full buffer, which would duplicate the
    /// already-written prefix), and a bounded retry must send only that suffix so
    /// the peer decodes the original frame stream exactly once.
    #[tokio::test]
    async fn flush_out_partial_write_retry_sends_only_the_suffix() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let mut client = tokio::net::TcpStream::connect(addr).await.unwrap();
        client.set_nodelay(true).unwrap();
        let (server_sock, _) = listener.accept().await.unwrap();
        let (_read, write) = server_sock.into_split();

        let shutdown = test_shutdown();
        let mut conn = Connection::new(
            ConnectionId(1),
            addr,
            test_config(),
            Arc::clone(&shutdown),
            write,
        );
        conn.set_outbound_protocol(ConnectionProtocol::Login);

        // A stream of valid VarInt21 frames (each payload under the 2 MiB frame
        // cap), totaling well above any socket buffer so one write cannot
        // complete before the peer reads.
        let payloads: [&[u8]; 6] = [
            &[0x11u8; 1024 * 1024],
            &[0x22, 0x33, 0x44],
            &[0x55u8; 1024 * 1024],
            &[0x66u8; 1024 * 1024],
            &[0x77u8; 1024 * 1024],
            &[0x88u8; 1024 * 1024],
        ];
        let mut stream = Vec::new();
        for payload in payloads {
            stream.extend_from_slice(&encode_frame(payload).unwrap());
        }
        conn.queue_raw_frame(Bytes::from(stream.clone()));

        let task = tokio::spawn(async move {
            let result = conn.flush_out().await;
            (result, conn)
        });

        // Wait for the flush to fill the socket buffer and block (the client
        // never reads yet), then interrupt it with shutdown.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        shutdown.request();

        let (result, mut conn) = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .expect("flush_out did not abort on shutdown")
            .unwrap();
        assert!(
            result.is_err(),
            "flush_out must be interrupted by shutdown mid-write"
        );
        // A partial prefix reached the wire: the authoritative out_buf holds a
        // non-empty proper suffix of the stream (never the full buffer, which
        // would re-send the already-written prefix).
        assert!(
            !conn.out_buf.is_empty(),
            "an unwritten suffix must remain after the interrupt"
        );
        assert!(
            conn.out_buf.len() < stream.len(),
            "a partial prefix must have been written before the interrupt"
        );
        assert!(
            stream.ends_with(&conn.out_buf[..]),
            "the remaining buffer must be exactly the unwritten suffix"
        );

        // Bounded retry sends only the suffix; the client drains concurrently.
        let reader = tokio::spawn(async move {
            let mut rest = Vec::new();
            client.read_to_end(&mut rest).await.unwrap();
            rest
        });
        conn.flush_out_bounded(std::time::Duration::from_secs(5))
            .await
            .expect("bounded retry must deliver the suffix to a reading peer");
        drop(conn); // close the write half so the reader sees EOF
        let received = reader.await.unwrap();

        // The reader saw the already-buffered prefix followed by exactly the
        // suffix the retry wrote: the whole stream, byte-for-byte, once.
        assert_eq!(received.len(), stream.len(), "frame stream length mismatch");
        assert!(
            received == stream,
            "the peer must receive the original frame stream exactly once"
        );

        // And it decodes as the three original frames (no corruption).
        let decoder = Varint21FrameDecoder::new(None);
        let mut buf = BytesMut::from(&received[..]);
        let mut decoded = Vec::new();
        while let Some(frame) = decoder.decode(&mut buf).unwrap() {
            decoded.push(frame.to_vec());
        }
        assert_eq!(decoded.len(), 6, "six frames decoded");
        assert_eq!(decoded[0], vec![0x11u8; 1024 * 1024]);
        assert_eq!(decoded[1], vec![0x22, 0x33, 0x44]);
        assert_eq!(decoded[2], vec![0x55u8; 1024 * 1024]);
        assert_eq!(decoded[3], vec![0x66u8; 1024 * 1024]);
        assert_eq!(decoded[4], vec![0x77u8; 1024 * 1024]);
        assert_eq!(decoded[5], vec![0x88u8; 1024 * 1024]);
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

    /// A sustained sender the tick keeps up with is never disconnected even when
    /// the channel never becomes fully empty: the admission window resets on
    /// observed drain progress, not on the channel reaching zero. A small
    /// channel (capacity 8, below the 1024 budget) with a partial drainer that
    /// always leaves frames in the channel would, under an empty-only reset,
    /// accumulate the window past the budget and falsely disconnect. The
    /// progress-based reset never trips: 1200 frames (above the 1024 budget) are
    /// forwarded and drained while the channel always holds frames.
    #[tokio::test]
    async fn forward_play_window_resets_on_progress_with_never_empty_small_channel() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(8);

        let mut encoder = CompressionEncoder::new(256);
        let frame = wire_frame(&mut encoder, &[0x00]);
        let mut batch = BytesMut::new();
        for _ in 0..4 {
            batch.extend_from_slice(&frame);
        }
        // Prefill 4 frames: the channel is never empty from here on.
        for _ in 0..4 {
            in_tx
                .try_send(ServerboundFrame {
                    bytes: frame.clone(),
                })
                .unwrap();
        }
        // 300 cycles x 4 frames = 1200 frames forwarded, well above the 1024
        // budget. Each cycle sends 4 (channel 4 -> 8) then drains 4 (8 -> 4), so
        // the channel is never empty yet the tick (the test) makes progress
        // every cycle.
        for _ in 0..300 {
            conn.forward_play(Some(&batch), &in_tx).await.unwrap();
            for _ in 0..4 {
                in_rx.try_recv().unwrap();
            }
        }
        // 1200 sent + 4 prefilled = 1204; 1200 drained, the 4 prefilled remain.
        let mut remaining = 0;
        while in_rx.try_recv().is_ok() {
            remaining += 1;
        }
        assert_eq!(
            remaining, 4,
            "the 4 prefilled frames are still in the channel"
        );
    }

    /// The same progress-based reset with a large channel: partial drain
    /// progress (not a full empty) between batches resets the window, so a
    /// sender that sustains more than the budget over time is not disconnected.
    #[tokio::test]
    async fn forward_play_window_resets_on_partial_drain_progress() {
        let mut conn = new_connection().await;
        conn.setup_compression(256, true);
        let (in_tx, mut in_rx) = tokio::sync::mpsc::channel::<ServerboundFrame>(8192);

        let mut encoder = CompressionEncoder::new(256);
        let frame = wire_frame(&mut encoder, &[0x00]);
        let mut batch = BytesMut::new();
        for _ in 0..200 {
            batch.extend_from_slice(&frame);
        }
        for _ in 0..6 {
            // 200 frames per call; the channel is never fully drained between
            // calls (only 100 of the 200 are consumed), yet the drain progress
            // resets the window each call.
            conn.forward_play(Some(&batch), &in_tx).await.unwrap();
            for _ in 0..100 {
                in_rx.try_recv().unwrap();
            }
        }
        // 1200 frames forwarded over the 1024 budget, never disconnected.
        let mut remaining = 0;
        while in_rx.try_recv().is_ok() {
            remaining += 1;
        }
        assert_eq!(remaining, 600, "1200 sent, 600 drained, 600 retained");
    }

    /// The saturation boundary, documented as anti-flood: a sender whose observed
    /// channel capacity never exceeds the previous push's (every drained slot
    /// refilled before the next check — full lockstep saturation) never resets,
    /// so its window accumulates to the budget and it is disconnected. The
    /// progress-based reset cannot fire because there is no observable progress.
    #[test]
    fn admission_step_saturated_channel_accumulates_to_budget() {
        // Saturated: current capacity == last capacity (0). No reset; the window
        // accumulates. One frame under the budget is accepted.
        let (bytes, frames) = admission_step(0, 0, 0, MAX_INBOUND_FRAMES_PER_DRAIN - 1, 1).unwrap();
        assert_eq!(frames, MAX_INBOUND_FRAMES_PER_DRAIN);
        assert_eq!(bytes, 1);
        // The next frame trips the budget exactly at the per-tick cap.
        let err = admission_step(0, 0, bytes, frames, 1).unwrap_err();
        assert!(
            matches!(err, DisconnectReason::InboundOverflow(_)),
            "saturated sender trips exactly at the per-tick cap"
        );
    }

    /// The drain-progress side of the same boundary: any observed progress
    /// (current capacity above last) resets the window, so a keeping-up sender is
    /// never disconnected no matter how much it has sent.
    #[test]
    fn admission_step_observed_progress_resets_window() {
        // A window far past the budget is reset by a single observed drain.
        let (bytes, frames) = admission_step(
            0,
            1,
            MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN,
            MAX_INBOUND_FRAMES_PER_DRAIN,
            1,
        )
        .unwrap();
        assert_eq!(bytes, 1, "window restarts at the observed frame");
        assert_eq!(frames, 1);
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
