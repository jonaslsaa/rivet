//! A TCP proxy that frames the protocol-776 wire stream in both directions,
//! tracks the connection state through the join path, and records every packet
//! as (state, direction, id, body) at the framing boundary.
//!
//! The proxy is deliberately byte-transparent: it forwards the exact raw bytes
//! observed on each socket (so the client and server see an unmodified stream)
//! and additionally parses each frame to record it. Compression is handled by
//! watching the server's `login_compression` packet and switching the framer
//! for both directions from that point on.

use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::frame::{self, Compression, PacketFrame};
use crate::packet::{CapturedPacket, Direction, State};

/// Shared proxy state mutated by both relay tasks.
#[derive(Debug)]
pub struct Shared {
    pub state: State,
    pub compression: Compression,
    pub login_finished: bool,
    pub packets: Vec<CapturedPacket>,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            state: State::Handshake,
            compression: Compression::Off,
            login_finished: false,
            packets: Vec::new(),
        }
    }
}

impl Shared {
    /// Apply a parsed frame to the state machine and record it.
    ///
    /// The packet is recorded with the connection state it was observed in
    /// (the state BEFORE any transition this frame triggers): the handshake
    /// intention belongs to `handshake`, the login hello to `login`, etc.
    fn record(&mut self, direction: Direction, frame: &PacketFrame) {
        let observed_state = self.state;
        match (self.state, direction, frame.id) {
            // Handshake intention (client → server): switch to the requested
            // next state.
            (State::Handshake, Direction::Serverbound, 0) => {
                if let Some(next) = intention_next_state(&frame.body) {
                    self.state = match next {
                        1 => State::Status,
                        2 => State::Login,
                        _ => self.state,
                    };
                }
            }
            // Server negotiates compression after login start.
            (State::Login, Direction::Clientbound, 3) => {
                let mut off = 0;
                let threshold = frame::read_varint(&frame.body, &mut off).unwrap_or(-1);
                self.compression = if threshold >= 0 {
                    Compression::On
                } else {
                    Compression::Off
                };
            }
            // Login finished (server → client): the client then sends
            // login_acknowledged.
            (State::Login, Direction::Clientbound, 2) => {
                self.login_finished = true;
            }
            // Login acknowledged (client → server): enter configuration.
            (State::Login, Direction::Serverbound, 3) if self.login_finished => {
                self.state = State::Configuration;
            }
            // Finish configuration (client → server): enter play.
            (State::Configuration, Direction::Serverbound, 3) => {
                self.state = State::Play;
            }
            _ => {}
        }
        self.packets.push(CapturedPacket {
            state: observed_state,
            direction,
            id: frame.id,
            body: frame.body.clone(),
        });
    }
}

/// Parse the `next_state` field out of the handshake intention body
/// (`[VarInt protocol_version][String server_address][u16 server_port][VarInt next_state]`).
fn intention_next_state(body: &[u8]) -> Option<i32> {
    let mut off = 0;
    let _protocol_version = frame::read_varint(body, &mut off)?;
    let addr_len = frame::read_varint(body, &mut off)?;
    if addr_len < 0 {
        return None;
    }
    let end = off.checked_add(addr_len as usize)?;
    body.get(off..end)?;
    off = end;
    let _port = frame::read_bytes(body, &mut off, 2)?;
    frame::read_varint(body, &mut off)
}

/// The largest frame the join capture will legitimately carry: the full
/// 117-chunk superflat view stream fits comfortably under this cap. A corrupted
/// or hostile stream must fail loudly rather than attempt a multi-GB allocation
/// (the frame length VarInt alone can encode up to 2^35).
const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Read one complete raw frame (`[VarInt length][payload]`) from `reader`.
/// Returns `Ok(None)` on clean EOF at a frame boundary.
async fn read_frame_raw<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut length: usize = 0;
    let mut shift = 0;
    loop {
        let mut byte = [0u8; 1];
        let n = reader.read(&mut byte).await?;
        if n == 0 {
            if length == 0 && shift == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "EOF mid-varint length",
            ));
        }
        length |= usize::from(byte[0] & 0x7F) << shift;
        if byte[0] & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift > 35 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "oversized length varint",
            ));
        }
    }
    if length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame length {length} exceeds the {MAX_FRAME_BYTES}-byte capture cap"),
        ));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload).await?;
    let mut raw = Vec::with_capacity(payload.len() + 8);
    frame::write_varint(&mut raw, length as i32);
    raw.extend_from_slice(&payload);
    Ok(Some(raw))
}

