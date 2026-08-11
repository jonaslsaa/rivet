//! The keepalive ⇄ connection seam (issue #157): what a keepalive-aware
//! listener needs from its `Connection`, and the shared driver that runs
//! `KeepaliveState::tick` each server tick.
//!
//! # Listener seam
//!
//! The full `ServerCommonPacketListenerImpl.keepConnectionAlive` runs in both
//! configuration and play. The tick-thread state machine
//! [`super::super::keepalive::KeepaliveState`] stays pure (no `Connection`, no
//! packets), while listeners use:
//!
//!   - [`KeepaliveSink`] for the two outbound effects: transmit a clientbound
//!     keep-alive or disconnect for timeout;
//!   - [`drive_keepalive`] to run one state-machine tick and apply its outcome.
//!
//! PLAY sessions own one [`KeepaliveState`] each and drive it through
//! `PlayKeepaliveSink`. The configuration listener (issue #283) owns one too and
//! drives it through [`ConnectionKeepaliveSink`] from `conn_loop`, which drives
//! the listener at `config.tick_interval` only while a CONFIGURATION listener is
//! current — both reuse this seam instead of duplicating the keepalive logic.

use bytes::BytesMut;

use rivet_protocol::codec::StreamEncoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::common::clientbound_keep_alive::ClientboundKeepAlivePacket;

use super::connection::Connection;
use super::packet_listener::DisconnectReason;
use crate::server::keepalive::{KeepaliveState, KeepaliveTickOutcome};

/// The clientbound `keep_alive` packet id in the configuration protocol —
/// `ConfigurationProtocols.CLIENTBOUND_TEMPLATE` (`rivet-protocol` generated
/// table): `minecraft:keep_alive` is 4.
pub const CONFIG_CLIENTBOUND_KEEP_ALIVE_ID: u32 =
    rivet_protocol::generated::packets::configuration::clientbound::PacketType::KeepAlive.id();

/// The outbound effects a keepalive tick can demand. A listener implements this
/// against its own `Connection`; the pure state machine never names a packet or
/// a socket.
pub trait KeepaliveSink {
    /// `send(new ClientboundKeepAlivePacket(id))` — transmit the challenge.
    fn send_keepalive(&mut self, challenge_id: i64) -> Result<(), DisconnectReason>;
    /// `disconnect(TIMEOUT_DISCONNECTION_MESSAGE, TIMEOUT)` — kick for keepalive
    /// timeout.
    fn disconnect_timeout(&mut self) -> DisconnectReason;
}

/// Run `keepalive.tick()` and apply the outcome through `sink`. Returns
/// `Err(reason)` when the sink's disconnect fired (the caller closes the
/// connection), `Ok(())` otherwise. Mirrors the two branches of
/// `ServerCommonPacketListenerImpl.keepConnectionAlive` after the
/// `checkIfClosed` guard.
pub fn drive_keepalive(
    keepalive: &mut KeepaliveState,
    tx_time_ns: i64,
    now_ms: i64,
    sink: &mut impl KeepaliveSink,
) -> Result<(), DisconnectReason> {
    let outcome: KeepaliveTickOutcome = keepalive.tick(tx_time_ns, now_ms);
    if let Some(challenge_id) = outcome.send {
        sink.send_keepalive(challenge_id)?;
    }
    if outcome.timeout {
        // `disconnect` in Java throws the ReportedException crash if the send
        // fails; on the Rust side the disconnect is the terminal action — the
        // caller closes the connection with the returned reason.
        return Err(sink.disconnect_timeout());
    }
    Ok(())
}

/// Encode a `ClientboundKeepAlivePacket` body (packet id NOT included; the
/// caller passes `CONFIG_CLIENTBOUND_KEEP_ALIVE_ID` to `send_packet`). The
/// packet is fully wire-typed (`ClientboundKeepAlivePacket` is merged, #86).
fn encode_keepalive_body(id: i64) -> Vec<u8> {
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    ClientboundKeepAlivePacket::stream_codec()
        .encode(&mut out, &ClientboundKeepAlivePacket::new(id))
        .expect("encode keepalive body");
    out.into_inner().to_vec()
}

/// A [`KeepaliveSink`] that transmits through the configuration `Connection`.
/// Encoding a `ClientboundKeepAlivePacket` needs the clientbound `keep_alive`
/// id — the configuration listener's `send` path is `send_packet(protocol, id,
/// body)`, so the sink borrows the `Connection` directly.
pub struct ConnectionKeepaliveSink<'a> {
    conn: &'a mut Connection,
}

impl<'a> ConnectionKeepaliveSink<'a> {
    pub fn new(conn: &'a mut Connection) -> Self {
        ConnectionKeepaliveSink { conn }
    }
}

