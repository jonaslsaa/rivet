//! `net.minecraft.world.level.block.Rotation` — the four block rotations.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/block/Rotation.java`.
//!
//! RivetTodo(#126): `CODEC`/`STREAM_CODEC`/`BY_ID`/`LEGACY_CODEC` (codec
//! surface → rivet-protocol). `rotation()`/`OctahedralGroup` (JOML) and
//! `getRandom`/`getShuffled` (RNG in rivet-util) defer with their owning units.

use super::direction::Direction;
use rivet_serialization::codec::JavaEquals;

/// `Rotation` — a quarter-turn rotation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rotation {
    None,
    Clockwise90,
    Clockwise180,
    Counterclockwise90,
}

impl JavaEquals for Rotation {
    fn java_equals(&self, other: &Self) -> bool {
        // Java enum `equals` is reference identity; enum constants are
        // singletons, so value equality (`==`) is identical.
        self == other
    }
}

impl Rotation {
    /// `Rotation.VALUES` — enum order.
    pub const VALUES: [Rotation; 4] = [
        Rotation::None,
        Rotation::Clockwise90,
        Rotation::Clockwise180,
        Rotation::Counterclockwise90,
    ];

    /// `Rotation.getRotated(Rotation)` — composition.
    pub fn get_rotated(&self, rot: Rotation) -> Rotation {
        match rot {
            Rotation::Clockwise90 => match self {
                Rotation::None => Rotation::Clockwise90,
                Rotation::Clockwise90 => Rotation::Clockwise180,
                Rotation::Clockwise180 => Rotation::Counterclockwise90,
                Rotation::Counterclockwise90 => Rotation::None,
            },
            Rotation::Clockwise180 => match self {
                Rotation::None => Rotation::Clockwise180,
                Rotation::Clockwise90 => Rotation::Counterclockwise90,
                Rotation::Clockwise180 => Rotation::None,
                Rotation::Counterclockwise90 => Rotation::Clockwise90,
            },
            Rotation::Counterclockwise90 => match self {
                Rotation::None => Rotation::Counterclockwise90,
                Rotation::Clockwise90 => Rotation::None,
                Rotation::Clockwise180 => Rotation::Clockwise90,
                Rotation::Counterclockwise90 => Rotation::Clockwise180,
            },
            Rotation::None => *self,
        }
    }

    /// `Rotation.rotate(Direction)`.
    pub fn rotate(&self, direction: &Direction) -> Direction {
        if direction.get_axis() == super::direction::Axis::Y {
            return *direction;
        }
        match self {
            Rotation::Clockwise90 => direction.get_clock_wise(),
            Rotation::Clockwise180 => direction.get_opposite(),
            Rotation::Counterclockwise90 => direction.get_counter_clock_wise(),
            Rotation::None => *direction,
        }
    }

    /// `Rotation.rotate(int rotation, int steps)`.
    pub fn rotate_int(&self, rotation: i32, steps: i32) -> i32 {
        match self {
            Rotation::Clockwise90 => (rotation + steps / 4) % steps,
            Rotation::Clockwise180 => (rotation + steps / 2) % steps,
            Rotation::Counterclockwise90 => (rotation + steps * 3 / 4) % steps,
            Rotation::None => rotation,
        }
    }

    /// `Rotation.getSerializedName()`.
    pub fn get_serialized_name(&self) -> &'static str {
        match self {
            Rotation::None => "none",
            Rotation::Clockwise90 => "clockwise_90",
            Rotation::Clockwise180 => "180",
            Rotation::Counterclockwise90 => "counterclockwise_90",
        }
    }
}

impl std::fmt::Display for Rotation {
    /// `Rotation.toString()` — Java does not override `toString()`, so it
    /// returns the enum constant name (unlike `getSerializedName()`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Rotation::None => "NONE",
            Rotation::Clockwise90 => "CLOCKWISE_90",
            Rotation::Clockwise180 => "CLOCKWISE_180",
            Rotation::Counterclockwise90 => "COUNTERCLOCKWISE_90",
        })
    }
}
