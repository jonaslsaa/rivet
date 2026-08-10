//! `net.minecraft.world.level.block.state.properties` — the typed leaf value
//! classes and the `BlockStateProperties` constant facade (issue #228), the
//! worldgen-reachable subset.
//!
//! The three `Property<T>` classes (`BooleanProperty`/`IntegerProperty`/
//! `EnumProperty`) already collapse into the id-keyed [`Property`] in
//! `block_state_property.rs`; the generated tables carry every property's
//! ordered value set, so the remaining gap is the **typed value classes** —
//! the `StringRepresentable` enums worldgen/lighting/structure pieces set on
//! states (`state.setValue(SlabBlock.TYPE, SlabType.DOUBLE)`) and the named
//! `BlockStateProperties` constants the feature/carver/foliage code references
//! (`state.hasProperty(BlockStateProperties.WATERLOGGED)`, etc.).
//!
//! ## Boundary (chosen from actual Java imports)
//!
//! `working/Paper/.../world/level/block/state/properties/*.java` has 33
//! classes. The ten enums here are the ones `levelgen`/`lighting`/structure
//! pieces import directly (grep over `levelgen/` + `lighting/`):
//! `DoubleBlockHalf`, `Half`, `SlabType`, `AttachFace`, `RailShape`,
//! `RedstoneSide`, `StairsShape`, `SpeleothemThickness`, `BambooLeaves`,
//! `CreakingHeartState`. Each is a pure `StringRepresentable` leaf (no
//! `Block`/`BlockState` dependency — only `StringRepresentable`, and
//! `DoubleBlockHalf`'s `Direction`), so they port without dragging the full
//! block SCC. The facade constants included are exactly those whose value
//! class is already ported (Boolean/Integer/`Direction`/`Direction.Axis`/the
//! ten enums). The worldgen/lighting code references these constants directly
//! (`BlockStateProperties.WATERLOGGED` 11x, `FACING` 2x, `PERSISTENT`,
//! `DISTANCE`).
//!
//! RivetTodo(#228): the remaining `block.state.properties` classes are
//! deferred with the full unit — `BlockStateProperties` constants whose value
//! class is not yet ported (`ORIENTATION`/`FrontAndTop`,
//! `COPPER_GOLEM_POSE`/`CopperGolemStatueBlock.Pose`,
//! `TRIAL_SPAWNER_STATE`/`TrialSpawnerState`, `VAULT_STATE`/`VaultState`),
//! plus the un-ported leaf enums and their constants (`BellAttachType`,
//! `WallSide`, `SideChainPart`, `BedPart`, `ChestType`, `ComparatorMode`,
//! `DoorHingeSide`, `NoteBlockInstrument`, `PistonType`, `Tilt`,
//! `SculkSensorPhase`, `StructureMode`, `TestBlockMode`, `PotentSulfurState`,
//! `WoodType`, `BlockSetType`), and `EnumProperty`'s filtered-constructor
//! variants whose value set is not already a generated id: `FACING_HOPPER`'s
//! `facing` = `Facing3` ([down, north, south, west, east]) and
//! `VERTICAL_DIRECTION`'s `vertical_direction` = `VerticalDirection` ([up,
//! down]) ARE representable and ported; `RAIL_SHAPE_STRAIGHT`'s four-shape
//! filter is deferred with the full unit. Three Java constants have **no
//! generated id at all** because no 26.2 *block* registers their property
//! (`FALLING`, `MAP`) or their exact value range (`LEVEL_FLOWING`'s `level`
//! 1..=8; water/lava use `level` 0..=15), so they are omitted rather than
//! fabricated. The `material` fluid surface (`Fluid`/`FluidState`/`Fluids`)
//! stays deferred too — the heightmap `has_fluid` predicate is already served
//! by the behavior word's `fluid_empty` bit, so no worldgen slice needs the
//! fluid classes yet.
//!
//! ## Placement
//!
//! These are pure value types of `block.state.properties` (the MANIFEST unit
//! targets `rivet-world`), but `Property`/`BlockState` already live in
//! `rivet-registry` (OWNERSHIP.md §Registries — pure value types resolve by
//! id, no world dependency), and `DoubleBlockHalf` needs `core::Direction`.
//! The enums + facade stay in `rivet-registry` behind the `blocks` feature
//! like the tables they decode; `rivet-world` worldgen reads them through this
//! crate.

use crate::block_state_property::{Property, PropertyValue};
use crate::core::Direction;
use crate::generated::block_properties::BlockPropertyId;
use rivet_util::string_representable::StringRepresentable;

// ---------------------------------------------------------------------------
// The ten worldgen-reachable leaf value enums
// ---------------------------------------------------------------------------

/// `DoubleBlockHalf` — the `half` value class of two-block-tall blocks
/// (`DoorBlock.HALF`, `TallFlowerBlock.HALF`, …). `UPPER`/`LOWER` with
/// `Direction`-carrying `getDirectionToOther`/`getOtherHalf`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DoubleBlockHalf {
    /// `UPPER` — the top half; `directionToOther = Direction.DOWN`.
    Upper,
    /// `LOWER` — the bottom half; `directionToOther = Direction.UP`.
    Lower,
}

impl DoubleBlockHalf {
    /// `getDirectionToOther()` — the direction from this half to the other.
    pub const fn get_direction_to_other(&self) -> Direction {
        match self {
            DoubleBlockHalf::Upper => Direction::Down,
            DoubleBlockHalf::Lower => Direction::Up,
        }
    }

    /// `getOtherHalf()` — `UPPER <-> LOWER`.
    pub const fn get_other_half(&self) -> DoubleBlockHalf {
        match self {
            DoubleBlockHalf::Upper => DoubleBlockHalf::Lower,
            DoubleBlockHalf::Lower => DoubleBlockHalf::Upper,
        }
    }
}

impl StringRepresentable for DoubleBlockHalf {
    fn get_serialized_name(&self) -> &str {
        match self {
            DoubleBlockHalf::Upper => "upper",
            DoubleBlockHalf::Lower => "lower",
        }
    }
}

/// `Half` — the `half` value class of stair/slab-ish single-block halves
/// (`StairBlock.HALF`: `TOP`/`BOTTOM`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Half {
    /// `TOP`.
    Top,
    /// `BOTTOM`.
    Bottom,
}