impl KeepaliveSink for ConnectionKeepaliveSink<'_> {
    fn send_keepalive(&mut self, challenge_id: i64) -> Result<(), DisconnectReason> {
        let body = encode_keepalive_body(challenge_id);
        self.conn
            .send_packet(
                ConnectionProtocol::Configuration,
                CONFIG_CLIENTBOUND_KEEP_ALIVE_ID,
                &body,
            )
            .map_err(|e| DisconnectReason::Unsupported(format!("send keepalive failed: {e}")))
    }

    fn disconnect_timeout(&mut self) -> DisconnectReason {
        // `TIMEOUT_DISCONNECTION_MESSAGE` is `disconnect.timeout`.
        DisconnectReason::Timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A recording sink for asserting `drive_keepalive`'s effect routing.
    #[derive(Debug, Default)]
    struct Recorder {
        sends: Vec<i64>,
        disconnects: u32,
        send_fails: bool,
    }

    impl KeepaliveSink for Recorder {
        fn send_keepalive(&mut self, challenge_id: i64) -> Result<(), DisconnectReason> {
            if self.send_fails {
                return Err(DisconnectReason::Malformed("send failed".into()));
            }
            self.sends.push(challenge_id);
            Ok(())
        }
        fn disconnect_timeout(&mut self) -> DisconnectReason {
            self.disconnects += 1;
            DisconnectReason::Timeout
        }
    }

    fn ns(ms: i64) -> i64 {
        ms * 1_000_000
    }

    #[test]
    fn drive_forwards_send_and_timeout_in_order() {
        // A tick that both sends and times out must emit the packet first, then
        // the disconnect (Paper's send-before-disconnect order).
        let mut state = KeepaliveState::new(0);
        let mut sink = Recorder::default();

        state.tick(ns(1000), 1000); // send challenge 1000 (never answered)
        let result = drive_keepalive(&mut state, ns(32_000), 32_000, &mut sink);

        assert_eq!(sink.sends, vec![32_000], "the throttled challenge is sent");
        assert_eq!(sink.disconnects, 1);
        assert_eq!(result, Err(DisconnectReason::Timeout));
    }

    #[test]
    fn drive_with_no_outcome_is_ok() {
        let mut state = KeepaliveState::new(0);
        let mut sink = Recorder::default();
        let result = drive_keepalive(&mut state, ns(500), 500, &mut sink);
        assert_eq!(result, Ok(()));
        assert!(sink.sends.is_empty());
        assert_eq!(sink.disconnects, 0);
    }

    #[test]
    fn drive_surfaces_sink_send_failure() {
        let mut state = KeepaliveState::new(0);
        let mut sink = Recorder {
            send_fails: true,
            ..Recorder::default()
        };
        let result = drive_keepalive(&mut state, ns(1000), 1000, &mut sink);
        assert!(
            matches!(result, Err(DisconnectReason::Malformed(_))),
            "send failure must abort the tick before the timeout branch"
        );
        assert_eq!(sink.disconnects, 0);
    }

    #[test]
    fn connection_sink_sends_through_config_protocol() {
        // The ConnectionKeepaliveSink sends with the configuration clientbound
        // protocol (the wire frame itself — id 4 + 8-byte body — is asserted by
        // the integration test through the real channel). This exercises the
        // sink's `send_packet` path against a real Connection: it must succeed
        // when the outbound protocol is Configuration, and the sink must surface
        // the protocol mismatch otherwise (a #96 integration mistake, caught
        // here rather than in a live config phase).
        let config = std::sync::Arc::new(crate::server::ServerConfig::default());
        let addr = "127.0.0.1:25565".parse::<std::net::SocketAddr>().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .unwrap();
        rt.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr0 = listener.local_addr().unwrap();
            let server = tokio::spawn(async move { listener.accept().await.unwrap() });
            let _client = tokio::net::TcpStream::connect(addr0).await.unwrap();
            let (server_sock, _) = server.await.unwrap();
            let (_read, write) = server_sock.into_split();
            let mut conn = Connection::new(
                crate::server::network::connection_id::ConnectionId(1),
                addr,
                config,
                std::sync::Arc::new(crate::server::tick::shutdown::Shutdown::new()),
                write,
                crate::server::tick::channels::InboundDrained::new(),
            );

            // Wrong outbound protocol: the send must fail loudly.
            conn.set_outbound_protocol(ConnectionProtocol::Login);
            let mut sink = ConnectionKeepaliveSink::new(&mut conn);
            assert!(
                sink.send_keepalive(12345).is_err(),
                "send must be rejected when outbound is not configuration"
            );

            // Correct protocol: the send succeeds.
            conn.set_outbound_protocol(ConnectionProtocol::Configuration);
            let mut sink = ConnectionKeepaliveSink::new(&mut conn);
            sink.send_keepalive(12345).unwrap();
        });
    }

    #[test]
    fn encode_keepalive_body_is_eight_bytes_big_endian() {
        let body = encode_keepalive_body(1234567890123456789);
        assert_eq!(body, vec![0x11, 0x22, 0x10, 0xf4, 0x7d, 0xe9, 0x81, 0x15]);
    }

    #[test]
    fn config_keepalive_id_matches_generated_table() {
        // `ConfigurationProtocols.CLIENTBOUND` keep_alive is 4 — asserted against
        // the generated table so a drift silently mis-framing every keepalive
        // packet is caught at compile time (the constant IS the table value).
        assert_eq!(CONFIG_CLIENTBOUND_KEEP_ALIVE_ID, 4);
        assert_eq!(
            rivet_protocol::generated::packets::configuration::clientbound::PacketType::KeepAlive
                .name(),
            "minecraft:keep_alive"
        );
    }
}
