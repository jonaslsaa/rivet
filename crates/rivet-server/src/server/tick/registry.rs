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
use crate::server::network::packet_listener::DisconnectReason;

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
#[derive(Debug)]
pub struct ConnectionRegistry {
    connections: HashMap<ConnectionId, ConnChannels>,
    /// The rotating round-robin drain cursor: the connection at which the next
    /// tick's aggregate drain starts (ascending `ConnectionId` order, wrapping).
    /// The tick loop advances it past the connections it gave budget to, so the
    /// aggregate inbound budget ([`MAX_INBOUND_FRAMES_PER_TICK`] /
    /// [`MAX_INBOUND_BYTES_PER_TICK`]) cannot indefinitely favor the same fixed
    /// subset of connections. A stable connection that sorts last is still
    /// served on its turn. See [`Self::rotated_ids`].
    next_drain: ConnectionId,
    /// The last disconnect reason observed per connection, recorded when the
    /// tick removes the entry (a `Disconnect` lifecycle event, a closed inbound
    /// channel, or an outbound-overload prune). The session manager's prune
    /// consumes it via [`Self::take_disconnect_reason`] when a session ends, to
    /// report why on the `RIVET_SESSION_END` trace path; the tick loop drops
    /// whatever it did not consume at the tick boundary ([`Self::drain_unconsumed_disconnect_reasons`]),
    /// so the map is empty between ticks even on a server with no session
    /// manager. Tick-owned — the same OWNERSHIP exception as the registry itself
    /// — so no cross-thread state exists.
    last_disconnect_reason: HashMap<ConnectionId, DisconnectReason>,
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        ConnectionRegistry {
            connections: HashMap::new(),
            next_drain: ConnectionId(0),
            last_disconnect_reason: HashMap::new(),
        }
    }
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        ConnectionRegistry::default()
    }

    /// Consume the recorded disconnect reason for a connection the tick removed
    /// (the session manager's prune uses this to report why a session ended).
    pub fn take_disconnect_reason(&mut self, id: ConnectionId) -> Option<DisconnectReason> {
        self.last_disconnect_reason.remove(&id)
    }

    /// Drop every recorded disconnect reason the session manager did not
    /// consume. The tick loop calls this at the end of every tick, whether or
    /// not a session manager is registered: a reason is recorded only when a
    /// connection entry is removed, so by the time the session manager's prune
    /// has run, any reason left in the map belongs to a connection that never
    /// reached play (a status/handshake/login close, or a session-less overflow
    /// prune) and will never be consumed — without this, a server with no
    /// session manager (or one whose connections close before play) would grow
    /// the map without bound. Draining at the tick boundary keeps the map empty
    /// between ticks: bounded to the reasons recorded and consumed within one
    /// tick.
    pub fn drain_unconsumed_disconnect_reasons(&mut self) {
        self.last_disconnect_reason.clear();
    }

    /// Test-only: the count of recorded-but-unconsumed disconnect reasons. The
    /// tick-loop counterfactual proves this is zero between ticks; production
    /// code never needs to observe it (reasons are consumed by the session
    /// manager's prune or drained at the tick boundary).
    #[cfg(test)]
    pub(crate) fn disconnect_reason_count(&self) -> usize {
        self.last_disconnect_reason.len()
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

    /// Connection ids in deterministic ascending `ConnectionId` order, rotated
    /// to start at or after [`Self::next_drain`] (wrapping). The tick loop drains
    /// in this order, so the rotating start gives every connection its turn at
    /// the head of the aggregate budget instead of permanently favoring the
    /// connections that happen to sort first (a fixed order lets the connections
    /// at the tail starve indefinitely once the aggregate budget is exhausted
    /// before reaching them).
    ///
    /// The rotation is deterministic (sorted, not hash order): the fairness
    /// tests depend on it, and a `HashMap` iteration order is unspecified.
    pub fn rotated_ids(&self) -> Vec<ConnectionId> {
        let mut ids: Vec<ConnectionId> = self.connections.keys().copied().collect();
        ids.sort_unstable();
        // `ConnectionId`s are unique per boot, so a cursor pointing at a removed
        // connection simply starts the rotation at the next higher id (wrapping
        // when the cursor is past every id — `rotate_left(len)` is a no-op).
        let pos = ids.partition_point(|id| *id < self.next_drain);
        ids.rotate_left(pos);
        ids
    }

    /// Set the rotating drain cursor for the next drain pass (called by the tick
    /// loop after each aggregate drain).
    pub fn advance_drain_cursor(&mut self, next: ConnectionId) {
        self.next_drain = next;
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
            LifecycleEvent::Disconnect { id, reason } => {
                self.connections.remove(&id);
                self.last_disconnect_reason.insert(id, reason);
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
            self.last_disconnect_reason
                .insert(id, DisconnectReason::EndOfStream);
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
                self.last_disconnect_reason
                    .insert(id, DisconnectReason::Overflow);
                Err(OutboundError::Overflow(id))
            }
            Err(TrySendError::Closed(_)) => {
                self.connections.remove(&id);
                self.last_disconnect_reason
                    .insert(id, DisconnectReason::EndOfStream);
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

    /// The rotating drain order is deterministic by ascending `ConnectionId`,
    /// never the unspecified `HashMap` iteration order, and starts at the
    /// cursor (wrapping).
    #[test]
    fn rotated_ids_are_sorted_and_rotated_deterministically() {
        let mut reg = ConnectionRegistry::new();
        // Connected out of id order: the rotation is by id, not insertion.
        for id in [
            ConnectionId(5),
            ConnectionId(1),
            ConnectionId(9),
            ConnectionId(3),
        ] {
            connect(&mut reg, id, 4, 4);
        }
        // Cursor 0 (default): ascending id order.
        assert_eq!(
            reg.rotated_ids(),
            vec![
                ConnectionId(1),
                ConnectionId(3),
                ConnectionId(5),
                ConnectionId(9)
            ]
        );
        // A cursor at 3 starts the rotation there, wrapping 9 before 1.
        reg.advance_drain_cursor(ConnectionId(3));
        assert_eq!(
            reg.rotated_ids(),
            vec![
                ConnectionId(3),
                ConnectionId(5),
                ConnectionId(9),
                ConnectionId(1)
            ]
        );
        // A cursor past every id wraps fully back to the head (no-op rotation).
        reg.advance_drain_cursor(ConnectionId(10));
        assert_eq!(
            reg.rotated_ids(),
            vec![
                ConnectionId(1),
                ConnectionId(3),
                ConnectionId(5),
                ConnectionId(9)
            ]
        );
        // A cursor pointing at a removed id falls through to the next higher.
        reg.advance_drain_cursor(ConnectionId(6));
        assert_eq!(
            reg.rotated_ids(),
            vec![
                ConnectionId(9),
                ConnectionId(1),
                ConnectionId(3),
                ConnectionId(5)
            ]
        );
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

    /// Load-bearing counterfactual for the disconnect-reason leak: every
    /// recorded disconnect reason is drained by the end of the tick. The registry
    /// records a reason on each of the three removal paths — a lifecycle
    /// `Disconnect`, a closed inbound channel, and an outbound-overflow prune —
    /// and the tick loop drains whatever the session manager did not consume at
    /// the tick boundary. So even a server with **no session manager**
    /// (`enable_join=false` — the status/offline-login boot) must show an empty
    /// reason map after every tick: a single connect/disconnect cycle must not
    /// retain one entry per disconnected `ConnectionId` over a long run.
    ///
    /// This test drives the three removal paths against the same registry
    /// without any session-manager tickable registered, asserting that each
    /// path's reason is gone after the very next `drain_unconsumed_disconnect_reasons()`
    /// — the same call `run_tick` performs every tick. A regression that stopped
    /// draining (or recorded a reason the drain missed) fails here on a
    /// long-lived, many-cycle loop, never silently growing memory.
    #[test]
    fn disconnect_reasons_are_drained_each_tick_without_a_session_manager() {
        let mut reg = ConnectionRegistry::new();
        // Path 1: a lifecycle Disconnect event (a status/handshake/login close
        // with no session — exactly what an `enable_join=false` boot sees).
        for round in 0..512 {
            let id = ConnectionId(round * 3 + 1);
            connect(&mut reg, id, 4, 4);
            reg.apply(LifecycleEvent::Disconnect {
                id,
                reason: DisconnectReason::Timeout,
            });
            assert!(!reg.contains(id), "disconnect removed the entry");
            assert_eq!(
                reg.take_disconnect_reason(id),
                Some(DisconnectReason::Timeout),
                "the lifecycle reason is recorded"
            );
            reg.drain_unconsumed_disconnect_reasons();
            assert_eq!(
                reg.disconnect_reason_count(),
                0,
                "tick boundary leaves no unconsumed reason"
            );
        }

        // Path 2: a closed inbound channel (the network task dropped its
        // sender; the tick's drain prunes the entry and records EndOfStream).
        for round in 0..512 {
            let id = ConnectionId(round * 3 + 2);
            let (in_tx, _out_rx) = connect(&mut reg, id, 4, 4);
            drop(in_tx); // the connection task is gone
            match reg.drain_one(id) {
                DrainOutcome::Closed => {}
                other => panic!("closed channel drains as Closed, got {other:?}"),
            }
            assert!(!reg.contains(id), "the closed channel pruned the entry");
            assert_eq!(
                reg.take_disconnect_reason(id),
                Some(DisconnectReason::EndOfStream),
                "the EOF prune recorded its reason"
            );
            reg.drain_unconsumed_disconnect_reasons();
            assert_eq!(
                reg.disconnect_reason_count(),
                0,
                "tick boundary leaves no unconsumed reason"
            );
        }

        // Path 3: an outbound-overflow prune (the tick dropped the connection
        // when its outbound channel was full — the backpressure policy). The
        // channel has capacity 1 and is never drained, so the first send fills
        // it and the second overflows, pruning the entry and recording
        // `Overflow`.
        for round in 0..512 {
            let id = ConnectionId(round * 3 + 3);
            let (_in_tx, out_rx) = connect(&mut reg, id, 4, 1);
            let _out_rx = out_rx; // never drained: the channel stays full
            assert_eq!(
                reg.send(
                    id,
                    OutboundEvent::Disconnect {
                        reason: DisconnectReason::Overflow,
                    },
                ),
                Ok(()),
                "the first send fills the outbound channel"
            );
            let err = reg
                .send(
                    id,
                    OutboundEvent::Disconnect {
                        reason: DisconnectReason::Overflow,
                    },
                )
                .unwrap_err();
            assert_eq!(err, OutboundError::Overflow(id));
            assert!(!reg.contains(id), "overflow pruned the entry");
            assert_eq!(
                reg.take_disconnect_reason(id),
                Some(DisconnectReason::Overflow),
                "the overflow prune recorded its reason"
            );
            reg.drain_unconsumed_disconnect_reasons();
            assert_eq!(
                reg.disconnect_reason_count(),
                0,
                "tick boundary leaves no unconsumed reason"
            );
        }
    }
}
