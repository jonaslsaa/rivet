//! Paper 26.2 reach, hit-location, and sequence validation helpers.
//!
//! Mirrors:
//! - `net.minecraft.world.entity.ai.attributes.Attributes.BLOCK_INTERACTION_RANGE` (default `4.5`)
//! - `net.minecraft.world.phys.AABB(BlockPos)` and `AABB.distanceToSqr(Vec3)`
//! - `net.minecraft.world.entity.player.Player.isWithinBlockInteractionRange(BlockPos, double)`
//! - `net.minecraft.server.network.ServerGamePacketListenerImpl.handleUseItemOn` hit offset check
//! - `net.minecraft.server.network.ServerGamePacketListenerImpl.ackBlockChangesUpTo` and `tick`

use rivet_registry::core::{BlockPos, Vec3};
use thiserror::Error;

use super::packets::ClientboundBlockChangedAckPacket;

/// `Attributes.BLOCK_INTERACTION_RANGE` default value (`4.5`).
pub const DEFAULT_BLOCK_INTERACTION_RANGE: f64 = 4.5;

/// Buffer passed by `ServerGamePacketListenerImpl` to `isWithinBlockInteractionRange(pos, 1.0)`.
pub const INTERACTION_RANGE_BUFFER: f64 = 1.0;

/// Maximum per-axis distance from block center allowed in `handleUseItemOn`.
pub const MAX_HIT_LOCATION_OFFSET: f64 = 1.0000001;

/// Paper's exact disconnect reason for negative sequence numbers in `ackBlockChangesUpTo`.
pub const INVALID_SEQUENCE_DISCONNECT_MESSAGE: &str = "Expected packet sequence nr >= 0";

/// Exact `java.lang.Math.max(double, double)` semantics per `PORTING.md`:
/// propagates NaN, returns `+0.0` for mixed-sign zeroes, and returns `a` for same-sign zeroes.
#[inline]
fn java_max_f64(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_positive() || b.is_sign_positive() {
            0.0
        } else {
            -0.0
        };
    }
    if a >= b { a } else { b }
}

/// `new AABB(pos).distanceToSqr(point)`.
///
/// Exact translation of `AABB.java`:
/// ```java
/// double dx = Math.max(Math.max(this.minX - point.x, point.x - this.maxX), 0.0);
/// double dy = Math.max(Math.max(this.minY - point.y, point.y - this.maxY), 0.0);
/// double dz = Math.max(Math.max(this.minZ - point.z, point.z - this.maxZ), 0.0);
/// return Mth.lengthSquared(dx, dy, dz);
/// ```
pub fn aabb_block_distance_to_sqr(pos: BlockPos, point: Vec3) -> f64 {
    let min_x = pos.get_x() as f64;
    let min_y = pos.get_y() as f64;
    let min_z = pos.get_z() as f64;
    let max_x = (pos.get_x().wrapping_add(1)) as f64;
    let max_y = (pos.get_y().wrapping_add(1)) as f64;
    let max_z = (pos.get_z().wrapping_add(1)) as f64;

    let dx = java_max_f64(java_max_f64(min_x - point.x, point.x - max_x), 0.0);
    let dy = java_max_f64(java_max_f64(min_y - point.y, point.y - max_y), 0.0);
    let dz = java_max_f64(java_max_f64(min_z - point.z, point.z - max_z), 0.0);
    dx * dx + dy * dy + dz * dz
}

/// `Player.isWithinBlockInteractionRange(BlockPos pos, double buffer)`.
///
/// ```java
/// double maxRange = this.blockInteractionRange() + buffer;
/// return new AABB(pos).distanceToSqr(this.getEyePosition()) < maxRange * maxRange;
/// ```
pub fn is_within_block_interaction_range(
    eye_position: Vec3,
    block_interaction_range: f64,
    pos: BlockPos,
    buffer: f64,
) -> bool {
    if !eye_position.x.is_finite() || !eye_position.y.is_finite() || !eye_position.z.is_finite() {
        return false;
    }
    let max_range = block_interaction_range + buffer;
    if !max_range.is_finite() || max_range < 0.0 {
        return false;
    }
    aabb_block_distance_to_sqr(pos, eye_position) < max_range * max_range
}