impl StringRepresentable for Half {
    fn get_serialized_name(&self) -> &str {
        match self {
            Half::Top => "top",
            Half::Bottom => "bottom",
        }
    }
}

/// `SlabType` — the `type` value class of slabs (`SlabBlock.TYPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlabType {
    /// `TOP`.
    Top,
    /// `BOTTOM`.
    Bottom,
    /// `DOUBLE`.
    Double,
}

impl StringRepresentable for SlabType {
    fn get_serialized_name(&self) -> &str {
        match self {
            SlabType::Top => "top",
            SlabType::Bottom => "bottom",
            SlabType::Double => "double",
        }
    }
}

/// `AttachFace` — the `face` value class of wall-mounted blocks (`LeverBlock
/// .FACE`, buttons, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttachFace {
    /// `FLOOR`.
    Floor,
    /// `WALL`.
    Wall,
    /// `CEILING`.
    Ceiling,
}

impl StringRepresentable for AttachFace {
    fn get_serialized_name(&self) -> &str {
        match self {
            AttachFace::Floor => "floor",
            AttachFace::Wall => "wall",
            AttachFace::Ceiling => "ceiling",
        }
    }
}

/// `RailShape` — the `shape` value class of rails (`RailBlock.SHAPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RailShape {
    /// `NORTH_SOUTH`.
    NorthSouth,
    /// `EAST_WEST`.
    EastWest,
    /// `ASCENDING_EAST`.
    AscendingEast,
    /// `ASCENDING_WEST`.
    AscendingWest,
    /// `ASCENDING_NORTH`.
    AscendingNorth,
    /// `ASCENDING_SOUTH`.
    AscendingSouth,
    /// `SOUTH_EAST`.
    SouthEast,
    /// `SOUTH_WEST`.
    SouthWest,
    /// `NORTH_WEST`.
    NorthWest,
    /// `NORTH_EAST`.
    NorthEast,
}

impl RailShape {
    /// `isSlope()` — one of the four ascending shapes.
    pub const fn is_slope(&self) -> bool {
        matches!(
            self,
            RailShape::AscendingNorth
                | RailShape::AscendingEast
                | RailShape::AscendingSouth
                | RailShape::AscendingWest
        )
    }
}

impl StringRepresentable for RailShape {
    fn get_serialized_name(&self) -> &str {
        match self {
            RailShape::NorthSouth => "north_south",
            RailShape::EastWest => "east_west",
            RailShape::AscendingEast => "ascending_east",
            RailShape::AscendingWest => "ascending_west",
            RailShape::AscendingNorth => "ascending_north",
            RailShape::AscendingSouth => "ascending_south",
            RailShape::SouthEast => "south_east",
            RailShape::SouthWest => "south_west",
            RailShape::NorthWest => "north_west",
            RailShape::NorthEast => "north_east",
        }
    }
}

/// `RedstoneSide` — the per-direction value class of redstone wire
/// (`RedStoneWireBlock.NORTH/SOUTH/EAST/WEST`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RedstoneSide {
    /// `UP`.
    Up,
    /// `SIDE`.
    Side,
    /// `NONE`.
    None,
}

impl RedstoneSide {
    /// `isConnected()` — `this != NONE`.
    pub const fn is_connected(&self) -> bool {
        !matches!(self, RedstoneSide::None)
    }
}

impl StringRepresentable for RedstoneSide {
    fn get_serialized_name(&self) -> &str {
        match self {
            RedstoneSide::Up => "up",
            RedstoneSide::Side => "side",
            RedstoneSide::None => "none",
        }
    }
}

/// `StairsShape` — the `shape` value class of stairs (`StairBlock.SHAPE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StairsShape {
    /// `STRAIGHT`.
    Straight,
    /// `INNER_LEFT`.
    InnerLeft,
    /// `INNER_RIGHT`.
    InnerRight,
    /// `OUTER_LEFT`.
    OuterLeft,
    /// `OUTER_RIGHT`.
    OuterRight,
}

impl StringRepresentable for StairsShape {
    fn get_serialized_name(&self) -> &str {
        match self {
            StairsShape::Straight => "straight",
            StairsShape::InnerLeft => "inner_left",
            StairsShape::InnerRight => "inner_right",
            StairsShape::OuterLeft => "outer_left",
            StairsShape::OuterRight => "outer_right",
        }
    }
}

/// `SpeleothemThickness` — the `thickness` value class of pointed dripstone
/// (`PointedDripstoneBlock.THICKNESS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpeleothemThickness {
    /// `TIP_MERGE`.
    TipMerge,
    /// `TIP`.
    Tip,
    /// `FRUSTUM`.
    Frustum,
    /// `MIDDLE`.
    Middle,
    /// `BASE`.
    Base,
}

impl StringRepresentable for SpeleothemThickness {
    fn get_serialized_name(&self) -> &str {
        match self {
            SpeleothemThickness::TipMerge => "tip_merge",
            SpeleothemThickness::Tip => "tip",
            SpeleothemThickness::Frustum => "frustum",
            SpeleothemThickness::Middle => "middle",
            SpeleothemThickness::Base => "base",
        }
    }
}

/// `BambooLeaves` — the `leaves` value class of bamboo stalks
/// (`BambooStalkBlock.LEAVES`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BambooLeaves {
    /// `NONE`.
    None,
    /// `SMALL`.
    Small,
    /// `LARGE`.
    Large,
}

impl StringRepresentable for BambooLeaves {
    fn get_serialized_name(&self) -> &str {
        match self {
            BambooLeaves::None => "none",
            BambooLeaves::Small => "small",
            BambooLeaves::Large => "large",
        }
    }
}

/// `CreakingHeartState` — the `creaking_heart_state` value class of the
/// creaking heart block (`CreakingHeartBlock.STATE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CreakingHeartState {
    /// `UPROOTED`.
    Uprooted,
    /// `DORMANT`.
    Dormant,
    /// `AWAKE`.
    Awake,
}

impl StringRepresentable for CreakingHeartState {
    fn get_serialized_name(&self) -> &str {
        match self {
            CreakingHeartState::Uprooted => "uprooted",
            CreakingHeartState::Dormant => "dormant",
            CreakingHeartState::Awake => "awake",
        }
    }
}

