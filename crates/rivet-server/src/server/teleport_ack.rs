//! Pure teleport-acknowledgement state machine (issue #158 precursor).
//!
//! Port of the teleport halves of `ServerGamePacketListenerImpl` (working/Paper,
//! `paper-server/src/minecraft/java/net/minecraft/server/network/
//! ServerGamePacketListenerImpl.java`): the `awaitingTeleport` id increment and
//! wrap in `internalTeleport`, and the accept / mismatch handling in
//! `handleAcceptTeleportPacket`. Extracted as a pure, deterministic state
//! machine so the ack logic is exhaustively testable before the #96 play
//! handoff and a live `Connection` exist.
//!
//! Out of scope (deferred to the M3 anti-cheat port, issue #158): the
//! `awaitingTeleportTime` 20-tick re-sync machine (`updateAwaitingTeleport`),
//! packet emission, the `lastGoodX/Y/Z` sync from an accepted position (lives in
//! `movement_math`), and the disconnect side effect — the kick is reported via
//! [`TeleportAckOutcome`] instead.

/// The `awaitingTeleport` id embedded in `ClientboundPlayerPositionPacket` and
/// matched by `ServerboundAcceptTeleportationPacket`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TeleportAckState {
    awaiting_teleport: i32,
    awaiting_position: Option<[f64; 3]>,
}

/// Outcome of [`TeleportAckState::accept`], mirroring
/// `handleAcceptTeleportPacket`'s three paths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TeleportAckOutcome {
    /// `packet.getId() == awaitingTeleport` and a pending position exists: the
    /// server snaps to the awaited position and clears the pending marker.
    Accepted { x: f64, y: f64, z: f64 },
    /// `packet.getId() == awaitingTeleport` but `awaitingPositionFromClient` is
    /// null: Paper disconnects with `multiplayer.disconnect.
    /// invalid_player_movement`.
    InvalidMovementKick,
    /// `packet.getId() != awaitingTeleport`: no-op, state unchanged.
    Ignored,
}

impl Default for TeleportAckState {
    /// Fresh state: `awaitingTeleport = 0`, no pending position (the Java
    /// field defaults).
    fn default() -> Self {
        TeleportAckState {
            awaiting_teleport: 0,
            awaiting_position: None,
        }
    }
}

impl TeleportAckState {
    /// Fresh state, equivalent to `Default`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `internalTeleport`'s id advance: `if (++awaitingTeleport ==
    /// Integer.MAX_VALUE) awaitingTeleport = 0;`, then the awaited position is
    /// recorded (the pending marker is overwritten, as Java overwrites
    /// `awaitingPositionFromClient` unconditionally). Returns the id to embed
    /// in the clientbound teleport packet.
    ///
    /// Wrapping arithmetic is sacred (PORTING.md): `++` on `i32::MAX` wraps to
    /// `i32::MIN` and the `== i32::MAX` test is false, so only an increment
    /// that *lands on* `i32::MAX` resets to 0.
    pub fn begin_teleport(&mut self, x: f64, y: f64, z: f64) -> i32 {
        let next = self.awaiting_teleport.wrapping_add(1);
        self.awaiting_teleport = if next == i32::MAX { 0 } else { next };
        self.awaiting_position = Some([x, y, z]);
        self.awaiting_teleport
    }

    /// `handleAcceptTeleportPacket`, minus the entity/level side effects.
    pub fn accept(&mut self, id: i32) -> TeleportAckOutcome {
        if id != self.awaiting_teleport {
            return TeleportAckOutcome::Ignored;
        }
        match self.awaiting_position.take() {
            None => TeleportAckOutcome::InvalidMovementKick,
            Some([x, y, z]) => TeleportAckOutcome::Accepted { x, y, z },
        }
    }

    /// The current awaited id (the id of the in-flight teleport packet).
    pub fn awaiting_teleport(&self) -> i32 {
        self.awaiting_teleport
    }

