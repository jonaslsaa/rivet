//! The tick-side connection registry: `ConnectionId → ConnChannels`, owned by
//! the tick thread. This is the tick side of the OWNERSHIP "connection registry"
//! shared-state exception; everything here is game-adjacent state and lives on
//! the tick thread only.

use std::collections::HashMap;

use tokio::sync::mpsc::error::{TryRecvError, TrySendError};

use rivet_protocol::protocol::common::client_information::ClientInformation;
use rivet_registry::core::GameProfile;

use super::channels::{
    ConnChannels, LifecycleEvent, MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN,
    MAX_INBOUND_FRAMES_PER_DRAIN, OutboundEvent, ServerboundFrame,
};
use crate::server::network::connection_id::ConnectionId;

/// Outcome of draining one connection's inbound channel.
#[derive(Debug, Clone)]
pub enum DrainOutcome {
    /// Frames drained in order (FIFO per connection).
    Drained(Vec<ServerboundFrame>),
    /// The network task dropped its sender (connection gone); the registry entry
    /// was removed.
    Closed,
}

/// Why a tick-side outbound enqueue failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OutboundError {
    #[error("connection {0} is not registered")]
    Gone(ConnectionId),
    /// Overload policy fired: the connection's outbound channel is full, so it
    /// is disconnected (see `ConnectionRegistry::send`).
    #[error("connection {0} outbound channel full")]
    Overflow(ConnectionId),
}