// ---------------------------------------------------------------------------
// From<leaf enum> for PropertyValue — connect the typed values to BlockState
// ---------------------------------------------------------------------------

// Each leaf enum is its property's typed value class (`state.setValue(
// SlabBlock.TYPE, SlabType.DOUBLE)` in Java). The `From` impls let callers pass
// the enum directly to the typed `BlockState::set_value`/`try_set_value`
// helpers instead of hand-writing the `PropertyValue::Enum("...")` string.

macro_rules! impl_from_leaf_enum {
    ($($enum:ident => { $($variant:ident => $name:literal),+ $(,)? }),+ $(,)?) => {
        $(
            impl From<$enum> for PropertyValue {
                fn from(value: $enum) -> PropertyValue {
                    match value {
                        $($enum::$variant => PropertyValue::Enum($name),)+
                    }
                }
            }
        )+
    };
}

impl_from_leaf_enum! {
    DoubleBlockHalf => { Upper => "upper", Lower => "lower" },
    Half => { Top => "top", Bottom => "bottom" },
    SlabType => { Top => "top", Bottom => "bottom", Double => "double" },
    AttachFace => { Floor => "floor", Wall => "wall", Ceiling => "ceiling" },
    RailShape => {
        NorthSouth => "north_south", EastWest => "east_west",
        AscendingEast => "ascending_east", AscendingWest => "ascending_west",
        AscendingNorth => "ascending_north", AscendingSouth => "ascending_south",
        SouthEast => "south_east", SouthWest => "south_west",
        NorthWest => "north_west", NorthEast => "north_east"
    },
    RedstoneSide => { Up => "up", Side => "side", None => "none" },
    StairsShape => {
        Straight => "straight", InnerLeft => "inner_left",
        InnerRight => "inner_right", OuterLeft => "outer_left", OuterRight => "outer_right"
    },
    SpeleothemThickness => {
        TipMerge => "tip_merge", Tip => "tip",
        Frustum => "frustum", Middle => "middle", Base => "base"
    },
    BambooLeaves => { None => "none", Small => "small", Large => "large" },
    CreakingHeartState => { Uprooted => "uprooted", Dormant => "dormant", Awake => "awake" },
}

// ---------------------------------------------------------------------------
// BlockStateProperties — the named constant facade
// ---------------------------------------------------------------------------

/// `BlockStateProperties` — the named property constants worldgen/lighting
/// reference (`hasProperty(BlockStateProperties.WATERLOGGED)`, `setValue(
/// BlockStateProperties.FACING, direction)`, …), as [`Property`] handles into
/// the generated property table.
///
/// Every constant here keeps its Java serialized name and ordered value set
/// (`Property::from_id` reads the generated `BLOCK_PROPERTY_VALUES` slice —
/// the validated Java `Block` state definitions). The set is exactly the
/// constants whose value class is ported (Boolean / Integer / `Direction` /
/// `Direction.Axis` / the ten leaf enums above); the rest are deferred (see the
/// module `RivetTodo(#228)`).
pub struct BlockStateProperties;

impl BlockStateProperties {
    // --- Boolean value class ------------------------------------------------

