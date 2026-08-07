//! Pure movement validation/math precursors (issue #158).
//!
//! Port of the arithmetic halves of `ServerGamePacketListenerImpl.handleMovePlayer`
//! (working/Paper, `paper-server/src/minecraft/java/net/minecraft/server/network/
//! ServerGamePacketListenerImpl.java`) that are independent of a live
//! `Connection`, `ServerPlayer`, level, or entity: the invalid-value gate
//! (`containsInvalidValues`), the Paper position/rotation clamps and wrapping,
//! the Paper "fix large move vectors killing the server" hardening
//! (`movedDist`), and the permissive moved-too-quickly threshold. Extracted
//! pure so every branch is deterministically testable before the #96 play
//! handoff exists.
//!
//! Out of scope (deferred to the M3 anti-cheat port, issue #158): `movedWrongly`,
//! `clientIsFloating` gravity kick, `shouldCheckPlayerMovement` gamerule gating,
//! the `allowedPlayerTicks` too-frequent-packet machine, config loading for
//! `movedTooQuicklyMultiplier` (the default is pinned here), and the
//! teleport/event side effects. The too-quickly check is a pure predicate —
//! the "permissive" M1 anti-cheat stub decides what to do with a violation.

use rivet_util::mth::{clamp_f64, max_f64, square_f64, wrap_degrees_f32};

/// `clampHorizontal` bound: `Mth.clamp(value, -3.0E7, 3.0E7)`.
pub const MAX_HORIZONTAL: f64 = 3.0E7;
/// `clampVertical` bound: `Mth.clamp(value, -2.0E7, 2.0E7)`.
pub const MAX_VERTICAL: f64 = 2.0E7;
/// Spigot `movedTooQuicklyMultiplier` default (`settings.moved-too-quickly-
/// multiplier`, `SpigotConfig` default 10.0).
pub const DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER: f64 = 10.0;
/// `metersPerTick` for ground movement in the too-quickly threshold.
pub const METERS_PER_TICK_WALKING: f64 = 100.0;
/// `metersPerTick` for elytra flight.
pub const METERS_PER_TICK_FLYING: f64 = 300.0;

/// `containsInvalidValues(x, y, z, yRot, xRot)` — the handleMovePlayer gate.
///
/// `x`/`y`/`z` are checked with `Double.isNaN` only: infinity is *accepted* for
/// positions. `yRot`/`xRot` use Guava `Floats.isFinite` (= `Float.isFinite`),
/// which rejects both NaN and infinity.
pub fn contains_invalid_values(x: f64, y: f64, z: f64, y_rot: f32, x_rot: f32) -> bool {
    x.is_nan() || y.is_nan() || z.is_nan() || !y_rot.is_finite() || !x_rot.is_finite()
}

/// Paper `clampHorizontal(double)`.
pub fn clamp_horizontal(value: f64) -> f64 {
    clamp_f64(value, -MAX_HORIZONTAL, MAX_HORIZONTAL)
}

/// Paper `clampVertical(double)`.
pub fn clamp_vertical(value: f64) -> f64 {
    clamp_f64(value, -MAX_VERTICAL, MAX_VERTICAL)
}

/// The clamped position and wrapped rotation the server accepts from a
/// `ServerboundMovePlayerPacket`, mirroring handleMovePlayer's preamble:
/// `Mth.wrapDegrees(packet.getYRot(player.getYRot()))` for both rotations and
/// `clampHorizontal`/`clampVertical` on the packet position, with the player's
/// current values as fallbacks for absent packet fields. Clamping applies to
/// the fallback too (Java clamps `packet.getX(player.getX())`, not the raw
/// packet value).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveTargets {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub y_rot: f32,
    pub x_rot: f32,
}

/// Builds [`MoveTargets`] from a move packet's optional fields plus the
/// player's current position/rotation.
#[allow(clippy::too_many_arguments)]
pub fn build_move_targets(
    packet_x: Option<f64>,
    packet_y: Option<f64>,
    packet_z: Option<f64>,
    packet_y_rot: Option<f32>,
    packet_x_rot: Option<f32>,
    player_x: f64,
    player_y: f64,
    player_z: f64,
    player_y_rot: f32,
    player_x_rot: f32,
) -> MoveTargets {
    MoveTargets {
        x: clamp_horizontal(packet_x.unwrap_or(player_x)),
        y: clamp_vertical(packet_y.unwrap_or(player_y)),
        z: clamp_horizontal(packet_z.unwrap_or(player_z)),
        y_rot: wrap_degrees_f32(packet_y_rot.unwrap_or(player_y_rot)),
        x_rot: wrap_degrees_f32(packet_x_rot.unwrap_or(player_x_rot)),
    }
}

