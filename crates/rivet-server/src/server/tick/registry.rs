//! The tick-side connection registry: `ConnectionId → ConnChannels`, owned by
//! the tick thread. This is the tick side of the OWNERSHIP "connection registry"
//! shared-state exception; everything here is game-adjacent state and lives on
//! the tick thread only.

use std::collections::HashMap;

use tokio::sync::mpsc::error::{TryRecvError, TrySendError};

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
            } => {
                // `ConnectionId`s are unique per boot, so an existing entry is
                // a stale prune (channel ends already dropped); replacing it is
                // the correct outcome either way.
                self.connections
                    .insert(id, ConnChannels::new(id, remote, in_rx, out_tx));
            }
            LifecycleEvent::Disconnect { id, .. } => {
                self.connections.remove(&id);
            }
        }
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
    /// drains (the tokio-side admission window can race an observed-empty
    /// channel mid-drain and reset, so this cap is what actually stops a single
    /// tick from processing a multi-GiB flood). The cap is checked before
    /// receiving, so the drain stops exactly at the budget and the excess is
    /// deterministically *retained* in the channel — drained on a later tick,
    /// or pruned when the sender's admission cap disconnects the flooding
    /// client. No frame is dropped: a frame is only dequeued when the drain is
    /// still under both caps.
    pub fn drain_one(&mut self, id: ConnectionId) -> DrainOutcome {
        let mut frames = Vec::new();
        let mut drained_bytes = 0usize;
        let mut closed = false;
        if let Some(conn) = self.connections.get_mut(&id) {
            loop {
                // Per-tick budget: stop before receiving more so the excess
                // stays in the channel (deterministic retention). A single
                // frame can never exceed the byte cap alone (the per-frame
                // decompressed maximum is 8 MiB, below the 16 MiB budget), so
                // this never leaves a frame permanently undeliverable.
                if frames.len() >= MAX_INBOUND_FRAMES_PER_DRAIN
                    || drained_bytes >= MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN
                {
                    break;
                }
                match conn.in_rx.try_recv() {
                    Ok(frame) => {
                        drained_bytes += frame.bytes.len();
                        frames.push(frame);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        closed = true;
                        break;
                    }
                }
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
    fn drain_one_bounds_concurrent_refill_to_the_frame_budget() {
        // A sender concurrently refills the channel while drain_one drains,
        // racing the observed-empty reset of the tokio-side admission window.
        // The tick-side budget is authoritative: a single drain_one call never
        // delivers more than MAX_INBOUND_FRAMES_PER_DRAIN frames even under
        // continuous refill (the channel capacity is above the cap so the cap,
        // not the channel depth, is the binding constraint).
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        // Capacity 4096 > the 1024 frame cap, so a full channel can exceed the
        // cap — the drain must still stop at the cap.
        let (in_tx, _out_rx) = connect(&mut reg, id, 4096, 4);

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_c = std::sync::Arc::clone(&stop);
        let tx = in_tx.clone();
        let sender = std::thread::spawn(move || {
            while !stop_c.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = tx.try_send(frame(0));
            }
        });

        // Many drains under continuous refill: every call delivers at most the
        // frame-count budget (1-byte frames keep the byte budget far away).
        for _ in 0..200 {
            match reg.drain_one(id) {
                DrainOutcome::Drained(frames) => {
                    assert!(
                        frames.len() <= MAX_INBOUND_FRAMES_PER_DRAIN,
                        "drained {} frames in one tick (budget {})",
                        frames.len(),
                        MAX_INBOUND_FRAMES_PER_DRAIN
                    );
                }
                DrainOutcome::Closed => panic!("connection should stay open"),
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        sender.join().unwrap();
    }

    #[test]
    fn drain_one_bounds_bytes_per_tick_under_concurrent_refill() {
        // The byte-cap half of the concurrent-refill guarantee: a sender
        // refills continuously with 1 MiB frames while drain_one runs. The
        // pre-receive byte check is race-free (no await between the check and
        // the receive), so one drain_one call never delivers more than
        // MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN bytes even under refill.
        let mut reg = ConnectionRegistry::new();
        let id = ConnectionId(1);
        let (in_tx, _out_rx) = connect(&mut reg, id, 4096, 4);

        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_c = std::sync::Arc::clone(&stop);
        let tx = in_tx.clone();
        let sender = std::thread::spawn(move || {
            while !stop_c.load(std::sync::atomic::Ordering::Relaxed) {
                let _ = tx.try_send(frame_of_size(1024 * 1024));
            }
        });

        for _ in 0..50 {
            match reg.drain_one(id) {
                DrainOutcome::Drained(frames) => {
                    let total: usize = frames.iter().map(|f| f.bytes.len()).sum();
                    assert!(
                        total <= MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN,
                        "drained {total} bytes in one tick (budget {MAX_INBOUND_DECOMPRESSED_BYTES_PER_DRAIN})"
                    );
                }
                DrainOutcome::Closed => panic!("connection should stay open"),
            }
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        sender.join().unwrap();
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
