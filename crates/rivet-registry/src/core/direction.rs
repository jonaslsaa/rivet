//! `net.minecraft.core.Direction` — the six axis-aligned directions plus the
//! `Axis`, `AxisDirection` and `Plane` value types.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/core/Direction.java`.
//! Preserves the `data3d`/`data2d`/`oppositeIndex` tables and the wrapping
//! `Mth.abs` indexing of `from3DDataValue`/`from2DDataValue`.
//!
//! Deferred (leaf types owned by other units, PORTING.md): JOML (`Vec3`,
//! `Vector3f`, `Quaternionf`, `Matrix4fc` — `getUnitVec3`, `step`, `getRotation`,
//! `rotate(Matrix4fc, …)`), `Entity`-based methods (`orderedByNearest`,
//! `getFacingAxis`), `RandomSource` methods (`getRandom`, `allShuffled`),
//! `axisStepOrder` (JOML `Vec3`), and `moonrise$uniqueId`. RivetTodo(#126): the
//! remaining codec surface (`CODEC`'s sibling constants `VERTICAL_CODEC`,
//! `byName`, `STREAM_CODEC`, `LEGACY_ID_CODEC_*`) defers with the protocol
//! codec surface; `CODEC` itself is ported here as [`direction_codec`] (the
//! `StringRepresentable.fromEnum` ops form) because
//! `HasSturdyFacePredicate.CODEC` (issue #180) reads `Direction.CODEC.fieldOf
//! ("direction")`.

use super::vec3i::Vec3i;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::extra_codecs;
use rivet_util::mth;
use std::sync::Arc;

/// A direction. Variant order (and therefore `Ord`/`Hash`/iteration order)
/// matches Java's `Direction.values()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

/// The `data3d` value (`get3DDataValue`), Java enum-constructor arg 0.
fn data_3d(d: Direction) -> i32 {
    match d {
        Direction::Down => 0,
        Direction::Up => 1,
        Direction::North => 2,
        Direction::South => 3,
        Direction::West => 4,
        Direction::East => 5,
    }
}

/// The `data2d` value (`get2DDataValue`), Java enum-constructor arg 2.
fn data_2d(d: Direction) -> i32 {
    match d {
        Direction::Down => -1,
        Direction::Up => -1,
        Direction::North => 2,
        Direction::South => 0,
        Direction::West => 1,
        Direction::East => 3,
    }
}

/// `BY_3D_DATA` — `values()` sorted by `data3d` (already in enum order).
const BY_3D_DATA: [Direction; 6] = [
    Direction::Down,
    Direction::Up,
    Direction::North,
    Direction::South,
    Direction::West,
    Direction::East,
];

/// `BY_2D_DATA` — horizontal `values()` sorted by `data2d`.
const BY_2D_DATA: [Direction; 4] = [
    Direction::South,
    Direction::West,
    Direction::North,
    Direction::East,
];

impl Direction {
    pub const VALUES: [Direction; 6] = BY_3D_DATA;

    /// The `data3d` values, in `values()` order: `DOWN=0, UP=1, NORTH=2,
    /// SOUTH=3, WEST=4, EAST=5`.
    pub fn get_3d_data_value(&self) -> i32 {
        data_3d(*self)
    }

    /// The `data2d` values: `NORTH=2, SOUTH=0, WEST=1, EAST=3`, `-1` for the
    /// vertical directions.
    pub fn get_2d_data_value(&self) -> i32 {
        data_2d(*self)
    }

    /// `Direction.getStepX()`.
    pub fn step_x(&self) -> i32 {
        match self {
            Direction::West => -1,
            Direction::East => 1,
            _ => 0,
        }
    }

    /// `Direction.getStepY()`.
    pub fn step_y(&self) -> i32 {
        match self {
            Direction::Down => -1,
            Direction::Up => 1,
            _ => 0,
        }
    }

    /// `Direction.getStepZ()`.
    pub fn step_z(&self) -> i32 {
        match self {
            Direction::North => -1,
            Direction::South => 1,
            _ => 0,
        }
    }

    /// `Direction.getOpposite()` — table `oppositeIndex`.
    pub fn get_opposite(&self) -> Direction {
        match self {
            Direction::Down => Direction::Up,
            Direction::Up => Direction::Down,
            Direction::North => Direction::South,
            Direction::South => Direction::North,
            Direction::West => Direction::East,
            Direction::East => Direction::West,
        }
    }

