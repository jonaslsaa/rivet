//! Generated block-state global-id table integration tests (issue #154). Gated
//! behind the `blocks` feature + `cfg(test)`.
//!
//! These test the dense global-id mapping `BlockId + property value indices ->
//! StateId` / `StateId -> BlockId + value indices` that `PalettedContainer`/
//! `Palette` use as the wire global palette index (the forward/reverse lookup
//! that feeds #108). The tables are codegen-owned (source:
//! `data/reports/blocks.json`, the vanilla `BlockListReport`; the oracle-
//! conformance test in tools/rivet-codegen re-derives every one of the 32366
//! ids from the report); this module reads them only and lives OUTSIDE
//! `src/generated/` (the codegen golden drift test asserts that dir contains
//! exactly the generated files).

use crate::generated::block_properties::{
    BLOCK_PROPERTY_NAMES, BLOCK_PROPERTY_VALUES, BLOCK_STATE_SHAPES, MAX_BLOCK_STATE_PROPERTY_COUNT,
};
use crate::generated::block_states::{
    BLOCK_STATE_BASES, BLOCK_STATE_COUNT, GLOBAL_PALETTE_BITS, StateId, block_of, default_state,
    is_valid, shape_of, state_id, values_of,
};
use crate::generated::blocks::{BLOCK_BY_ID, BlockId};

/// Map `(property name, value name)` pairs to digit indices, in the block's
/// declaration order. Keeps the golden probes readable and independent of the
/// raw `BlockPropertyId` discriminant numbers.
fn digits(block: &str, pairs: &[(&str, &str)]) -> (BlockId, Vec<u16>) {
    let block = BlockId::from_name(block).expect("block in generated table");
    let shape = shape_of(block);
    let mut values = vec![0u16; shape.len()];
    for (prop_name, value_name) in pairs {
        let pos = shape
            .iter()
            .position(|&p| BLOCK_PROPERTY_NAMES[p as usize] == *prop_name)
            .unwrap_or_else(|| panic!("{block:?} has no property `{prop_name}`"));
        let prop_values = BLOCK_PROPERTY_VALUES[shape[pos] as usize];
        let value_pos = prop_values
            .iter()
            .position(|&v| v == *value_name)
            .unwrap_or_else(|| panic!("`{prop_name}` has no value `{value_name}`"));
        values[pos] = value_pos as u16;
    }
    (block, values)
}

/// Golden probes against Paper's ground truth (pinned in `data/reports/blocks.json`):
/// air is state 0, acacia_button's default (wall/north/false) is 10780,
/// redstone_wire spans 4011..5306 (1296 states), chest's first state
/// (single/north/true) is 3987.
#[test]
fn golden_probes_match_paper_state_ids() {
    // air: single state, empty property shape, id 0.
    assert_eq!(state_id(BlockId::from_id(0), &[]), StateId(0));
    assert_eq!(default_state(BlockId::from_id(0)), StateId(0));

    // acacia_button default {face: wall, facing: north, powered: false}.
    let (button, button_vals) = digits(
        "minecraft:acacia_button",
        &[("face", "wall"), ("facing", "north"), ("powered", "false")],
    );
    assert_eq!(button.id(), 447);
    assert_eq!(state_id(button, &button_vals), StateId(10780));
    assert_eq!(default_state(button), StateId(10780));

    // redstone_wire: first state 4011 ({east: up, north: up, power: 0, ...}),
    // last 5306 ({east: none, north: none, power: 15, ...}); 1296 states.
    let (wire, wire_vals) = digits(
        "minecraft:redstone_wire",
        &[
            ("east", "up"),
            ("north", "up"),
            ("power", "0"),
            ("south", "up"),
            ("west", "up"),
        ],
    );
    assert_eq!(state_id(wire, &wire_vals), StateId(4011));
    let (wire_last, wire_last_vals) = digits(
        "minecraft:redstone_wire",
        &[
            ("east", "none"),
            ("north", "none"),
            ("power", "15"),
            ("south", "none"),
            ("west", "none"),
        ],
    );
    assert_eq!(state_id(wire_last, &wire_last_vals), StateId(5306));

    // chest first state {type: single, facing: north, waterlogged: true}.
    let (chest, chest_vals) = digits(
        "minecraft:chest",
        &[
            ("type", "single"),
            ("facing", "north"),
            ("waterlogged", "true"),
        ],
    );
    assert_eq!(state_id(chest, &chest_vals), StateId(3987));

    // The wire width the global palette needs.
    assert_eq!(BLOCK_STATE_COUNT, 32366);
    assert_eq!(GLOBAL_PALETTE_BITS, 15);
}

