use std::net::SocketAddr;
use std::sync::Arc;

use bytes::{Bytes, BytesMut};
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::varint21_frame_decoder::Varint21FrameDecoder;
use rivet_protocol::varint21_length_field_prepender::encode_frame;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;

use super::connection_id::ConnectionId;
use super::packet_listener::{DisconnectReason, ListenerOutcome, PacketListener};
use crate::server::ServerConfig;

/// The per-connection state machine on the tokio side. Mirrors the parts of
/// `net.minecraft.network.Connection` that matter before the play state: the
/// VarInt21 frame decoder, the outbound protocol, and the packet listener that
/// dispatches each fully-framed inbound packet. Play-state packets cross to the
/// tick thread over channels keyed by `ConnectionId` (OWNERSHIP §Network) — that
/// handoff is sub-issue #93 and is not built here.
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
    outbound_protocol: Option<ConnectionProtocol>,
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
            outbound_protocol: None,
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

    /// Append inbound bytes, decode every complete frame, and dispatch it to the
    /// current listener. Returns `Err(reason)` on the first corrupted frame or
    /// listener rejection; the per-connection task then closes the socket.
    /// A `ListenerOutcome::Switch` replaces the listener (the handshake→
    /// status/login boundary).
    pub fn process_inbound(
        &mut self,
        data: &[u8],
        listener: &mut Box<dyn PacketListener>,
    ) -> Result<(), DisconnectReason> {
        self.read_buf.extend_from_slice(data);
        loop {
            let frame: Bytes = match self.decoder.decode(&mut self.read_buf) {
                Ok(Some(frame)) => frame,
                Ok(None) => break, // need more bytes
                Err(e) => {
                    return Err(DisconnectReason::Malformed(format!(
                        "corrupted frame: {}",
                        e.message
                    )));
                }
            };
            let config = Arc::clone(&self.config);
            match listener.handle_frame(frame, self, &config) {
                Ok(ListenerOutcome::Keep) => {}
                Ok(ListenerOutcome::Switch(next)) => *listener = next,
                Err(reason) => return Err(reason),
            }
        }
        Ok(())
    }

    /// `connection.send(Packet)` — encodes `packet_id ++ body` as one VarInt21
    /// frame and queues it. The write half is only touched by `flush_out`, so the
    /// encode/queue step is synchronous (mirrors netty queueing a write on the
    /// event loop).
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
        let frame = encode_frame(&payload).map_err(|e| e.message)?;
        self.out_buf.extend_from_slice(&frame);
        Ok(())
    }

    /// Flush pending outbound frames to the socket.
    pub async fn flush_out(&mut self) -> std::io::Result<()> {
        let pending = std::mem::take(&mut self.out_buf);
        if !pending.is_empty() {
            self.write.write_all(&pending).await?;
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
