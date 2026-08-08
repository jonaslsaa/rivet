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