/// The `firstGoodX/Y/Z` and `lastGoodX/Y/Z` anchors used by the Paper
/// large-move-vector hardening and the accepted-move bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MoveState {
    first_good: [f64; 3],
    last_good: [f64; 3],
}

impl MoveState {
    /// A fresh state anchored at the spawn position (both anchors equal).
    pub fn new(x: f64, y: f64, z: f64) -> Self {
        MoveState {
            first_good: [x, y, z],
            last_good: [x, y, z],
        }
    }

    /// `resetPosition()` — called at spawn and at the top of each `tickPlayer()`.
    pub fn reset_position(&mut self, x: f64, y: f64, z: f64) {
        self.first_good = [x, y, z];
        self.last_good = [x, y, z];
    }

    /// After a successful move accept: `lastGoodX/Y/Z = player.getX/Y/Z()`.
    pub fn on_move_accepted(&mut self, x: f64, y: f64, z: f64) {
        self.last_good = [x, y, z];
    }

    /// The `firstGoodX/Y/Z` anchor.
    pub fn first_good(&self) -> [f64; 3] {
        self.first_good
    }

    /// The `lastGoodX/Y/Z` anchor.
    pub fn last_good(&self) -> [f64; 3] {
        self.last_good
    }
}

/// Paper "fix large move vectors killing the server": the moved distance
/// (squared) fed to the too-quickly check is the max over the delta from
/// `firstGood`, the delta from the tick's starting position minus 1, and the
/// delta from `lastGood` minus 1. The max is Java `Math.max`, so a NaN in any
/// frame (e.g. a NaN packet position that slipped past the invalid-value gate)
/// propagates through the whole chain — Rust `f64::max` would drop it.
pub fn moved_distance_sqr(
    target: [f64; 3],
    first_good: [f64; 3],
    start: [f64; 3],
    last_good: [f64; 3],
) -> f64 {
    let sq_delta = |a: f64, b: f64| {
        let d = a - b;
        d * d
    };
    let mut moved = sq_delta(target[0], first_good[0])
        + sq_delta(target[1], first_good[1])
        + sq_delta(target[2], first_good[2]);
    moved = max_f64(
        moved,
        sq_delta(target[0], start[0])
            + sq_delta(target[1], start[1])
            + sq_delta(target[2], start[2])
            - 1.0,
    );
    moved = max_f64(
        moved,
        sq_delta(target[0], last_good[0])
            + sq_delta(target[1], last_good[1])
            + sq_delta(target[2], last_good[2])
            - 1.0,
    );
    moved
}

/// The `speed` fed to the too-quickly threshold: `getFlyingSpeed() * 20f` or
/// `getWalkingSpeed() * 10f`, float arithmetic widened to `double` (Java
/// `Abilities` defaults: flying 0.05F, walking 0.1F, both yielding 1.0).
pub fn movement_speed(flying: bool, flying_speed: f32, walking_speed: f32) -> f64 {
    if flying {
        (flying_speed * 20.0_f32) as f64
    } else {
        (walking_speed * 10.0_f32) as f64
    }
}