/// The tick thread's connections, keyed by `ConnectionId` (OWNERSHIP §Network:
/// the tick thread owns the inbound receivers / outbound senders).
#[derive(Debug, Default)]
pub struct ConnectionRegistry {
    connections: HashMap<ConnectionId, ConnChannels>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        ConnectionRegistry::default()
    }

    pub fn len(&self) -> usize {
        self.connections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    pub fn contains(&self, id: ConnectionId) -> bool {
        self.connections.contains_key(&id)
    }

    pub fn get(&self, id: ConnectionId) -> Option<&ConnChannels> {
        self.connections.get(&id)
    }

    pub fn ids(&self) -> impl Iterator<Item = ConnectionId> + '_ {
        self.connections.keys().copied()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&ConnectionId, &ConnChannels)> {
        self.connections.iter()
    }

    /// Apply one lifecycle event (the tick thread's registration source).
    pub fn apply(&mut self, event: LifecycleEvent) {
        match event {
            LifecycleEvent::Connect {
                id,
                remote,
                in_rx,
                out_tx,
                drained,
            } => {
                // `ConnectionId`s are unique per boot, so an existing entry is
                // a stale prune (channel ends already dropped); replacing it is
                // the correct outcome either way.
                self.connections
                    .insert(id, ConnChannels::new(id, remote, in_rx, out_tx, drained));
            }
            LifecycleEvent::Disconnect { id, .. } => {
                self.connections.remove(&id);
            }
            LifecycleEvent::EnterPlay {
                id,
                profile,
                client_information,
            } => {
                if let Some(conn) = self.connections.get_mut(&id) {
                    conn.set_play_handoff(profile, client_information);
                }
                // An EnterPlay for an unknown connection is a race where the
                // connection already closed; the handoff is dropped.
            }
        }
    }

    /// Consume a connection's configuration→play handoff ([`LifecycleEvent::EnterPlay`])
    /// so the session manager can spawn its join burst exactly once.
    pub fn take_play_handoff(
        &mut self,
        id: ConnectionId,
    ) -> Option<(GameProfile, ClientInformation)> {
        self.connections
            .get_mut(&id)
            .and_then(ConnChannels::take_play_handoff)
    }

    /// Drain one connection's inbound frames in FIFO order, bounded by the
    /// per-tick inbound budget ([`MAX_INBOUND_FRAMES_PER_DRAIN`] /
    /// [`MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN`]). A closed channel (the
    /// network task dropped its sender) removes the connection — the registry
    /// self-heals without needing a `Disconnect` event.
    ///
    /// The budget is the authoritative per-tick bound (OWNERSHIP §Network): one
    /// tick never delivers more than `MAX_INBOUND_FRAMES_PER_DRAIN` frames or
    /// `MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN` bytes from one connection,
    /// even against a sender that concurrently refills the channel while this
    /// drains (the tokio-side admission window can race the drain-progress
    /// reset mid-drain, so this cap is what actually stops a single tick from
    /// processing a multi-GiB flood). No frame is dropped and the cap is strict:
    ///
    /// - the frame-count cap is checked before receiving, so the excess stays
    ///   in the channel (deterministic retention);
    /// - the byte cap is strict: a frame that would push the drain *over* the
    ///   budget is not delivered — it is preserved in the connection's
    ///   [`ConnChannels::pending_frame`] slot and delivered by the next drain.
    ///
    /// Retained frames are drained on a later tick, or pruned when the sender's
    /// admission cap disconnects the flooding client.
    pub fn drain_one(&mut self, id: ConnectionId) -> DrainOutcome {
        self.drain_one_bounded(
            id,
            MAX_INBOUND_FRAMES_PER_DRAIN,
            MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN,
        )
    }

    /// [`Self::drain_one`] with explicit budgets. Used by the tick loop to apply
    /// the aggregate per-tick budget (`MAX_INBOUND_FRAMES_PER_TICK` /
    /// `MAX_INBOUND_BYTES_PER_TICK`) across all connections: the remaining
    /// aggregate budget is passed in, so one tick never delivers more than the
    /// aggregate cap either. Same strict byte cap / pending-frame retention as
    /// `drain_one`.
    pub fn drain_one_bounded(
        &mut self,
        id: ConnectionId,
        max_frames: usize,
        max_bytes: usize,
    ) -> DrainOutcome {
        let mut frames = Vec::new();
        let mut drained_bytes = 0usize;
        let mut closed = false;
        if let Some(conn) = self.connections.get_mut(&id) {
            loop {
                // Frame-count cap checked before receiving so the excess stays
                // in the channel (deterministic retention).
                if frames.len() >= max_frames || drained_bytes >= max_bytes {
                    break;
                }
                let frame = match conn.pending_frame.take() {
                    Some(pending) => pending,
                    None => match conn.in_rx.try_recv() {
                        Ok(frame) => frame,
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            closed = true;
                            break;
                        }
                    },
                };
                // Strict byte cap: delivering this frame would push the drain
                // over the budget, so preserve it for the next drain instead of
                // overshooting. A frame alone never exceeds the per-connection
                // byte cap (8 MiB max < 16 MiB budget), so it cannot stall.
                if drained_bytes + frame.bytes.len() > max_bytes {
                    conn.pending_frame = Some(frame);
                    break;
                }
                drained_bytes += frame.bytes.len();
                frames.push(frame);
            }
            // Record delivered frames on the shared progress counter, so the
            // connection's admission window sees this tick's progress.
            if !frames.is_empty() {
                conn.drained.record_drained(frames.len());
            }
        }
        if closed {
            self.connections.remove(&id);
            return DrainOutcome::Closed;
        }
        DrainOutcome::Drained(frames)
    }

    /// Enqueue an outbound event for a connection. On overflow or a dead
    /// connection the entry is removed and the error reports which policy fired.
    ///
    /// Overload policy: a client that cannot drain its tick→network channel is
    /// disconnected — Paper disconnects on outbound overflow. Because the
    /// channel is full there is no room to send an in-band `Disconnect`, so the
    /// tick side drops `out_tx`; the per-connection task sees `recv() == None`
    /// and closes the socket (flushing anything already queued).
    pub fn send(&mut self, id: ConnectionId, event: OutboundEvent) -> Result<(), OutboundError> {
        let Some(conn) = self.connections.get_mut(&id) else {
            return Err(OutboundError::Gone(id));
        };
        match conn.out_tx.try_send(event) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.connections.remove(&id);
                Err(OutboundError::Overflow(id))
            }
            Err(TrySendError::Closed(_)) => {
                self.connections.remove(&id);
                Err(OutboundError::Gone(id))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use crate::server::network::packet_listener::DisconnectReason;
    use crate::server::tick::channels::InboundDrained;
    use bytes::Bytes;
    use tokio::sync::mpsc;

    const REMOTE: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 25565);

    fn frame(byte: u8) -> ServerboundFrame {
        ServerboundFrame {
            bytes: Bytes::from(vec![byte]),
        }
    }

    fn frame_of_size(len: usize) -> ServerboundFrame {
        ServerboundFrame {
            bytes: Bytes::from(vec![0u8; len]),
        }
    }

    fn connect(
        reg: &mut ConnectionRegistry,
        id: ConnectionId,
        in_cap: usize,
        out_cap: usize,
    ) -> (
        mpsc::Sender<ServerboundFrame>,
        mpsc::Receiver<OutboundEvent>,
    ) {
        let (in_tx, in_rx) = mpsc::channel(in_cap);
        let (out_tx, out_rx) = mpsc::channel(out_cap);
        reg.apply(LifecycleEvent::Connect {
            id,
            remote: REMOTE,
            in_rx,
            out_tx,
            drained: InboundDrained::new(),
        });
        (in_tx, out_rx)
    }

    #[test]
    fn connect_registers_and_disconnect_removes() {
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, mut out_rx) = connect(&mut reg, id, 4, 4);
        assert!(reg.contains(id));
        assert_eq!(reg.len(), 1);

        reg.apply(LifecycleEvent::Disconnect {
            id,
            reason: DisconnectReason::EndOfStream,
        });
        assert!(!reg.contains(id));
        assert!(reg.is_empty());
        // The channel ends on both sides see the removal.
        assert!(in_tx.try_send(frame(0)).is_err());
        assert!(out_rx.try_recv().is_err());
    }

    #[test]
    fn drain_preserves_per_connection_order() {
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 16, 4);
        for b in 0..5u8 {
            in_tx.try_send(frame(b)).unwrap();
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                let got: Vec<u8> = frames.iter().map(|f| f.bytes[0]).collect();
                assert_eq!(got, vec![0, 1, 2, 3, 4]);
            }
            DrainOutcome::Closed => panic!("connection should not have closed"),
        }
        // Second drain is empty but keeps the connection.
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => assert!(frames.is_empty()),
            DrainOutcome::Closed => panic!("connection should not have closed"),
        }
        assert!(reg.contains(id));
    }

    #[test]
    fn drain_one_exact_frame_count_at_budget() {
        // Deterministic prefill: exactly MAX_INBOUND_FRAMES_PER_DRAIN frames
        // (1-byte each, so the byte budget is far away). The drain delivers
        // exactly the budget and the channel is empty.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 4096, 4);
        for _ in 0..MAX_INBOUND_FRAMES_PER_DRAIN {
            in_tx.try_send(frame(0)).unwrap();
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), MAX_INBOUND_FRAMES_PER_DRAIN);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        // Second drain: empty.
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => assert!(frames.is_empty()),
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
    }

    #[test]
    fn drain_one_exact_frame_count_retention() {
        // Deterministic prefill: MAX_INBOUND_FRAMES_PER_DRAIN + 1 frames. The
        // drain delivers exactly the budget and the 1025th is retained in the
        // channel (never dropped) — delivered by the next drain.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 4096, 4);
        for _ in 0..=MAX_INBOUND_FRAMES_PER_DRAIN {
            in_tx.try_send(frame(0)).unwrap();
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), MAX_INBOUND_FRAMES_PER_DRAIN);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        // The excess frame is retained, not lost.
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), 1);
                assert_eq!(frames[0].bytes.len(), 1);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
    }

    #[test]
    fn drain_one_strict_byte_cap_preserves_straddling_frame() {
        // Deterministic straddling: three 7 MiB frames, budget 16 MiB. The
        // third would push the drain over the strict byte cap, so it is NOT
        // delivered — it is preserved in the pending-frame slot and delivered
        // by the next drain. Per-drain delivery never exceeds the cap.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 64, 4);
        let frame_size = 7 * 1024 * 1024;
        for _ in 0..3 {
            in_tx.try_send(frame_of_size(frame_size)).unwrap();
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                let total: usize = frames.iter().map(|f| f.bytes.len()).sum();
                assert_eq!(
                    frames.len(),
                    2,
                    "third frame must be preserved, not delivered"
                );
                assert_eq!(total, 14 * 1024 * 1024);
                assert!(total <= MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        // The preserved straddling frame is delivered by the next drain.
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), 1);
                assert_eq!(frames[0].bytes.len(), frame_size);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
    }

    #[test]
    fn drain_one_strict_byte_cap_at_8mib_minus_one_boundary() {
        // Two 8 MiB − 1 frames (16,777,214 bytes total) exactly fit the 16 MiB
        // budget; a third straddles it and is preserved as pending.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 64, 4);
        let frame_size = 8 * 1024 * 1024 - 1; // 8_388_607
        for _ in 0..3 {
            in_tx.try_send(frame_of_size(frame_size)).unwrap();
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), 2);
                assert!(
                    frames.iter().map(|f| f.bytes.len()).sum::<usize>()
                        <= MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN
                );
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), 1);
                assert_eq!(frames[0].bytes.len(), frame_size);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
    }

    #[test]
    fn drain_one_bounded_respects_smaller_budget() {
        // The aggregate-budget path: drain_one_bounded with a remaining budget
        // smaller than the per-connection cap must deliver at most that budget
        // and retain the excess for a later drain.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 64, 4);
        for _ in 0..3 {
            in_tx.try_send(frame(0)).unwrap();
        }
        match reg.drain_one_bounded(id, 2, MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN) {
            DrainOutcome::Drained(frames) => assert_eq!(frames.len(), 2),
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        match reg.drain_one_bounded(id, 1, MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN) {
            DrainOutcome::Drained(frames) => assert_eq!(frames.len(), 1),
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        // A zero remaining budget delivers nothing.
        match reg.drain_one_bounded(id, 0, 0) {
            DrainOutcome::Drained(frames) => assert!(frames.is_empty()),
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
    }

    #[test]
    fn drain_one_bounds_decompressed_bytes_per_tick() {
        // Three 8 MiB frames: the byte budget (16 MiB) trips after two. The cap
        // is checked before receiving, so exactly two (16 MiB) are delivered and
        // the third stays retained in the channel — one tick never processes
        // more than MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN bytes.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 64, 4);
        for _ in 0..3 {
            in_tx.try_send(frame_of_size(8 * 1024 * 1024)).unwrap();
        }
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                let total: usize = frames.iter().map(|f| f.bytes.len()).sum();
                assert_eq!(frames.len(), 2);
                assert_eq!(total, 16 * 1024 * 1024);
                assert!(total <= MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
        // The third frame is retained for a later drain, not lost.
        match reg.drain_one(id) {
            DrainOutcome::Drained(frames) => {
                assert_eq!(frames.len(), 1);
                assert_eq!(frames[0].bytes.len(), 8 * 1024 * 1024);
            }
            DrainOutcome::Closed => panic!("connection should stay open"),
        }
    }

    #[test]
    fn closed_inbound_prunes_connection() {
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 4, 4);
        // The network task is gone (its sender dropped) without sending a
        // Disconnect event; the registry self-heals on the next drain.
        drop(in_tx);
        assert!(matches!(reg.drain_one(id), DrainOutcome::Closed));
        assert!(!reg.contains(id));
    }

    #[test]
    fn outbound_overflow_disconnects_connection() {
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (_in_tx, mut out_rx) = connect(&mut reg, id, 4, 1);

        assert!(
            reg.send(
                id,
                OutboundEvent::Packet {
                    frame: Bytes::from_static(b"a")
                }
            )
            .is_ok()
        );
        let err = reg
            .send(
                id,
                OutboundEvent::Packet {
                    frame: Bytes::from_static(b"b"),
                },
            )
            .unwrap_err();
        assert_eq!(err, OutboundError::Overflow(id));
        // The connection was removed...
        assert!(!reg.contains(id));
        // ...and the network side observes the channel close after the packet
        // it already drained (blocking ops work without a runtime).
        assert!(matches!(
            out_rx.blocking_recv(),
            Some(OutboundEvent::Packet { .. })
        ));
        assert!(out_rx.blocking_recv().is_none());
    }

    #[test]
    fn send_to_unknown_connection_is_gone() {
        let mut reg = ConnectionRegistry::new();
        let err = reg
            .send(
                ConnectionId(99),
                OutboundEvent::Packet {
                    frame: Bytes::new(),
                },
            )
            .unwrap_err();
        assert_eq!(err, OutboundError::Gone(ConnectionId(99)));
    }

    #[test]
    fn dead_outbound_prunes_connection() {
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (_in_tx, out_rx) = connect(&mut reg, id, 4, 4);
        // The connection task dropped its outbound receiver (socket closed).
        drop(out_rx);
        let err = reg
            .send(
                id,
                OutboundEvent::Packet {
                    frame: Bytes::new(),
                },
            )
            .unwrap_err();
        assert_eq!(err, OutboundError::Gone(id));
        assert!(!reg.contains(id));
    }

    #[test]
    fn connect_replaces_stale_entry() {
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _) = connect(&mut reg, id, 4, 4);
        // A re-registration with the same id replaces the old channels.
        connect(&mut reg, id, 4, 4);
        assert_eq!(reg.len(), 1);
        // The old sender sees its receiver dropped.
        assert!(in_tx.try_send(frame(0)).is_err());
    }
}
