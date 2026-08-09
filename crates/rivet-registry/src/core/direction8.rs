//! `net.minecraft.core.Direction8` — the eight horizontal directions (N, NE,
//! E, SE, S, SW, W, NW), each a one- or two-element set of `Direction`s plus a
//! horizontal step vector.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/Direction8.java`.
//! The `step` is accumulated from the member directions' `getStepX/Y/Z`
//! (horizontal members have zero `y`, so the sum's `y` is always `0`).
//! `getDirections()` returns a guava `Sets.immutableEnumSet`, whose iteration
//! order is `Direction.values()` (declaration) order — preserved here as a
//! `Vec<Direction>` in that order.

use super::direction::Direction;

/// `Direction8` — an eight-direction horizontal facing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction8 {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Direction8 {
    /// `Direction8.values()` — the eight directions in ordinal order (the
    /// bitmask order `UpgradeData`'s `Sides` byte uses).
    pub const fn all() -> [Direction8; 8] {
        [
            Direction8::North,
            Direction8::NorthEast,
            Direction8::East,
            Direction8::SouthEast,
            Direction8::South,
            Direction8::SouthWest,
            Direction8::West,
            Direction8::NorthWest,
        ]
    }

    /// `Direction8.getDirections()` — the member `Direction`s in
    /// `Direction.values()` order.
    pub fn get_directions(&self) -> Vec<Direction> {
        match self {
            Direction8::North => vec![Direction::North],
            Direction8::NorthEast => vec![Direction::North, Direction::East],
            Direction8::East => vec![Direction::East],
            Direction8::SouthEast => vec![Direction::South, Direction::East],
            Direction8::South => vec![Direction::South],
            Direction8::SouthWest => vec![Direction::South, Direction::West],
            Direction8::West => vec![Direction::West],
            Direction8::NorthWest => vec![Direction::North, Direction::West],
        }
    }

    /// `Direction8.getStepX()` — the summed `getStepX` of the members.
    pub fn get_step_x(&self) -> i32 {
        match self {
            Direction8::North => 0,
            Direction8::NorthEast => 1,
            Direction8::East => 1,
            Direction8::SouthEast => 1,
            Direction8::South => 0,
            Direction8::SouthWest => -1,
            Direction8::West => -1,
            Direction8::NorthWest => -1,
        }
    }

    /// `Direction8.getStepZ()` — the summed `getStepZ` of the members.
    pub fn get_step_z(&self) -> i32 {
        match self {
            Direction8::North => -1,
            Direction8::NorthEast => -1,
            Direction8::East => 0,
            Direction8::SouthEast => 1,
            Direction8::South => 1,
            Direction8::SouthWest => 1,
            Direction8::West => 0,
            Direction8::NorthWest => -1,
        }
    }
}

impl std::fmt::Display for Direction8 {
    /// `Direction8.toString()` — Java does not override `toString()`, so it
    /// returns the enum constant name.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Direction8::North => "NORTH",
            Direction8::NorthEast => "NORTH_EAST",
            Direction8::East => "EAST",
            Direction8::SouthEast => "SOUTH_EAST",
            Direction8::South => "SOUTH",
            Direction8::SouthWest => "SOUTH_WEST",
            Direction8::West => "WEST",
            Direction8::NorthWest => "NORTH_WEST",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_x_values() {
        let expected = [
            (Direction8::North, 0),
            (Direction8::NorthEast, 1),
            (Direction8::East, 1),
            (Direction8::SouthEast, 1),
            (Direction8::South, 0),
            (Direction8::SouthWest, -1),
            (Direction8::West, -1),
            (Direction8::NorthWest, -1),
        ];
        for (d, step) in expected {
            assert_eq!(d.get_step_x(), step);
        }
    }

    #[test]
    fn step_z_values() {
        let expected = [
            (Direction8::North, -1),
            (Direction8::NorthEast, -1),
            (Direction8::East, 0),
            (Direction8::SouthEast, 1),
            (Direction8::South, 1),
            (Direction8::SouthWest, 1),
            (Direction8::West, 0),
            (Direction8::NorthWest, -1),
        ];
        for (d, step) in expected {
            assert_eq!(d.get_step_z(), step);
        }
    }

    #[test]
    fn step_is_the_sum_of_member_direction_steps() {
        // Cross-checks the two step tables against the Java accumulation rule:
        // step = sum over getDirections() of Direction.getStepX/Z.
        for d in [
            Direction8::North,
            Direction8::NorthEast,
            Direction8::East,
            Direction8::SouthEast,
            Direction8::South,
            Direction8::SouthWest,
            Direction8::West,
            Direction8::NorthWest,
        ] {
            let sx: i32 = d.get_directions().iter().map(|dir| dir.step_x()).sum();
            let sz: i32 = d.get_directions().iter().map(|dir| dir.step_z()).sum();
            assert_eq!(d.get_step_x(), sx, "stepX mismatch for {d}");
            assert_eq!(d.get_step_z(), sz, "stepZ mismatch for {d}");
        }
    }

    #[test]
    fn get_directions_sets_and_order() {
        assert_eq!(Direction8::North.get_directions(), vec![Direction::North]);
        assert_eq!(
            Direction8::NorthEast.get_directions(),
            vec![Direction::North, Direction::East]
        );
        assert_eq!(Direction8::East.get_directions(), vec![Direction::East]);
        assert_eq!(
            Direction8::SouthEast.get_directions(),
            vec![Direction::South, Direction::East]
        );
        assert_eq!(Direction8::South.get_directions(), vec![Direction::South]);
        assert_eq!(
            Direction8::SouthWest.get_directions(),
            vec![Direction::South, Direction::West]
        );
        assert_eq!(Direction8::West.get_directions(), vec![Direction::West]);
        assert_eq!(
            Direction8::NorthWest.get_directions(),
            vec![Direction::North, Direction::West]
        );
    }

    #[test]
    fn display_matches_enum_constant_names() {
        assert_eq!(Direction8::North.to_string(), "NORTH");
        assert_eq!(Direction8::NorthEast.to_string(), "NORTH_EAST");
        assert_eq!(Direction8::SouthEast.to_string(), "SOUTH_EAST");
        assert_eq!(Direction8::SouthWest.to_string(), "SOUTH_WEST");
        assert_eq!(Direction8::NorthWest.to_string(), "NORTH_WEST");
    }
}