/// Relay `reader → writer`, recording each frame and forwarding the raw bytes.
async fn relay<R, W>(
    mut reader: R,
    mut writer: W,
    shared: Arc<Mutex<Shared>>,
    direction: Direction,
    shutdown: Arc<tokio::sync::Notify>,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    loop {
        tokio::select! {
            _ = shutdown.notified() => break,
            frame = read_frame_raw(&mut reader) => {
                let raw = match frame {
                    Ok(Some(raw)) => raw,
                    Ok(None) => break,
                    Err(e) => return Err(e),
                };
                let parsed = {
                    let mut shared = shared.lock().expect("proxy lock poisoned");
                    let compression = shared.compression;
                    match frame::parse_frame(&raw, compression) {
                        Some(frame) => {
                            shared.record(direction, &frame);
                            Some(frame)
                        }
                        None => None,
                    }
                };
                if parsed.is_none() {
                    // Unparseable frame — still forward the bytes, but do not
                    // record. This should not happen on the join path; the
                    // capture will surface the gap.
                }
                writer.write_all(&raw).await?;
            }
        }
    }
    shutdown.notify_waiters();
    Ok(())
}

/// Run the proxy for one client connection: accept on `proxy_addr`, connect to
/// `server_addr`, relay both directions, and return the shared capture once the
/// connection closes.
pub async fn run(
    proxy_addr: SocketAddr,
    server_addr: SocketAddr,
) -> io::Result<Arc<Mutex<Shared>>> {
    let listener = TcpListener::bind(proxy_addr).await?;
    let (client, _) = listener.accept().await?;
    let server = TcpStream::connect(server_addr).await?;

    let shared = Arc::new(Mutex::new(Shared::default()));
    let shutdown = Arc::new(tokio::sync::Notify::new());

    let (client_r, client_w) = client.into_split();
    let (server_r, server_w) = server.into_split();

    let c2s = tokio::spawn(relay(
        client_r,
        server_w,
        shared.clone(),
        Direction::Serverbound,
        shutdown.clone(),
    ));
    let s2c = tokio::spawn(relay(
        server_r,
        client_w,
        shared.clone(),
        Direction::Clientbound,
        shutdown.clone(),
    ));

    // Wait for either relay to finish (the client disconnecting ends the join
    // capture), then let the other settle.
    let _ = tokio::try_join!(c2s, s2c);
    Ok(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intention_next_state_parses() {
        // protocol 776, address "127.0.0.1" (9 bytes), port 25598, next_state 2.
        let mut body = Vec::new();
        frame::write_varint(&mut body, 776);
        frame::write_varint(&mut body, 9);
        body.extend_from_slice(b"127.0.0.1");
        body.extend_from_slice(&25598u16.to_be_bytes());
        frame::write_varint(&mut body, 2);
        assert_eq!(intention_next_state(&body), Some(2));
    }

    #[test]
    fn shared_tracks_login_to_configuration() {
        let mut shared = Shared::default();
        // intention → login
        let mut body = Vec::new();
        frame::write_varint(&mut body, 776);
        frame::write_varint(&mut body, 9);
        body.extend_from_slice(b"127.0.0.1");
        body.extend_from_slice(&25598u16.to_be_bytes());
        frame::write_varint(&mut body, 2);
        let frame = PacketFrame { id: 0, body };
        shared.record(Direction::Serverbound, &frame);
        assert_eq!(shared.state, State::Login);

        // server: login_compression (threshold 256)
        let mut body = Vec::new();
        frame::write_varint(&mut body, 256);
        shared.record(Direction::Clientbound, &PacketFrame { id: 3, body });
        assert_eq!(shared.compression, Compression::On);

        // server: login_finished
        shared.record(
            Direction::Clientbound,
            &PacketFrame {
                id: 2,
                body: vec![0x01, 0x02],
            },
        );
        assert!(shared.login_finished);

        // client: login_acknowledged → configuration
        shared.record(
            Direction::Serverbound,
            &PacketFrame {
                id: 3,
                body: vec![],
            },
        );
        assert_eq!(shared.state, State::Configuration);

        // client: finish_configuration → play
        shared.record(
            Direction::Serverbound,
            &PacketFrame {
                id: 3,
                body: vec![],
            },
        );
        assert_eq!(shared.state, State::Play);
    }

    #[test]
    fn shared_records_packets_with_their_observed_state() {
        let mut shared = Shared::default();
        // A packet before the intention is recorded as handshake.
        shared.record(
            Direction::Serverbound,
            &PacketFrame {
                id: 0,
                body: vec![0x00],
            },
        );
        assert_eq!(shared.packets[0].state, State::Handshake);
    }
}