    /// `ATTACHED` (`attached`).
    pub const ATTACHED: Property = Property::from_id(BlockPropertyId::Attached);
    /// `BERRIES` (`berries`).
    pub const BERRIES: Property = Property::from_id(BlockPropertyId::Berries);
    /// `BLOOM` (`bloom`).
    pub const BLOOM: Property = Property::from_id(BlockPropertyId::Bloom);
    /// `BOTTOM` (`bottom`).
    pub const BOTTOM: Property = Property::from_id(BlockPropertyId::Bottom);
    /// `CAN_SUMMON` (`can_summon`).
    pub const CAN_SUMMON: Property = Property::from_id(BlockPropertyId::CanSummon);
    /// `CONDITIONAL` (`conditional`).
    pub const CONDITIONAL: Property = Property::from_id(BlockPropertyId::Conditional);
    /// `DISARMED` (`disarmed`).
    pub const DISARMED: Property = Property::from_id(BlockPropertyId::Disarmed);
    /// `DRAG` (`drag`).
    pub const DRAG: Property = Property::from_id(BlockPropertyId::Drag);
    /// `ENABLED` (`enabled`).
    pub const ENABLED: Property = Property::from_id(BlockPropertyId::Enabled);
    /// `EXTENDED` (`extended`).
    pub const EXTENDED: Property = Property::from_id(BlockPropertyId::Extended);
    /// `EYE` (`eye`).
    pub const EYE: Property = Property::from_id(BlockPropertyId::Eye);
    // `FALLING` (`falling`) is deliberately absent: no 26.2 block registers a
    // `falling` property, so there is no generated id to map it to. It is
    // deferred with the full unit.
    /// `HANGING` (`hanging`).
    pub const HANGING: Property = Property::from_id(BlockPropertyId::Hanging);
    /// `HAS_BOTTLE_0` (`has_bottle_0`).
    pub const HAS_BOTTLE_0: Property = Property::from_id(BlockPropertyId::HasBottle0);
    /// `HAS_BOTTLE_1` (`has_bottle_1`).
    pub const HAS_BOTTLE_1: Property = Property::from_id(BlockPropertyId::HasBottle1);
    /// `HAS_BOTTLE_2` (`has_bottle_2`).
    pub const HAS_BOTTLE_2: Property = Property::from_id(BlockPropertyId::HasBottle2);
    /// `HAS_RECORD` (`has_record`).
    pub const HAS_RECORD: Property = Property::from_id(BlockPropertyId::HasRecord);
    /// `HAS_BOOK` (`has_book`).
    pub const HAS_BOOK: Property = Property::from_id(BlockPropertyId::HasBook);
    /// `INVERTED` (`inverted`).
    pub const INVERTED: Property = Property::from_id(BlockPropertyId::Inverted);
    /// `IN_WALL` (`in_wall`).
    pub const IN_WALL: Property = Property::from_id(BlockPropertyId::InWall);
    /// `LIT` (`lit`).
    pub const LIT: Property = Property::from_id(BlockPropertyId::Lit);
    /// `LOCKED` (`locked`).
    pub const LOCKED: Property = Property::from_id(BlockPropertyId::Locked);
    /// `NATURAL` (`natural`).
    pub const NATURAL: Property = Property::from_id(BlockPropertyId::Natural);
    /// `OCCUPIED` (`occupied`).
    pub const OCCUPIED: Property = Property::from_id(BlockPropertyId::Occupied);
    /// `OPEN` (`open`).
    pub const OPEN: Property = Property::from_id(BlockPropertyId::Open);
    /// `PERSISTENT` (`persistent`) — consumed by `FoliagePlacer`.
    pub const PERSISTENT: Property = Property::from_id(BlockPropertyId::Persistent);
    /// `POWERED` (`powered`).
    pub const POWERED: Property = Property::from_id(BlockPropertyId::Powered);
    /// `SHORT` (`short`).
    pub const SHORT: Property = Property::from_id(BlockPropertyId::Short);
    /// `SHRIEKING` (`shrieking`).
    pub const SHRIEKING: Property = Property::from_id(BlockPropertyId::Shrieking);
    /// `SIGNAL_FIRE` (`signal_fire`).
    pub const SIGNAL_FIRE: Property = Property::from_id(BlockPropertyId::SignalFire);
    /// `SNOWY` (`snowy`).
    pub const SNOWY: Property = Property::from_id(BlockPropertyId::Snowy);
    /// `TIP` (`tip`).
    pub const TIP: Property = Property::from_id(BlockPropertyId::Tip);
    /// `TRIGGERED` (`triggered`).
    pub const TRIGGERED: Property = Property::from_id(BlockPropertyId::Triggered);
    /// `UNSTABLE` (`unstable`).
    pub const UNSTABLE: Property = Property::from_id(BlockPropertyId::Unstable);
    /// `WATERLOGGED` (`waterlogged`) — the most worldgen-referenced constant
    /// (carvers, `WaterloggedVegetationPatchFeature`, `RootPlacer`,
    /// `FoliagePlacer`, `GeodeFeature`, …).
    pub const WATERLOGGED: Property = Property::from_id(BlockPropertyId::Waterlogged);
    /// `UP` (`up`).
    pub const UP: Property = Property::from_id(BlockPropertyId::Up);
    /// `DOWN` (`down`).
    pub const DOWN: Property = Property::from_id(BlockPropertyId::Down);
    /// `NORTH` (`north`).
    pub const NORTH: Property = Property::from_id(BlockPropertyId::North);
    /// `EAST` (`east`).
    pub const EAST: Property = Property::from_id(BlockPropertyId::East);
    /// `SOUTH` (`south`).
    pub const SOUTH: Property = Property::from_id(BlockPropertyId::South);
    /// `WEST` (`west`).
    pub const WEST: Property = Property::from_id(BlockPropertyId::West);
    /// `SLOT_0_OCCUPIED` (`slot_0_occupied`).
    pub const SLOT_0_OCCUPIED: Property = Property::from_id(BlockPropertyId::Slot0Occupied);
    /// `SLOT_1_OCCUPIED` (`slot_1_occupied`).
    pub const SLOT_1_OCCUPIED: Property = Property::from_id(BlockPropertyId::Slot1Occupied);
    /// `SLOT_2_OCCUPIED` (`slot_2_occupied`).
    pub const SLOT_2_OCCUPIED: Property = Property::from_id(BlockPropertyId::Slot2Occupied);
    /// `SLOT_3_OCCUPIED` (`slot_3_occupied`).
    pub const SLOT_3_OCCUPIED: Property = Property::from_id(BlockPropertyId::Slot3Occupied);
    /// `SLOT_4_OCCUPIED` (`slot_4_occupied`).
    pub const SLOT_4_OCCUPIED: Property = Property::from_id(BlockPropertyId::Slot4Occupied);
    /// `SLOT_5_OCCUPIED` (`slot_5_occupied`).
    pub const SLOT_5_OCCUPIED: Property = Property::from_id(BlockPropertyId::Slot5Occupied);
    /// `CRACKED` (`cracked`).
    pub const CRACKED: Property = Property::from_id(BlockPropertyId::Cracked);
    /// `CRAFTING` (`crafting`).
    pub const CRAFTING: Property = Property::from_id(BlockPropertyId::Crafting);
    /// `OMINOUS` (`ominous`).
    pub const OMINOUS: Property = Property::from_id(BlockPropertyId::Ominous);
    // `MAP` (`map`) is deliberately absent: no 26.2 block registers a `map`
    // property, so there is no generated id to map it to. It is deferred with
    // the full unit.

    // --- Integer value class ------------------------------------------------

    /// `MAX_AGE_1` (= 1).
    pub const MAX_AGE_1: i32 = 1;
    /// `MAX_AGE_2` (= 2).
    pub const MAX_AGE_2: i32 = 2;
    /// `MAX_AGE_3` (= 3).
    pub const MAX_AGE_3: i32 = 3;
    /// `MAX_AGE_4` (= 4).
    pub const MAX_AGE_4: i32 = 4;
    /// `MAX_AGE_5` (= 5).
    pub const MAX_AGE_5: i32 = 5;
    /// `MAX_AGE_7` (= 7).
    pub const MAX_AGE_7: i32 = 7;
    /// `MAX_AGE_15` (= 15).
    pub const MAX_AGE_15: i32 = 15;
    /// `MAX_AGE_25` (= 25).
    pub const MAX_AGE_25: i32 = 25;
    /// `MAX_DISTANCE` (= 7) — the leaves `distance` max, consumed by
    /// `TreeFeature` (`DISTANCE` set to the smallest distance).
    pub const MAX_DISTANCE: i32 = 7;
    /// `MIN_LEVEL` (= 0).
    pub const MIN_LEVEL: i32 = 0;
    /// `MIN_LEVEL_CAULDRON` (= 1).
    pub const MIN_LEVEL_CAULDRON: i32 = 1;
    /// `MAX_LEVEL_3` (= 3).
    pub const MAX_LEVEL_3: i32 = 3;
    /// `MAX_LEVEL_8` (= 8).
    pub const MAX_LEVEL_8: i32 = 8;
    /// `MAX_LEVEL_15` (= 15).
    pub const MAX_LEVEL_15: i32 = 15;
    /// `STABILITY_MAX_DISTANCE` (= 7).
    pub const STABILITY_MAX_DISTANCE: i32 = 7;
    /// `MIN_RESPAWN_ANCHOR_CHARGES` (= 0).
    pub const MIN_RESPAWN_ANCHOR_CHARGES: i32 = 0;
    /// `MAX_RESPAWN_ANCHOR_CHARGES` (= 4).
    pub const MAX_RESPAWN_ANCHOR_CHARGES: i32 = 4;