    /// `Direction.getAxis()`.
    pub fn get_axis(&self) -> Axis {
        match self {
            Direction::Down | Direction::Up => Axis::Y,
            Direction::North | Direction::South => Axis::Z,
            Direction::West | Direction::East => Axis::X,
        }
    }

    /// `Direction.getAxisDirection()`.
    pub fn get_axis_direction(&self) -> AxisDirection {
        match self {
            Direction::Down | Direction::North | Direction::West => AxisDirection::Negative,
            Direction::Up | Direction::South | Direction::East => AxisDirection::Positive,
        }
    }

    /// `Direction.getName()`.
    pub fn get_name(&self) -> &'static str {
        match self {
            Direction::Down => "down",
            Direction::Up => "up",
            Direction::North => "north",
            Direction::South => "south",
            Direction::West => "west",
            Direction::East => "east",
        }
    }

    /// `Direction.getSerializedName()` — same string as `getName`.
    pub fn get_serialized_name(&self) -> &'static str {
        self.get_name()
    }

    /// `Direction.getUnitVec3i()` — the `Vec3i` normal.
    pub fn get_unit_vec3i(&self) -> Vec3i {
        Vec3i::new(self.step_x(), self.step_y(), self.step_z())
    }

    /// `Direction.stream()` — `values()` in enum order.
    pub fn stream() -> [Direction; 6] {
        BY_3D_DATA
    }

    /// `Direction.from3DDataValue(int)` — `BY_3D_DATA[Mth.abs(data % 6)]`
    /// (wrapping `Mth.abs`, so negative inputs wrap the index).
    pub fn from_3d_data_value(data: i32) -> Direction {
        BY_3D_DATA[mth::abs_i32(data % 6) as usize]
    }

    /// `Direction.from2DDataValue(int)` — `BY_2D_DATA[Mth.abs(data % 4)]`.
    pub fn from_2d_data_value(data: i32) -> Direction {
        BY_2D_DATA[mth::abs_i32(data % 4) as usize]
    }

    /// `Direction.fromYRot(double)` — `from2DDataValue(Mth.floor(yRot / 90.0 +
    /// 0.5) & 3)`.
    pub fn from_y_rot(y_rot: f64) -> Direction {
        Direction::from_2d_data_value(mth::floor_d(y_rot / 90.0 + 0.5) & 3)
    }

    /// `Direction.fromAxisAndDirection(Axis, AxisDirection)`.
    pub fn from_axis_and_direction(axis: Axis, direction: AxisDirection) -> Direction {
        match axis {
            Axis::X => {
                if direction == AxisDirection::Positive {
                    Direction::East
                } else {
                    Direction::West
                }
            }
            Axis::Y => {
                if direction == AxisDirection::Positive {
                    Direction::Up
                } else {
                    Direction::Down
                }
            }
            Axis::Z => {
                if direction == AxisDirection::Positive {
                    Direction::South
                } else {
                    Direction::North
                }
            }
        }
    }

    /// `Direction.getYRot(Direction)` — horizontal directions only; vertical
    /// directions throw in Java.
    pub fn get_y_rot(direction: Direction) -> f32 {
        match direction {
            Direction::North => 180.0,
            Direction::South => 0.0,
            Direction::West => 90.0,
            Direction::East => -90.0,
            Direction::Down | Direction::Up => panic!("No y-Rot for vertical axis: {direction}"),
        }
    }

    /// `Direction.toYRot()` — `(data2d & 3) * 90`.
    pub fn to_y_rot(&self) -> f32 {
        (data_2d(*self) & 3) as f32 * 90.0
    }

    /// `Direction.getClockWise(Direction.Axis)` (Rust cannot overload the
    /// no-arg `getClockWise()`, so the axis overload is `get_clock_wise_axis`).
    pub fn get_clock_wise_axis(&self, axis: Axis) -> Direction {
        match axis {
            Axis::X => {
                if *self != Direction::West && *self != Direction::East {
                    self.get_clock_wise_x()
                } else {
                    *self
                }
            }
            Axis::Y => {
                if *self != Direction::Up && *self != Direction::Down {
                    self.get_clock_wise()
                } else {
                    *self
                }
            }
            Axis::Z => {
                if *self != Direction::North && *self != Direction::South {
                    self.get_clock_wise_z()
                } else {
                    *self
                }
            }
        }
    }

    /// `Direction.getCounterClockWise(Direction.Axis)`.
    pub fn get_counter_clock_wise_axis(&self, axis: Axis) -> Direction {
        match axis {
            Axis::X => {
                if *self != Direction::West && *self != Direction::East {
                    self.get_counter_clock_wise_x()
                } else {
                    *self
                }
            }
            Axis::Y => {
                if *self != Direction::Up && *self != Direction::Down {
                    self.get_counter_clock_wise()
                } else {
                    *self
                }
            }
            Axis::Z => {
                if *self != Direction::North && *self != Direction::South {
                    self.get_counter_clock_wise_z()
                } else {
                    *self
                }
            }
        }
    }

    /// `Direction.getClockWise()` — rotation around the Y axis.
    pub fn get_clock_wise(&self) -> Direction {
        match self {
            Direction::North => Direction::East,
            Direction::South => Direction::West,
            Direction::West => Direction::North,
            Direction::East => Direction::South,
            Direction::Down | Direction::Up => panic!("Unable to get Y-rotated facing of {self}"),
        }
    }

    /// `Direction.getCounterClockWise()`.
    pub fn get_counter_clock_wise(&self) -> Direction {
        match self {
            Direction::North => Direction::West,
            Direction::South => Direction::East,
            Direction::West => Direction::South,
            Direction::East => Direction::North,
            Direction::Down | Direction::Up => panic!("Unable to get CCW facing of {self}"),
        }
    }

    /// `Direction.getClockWiseX()`.
    pub fn get_clock_wise_x(&self) -> Direction {
        match self {
            Direction::Down => Direction::South,
            Direction::Up => Direction::North,
            Direction::North => Direction::Down,
            Direction::South => Direction::Up,
            Direction::West | Direction::East => panic!("Unable to get X-rotated facing of {self}"),
        }
    }

    /// `Direction.getCounterClockWiseX()`.
    pub fn get_counter_clock_wise_x(&self) -> Direction {
        match self {
            Direction::Down => Direction::North,
            Direction::Up => Direction::South,
            Direction::North => Direction::Up,
            Direction::South => Direction::Down,
            Direction::West | Direction::East => panic!("Unable to get X-rotated facing of {self}"),
        }
    }

    /// `Direction.getClockWiseZ()`.
    pub fn get_clock_wise_z(&self) -> Direction {
        match self {
            Direction::Down => Direction::West,
            Direction::Up => Direction::East,
            Direction::West => Direction::Up,
            Direction::East => Direction::Down,
            Direction::North | Direction::South => {
                panic!("Unable to get Z-rotated facing of {self}")
            }
        }
    }

    /// `Direction.getCounterClockWiseZ()`.
    pub fn get_counter_clock_wise_z(&self) -> Direction {
        match self {
            Direction::Down => Direction::East,
            Direction::Up => Direction::West,
            Direction::West => Direction::Down,
            Direction::East => Direction::Up,
            Direction::North | Direction::South => {
                panic!("Unable to get Z-rotated facing of {self}")
            }
        }
    }

    /// `Direction.getNearest(int, int, int, @Nullable Direction)` — strict
    /// `>` comparisons; returns `or_else` when no axis dominates.
    pub fn get_nearest(x: i32, y: i32, z: i32, or_else: Option<Direction>) -> Option<Direction> {
        let abs_x = x.wrapping_abs();
        let abs_y = y.wrapping_abs();
        let abs_z = z.wrapping_abs();
        if abs_x > abs_z && abs_x > abs_y {
            Some(if x < 0 {
                Direction::West
            } else {
                Direction::East
            })
        } else if abs_z > abs_x && abs_z > abs_y {
            Some(if z < 0 {
                Direction::North
            } else {
                Direction::South
            })
        } else if abs_y > abs_x && abs_y > abs_z {
            Some(if y < 0 {
                Direction::Down
            } else {
                Direction::Up
            })
        } else {
            or_else
        }
    }

    /// `Direction.getNearest(Vec3i, @Nullable Direction)`.
    pub fn get_nearest_vec3i(vec: &Vec3i, or_else: Option<Direction>) -> Option<Direction> {
        Direction::get_nearest(vec.get_x(), vec.get_y(), vec.get_z(), or_else)
    }

    /// `Direction.getApproximateNearest(float, float, float)` — maximum dot
    /// product over `values()`; ties go to the first direction (enum order).
    pub fn get_approximate_nearest_f32(dx: f32, dy: f32, dz: f32) -> Direction {
        let mut result = Direction::North;
        // Java `Float.MIN_VALUE` is the smallest positive subnormal
        // (~1.4e-45), *not* `f32::MIN` (most negative). For a zero input the
        // first `dot > highestDot` never fires, so `NORTH` (the seed) wins.
        let mut highest_dot = f32::from_bits(1);
        for direction in BY_3D_DATA {
            let dot = dx * direction.step_x() as f32
                + dy * direction.step_y() as f32
                + dz * direction.step_z() as f32;
            if dot > highest_dot {
                highest_dot = dot;
                result = direction;
            }
        }
        result
    }

    /// `Direction.getApproximateNearest(double, double, double)` — the double
    /// overload casts to float first.
    pub fn get_approximate_nearest(dx: f64, dy: f64, dz: f64) -> Direction {
        Direction::get_approximate_nearest_f32(dx as f32, dy as f32, dz as f32)
    }

    /// `Direction.get(AxisDirection, Axis)`.
    pub fn get(axis_direction: AxisDirection, axis: Axis) -> Direction {
        for direction in BY_3D_DATA {
            if direction.get_axis_direction() == axis_direction && direction.get_axis() == axis {
                return direction;
            }
        }
        panic!("No such direction: {axis_direction} {axis}")
    }

    /// `Direction.isFacingAngle(float)` — `normal · (sin/cos of yAngle) > 0`.
    pub fn is_facing_angle(&self, y_angle: f32) -> bool {
        let radians = y_angle * mth::DEG_TO_RAD;
        let dx = -mth::sin(radians as f64);
        let dz = mth::cos(radians as f64);
        self.step_x() as f32 * dx + self.step_z() as f32 * dz > 0.0
    }

    /// `Direction.Axis.test(Direction)` — `input.getAxis() == this`.
    pub fn is_on_axis(&self, axis: Axis) -> bool {
        self.get_axis() == axis
    }
}