/// Every block's default state sits inside its own `[base, base + count)` range
/// and matches the report's `default` marker, and bases are strictly increasing
/// (blocks are assigned states block after block in registry order).
#[test]
fn bases_and_defaults_are_consistent() {
    assert_eq!(BLOCK_STATE_BASES.len(), BLOCK_BY_ID.len());
    let mut prev_base = None;
    for (block_id, anchor) in BLOCK_STATE_BASES.iter().enumerate() {
        let block = BlockId::from_id(block_id as u16);
        // Base/count/default all within the table; the range is non-empty.
        assert!(anchor.count > 0);
        assert!(anchor.base + anchor.count <= BLOCK_STATE_COUNT);
        assert!(
            (anchor.base..anchor.base + anchor.count).contains(&anchor.default),
            "default {} out of range for block {}",
            anchor.default,
            BLOCK_BY_ID[block_id]
        );
        assert_eq!(default_state(block), StateId(anchor.default));
        // Strictly increasing bases (implied by the dense global range check in
        // the codegen, but asserted here so the emitted table is self-checking).
        if let Some(prev) = prev_base {
            assert!(prev < anchor.base);
        }
        prev_base = Some(anchor.base);
    }
}

/// The full 0..32365 id space is a bijection: `values_of` decomposes any state
/// id, and `state_id` re-composes it to the same id (and the same owning
/// block). This is the wire round-trip `PalettedContainer` relies on.
#[test]
fn every_global_id_round_trips() {
    // The scratch buffer is sized to the codegen-emitted max property count,
    // so it always fits any block's shape. state_id requires
    // `values.len() == shape.len()`, so the recomposition passes exactly the
    // block's shape length.
    let mut buf = [0u16; MAX_BLOCK_STATE_PROPERTY_COUNT];
    for id in 0..BLOCK_STATE_COUNT {
        let id = StateId(id);
        values_of(id, &mut buf);
        let block = block_of(id);
        let shape = shape_of(block);
        assert_eq!(
            state_id(block, &buf[..shape.len()]),
            id,
            "round-trip failed for id {id:?}"
        );
    }
}

/// The buffer bound `MAX_BLOCK_STATE_PROPERTY_COUNT` must be at least every
/// block's shape length, or `values_of` would write out of bounds for the
/// widest block. The counterfactual: a block with more properties than the
/// emitted constant fails here, so a future MC that grows a shape is caught at
/// test time rather than panicking in release.
#[test]
fn max_shape_len_bounds_every_shape() {
    let widest = BLOCK_STATE_SHAPES
        .iter()
        .map(|(_, shape)| shape.len())
        .max()
        .expect("non-empty shape table");
    assert!(
        widest <= MAX_BLOCK_STATE_PROPERTY_COUNT,
        "widest shape has {widest} properties but buffer is sized to \
         {MAX_BLOCK_STATE_PROPERTY_COUNT}"
    );
}

/// The emitted `behavior_of` must reproduce the RLE table for every state in
/// the real table — including both boundaries of every run and the out-of-range
/// fallback. This walks the ACTUAL generated function (not a re-implementation)
/// against an independent linear-scan decode, so a `partition_point` off-by-one
/// anywhere in the 16753-run table fails here.
#[test]
fn behavior_of_matches_rle_table_for_every_state() {
    use crate::generated::block_behaviors::BLOCK_BEHAVIOR_RUNS;
    use crate::generated::block_behaviors::behavior_of;
    use crate::generated::block_states::BLOCK_STATE_COUNT;

    // Independent linear-scan decode: the word of the run whose span covers id,
    // else the air fallback (run 0).
    let decode = |id: u32| -> u32 {
        for &(start, len, word) in BLOCK_BEHAVIOR_RUNS {
            if id >= start as u32 && id < start as u32 + len as u32 {
                return word;
            }
        }
        BLOCK_BEHAVIOR_RUNS[0].2
    };

    // The table must densely partition 0..BLOCK_STATE_COUNT, or a real state
    // would decode to the fallback without being out of range.
    let mut next = 0u32;
    for &(start, len, _) in BLOCK_BEHAVIOR_RUNS {
        assert_eq!(start as u32, next, "runs not densely packed");
        next += len as u32;
    }
    assert_eq!(next, BLOCK_STATE_COUNT as u32);

    // Every in-range state: emitted decode == independent decode.
    for id in 0..BLOCK_STATE_COUNT {
        assert_eq!(
            behavior_of(StateId(id)),
            decode(id as u32),
            "behavior_of diverged from RLE table at state {id}"
        );
    }
    // Out-of-range ids fall back to air (state 0's word).
    assert_eq!(behavior_of(StateId(BLOCK_STATE_COUNT)), decode(0));
    assert_eq!(behavior_of(StateId(u16::MAX)), decode(0));
}