    /// `AGE_1` (`age`, 0..=1).
    pub const AGE_1: Property = Property::from_id(BlockPropertyId::Age7);
    /// `AGE_2` (`age`, 0..=2).
    pub const AGE_2: Property = Property::from_id(BlockPropertyId::Age5);
    /// `AGE_3` (`age`, 0..=3).
    pub const AGE_3: Property = Property::from_id(BlockPropertyId::Age4);
    /// `AGE_4` (`age`, 0..=4).
    pub const AGE_4: Property = Property::from_id(BlockPropertyId::Age);
    /// `AGE_5` (`age`, 0..=5).
    pub const AGE_5: Property = Property::from_id(BlockPropertyId::Age6);
    /// `AGE_7` (`age`, 0..=7).
    pub const AGE_7: Property = Property::from_id(BlockPropertyId::Age3);
    /// `AGE_15` (`age`, 0..=15).
    pub const AGE_15: Property = Property::from_id(BlockPropertyId::Age2);
    /// `AGE_25` (`age`, 0..=25).
    pub const AGE_25: Property = Property::from_id(BlockPropertyId::Age8);
    /// `BITES` (`bites`, 0..=6).
    pub const BITES: Property = Property::from_id(BlockPropertyId::Bites);
    /// `CANDLES` (`candles`, 1..=4).
    pub const CANDLES: Property = Property::from_id(BlockPropertyId::Candles);
    /// `DELAY` (`delay`, 1..=4).
    pub const DELAY: Property = Property::from_id(BlockPropertyId::Delay);
    /// `DISTANCE` (`distance`, 1..=7) — consumed by `TreeFeature`.
    pub const DISTANCE: Property = Property::from_id(BlockPropertyId::Distance);
    /// `EGGS` (`eggs`, 1..=4).
    pub const EGGS: Property = Property::from_id(BlockPropertyId::Eggs);
    /// `HATCH` (`hatch`, 0..=2).
    pub const HATCH: Property = Property::from_id(BlockPropertyId::Hatch);
    /// `LAYERS` (`layers`, 1..=8).
    pub const LAYERS: Property = Property::from_id(BlockPropertyId::Layers);
    /// `LEVEL_CAULDRON` (`level`, 1..=3).
    pub const LEVEL_CAULDRON: Property = Property::from_id(BlockPropertyId::Level2);
    /// `LEVEL_COMPOSTER` (`level`, 0..=8).
    pub const LEVEL_COMPOSTER: Property = Property::from_id(BlockPropertyId::Level3);
    // `LEVEL_FLOWING` (`level`, 1..=8) is deliberately absent: it is the
    // flowing-fluid property declared by `BlockStateProperties.java` for the
    // fluid classes, and no 26.2 *block* registers that range (water/lava use
    // `level` 0..=15), so there is no generated id to map it to. It is deferred
    // with the `material` fluid surface.
    /// `LEVEL_HONEY` (`honey_level`, 0..=5).
    pub const LEVEL_HONEY: Property = Property::from_id(BlockPropertyId::HoneyLevel);
    /// `LEVEL` (`level`, 0..=15).
    pub const LEVEL: Property = Property::from_id(BlockPropertyId::Level);
    /// `MOISTURE` (`moisture`, 0..=7).
    pub const MOISTURE: Property = Property::from_id(BlockPropertyId::Moisture);
    /// `NOTE` (`note`, 0..=24).
    pub const NOTE: Property = Property::from_id(BlockPropertyId::Note);
    /// `PICKLES` (`pickles`, 1..=4).
    pub const PICKLES: Property = Property::from_id(BlockPropertyId::Pickles);
    /// `POWER` (`power`, 0..=15).
    pub const POWER: Property = Property::from_id(BlockPropertyId::Power);
    /// `STAGE` (`stage`, 0..=1).
    pub const STAGE: Property = Property::from_id(BlockPropertyId::Stage);
    /// `STABILITY_DISTANCE` (`distance`, 0..=7).
    pub const STABILITY_DISTANCE: Property = Property::from_id(BlockPropertyId::Distance2);
    /// `RESPAWN_ANCHOR_CHARGES` (`charges`, 0..=4).
    pub const RESPAWN_ANCHOR_CHARGES: Property = Property::from_id(BlockPropertyId::Charges);
    /// `DRIED_GHAST_HYDRATION_LEVELS` (`hydration`, 0..=3).
    pub const DRIED_GHAST_HYDRATION_LEVELS: Property =
        Property::from_id(BlockPropertyId::Hydration);
    /// `ROTATION_16` (`rotation`, 0..=15).
    pub const ROTATION_16: Property = Property::from_id(BlockPropertyId::Rotation);
    /// `DUSTED` (`dusted`, 0..=3).
    pub const DUSTED: Property = Property::from_id(BlockPropertyId::Dusted);
    /// `FLOWER_AMOUNT` (`flower_amount`, 1..=4).
    pub const FLOWER_AMOUNT: Property = Property::from_id(BlockPropertyId::FlowerAmount);
    /// `SEGMENT_AMOUNT` (`segment_amount`, 1..=4).
    pub const SEGMENT_AMOUNT: Property = Property::from_id(BlockPropertyId::SegmentAmount);

    // --- Direction / Direction.Axis value classes ---------------------------