impl std::fmt::Display for Direction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.get_name())
    }
}

/// `Direction.CODEC` — `StringRepresentable.fromEnum(Direction::values)`, the
/// `orCompressed(Codec.stringResolver, ExtraCodecs.idResolverCodec)` form, as
/// the ops-generic `direction_codec::<Ops>()` factory.
///
/// The string resolver maps the enum's `getSerializedName()` (lowercase name)
/// to the variant; the compressed branch routes through the ordinal id codec
/// when `ops.compress_maps()` (the network/NBT compressed form).
pub fn direction_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Direction, Ops>> {
    let by_name = codec::string_resolver(
        Arc::new(|d: &Direction| Some(d.get_serialized_name().to_string())),
        Arc::new(|name: &String| direction_by_serialized_name(name)),
    );
    extra_codecs::or_compressed(
        by_name,
        extra_codecs::id_resolver_codec(
            Arc::new(|d: &Direction| *d as i32),
            Arc::new(|id: i32| {
                if id >= 0 && (id as usize) < Direction::VALUES.len() {
                    Some(Direction::VALUES[id as usize])
                } else {
                    None
                }
            }),
            -1,
        ),
    )
}

/// `StringRepresentable.byName(String)` — resolve the enum's serialized name
/// to its variant (Java `EnumCodec.byName`).
pub fn direction_by_serialized_name(name: &str) -> Option<Direction> {
    Direction::VALUES
        .iter()
        .find(|d| d.get_serialized_name() == name)
        .copied()
}