/// Out-of-range ids fall back to air (block 0, state 0), mirroring
/// `Block.stateById` / the global palette's missing-id behaviour.
#[test]
fn out_of_range_ids_fall_back_to_air() {
    assert_eq!(block_of(StateId(0)), BlockId::from_id(0));
    assert_eq!(block_of(StateId(BLOCK_STATE_COUNT)), BlockId::from_id(0));
    assert_eq!(block_of(StateId(u16::MAX)), BlockId::from_id(0));
    assert!(!is_valid(StateId(BLOCK_STATE_COUNT)));
    assert!(is_valid(StateId(BLOCK_STATE_COUNT - 1)));
    assert!(is_valid(StateId(0)));
}

/// The shape table (property ids per block) must be sorted by block id — the
/// forward/reverse lookups binary-search it. This is a property the emitted
/// table depends on, so it is asserted rather than assumed.
#[test]
fn shape_table_is_sorted_by_block_id() {
    for pair in BLOCK_STATE_SHAPES.windows(2) {
        assert!(pair[0].0 < pair[1].0);
    }
}

// ---------------------------------------------------------------------------
// Typed `block.state.properties` surface (issue #228)
// ---------------------------------------------------------------------------

use crate::block_state::BlockState;
use crate::block_state_properties::BlockStateProperties;
use crate::block_state_property::PropertyValue;

/// The default state of a block by name.
fn default_state_of(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).expect("block in generated table"))
}

