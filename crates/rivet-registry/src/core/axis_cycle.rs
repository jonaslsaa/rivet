//! `net.minecraft.core.AxisCycle` — cyclic permutation of the three axes.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/AxisCycle.java`.

use super::direction::Axis;

/// `AxisCycle` — a cyclic permutation of the axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AxisCycle {
    None,
    Forward,
    Backward,
}

impl AxisCycle {
    /// `AxisCycle.VALUES` — enum order.
    pub const VALUES: [AxisCycle; 3] = [AxisCycle::None, AxisCycle::Forward, AxisCycle::Backward];

    /// `AxisCycle.AXIS_VALUES` — `Direction.Axis.values()`.
    pub const AXIS_VALUES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// `AxisCycle.cycle(int, int, int, Axis)` — `axis.choose` of the permuted
    /// coordinate triple.
    pub fn cycle(&self, x: i32, y: i32, z: i32, axis: Axis) -> i32 {
        match self {
            AxisCycle::None => axis.choose(x, y, z),
            AxisCycle::Forward => axis.choose(z, x, y),
            AxisCycle::Backward => axis.choose(y, z, x),
        }
    }

    /// `AxisCycle.cycle(double, double, double, Axis)`.
    pub fn cycle_f64(&self, x: f64, y: f64, z: f64, axis: Axis) -> f64 {
        match self {
            AxisCycle::None => axis.choose_f64(x, y, z),
            AxisCycle::Forward => axis.choose_f64(z, x, y),
            AxisCycle::Backward => axis.choose_f64(y, z, x),
        }
    }

    /// `AxisCycle.cycle(Axis)` — `AXIS_VALUES[floorMod(ordinal + 1, 3)]` for
    /// FORWARD, `floorMod(ordinal - 1, 3)` for BACKWARD.
    pub fn cycle_axis(&self, axis: Axis) -> Axis {
        let ordinal = axis as i32;
        match self {
            AxisCycle::None => axis,
            AxisCycle::Forward => {
                AxisCycle::AXIS_VALUES[ordinal.wrapping_add(1).rem_euclid(3) as usize]
            }
            AxisCycle::Backward => {
                AxisCycle::AXIS_VALUES[ordinal.wrapping_sub(1).rem_euclid(3) as usize]
            }
        }
    }

    /// `AxisCycle.inverse()`.
    pub fn inverse(&self) -> AxisCycle {
        match self {
            AxisCycle::None => AxisCycle::None,
            AxisCycle::Forward => AxisCycle::Backward,
            AxisCycle::Backward => AxisCycle::Forward,
        }
    }

    /// `AxisCycle.between(Axis, Axis)` — `VALUES[floorMod(to.ordinal() -
    /// from.ordinal(), 3)]`.
    pub fn between(from: Axis, to: Axis) -> AxisCycle {
        AxisCycle::VALUES[(to as i32 - from as i32).rem_euclid(3) as usize]
    }

    /// `MutableBlockPos.set(AxisCycle, int, int, int)` companion: the cycle
    /// applied to the `(x, y, z)` triple as `(cycle(x,y,z,X), cycle(x,y,z,Y),
    /// cycle(x,y,z,Z))`.
    pub fn cycle_xyz(&self, x: i32, y: i32, z: i32) -> (i32, i32, i32) {
        (
            self.cycle(x, y, z, Axis::X),
            self.cycle(x, y, z, Axis::Y),
            self.cycle(x, y, z, Axis::Z),
        )
    }
}

impl std::fmt::Display for AxisCycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            AxisCycle::None => "NONE",
            AxisCycle::Forward => "FORWARD",
            AxisCycle::Backward => "BACKWARD",
        })
    }
}