/// The permissive moved-too-quickly predicate:
/// `movedDist - expectedDist > Math.max(metersPerTick,
/// Mth.square(movedTooQuicklyMultiplier * (float) deltaPackets * speed))`.
///
/// `deltaPackets` is expected pre-clamped to 1 by the caller when the client
/// sends too-frequently (that clamp is the `allowedPlayerTicks` machine, out of
/// scope here). The violation is reported, not acted on — the M1 anti-cheat
/// stub decides the response. The threshold `max` is Java `Math.max`: a NaN
/// squared term (from a NaN `speed`/multiplier) makes the threshold NaN and the
/// strict `>` comparison false, so the move is never flagged.
#[allow(clippy::too_many_arguments)]
pub fn moved_too_quickly(
    moved_dist: f64,
    expected_dist: f64,
    is_fall_flying: bool,
    delta_packets: i32,
    speed: f64,
    moved_too_quickly_multiplier: f64,
) -> bool {
    let meters_per_tick: f64 = if is_fall_flying {
        METERS_PER_TICK_FLYING
    } else {
        METERS_PER_TICK_WALKING
    };
    // Java: `(float) deltaPackets` then widened again for the double multiply.
    let term = moved_too_quickly_multiplier * (delta_packets as f32 as f64) * speed;
    moved_dist - expected_dist > max_f64(meters_per_tick, square_f64(term))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32;
    use std::f64;

    // ---- contains_invalid_values ----

    #[test]
    fn nan_position_or_rotation_is_invalid() {
        assert!(contains_invalid_values(f64::NAN, 0.0, 0.0, 0.0, 0.0));
        assert!(contains_invalid_values(0.0, f64::NAN, 0.0, 0.0, 0.0));
        assert!(contains_invalid_values(0.0, 0.0, f64::NAN, 0.0, 0.0));
        assert!(contains_invalid_values(0.0, 0.0, 0.0, f32::NAN, 0.0));
        assert!(contains_invalid_values(0.0, 0.0, 0.0, 0.0, f32::NAN));
    }

    #[test]
    fn infinite_rotation_is_invalid_but_infinite_position_is_valid() {
        // Float.isFinite rejects both NaN and infinities for rotations.
        assert!(contains_invalid_values(0.0, 0.0, 0.0, f32::INFINITY, 0.0));
        assert!(contains_invalid_values(
            0.0,
            0.0,
            0.0,
            0.0,
            f32::NEG_INFINITY
        ));
        assert!(contains_invalid_values(
            0.0,
            0.0,
            0.0,
            f32::NEG_INFINITY,
            f32::INFINITY
        ));
        // Double.isNaN accepts infinities for positions.
        assert!(!contains_invalid_values(f64::INFINITY, 0.0, 0.0, 0.0, 0.0));
        assert!(!contains_invalid_values(
            0.0,
            f64::NEG_INFINITY,
            0.0,
            0.0,
            0.0
        ));
        assert!(!contains_invalid_values(0.0, 0.0, f64::INFINITY, 0.0, 0.0));
        // Infinite positions are fine as long as the rotations are finite.
        assert!(!contains_invalid_values(
            f64::INFINITY,
            0.0,
            0.0,
            45.0,
            -30.0
        ));
    }

    #[test]
    fn finite_values_are_valid() {
        assert!(!contains_invalid_values(1.5, -2.5, 3.0, 45.0, -30.0));
        assert!(!contains_invalid_values(0.0, 0.0, 0.0, 0.0, 0.0));
        // Extreme-but-finite values are accepted.
        assert!(!contains_invalid_values(
            f64::MAX,
            -f64::MAX,
            1e308,
            f32::MAX,
            f32::MIN
        ));
    }

    // ---- clamps ----

    #[test]
    fn horizontal_clamp_bounds() {
        assert_eq!(clamp_horizontal(0.0), 0.0);
        assert_eq!(clamp_horizontal(3.0E7), 3.0E7);
        assert_eq!(clamp_horizontal(-3.0E7), -3.0E7);
        assert_eq!(clamp_horizontal(3.1E7), 3.0E7);
        assert_eq!(clamp_horizontal(-3.1E7), -3.0E7);
        assert_eq!(clamp_horizontal(f64::MAX), 3.0E7);
        assert_eq!(clamp_horizontal(f64::NEG_INFINITY), -3.0E7);
        assert!(
            clamp_horizontal(f64::NAN).is_nan(),
            "Mth.clamp propagates NaN"
        );
    }

    #[test]
    fn vertical_clamp_bounds() {
        assert_eq!(clamp_vertical(2.0E7), 2.0E7);
        assert_eq!(clamp_vertical(-2.0E7), -2.0E7);
        assert_eq!(clamp_vertical(2.1E7), 2.0E7);
        assert_eq!(clamp_vertical(-2.1E7), -2.0E7);
        assert_eq!(clamp_vertical(f64::INFINITY), 2.0E7);
        assert!(clamp_vertical(f64::NAN).is_nan());
    }

    // ---- rotation wrapping ----

    #[test]
    fn rotation_wrapping() {
        assert_eq!(wrap_degrees_f32(370.0), 10.0);
        assert_eq!(wrap_degrees_f32(-370.0), -10.0);
        assert_eq!(wrap_degrees_f32(180.0), -180.0);
        assert_eq!(wrap_degrees_f32(-180.0), -180.0);
        assert_eq!(wrap_degrees_f32(190.0), -170.0);
        assert_eq!(wrap_degrees_f32(-190.0), 170.0);
        assert_eq!(wrap_degrees_f32(540.0), -180.0);
        assert_eq!(wrap_degrees_f32(360.0), 0.0);
        assert_eq!(wrap_degrees_f32(0.0), 0.0);
        assert_eq!(wrap_degrees_f32(179.0), 179.0);
        assert_eq!(wrap_degrees_f32(-179.0), -179.0);
    }

    // ---- build_move_targets ----

    #[test]
    fn full_move_packet_is_clamped_and_wrapped() {
        let t = build_move_targets(
            Some(1.0E8),
            Some(1.0E8),
            Some(-1.0E8),
            Some(370.0),
            Some(190.0),
            0.0,
            0.0,
            0.0,
            0.0,
            0.0,
        );
        assert_eq!(t.x, 3.0E7);
        assert_eq!(t.y, 2.0E7);
        assert_eq!(t.z, -3.0E7);
        assert_eq!(t.y_rot, 10.0);
        assert_eq!(t.x_rot, -170.0);
    }

    #[test]
    fn absent_fields_fall_back_to_player_and_are_still_clamped_wrapped() {
        // Rotation-only packet: positions fall back to the player's.
        let t = build_move_targets(
            None,
            None,
            None,
            Some(100.0),
            Some(-100.0),
            5.0,
            6.0,
            7.0,
            370.0,
            -370.0,
        );
        assert_eq!(t.x, 5.0);
        assert_eq!(t.y, 6.0);
        assert_eq!(t.z, 7.0);
        assert_eq!(t.y_rot, 100.0);
        assert_eq!(t.x_rot, -100.0);
        // Position-only packet: rotations fall back to the player's and are
        // still wrapped.
        let t = build_move_targets(
            Some(1.0E8),
            Some(-1.0E8),
            Some(1.0),
            None,
            None,
            0.0,
            0.0,
            0.0,
            370.0,
            -370.0,
        );
        assert_eq!(t.x, 3.0E7);
        assert_eq!(t.y, -2.0E7);
        assert_eq!(t.z, 1.0);
        assert_eq!(t.y_rot, 10.0);
        assert_eq!(t.x_rot, -10.0);
    }

    // ---- MoveState ----

    #[test]
    fn move_state_anchors_and_updates() {
        let mut s = MoveState::new(1.0, 2.0, 3.0);
        assert_eq!(s.first_good(), [1.0, 2.0, 3.0]);
        assert_eq!(s.last_good(), [1.0, 2.0, 3.0]);
        s.on_move_accepted(4.0, 5.0, 6.0);
        assert_eq!(s.first_good(), [1.0, 2.0, 3.0], "firstGood survives a move");
        assert_eq!(s.last_good(), [4.0, 5.0, 6.0]);
        s.reset_position(7.0, 8.0, 9.0);
        assert_eq!(s.first_good(), [7.0, 8.0, 9.0]);
        assert_eq!(s.last_good(), [7.0, 8.0, 9.0]);
    }

    // ---- moved_distance_sqr (Paper large-move hardening) ----

    #[test]
    fn plain_small_move_uses_first_good_delta() {
        // All three reference frames agree; the result is the plain squared
        // distance from firstGood.
        let d = moved_distance_sqr(
            [5.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert_eq!(d, 25.0);
    }

    #[test]
    fn start_delta_minus_one_can_dominate() {
        // The current-tick (start) delta is large but firstGood is at the
        // target: the max picks the start delta minus 1.
        let d = moved_distance_sqr(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [50.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert_eq!(d, 50.0 * 50.0 - 1.0);
    }

    #[test]
    fn last_good_delta_minus_one_can_dominate() {
        let d = moved_distance_sqr(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
        );
        assert_eq!(d, 30.0 * 30.0 - 1.0);
    }

    #[test]
    fn first_good_delta_dominates_when_largest() {
        let d = moved_distance_sqr(
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert_eq!(d, 100.0 * 100.0);
    }

    #[test]
    fn minus_one_applies_to_start_and_last_not_first() {
        // A 1-block delta from start: squared minus 1 is 0, so a 1-block move
        // reports 0 from that frame (it must not report 1).
        let d = moved_distance_sqr(
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        );
        assert_eq!(d, 0.0);
    }

    #[test]
    fn max_is_computed_over_all_three_frames() {
        // firstGood far, lastGood far the other way, start near: firstGood
        // wins; perturb so lastGood wins.
        let d = moved_distance_sqr(
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [200.0, 0.0, 0.0],
        );
        assert_eq!(d, 200.0 * 200.0 - 1.0);
    }

    #[test]
    fn nan_target_propagates_through_the_max_chain() {
        // Java `Math.max` returns NaN if either operand is NaN: a NaN target
        // component poisons the firstGood delta, and both subsequent `Math.max`
        // calls carry it through. Rust `f64::max` would return the non-NaN
        // operand and mask the poison.
        let d = moved_distance_sqr(
            [f64::NAN, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert!(d.is_nan());
    }

    #[test]
    fn nan_in_start_or_last_good_frame_propagates() {
        // A NaN in either later reference frame lands in the second `Math.max`
        // operand and propagates (Java `Math.max(25.0, NaN)` is NaN).
        let d = moved_distance_sqr(
            [5.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [f64::NAN, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert!(d.is_nan());
        let d = moved_distance_sqr(
            [5.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [f64::NAN, 0.0, 0.0],
        );
        assert!(d.is_nan());
    }

    // ---- movement_speed ----

    #[test]
    fn default_abilities_yield_speed_one() {
        // Java defaults: flying 0.05F, walking 0.1F.
        assert_eq!(movement_speed(false, 0.05, 0.1), 1.0);
        assert_eq!(movement_speed(true, 0.05, 0.1), 1.0);
    }

    #[test]
    fn custom_abilities_scale_speed() {
        assert_eq!(movement_speed(false, 0.05, 0.2), 2.0);
        assert_eq!(movement_speed(true, 0.1, 0.1), 2.0);
        assert_eq!(movement_speed(true, 0.0, 0.1), 0.0);
    }

    // ---- moved_too_quickly (permissive threshold) ----

    #[test]
    fn walking_threshold_is_100_squared_units() {
        // speed 1.0, 1 packet/tick: (10.0 * 1 * 1)^2 = 100, max(100, 100) = 100.
        assert!(!moved_too_quickly(100.0, 0.0, false, 1, 1.0, 10.0));
        assert!(moved_too_quickly(100.0001, 0.0, false, 1, 1.0, 10.0));
        // Strict `>`: exactly the threshold is not a violation.
        assert!(!moved_too_quickly(100.0, 0.0, false, 1, 1.0, 10.0));
    }

    #[test]
    fn flying_threshold_is_300_squared_units() {
        assert!(!moved_too_quickly(300.0, 0.0, true, 1, 1.0, 10.0));
        assert!(moved_too_quickly(301.0, 0.0, true, 1, 1.0, 10.0));
        // At 100 the walker trips, the flier does not.
        assert!(!moved_too_quickly(100.0001, 0.0, true, 1, 1.0, 10.0));
    }

    #[test]
    fn more_packets_per_tick_raises_threshold() {
        // deltaPackets=2: (10 * 2 * 1)^2 = 400.
        assert!(!moved_too_quickly(400.0, 0.0, false, 2, 1.0, 10.0));
        assert!(moved_too_quickly(401.0, 0.0, false, 2, 1.0, 10.0));
    }

    #[test]
    fn expected_delta_movement_is_subtracted() {
        // A player's own estimated movement eats into the budget.
        assert!(!moved_too_quickly(110.0, 10.0, false, 1, 1.0, 10.0));
        assert!(moved_too_quickly(110.0, 0.0, false, 1, 1.0, 10.0));
    }

    #[test]
    fn multiplier_and_speed_scale_the_squared_term() {
        // Multiplier 20 with speed 1.0: (20 * 1 * 1)^2 = 400 > 100 base.
        assert!(!moved_too_quickly(400.0, 0.0, false, 1, 1.0, 20.0));
        assert!(moved_too_quickly(401.0, 0.0, false, 1, 1.0, 20.0));
        // Multiplier 5: (5)^2 = 25, below the 100 floor.
        assert!(!moved_too_quickly(100.0, 0.0, false, 1, 1.0, 5.0));
        assert!(moved_too_quickly(100.0001, 0.0, false, 1, 1.0, 5.0));
        // Speed 2.0 with default multiplier: (10 * 1 * 2)^2 = 400.
        assert!(!moved_too_quickly(400.0, 0.0, false, 1, 2.0, 10.0));
        assert!(moved_too_quickly(401.0, 0.0, false, 1, 2.0, 10.0));
    }

    #[test]
    fn delta_packets_cast_through_float() {
        // The Java `(float) deltaPackets` cast matters for large counts: a
        // f32-rounded count feeds the square, e.g. deltaPackets = 16,777,217
        // rounds to 16,777,216 in f32. Drive the predicate from a movedDist
        // strictly between the squared thresholds with and without the cast so
        // the test fails if the cast is dropped:
        //   with cast:    (10 * 16,777,216 * 1)^2 = 28,147,497,671,065,600
        //   without cast: (10 * 16,777,217 * 1)^2 = 28,147,501,026,508,900
        //   moved_dist   = 28,147,500,000,000,000  (strictly between)
        let between = 28_147_500_000_000_000.0;
        let big = moved_too_quickly(
            between,
            0.0,
            false,
            16_777_217,
            1.0,
            DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER,
        );
        let rounded = moved_too_quickly(
            between,
            0.0,
            false,
            16_777_216,
            1.0,
            DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER,
        );
        assert!(
            rounded,
            "16,777,216 casts losslessly and must trip at moved_dist"
        );
        assert!(
            big,
            "16,777,217 must round down in f32 and trip identically"
        );
        assert_eq!(big, rounded, "deltaPackets goes through a f32 cast");
    }

    #[test]
    fn nan_moved_or_expected_dist_never_flags() {
        // Java: a NaN operand makes the strict `>` comparison false, so the
        // move is never flagged regardless of the threshold.
        assert!(!moved_too_quickly(f64::NAN, 0.0, false, 1, 1.0, 10.0));
        assert!(!moved_too_quickly(1000.0, f64::NAN, false, 1, 1.0, 10.0));
    }

    #[test]
    fn nan_speed_or_multiplier_never_flags() {
        // A NaN speed or multiplier poisons the squared term. Java `Math.max`
        // turns the whole threshold NaN and the strict `>` is false — Rust
        // `f64::max` would fall back to the 100.0 floor and could flag.
        assert!(!moved_too_quickly(1000.0, 0.0, false, 1, f64::NAN, 10.0));
        assert!(!moved_too_quickly(1000.0, 0.0, false, 1, 1.0, f64::NAN));
    }

    // ---- full-pipeline composition (Paper handleMovePlayer arithmetic) ----

    #[test]
    fn big_but_legit_move_is_permissive_under_m1_stub() {
        // Player at spawn (0,64,0); a single-tick 10-block move with a small
        // estimated delta movement. Paper-hardened movedDist = max(100, 99, 99)
        // = 100; minus expected 0.1 → 99.9, which is NOT above the 100 floor.
        // The M1 stub lets it through (permissive).
        let target = [10.0, 64.0, 0.0];
        let spawn = [0.0, 64.0, 0.0];
        let moved = moved_distance_sqr(target, spawn, spawn, spawn);
        assert_eq!(moved, 100.0);
        assert!(!moved_too_quickly(
            moved,
            0.1,
            false,
            1,
            movement_speed(false, 0.05, 0.1),
            DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER,
        ));
    }

    #[test]
    fn abrupt_teleport_like_move_trips_the_threshold() {
        // An 11-block single-tick move: hardened movedDist = 121, above the 100
        // floor after the expected-dist subtraction → flagged.
        let target = [11.0, 64.0, 0.0];
        let spawn = [0.0, 64.0, 0.0];
        let moved = moved_distance_sqr(target, spawn, spawn, spawn);
        assert_eq!(moved, 121.0);
        assert!(moved_too_quickly(
            moved,
            0.1,
            false,
            1,
            movement_speed(false, 0.05, 0.1),
            DEFAULT_MOVED_TOO_QUICKLY_MULTIPLIER,
        ));
    }

    #[test]
    fn nan_position_flows_through_predicate_as_not_too_quickly() {
        // The invalid-value gate normally filters NaN positions before this
        // arithmetic runs; if one ever reaches it, Java's behavior is: hardened
        // movedDist is NaN, `NaN - expected > threshold` is false → not flagged.
        let moved = moved_distance_sqr(
            [f64::NAN, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0],
        );
        assert!(moved.is_nan());
        assert!(!moved_too_quickly(moved, 0.0, false, 1, 1.0, 10.0));
    }

    #[test]
    fn expected_dist_can_clear_a_borderline_move() {
        // 100.9 hardened minus a 0.9 expected delta = 100.0, exactly the
        // threshold (strict `>` is false).
        assert!(!moved_too_quickly(100.9, 0.9, false, 1, 1.0, 10.0));
        // 101.0 minus 0.9 = 100.1 → above.
        assert!(moved_too_quickly(101.0, 0.9, false, 1, 1.0, 10.0));
    }
}