    /// `HORIZONTAL_AXIS` (`axis`: x, z).
    pub const HORIZONTAL_AXIS: Property = Property::from_id(BlockPropertyId::Axis2);
    /// `AXIS` (`axis`: x, y, z) — the `RotatedPillarBlock.AXIS` value class.
    pub const AXIS: Property = Property::from_id(BlockPropertyId::Axis);
    /// `FACING` (`facing`: north, east, south, west, up, down) — consumed by
    /// `GeodeFeature` and carvers.
    pub const FACING: Property = Property::from_id(BlockPropertyId::Facing);
    /// `HORIZONTAL_FACING` (`facing`: north, south, west, east).
    pub const HORIZONTAL_FACING: Property = Property::from_id(BlockPropertyId::Facing2);
    /// `FACING_HOPPER` (`facing`: down, north, south, west, east) — the
    /// hopper filter `direction != UP`.
    pub const FACING_HOPPER: Property = Property::from_id(BlockPropertyId::Facing3);
    /// `VERTICAL_DIRECTION` (`vertical_direction`: up, down).
    pub const VERTICAL_DIRECTION: Property = Property::from_id(BlockPropertyId::VerticalDirection);

    // --- the ten leaf-enum value classes ------------------------------------

    /// `DOUBLE_BLOCK_HALF` (`half`: upper, lower).
    pub const DOUBLE_BLOCK_HALF: Property = Property::from_id(BlockPropertyId::Half);
    /// `HALF` (`half`: top, bottom) — the `StairBlock.HALF` value class.
    pub const HALF: Property = Property::from_id(BlockPropertyId::Half2);
    /// `SLAB_TYPE` (`type`: top, bottom, double).
    pub const SLAB_TYPE: Property = Property::from_id(BlockPropertyId::Type3);
    /// `RAIL_SHAPE` (`shape`: the ten rail shapes).
    pub const RAIL_SHAPE: Property = Property::from_id(BlockPropertyId::Shape3);
    /// `STAIRS_SHAPE` (`shape`: straight, inner_left, …).
    pub const STAIRS_SHAPE: Property = Property::from_id(BlockPropertyId::Shape2);
    /// `ATTACH_FACE` (`face`: floor, wall, ceiling).
    pub const ATTACH_FACE: Property = Property::from_id(BlockPropertyId::Face);
    /// `EAST_REDSTONE` (`east`: up, side, none).
    pub const EAST_REDSTONE: Property = Property::from_id(BlockPropertyId::East2);
    /// `NORTH_REDSTONE` (`north`: up, side, none).
    pub const NORTH_REDSTONE: Property = Property::from_id(BlockPropertyId::North2);
    /// `SOUTH_REDSTONE` (`south`: up, side, none).
    pub const SOUTH_REDSTONE: Property = Property::from_id(BlockPropertyId::South2);
    /// `WEST_REDSTONE` (`west`: up, side, none).
    pub const WEST_REDSTONE: Property = Property::from_id(BlockPropertyId::West2);
    /// `SPELEOTHEM_THICKNESS` (`thickness`: tip_merge, tip, frustum, middle,
    /// base).
    pub const SPELEOTHEM_THICKNESS: Property = Property::from_id(BlockPropertyId::Thickness);
    /// `BAMBOO_LEAVES` (`leaves`: none, small, large).
    pub const BAMBOO_LEAVES: Property = Property::from_id(BlockPropertyId::Leaves);
    /// `CREAKING_HEART_STATE` (`creaking_heart_state`: uprooted, dormant,
    /// awake).
    pub const CREAKING_HEART_STATE: Property =
        Property::from_id(BlockPropertyId::CreakingHeartState);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_state_property::PropertyValue;