/// `Direction.Axis` — an axis-aligned axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    pub const VALUES: [Axis; 3] = [Axis::X, Axis::Y, Axis::Z];

    /// `Axis.getName()`.
    pub fn get_name(&self) -> &'static str {
        match self {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }
    }

    /// `Axis.isVertical()`.
    pub fn is_vertical(&self) -> bool {
        *self == Axis::Y
    }

    /// `Axis.isHorizontal()`.
    pub fn is_horizontal(&self) -> bool {
        *self == Axis::X || *self == Axis::Z
    }

    /// `Axis.getPositive()`.
    pub fn get_positive(&self) -> Direction {
        match self {
            Axis::X => Direction::East,
            Axis::Y => Direction::Up,
            Axis::Z => Direction::South,
        }
    }

    /// `Axis.getNegative()`.
    pub fn get_negative(&self) -> Direction {
        match self {
            Axis::X => Direction::West,
            Axis::Y => Direction::Down,
            Axis::Z => Direction::North,
        }
    }

    /// `Axis.getDirections()` — `[positive, negative]`.
    pub fn get_directions(&self) -> [Direction; 2] {
        [self.get_positive(), self.get_negative()]
    }

    /// `Axis.getPlane()`.
    pub fn get_plane(&self) -> Plane {
        match self {
            Axis::X | Axis::Z => Plane::Horizontal,
            Axis::Y => Plane::Vertical,
        }
    }

    /// `Axis.choose(int, int, int)`.
    pub fn choose(&self, x: i32, y: i32, z: i32) -> i32 {
        match self {
            Axis::X => x,
            Axis::Y => y,
            Axis::Z => z,
        }
    }

    /// `Axis.choose(double, double, double)`.
    pub fn choose_f64(&self, x: f64, y: f64, z: f64) -> f64 {
        match self {
            Axis::X => x,
            Axis::Y => y,
            Axis::Z => z,
        }
    }

    /// `Axis.choose(boolean, boolean, boolean)`.
    pub fn choose_bool(&self, x: bool, y: bool, z: bool) -> bool {
        match self {
            Axis::X => x,
            Axis::Y => y,
            Axis::Z => z,
        }
    }
}