/// Paper-grounded typed-property assertions over real 26.2 blocks.
#[test]
fn typed_set_get_round_trips_on_real_blocks() {
    // oak_leaves: the TreeFeature/FoliagePlacer surface (DISTANCE, PERSISTENT,
    // WATERLOGGED).
    let leaves = default_state_of("minecraft:oak_leaves");
    assert!(leaves.has_property(BlockStateProperties::DISTANCE));
    assert!(leaves.has_property(BlockStateProperties::PERSISTENT));
    assert!(leaves.has_property(BlockStateProperties::WATERLOGGED));
    assert_eq!(
        leaves.get_value(BlockStateProperties::DISTANCE),
        Some(PropertyValue::Int(7))
    );
    assert_eq!(
        leaves.get_value(BlockStateProperties::PERSISTENT),
        Some(PropertyValue::Bool(false))
    );
    let near = leaves
        .set_value(BlockStateProperties::DISTANCE, PropertyValue::Int(1))
        .unwrap()
        .set_value(BlockStateProperties::PERSISTENT, PropertyValue::Bool(true))
        .unwrap();
    assert_eq!(
        near.get_value(BlockStateProperties::DISTANCE),
        Some(PropertyValue::Int(1))
    );
    assert_eq!(
        near.get_value(BlockStateProperties::PERSISTENT),
        Some(PropertyValue::Bool(true))
    );
    assert_eq!(near.block().id(), leaves.block().id());

    // oak_door: DoubleBlockHalf.HALF — set the top half (StrongholdPieces
    // builds doors with DoubleBlockHalf.UPPER).
    let door = default_state_of("minecraft:oak_door");
    assert!(door.has_property(BlockStateProperties::DOUBLE_BLOCK_HALF));
    assert_eq!(
        door.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF),
        Some(PropertyValue::Enum("lower"))
    );
    let top = door
        .set_value(
            BlockStateProperties::DOUBLE_BLOCK_HALF,
            PropertyValue::Enum("upper"),
        )
        .unwrap();
    assert_eq!(
        top.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF),
        Some(PropertyValue::Enum("upper"))
    );

    // oak_stairs: Half.HALF + StairsShape.SHAPE (SwampHutPiece sets
    // StairsShape.OUTER_LEFT/RIGHT).
    let stairs = default_state_of("minecraft:oak_stairs");
    assert!(stairs.has_property(BlockStateProperties::HALF));
    assert!(stairs.has_property(BlockStateProperties::STAIRS_SHAPE));
    assert_eq!(
        stairs.get_value(BlockStateProperties::HALF),
        Some(PropertyValue::Enum("bottom"))
    );
    let outer = stairs
        .set_value(
            BlockStateProperties::STAIRS_SHAPE,
            PropertyValue::Enum("outer_left"),
        )
        .unwrap();
    assert_eq!(
        outer.get_value(BlockStateProperties::STAIRS_SHAPE),
        Some(PropertyValue::Enum("outer_left"))
    );

    // oak_slab: SlabType.TYPE (StrongholdPieces builds SlabType.DOUBLE).
    let slab = default_state_of("minecraft:oak_slab");
    assert!(slab.has_property(BlockStateProperties::SLAB_TYPE));
    assert_eq!(
        slab.get_value(BlockStateProperties::SLAB_TYPE),
        Some(PropertyValue::Enum("bottom"))
    );
    let dbl = slab
        .set_value(
            BlockStateProperties::SLAB_TYPE,
            PropertyValue::Enum("double"),
        )
        .unwrap();
    assert_eq!(
        dbl.get_value(BlockStateProperties::SLAB_TYPE),
        Some(PropertyValue::Enum("double"))
    );

    // rail: RailShape.SHAPE (MineshaftPieces sets NORTH_SOUTH/EAST_WEST).
    let rail = default_state_of("minecraft:rail");
    assert!(rail.has_property(BlockStateProperties::RAIL_SHAPE));
    assert_eq!(
        rail.get_value(BlockStateProperties::RAIL_SHAPE),
        Some(PropertyValue::Enum("north_south"))
    );
    let ew = rail
        .set_value(
            BlockStateProperties::RAIL_SHAPE,
            PropertyValue::Enum("east_west"),
        )
        .unwrap();
    assert_eq!(
        ew.get_value(BlockStateProperties::RAIL_SHAPE),
        Some(PropertyValue::Enum("east_west"))
    );

    // redstone_wire: RedstoneSide per-direction + POWER (JungleTemplePiece
    // sets RedstoneSide.SIDE/UP).
    let wire = default_state_of("minecraft:redstone_wire");
    assert!(wire.has_property(BlockStateProperties::NORTH_REDSTONE));
    assert!(wire.has_property(BlockStateProperties::EAST_REDSTONE));
    assert!(wire.has_property(BlockStateProperties::POWER));
    let connected = wire
        .set_value(
            BlockStateProperties::NORTH_REDSTONE,
            PropertyValue::Enum("side"),
        )
        .unwrap()
        .set_value(BlockStateProperties::POWER, PropertyValue::Int(3))
        .unwrap();
    assert_eq!(
        connected.get_value(BlockStateProperties::NORTH_REDSTONE),
        Some(PropertyValue::Enum("side"))
    );
    assert_eq!(
        connected.get_value(BlockStateProperties::POWER),
        Some(PropertyValue::Int(3))
    );

    // pointed_dripstone: SpeleothemThickness.THICKNESS (SpeleothemUtils).
    let drip = default_state_of("minecraft:pointed_dripstone");
    assert!(drip.has_property(BlockStateProperties::SPELEOTHEM_THICKNESS));
    assert_eq!(
        drip.get_value(BlockStateProperties::SPELEOTHEM_THICKNESS),
        Some(PropertyValue::Enum("tip"))
    );
    let base = drip
        .set_value(
            BlockStateProperties::SPELEOTHEM_THICKNESS,
            PropertyValue::Enum("base"),
        )
        .unwrap();
    assert_eq!(
        base.get_value(BlockStateProperties::SPELEOTHEM_THICKNESS),
        Some(PropertyValue::Enum("base"))
    );

    // bamboo: BambooLeaves.LEAVES + STAGE (BambooFeature sets
    // BambooLeaves.LARGE and stage 1).
    let bamboo = default_state_of("minecraft:bamboo");
    assert!(bamboo.has_property(BlockStateProperties::BAMBOO_LEAVES));
    assert!(bamboo.has_property(BlockStateProperties::STAGE));
    assert_eq!(
        bamboo.get_value(BlockStateProperties::BAMBOO_LEAVES),
        Some(PropertyValue::Enum("none"))
    );
    let grown = bamboo
        .set_value(
            BlockStateProperties::BAMBOO_LEAVES,
            PropertyValue::Enum("large"),
        )
        .unwrap()
        .set_value(BlockStateProperties::STAGE, PropertyValue::Int(1))
        .unwrap();
    assert_eq!(
        grown.get_value(BlockStateProperties::BAMBOO_LEAVES),
        Some(PropertyValue::Enum("large"))
    );
    assert_eq!(
        grown.get_value(BlockStateProperties::STAGE),
        Some(PropertyValue::Int(1))
    );

    // creaking_heart: CreakingHeartState (the default is UPROOTED per
    // CreakingHeartBlock's default state; CreakingHeartDecorator sets DORMANT).
    let heart = default_state_of("minecraft:creaking_heart");
    assert!(heart.has_property(BlockStateProperties::CREAKING_HEART_STATE));
    assert_eq!(
        heart.get_value(BlockStateProperties::CREAKING_HEART_STATE),
        Some(PropertyValue::Enum("uprooted"))
    );
    let awake = heart
        .set_value(
            BlockStateProperties::CREAKING_HEART_STATE,
            PropertyValue::Enum("awake"),
        )
        .unwrap();
    assert_eq!(
        awake.get_value(BlockStateProperties::CREAKING_HEART_STATE),
        Some(PropertyValue::Enum("awake"))
    );
}