    /// `Property::get_value` — the property's allowed serialized names, in
    /// declaration order, matching the leaf enum's `get_serialized_name` for
    /// every variant. This is the Paper-grounded name/order check: the values
    /// come from the generated table (the validated Java `Block` state
    /// definitions), and the leaf enums are pinned to the Paper source.
    fn assert_enum_matches_property<E>(
        variants: &[E],
        prop: Property,
        expected: &'static [&'static str],
    ) where
        E: StringRepresentable,
    {
        // The generated property values must be exactly the Paper declaration
        // order the enum encodes.
        assert_eq!(prop.values(), expected, "property `{}`", prop.name());
        for (v, name) in variants.iter().zip(expected) {
            assert_eq!(
                v.get_serialized_name(),
                *name,
                "variant of `{}`",
                prop.name()
            );
            assert_eq!(
                prop.get_value(name),
                Some(PropertyValue::Enum(name)),
                "`{}` value `{name}`",
                prop.name()
            );
        }
        // Every leaf enum variant must be a legal value of its property.
        for v in variants {
            assert!(
                prop.get_value(v.get_serialized_name()).is_some(),
                "`{}` variant `{}` not allowed",
                prop.name(),
                v.get_serialized_name()
            );
        }
    }

    #[test]
    fn double_block_half_matches_paper() {
        assert_enum_matches_property(
            &[DoubleBlockHalf::Upper, DoubleBlockHalf::Lower],
            BlockStateProperties::DOUBLE_BLOCK_HALF,
            &["upper", "lower"],
        );
        // getDirectionToOther / getOtherHalf.
        assert_eq!(
            DoubleBlockHalf::Upper.get_direction_to_other(),
            Direction::Down
        );
        assert_eq!(
            DoubleBlockHalf::Lower.get_direction_to_other(),
            Direction::Up
        );
        assert_eq!(
            DoubleBlockHalf::Upper.get_other_half(),
            DoubleBlockHalf::Lower
        );
        assert_eq!(
            DoubleBlockHalf::Lower.get_other_half(),
            DoubleBlockHalf::Upper
        );
    }

    #[test]
    fn half_matches_paper() {
        assert_enum_matches_property(
            &[Half::Top, Half::Bottom],
            BlockStateProperties::HALF,
            &["top", "bottom"],
        );
    }

    #[test]
    fn slab_type_matches_paper() {
        assert_enum_matches_property(
            &[SlabType::Top, SlabType::Bottom, SlabType::Double],
            BlockStateProperties::SLAB_TYPE,
            &["top", "bottom", "double"],
        );
    }

    #[test]
    fn attach_face_matches_paper() {
        assert_enum_matches_property(
            &[AttachFace::Floor, AttachFace::Wall, AttachFace::Ceiling],
            BlockStateProperties::ATTACH_FACE,
            &["floor", "wall", "ceiling"],
        );
    }

    #[test]
    fn rail_shape_matches_paper() {
        assert_enum_matches_property(
            &[
                RailShape::NorthSouth,
                RailShape::EastWest,
                RailShape::AscendingEast,
                RailShape::AscendingWest,
                RailShape::AscendingNorth,
                RailShape::AscendingSouth,
                RailShape::SouthEast,
                RailShape::SouthWest,
                RailShape::NorthWest,
                RailShape::NorthEast,
            ],
            BlockStateProperties::RAIL_SHAPE,
            &[
                "north_south",
                "east_west",
                "ascending_east",
                "ascending_west",
                "ascending_north",
                "ascending_south",
                "south_east",
                "south_west",
                "north_west",
                "north_east",
            ],
        );
        // isSlope — exactly the four ascending shapes.
        assert!(RailShape::AscendingNorth.is_slope());
        assert!(RailShape::AscendingEast.is_slope());
        assert!(RailShape::AscendingSouth.is_slope());
        assert!(RailShape::AscendingWest.is_slope());
        assert!(!RailShape::NorthSouth.is_slope());
        assert!(!RailShape::SouthEast.is_slope());
    }

    #[test]
    fn redstone_side_matches_paper() {
        assert_enum_matches_property(
            &[RedstoneSide::Up, RedstoneSide::Side, RedstoneSide::None],
            BlockStateProperties::EAST_REDSTONE,
            &["up", "side", "none"],
        );
        // isConnected — `!= NONE`.
        assert!(RedstoneSide::Up.is_connected());
        assert!(RedstoneSide::Side.is_connected());
        assert!(!RedstoneSide::None.is_connected());
    }

    #[test]
    fn stairs_shape_matches_paper() {
        assert_enum_matches_property(
            &[
                StairsShape::Straight,
                StairsShape::InnerLeft,
                StairsShape::InnerRight,
                StairsShape::OuterLeft,
                StairsShape::OuterRight,
            ],
            BlockStateProperties::STAIRS_SHAPE,
            &[
                "straight",
                "inner_left",
                "inner_right",
                "outer_left",
                "outer_right",
            ],
        );
    }

    #[test]
    fn speleothem_thickness_matches_paper() {
        assert_enum_matches_property(
            &[
                SpeleothemThickness::TipMerge,
                SpeleothemThickness::Tip,
                SpeleothemThickness::Frustum,
                SpeleothemThickness::Middle,
                SpeleothemThickness::Base,
            ],
            BlockStateProperties::SPELEOTHEM_THICKNESS,
            &["tip_merge", "tip", "frustum", "middle", "base"],
        );
    }

    #[test]
    fn bamboo_leaves_matches_paper() {
        assert_enum_matches_property(
            &[BambooLeaves::None, BambooLeaves::Small, BambooLeaves::Large],
            BlockStateProperties::BAMBOO_LEAVES,
            &["none", "small", "large"],
        );
    }

    #[test]
    fn creaking_heart_state_matches_paper() {
        assert_enum_matches_property(
            &[
                CreakingHeartState::Uprooted,
                CreakingHeartState::Dormant,
                CreakingHeartState::Awake,
            ],
            BlockStateProperties::CREAKING_HEART_STATE,
            &["uprooted", "dormant", "awake"],
        );
    }

    #[test]
    fn worldgen_consumed_booleans_match_paper() {
        // WATERLOGGED is the most-referenced worldgen constant; PERSISTENT is
        // read by FoliagePlacer. Both are Boolean (`[true, false]`).
        assert_eq!(BlockStateProperties::WATERLOGGED.name(), "waterlogged");
        assert_eq!(
            BlockStateProperties::WATERLOGGED.values(),
            &["true", "false"]
        );
        assert_eq!(BlockStateProperties::PERSISTENT.name(), "persistent");
        assert_eq!(
            BlockStateProperties::PERSISTENT.values(),
            &["true", "false"]
        );
        assert_eq!(
            BlockStateProperties::WATERLOGGED.get_value("true"),
            Some(PropertyValue::Bool(true))
        );
    }

    #[test]
    fn direction_properties_match_paper() {
        // GeodeFeature sets FACING to a Direction; RotatedPillarBlock.AXIS is
        // Direction.Axis (x, y, z).
        assert_eq!(BlockStateProperties::FACING.name(), "facing");
        assert_eq!(
            BlockStateProperties::FACING.values(),
            &["north", "east", "south", "west", "up", "down"]
        );
        assert_eq!(BlockStateProperties::AXIS.values(), &["x", "y", "z"]);
        assert_eq!(BlockStateProperties::HORIZONTAL_AXIS.values(), &["x", "z"]);
        assert_eq!(
            BlockStateProperties::HORIZONTAL_FACING.values(),
            &["north", "south", "west", "east"]
        );
    }

    #[test]
    fn integer_properties_match_paper() {
        // DISTANCE is the leaves property TreeFeature sets; AGE_1..AGE_25
        // cover the crops. Values must be the exact Java `min..=max` ranges.
        assert_eq!(
            BlockStateProperties::DISTANCE.kind(),
            crate::block_state_property::PropertyKind::Int { min: 1, max: 7 }
        );
        assert_eq!(
            BlockStateProperties::MAX_DISTANCE,
            7,
            "TreeFeature clamps to MAX_DISTANCE"
        );
        assert_eq!(BlockStateProperties::POWER.values().len(), 16);
        assert_eq!(BlockStateProperties::NOTE.values().len(), 25);
        assert_eq!(BlockStateProperties::AGE_1.values(), &["0", "1"]);
    }

    /// Every facade constant must resolve to the property name Java's
    /// `BlockStateProperties` declares. This is the cross-check that catches a
    /// constant mapped to the wrong `BlockPropertyId` (the generated enum has
    /// duplicate serialized names — `facing`/`axis`/`level`/`age`/`shape` — so
    /// a compile passing is not enough to prove the mapping).
    #[test]
    fn every_facade_constant_maps_to_its_paper_name() {
        let cases: &[(&str, Property)] = &[
            ("attached", BlockStateProperties::ATTACHED),
            ("berries", BlockStateProperties::BERRIES),
            ("bloom", BlockStateProperties::BLOOM),
            ("bottom", BlockStateProperties::BOTTOM),
            ("can_summon", BlockStateProperties::CAN_SUMMON),
            ("conditional", BlockStateProperties::CONDITIONAL),
            ("disarmed", BlockStateProperties::DISARMED),
            ("drag", BlockStateProperties::DRAG),
            ("enabled", BlockStateProperties::ENABLED),
            ("extended", BlockStateProperties::EXTENDED),
            ("eye", BlockStateProperties::EYE),
            ("hanging", BlockStateProperties::HANGING),
            ("has_bottle_0", BlockStateProperties::HAS_BOTTLE_0),
            ("has_bottle_1", BlockStateProperties::HAS_BOTTLE_1),
            ("has_bottle_2", BlockStateProperties::HAS_BOTTLE_2),
            ("has_record", BlockStateProperties::HAS_RECORD),
            ("has_book", BlockStateProperties::HAS_BOOK),
            ("inverted", BlockStateProperties::INVERTED),
            ("in_wall", BlockStateProperties::IN_WALL),
            ("lit", BlockStateProperties::LIT),
            ("locked", BlockStateProperties::LOCKED),
            ("natural", BlockStateProperties::NATURAL),
            ("occupied", BlockStateProperties::OCCUPIED),
            ("open", BlockStateProperties::OPEN),
            ("persistent", BlockStateProperties::PERSISTENT),
            ("powered", BlockStateProperties::POWERED),
            ("short", BlockStateProperties::SHORT),
            ("shrieking", BlockStateProperties::SHRIEKING),
            ("signal_fire", BlockStateProperties::SIGNAL_FIRE),
            ("snowy", BlockStateProperties::SNOWY),
            ("tip", BlockStateProperties::TIP),
            ("triggered", BlockStateProperties::TRIGGERED),
            ("unstable", BlockStateProperties::UNSTABLE),
            ("waterlogged", BlockStateProperties::WATERLOGGED),
            ("up", BlockStateProperties::UP),
            ("down", BlockStateProperties::DOWN),
            ("north", BlockStateProperties::NORTH),
            ("east", BlockStateProperties::EAST),
            ("south", BlockStateProperties::SOUTH),
            ("west", BlockStateProperties::WEST),
            ("slot_0_occupied", BlockStateProperties::SLOT_0_OCCUPIED),
            ("slot_1_occupied", BlockStateProperties::SLOT_1_OCCUPIED),
            ("slot_2_occupied", BlockStateProperties::SLOT_2_OCCUPIED),
            ("slot_3_occupied", BlockStateProperties::SLOT_3_OCCUPIED),
            ("slot_4_occupied", BlockStateProperties::SLOT_4_OCCUPIED),
            ("slot_5_occupied", BlockStateProperties::SLOT_5_OCCUPIED),
            ("cracked", BlockStateProperties::CRACKED),
            ("crafting", BlockStateProperties::CRAFTING),
            ("ominous", BlockStateProperties::OMINOUS),
            ("age", BlockStateProperties::AGE_1),
            ("age", BlockStateProperties::AGE_2),
            ("age", BlockStateProperties::AGE_3),
            ("age", BlockStateProperties::AGE_4),
            ("age", BlockStateProperties::AGE_5),
            ("age", BlockStateProperties::AGE_7),
            ("age", BlockStateProperties::AGE_15),
            ("age", BlockStateProperties::AGE_25),
            ("bites", BlockStateProperties::BITES),
            ("candles", BlockStateProperties::CANDLES),
            ("delay", BlockStateProperties::DELAY),
            ("distance", BlockStateProperties::DISTANCE),
            ("eggs", BlockStateProperties::EGGS),
            ("hatch", BlockStateProperties::HATCH),
            ("layers", BlockStateProperties::LAYERS),
            ("level", BlockStateProperties::LEVEL_CAULDRON),
            ("level", BlockStateProperties::LEVEL_COMPOSTER),
            ("honey_level", BlockStateProperties::LEVEL_HONEY),
            ("level", BlockStateProperties::LEVEL),
            ("moisture", BlockStateProperties::MOISTURE),
            ("note", BlockStateProperties::NOTE),
            ("pickles", BlockStateProperties::PICKLES),
            ("power", BlockStateProperties::POWER),
            ("stage", BlockStateProperties::STAGE),
            ("distance", BlockStateProperties::STABILITY_DISTANCE),
            ("charges", BlockStateProperties::RESPAWN_ANCHOR_CHARGES),
            (
                "hydration",
                BlockStateProperties::DRIED_GHAST_HYDRATION_LEVELS,
            ),
            ("rotation", BlockStateProperties::ROTATION_16),
            ("dusted", BlockStateProperties::DUSTED),
            ("flower_amount", BlockStateProperties::FLOWER_AMOUNT),
            ("segment_amount", BlockStateProperties::SEGMENT_AMOUNT),
            ("axis", BlockStateProperties::HORIZONTAL_AXIS),
            ("axis", BlockStateProperties::AXIS),
            ("facing", BlockStateProperties::FACING),
            ("facing", BlockStateProperties::HORIZONTAL_FACING),
            ("facing", BlockStateProperties::FACING_HOPPER),
            (
                "vertical_direction",
                BlockStateProperties::VERTICAL_DIRECTION,
            ),
            ("half", BlockStateProperties::DOUBLE_BLOCK_HALF),
            ("half", BlockStateProperties::HALF),
            ("type", BlockStateProperties::SLAB_TYPE),
            ("shape", BlockStateProperties::RAIL_SHAPE),
            ("shape", BlockStateProperties::STAIRS_SHAPE),
            ("face", BlockStateProperties::ATTACH_FACE),
            ("east", BlockStateProperties::EAST_REDSTONE),
            ("north", BlockStateProperties::NORTH_REDSTONE),
            ("south", BlockStateProperties::SOUTH_REDSTONE),
            ("west", BlockStateProperties::WEST_REDSTONE),
            ("thickness", BlockStateProperties::SPELEOTHEM_THICKNESS),
            ("leaves", BlockStateProperties::BAMBOO_LEAVES),
            (
                "creaking_heart_state",
                BlockStateProperties::CREAKING_HEART_STATE,
            ),
        ];
        for (name, prop) in cases {
            assert_eq!(
                prop.name(),
                *name,
                "facade constant mapped to the wrong property id"
            );
        }
    }
}