impl std::fmt::Display for Axis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.get_name())
    }
}

/// `Direction.AxisDirection` — positive/negative along an axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AxisDirection {
    Positive,
    Negative,
}

impl AxisDirection {
    /// `AxisDirection.getStep()` — `POSITIVE=1, NEGATIVE=-1`.
    pub fn get_step(&self) -> i32 {
        match self {
            AxisDirection::Positive => 1,
            AxisDirection::Negative => -1,
        }
    }

    /// `AxisDirection.getName()`.
    pub fn get_name(&self) -> &'static str {
        match self {
            AxisDirection::Positive => "Towards positive",
            AxisDirection::Negative => "Towards negative",
        }
    }

    /// `AxisDirection.opposite()`.
    pub fn opposite(&self) -> AxisDirection {
        match self {
            AxisDirection::Positive => AxisDirection::Negative,
            AxisDirection::Negative => AxisDirection::Positive,
        }
    }
}

impl std::fmt::Display for AxisDirection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.get_name())
    }
}

/// `Direction.Plane` — the horizontal or vertical set of directions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Plane {
    Horizontal,
    Vertical,
}

impl Plane {
    /// The `faces` array (`NORTH, EAST, SOUTH, WEST` / `UP, DOWN`).
    pub fn faces(&self) -> &'static [Direction] {
        match self {
            Plane::Horizontal => &[
                Direction::North,
                Direction::East,
                Direction::South,
                Direction::West,
            ],
            Plane::Vertical => &[Direction::Up, Direction::Down],
        }
    }

    /// The `axis` array (`X, Z` / `Y`).
    pub fn axis(&self) -> &'static [Axis] {
        match self {
            Plane::Horizontal => &[Axis::X, Axis::Z],
            Plane::Vertical => &[Axis::Y],
        }
    }

    /// `Plane.test(Direction)` — `input.getAxis().getPlane() == this`.
    pub fn test(&self, direction: Option<Direction>) -> bool {
        match direction {
            Some(d) => d.get_axis().get_plane() == *self,
            None => false,
        }
    }

    /// `Plane.length()`.
    pub fn length(&self) -> usize {
        self.faces().len()
    }

    /// `Plane.stream()`.
    pub fn stream(&self) -> Vec<Direction> {
        self.faces().to_vec()
    }
}

impl std::fmt::Display for Plane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Plane::Horizontal => "HORIZONTAL",
            Plane::Vertical => "VERTICAL",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    #[test]
    fn codec_round_trips_and_resolves_by_serialized_name() {
        let codec = direction_codec::<JsonOps>();
        for d in Direction::VALUES {
            let encoded = codec
                .encode_start(&JsonOps::INSTANCE, &d)
                .result()
                .expect("encode should succeed")
                .clone();
            assert_eq!(encoded, json!(d.get_serialized_name()));
            let decoded = codec
                .parse(&JsonOps::INSTANCE, &encoded)
                .result()
                .expect("decode should succeed")
                .clone();
            assert_eq!(decoded, d);
        }
    }

    #[test]
    fn codec_rejects_unknown_name() {
        let codec = direction_codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!("upward"));
        assert!(result.is_error());
        assert_eq!(direction_by_serialized_name("upward"), None);
    }

    #[test]
    fn by_serialized_name_matches_get_name() {
        for d in Direction::VALUES {
            assert_eq!(
                direction_by_serialized_name(d.get_serialized_name()),
                Some(d)
            );
            assert_eq!(d.get_serialized_name(), d.get_name());
        }
    }
}