    /// Whether a teleport is pending acknowledgement.
    pub fn is_pending(&self) -> bool {
        self.awaiting_position.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_state_has_id_zero_and_no_pending() {
        let s = TeleportAckState::new();
        assert_eq!(s.awaiting_teleport(), 0);
        assert!(!s.is_pending());
    }

    #[test]
    fn accept_zero_before_any_teleport_kicks() {
        // The id matches (0 == 0) but no teleport is pending: Paper disconnects
        // with invalid_player_movement. This is a kick, not an ignore.
        let mut s = TeleportAckState::new();
        assert_eq!(s.accept(0), TeleportAckOutcome::InvalidMovementKick);
        assert!(!s.is_pending());
    }

    #[test]
    fn first_teleport_id_is_one() {
        let mut s = TeleportAckState::new();
        assert_eq!(s.begin_teleport(1.0, 2.0, 3.0), 1);
        assert!(s.is_pending());
    }

    #[test]
    fn ids_increment_sequentially() {
        let mut s = TeleportAckState::new();
        for i in 1..1000 {
            assert_eq!(s.begin_teleport(i as f64, 0.0, 0.0), i);
        }
    }

    #[test]
    fn accept_matching_id_returns_position_and_clears_pending() {
        let mut s = TeleportAckState::new();
        let id = s.begin_teleport(10.0, 20.0, 30.0);
        assert_eq!(
            s.accept(id),
            TeleportAckOutcome::Accepted {
                x: 10.0,
                y: 20.0,
                z: 30.0
            }
        );
        assert!(!s.is_pending());
        // The awaited id is unchanged after a successful accept.
        assert_eq!(s.awaiting_teleport(), id);
    }

    #[test]
    fn accept_wrong_id_is_ignored_and_pending_retained() {
        let mut s = TeleportAckState::new();
        s.begin_teleport(10.0, 20.0, 30.0);
        assert_eq!(s.accept(999), TeleportAckOutcome::Ignored);
        assert!(s.is_pending());
        // The pending position survives an ignored ack: the correct id still
        // accepts.
        assert_eq!(
            s.accept(1),
            TeleportAckOutcome::Accepted {
                x: 10.0,
                y: 20.0,
                z: 30.0
            }
        );
    }

    #[test]
    fn accept_matching_id_without_pending_kicks() {
        let mut s = TeleportAckState::new();
        s.begin_teleport(1.0, 2.0, 3.0);
        assert_eq!(
            s.accept(1),
            TeleportAckOutcome::Accepted {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        // Now the id matches but nothing is pending: kick.
        assert_eq!(s.accept(1), TeleportAckOutcome::InvalidMovementKick);
        // And still not pending afterwards.
        assert!(!s.is_pending());
    }

    #[test]
    fn second_teleport_overwrites_pending_position() {
        let mut s = TeleportAckState::new();
        s.begin_teleport(1.0, 1.0, 1.0);
        s.begin_teleport(2.0, 2.0, 2.0);
        // The first id no longer matches; the second does, with the newest
        // position.
        assert_eq!(s.accept(1), TeleportAckOutcome::Ignored);
        assert_eq!(
            s.accept(2),
            TeleportAckOutcome::Accepted {
                x: 2.0,
                y: 2.0,
                z: 2.0
            }
        );
    }

    #[test]
    fn id_resets_to_zero_when_increment_lands_on_i32_max() {
        // `awaitingTeleport` at MAX-1: `++` lands exactly on i32::MAX, the
        // `== i32::MAX` test is true, so the id resets to 0 and the packet is
        // sent with id 0 (the id i32::MAX itself is never sent).
        let mut s = TeleportAckState {
            awaiting_teleport: i32::MAX - 1,
            awaiting_position: Some([0.0, 0.0, 0.0]),
        };
        assert_eq!(s.begin_teleport(1.0, 0.0, 0.0), 0);
        assert_eq!(s.awaiting_teleport(), 0);
        // The next increment continues from 0.
        assert_eq!(s.begin_teleport(2.0, 0.0, 0.0), 1);
        assert_eq!(
            s.accept(1),
            TeleportAckOutcome::Accepted {
                x: 2.0,
                y: 0.0,
                z: 0.0
            }
        );
    }

    #[test]
    fn increment_at_i32_max_wraps_without_reset() {
        // `++` on i32::MAX wraps to i32::MIN in Java; the `== i32::MAX` test is
        // false, so the id does NOT reset to 0.
        let mut s = TeleportAckState {
            awaiting_teleport: i32::MAX,
            awaiting_position: Some([0.0, 0.0, 0.0]),
        };
        assert_eq!(s.begin_teleport(1.0, 0.0, 0.0), i32::MIN);
        assert_eq!(s.awaiting_teleport(), i32::MIN);
    }

    #[test]
    fn id_sequence_around_min_continues_negative() {
        let mut s = TeleportAckState {
            awaiting_teleport: i32::MIN,
            awaiting_position: Some([0.0, 0.0, 0.0]),
        };
        assert_eq!(s.begin_teleport(1.0, 0.0, 0.0), i32::MIN + 1);
    }
}