/// Validation failures for `BlockHitResult.getLocation()` in `handleUseItemOn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HitLocationError {
    #[error("hit location contains non-finite coordinates")]
    NonFinite,
    #[error("hit location exceeds 1.0000001 offset from block center")]
    OffsetOutOfRange,
}

/// Validates `BlockHitResult.getLocation()` against Paper's `handleUseItemOn` bounds:
/// 1. `location.isFinite()`
/// 2. `|location - Vec3.atCenterOf(pos)| < 1.0000001` on every axis.
pub fn validate_hit_location(pos: BlockPos, location: Vec3) -> Result<(), HitLocationError> {
    if !location.x.is_finite() || !location.y.is_finite() || !location.z.is_finite() {
        return Err(HitLocationError::NonFinite);
    }
    let dx = location.x - (pos.get_x() as f64 + 0.5);
    let dy = location.y - (pos.get_y() as f64 + 0.5);
    let dz = location.z - (pos.get_z() as f64 + 0.5);
    if dx.abs() < MAX_HIT_LOCATION_OFFSET
        && dy.abs() < MAX_HIT_LOCATION_OFFSET
        && dz.abs() < MAX_HIT_LOCATION_OFFSET
    {
        Ok(())
    } else {
        Err(HitLocationError::OffsetOutOfRange)
    }
}

/// Error returned when a client sends a negative block-change sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("Expected packet sequence nr >= 0 (got {sequence})")]
pub struct InvalidSequenceError {
    pub sequence: i32,
}

/// `ServerGamePacketListenerImpl.ackBlockChangesUpTo` state tracker.
///
/// Tracks the highest acknowledged sequence number (`-1` when none pending) and
/// flushes a single `ClientboundBlockChangedAckPacket` per tick. Out-of-order
/// non-negative sequences are coalesced with `max(seq, pending)` exactly as in Paper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SequenceAckTracker {
    ack_block_changes_up_to: i32,
}

impl Default for SequenceAckTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SequenceAckTracker {
    /// Initializes with `ackBlockChangesUpTo = -1`.
    pub const fn new() -> Self {
        Self {
            ack_block_changes_up_to: -1,
        }
    }

    /// `ackBlockChangesUpTo(int packetSequenceNr)`.
    ///
    /// Rejects `sequence < 0` (caller must disconnect the player with
    /// [`INVALID_SEQUENCE_DISCONNECT_MESSAGE`]); otherwise updates the high-water
    /// mark with `max(sequence, self.ack_block_changes_up_to)`.
    pub fn ack_up_to(&mut self, sequence: i32) -> Result<(), InvalidSequenceError> {
        if sequence < 0 {
            return Err(InvalidSequenceError { sequence });
        }
        self.ack_block_changes_up_to = self.ack_block_changes_up_to.max(sequence);
        Ok(())
    }

    /// Current pending high-water sequence, or `None` when `-1`.
    pub fn pending(&self) -> Option<i32> {
        if self.ack_block_changes_up_to > -1 {
            Some(self.ack_block_changes_up_to)
        } else {
            None
        }
    }

    /// Flushes `ClientboundBlockChangedAckPacket` during `ServerGamePacketListenerImpl.tick()`
    /// and resets `ackBlockChangesUpTo = -1`.
    pub fn flush_ack(&mut self) -> Option<ClientboundBlockChangedAckPacket> {
        if self.ack_block_changes_up_to > -1 {
            let seq = self.ack_block_changes_up_to;
            self.ack_block_changes_up_to = -1;
            Some(ClientboundBlockChangedAckPacket::new(seq))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_block_distance_to_sqr_propagates_nan_like_java_math_max() {
        let pos = BlockPos::new(1, 2, 3);
        assert!(aabb_block_distance_to_sqr(pos, Vec3::new(f64::NAN, 2.5, 3.5)).is_nan());
        assert!(aabb_block_distance_to_sqr(pos, Vec3::new(1.5, f64::NAN, 3.5)).is_nan());
        assert!(aabb_block_distance_to_sqr(pos, Vec3::new(1.5, 2.5, f64::NAN)).is_nan());
        assert_eq!(aabb_block_distance_to_sqr(pos, Vec3::new(1.5, 2.5, 3.5)), 0.0);
    }
}