/// `hasProperty` false and `trySetValue` no-op for a property the block does
/// not carry; `setValue` errors (Paper `IllegalArgumentException`).
#[test]
fn typed_helpers_absent_property_behavior() {
    let stone = default_state_of("minecraft:stone");
    assert!(!stone.has_property(BlockStateProperties::WATERLOGGED));
    assert_eq!(stone.get_value(BlockStateProperties::WATERLOGGED), None);
    // trySetValue returns the state unchanged.
    let same = stone
        .try_set_value(BlockStateProperties::WATERLOGGED, PropertyValue::Bool(true))
        .unwrap();
    assert_eq!(same.id(), stone.id());
    // setValue errors for the absent property — and it surfaces the
    // absent-property error *before* value validation (an invalid typed value
    // on an absent property still reports the absent property). Paper's
    // optimised-table `setValue` throws "Cannot set property … on …" for an
    // absent property; the "not an allowed value" error only applies to a
    // present property.
    assert_eq!(
        stone.set_value(
            BlockStateProperties::WATERLOGGED,
            PropertyValue::Enum("not_a_bool")
        ),
        Err(crate::block_state::BlockStateError::PropertyNotPresent(
            crate::generated::block_properties::BlockPropertyId::Waterlogged
        ))
    );

    // An invalid typed value for a *present* property errors (Java
    // IllegalArgumentException): DISTANCE is 1..=7, so 8 is out of range.
    let leaves = default_state_of("minecraft:oak_leaves");
    assert!(
        leaves
            .set_value(BlockStateProperties::DISTANCE, PropertyValue::Int(8))
            .is_err()
    );
    // An out-of-set enum name for a present enum property errors.
    assert!(
        leaves
            .set_value(
                BlockStateProperties::PERSISTENT,
                PropertyValue::Enum("maybe")
            )
            .is_err()
    );
    // The bool value on an enum property errors (kind mismatch).
    assert!(
        leaves
            .set_value(
                BlockStateProperties::STAIRS_SHAPE,
                PropertyValue::Bool(true)
            )
            .is_err()
    );
}

/// The typed leaf enums convert to `PropertyValue` and can be passed directly
/// to the typed helpers — the `state.setValue(SlabBlock.TYPE, SlabType.DOUBLE)`
/// ergonomics Java's worldgen code uses.
#[test]
fn typed_leaf_enums_flow_through_set_value() {
    use crate::block_state_properties::{DoubleBlockHalf, SlabType, StairsShape};

    // SlabType::Double on a slab (StrongholdPieces builds SlabType.DOUBLE).
    let slab = default_state_of("minecraft:oak_slab");
    let dbl = slab
        .set_value(BlockStateProperties::SLAB_TYPE, SlabType::Double)
        .unwrap();
    assert_eq!(
        dbl.get_value(BlockStateProperties::SLAB_TYPE),
        Some(PropertyValue::Enum("double"))
    );

    // DoubleBlockHalf::Upper on a door (StrongholdPieces doors).
    let door = default_state_of("minecraft:oak_door");
    let top = door
        .set_value(
            BlockStateProperties::DOUBLE_BLOCK_HALF,
            DoubleBlockHalf::Upper,
        )
        .unwrap();
    assert_eq!(
        top.get_value(BlockStateProperties::DOUBLE_BLOCK_HALF),
        Some(PropertyValue::Enum("upper"))
    );

    // StairsShape::OuterLeft on stairs (SwampHutPiece).
    let stairs = default_state_of("minecraft:oak_stairs");
    let outer = stairs
        .set_value(BlockStateProperties::STAIRS_SHAPE, StairsShape::OuterLeft)
        .unwrap();
    assert_eq!(
        outer.get_value(BlockStateProperties::STAIRS_SHAPE),
        Some(PropertyValue::Enum("outer_left"))
    );

    // try_set_value also accepts the typed enums, and no-ops for a block
    // without the property (Paper `trySetValue`).
    let stone = default_state_of("minecraft:stone");
    let same = stone
        .try_set_value(BlockStateProperties::SLAB_TYPE, SlabType::Double)
        .unwrap();
    assert_eq!(same.id(), stone.id());
}
