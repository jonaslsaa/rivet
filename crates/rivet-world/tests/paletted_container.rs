//! Integration tests for the `PalettedContainer`/`Palette`/`BitStorage` wire
//! format (#108), driven by the real generated block-state table.
//!
//! Every expected byte is derived from the Java layout (see the module docs in
//! `src/chunk/`), and the global ids come from `rivet-registry`'s generated
//! dense block-state table (which the codegen oracle cross-checks against
//! Paper's `BlockListReport`). Grounded fixtures: air=0, stone=1,
//! grass_block default=9, dirt=10, bedrock=85.

use bytes::BytesMut;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::generated::block_states::{
    BLOCK_STATE_COUNT, GLOBAL_PALETTE_BITS, StateId, is_valid,
};
use rivet_world::chunk::palette::GlobalIdMap;
use rivet_world::chunk::paletted_container::{PackedData, PalettedContainer};
use rivet_world::chunk::strategy::Strategy;

/// The dense generated block-state global id map: `StateId(n)` <-> global id
/// `n` for `0..BLOCK_STATE_COUNT` (32366 states, 15 bits).
#[derive(Clone, Copy)]
struct BlockStateGlobalMap;

impl GlobalIdMap<StateId> for BlockStateGlobalMap {
    fn get_id(&self, value: &StateId) -> i32 {
        value.0 as i32
    }

    fn by_id_or_throw(&self, id: i32) -> StateId {
        assert!(
            (0..BLOCK_STATE_COUNT as i32).contains(&id),
            "No value with id {id}"
        );
        StateId(id as u16)
    }

    fn size(&self) -> i32 {
        BLOCK_STATE_COUNT as i32
    }

    fn by_id(&self, id: i32) -> Option<StateId> {
        if (0..BLOCK_STATE_COUNT as i32).contains(&id) {
            Some(StateId(id as u16))
        } else {
            None
        }
    }

    fn clone_box(&self) -> Box<dyn GlobalIdMap<StateId>> {
        Box::new(*self)
    }
}

fn block_state_strategy() -> Strategy<StateId> {
    Strategy::create_for_block_states(Box::new(BlockStateGlobalMap))
}

fn write_to_bytes(c: &PalettedContainer<StateId>) -> Vec<u8> {
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    c.write(&mut buf);
    buf.into_inner().to_vec()
}

fn read_from_bytes(bytes: &[u8]) -> PalettedContainer<StateId> {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes));
    c.read(&mut buf);
    c
}

fn section_index(x: i32, y: i32, z: i32) -> usize {
    // Strategy.getIndex for block states: (y << 4 | z) << 4 | x.
    ((y << 4 | z) << 4 | x) as usize
}

// ---------------------------------------------------------------------------
// Single-value container (the all-air section)
// ---------------------------------------------------------------------------

#[test]
fn single_value_container_wire_is_two_bytes() {
    let c = PalettedContainer::new(StateId(0), block_state_strategy());
    assert_eq!(c.bits_per_entry(), 0);
    assert_eq!(c.get(7, 8, 9), StateId(0));
    assert_eq!(c.get_serialized_size(), 2);
    // bits byte 0x00, SingleValue palette: varint(0) = 0x00, no storage longs.
    assert_eq!(write_to_bytes(&c), vec![0x00, 0x00]);
}

#[test]
fn single_value_count_and_get_all() {
    let c = PalettedContainer::new(StateId(0), block_state_strategy());
    let mut counts = Vec::new();
    c.count(|state, n| counts.push((state, n)));
    assert_eq!(counts, vec![(StateId(0), 4096)]);
    let mut all = Vec::new();
    c.get_all(|s| all.push(s));
    assert_eq!(all, vec![StateId(0)]);
}

#[test]
fn single_value_round_trips_through_wire() {
    let c = PalettedContainer::new(StateId(85), block_state_strategy()); // bedrock section
    let bytes = write_to_bytes(&c);
    assert_eq!(bytes, vec![0x00, 0x55]); // bits 0, varint(85)
    let back = read_from_bytes(&bytes);
    assert_eq!(back.get(0, 0, 0), StateId(85));
    assert_eq!(back.get(15, 15, 15), StateId(85));
}

#[test]
fn read_into_same_bits_container_reuses_data_path() {
    // Java's `createOrReuseData` reuses the existing Data when the wire bits
    // match the current configuration; the port copies and overwrites it.
    // Reading a 4-bit wire into a container already at 4 bits must fully
    // replace both palette and storage from the wire.
    let mut src = PalettedContainer::new(StateId(0), block_state_strategy());
    src.set(0, 0, 0, StateId(85));
    let bytes = write_to_bytes(&src);

    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set(0, 0, 0, StateId(1)); // 4-bit, [air, stone]
    assert_eq!(c.bits_per_entry(), 4);
    let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    c.read(&mut buf);
    assert_eq!(c.bits_per_entry(), 4);
    assert_eq!(c.get(0, 0, 0), StateId(85));
    assert_eq!(c.get(1, 0, 0), StateId(0));
    let mut palette = Vec::new();
    c.for_each_in_palette(|s| palette.push(s));
    assert_eq!(palette, vec![StateId(0), StateId(85)]); // fully replaced
}

// ---------------------------------------------------------------------------
// Single value -> 4-bit linear resize
// ---------------------------------------------------------------------------

#[test]
fn resize_single_value_to_four_bit_linear() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    let prev = c.get_and_set(0, 0, 0, StateId(1)); // stone
    assert_eq!(prev, StateId(0));
    assert_eq!(c.bits_per_entry(), 4);
    assert_eq!(c.get(0, 0, 0), StateId(1));
    assert_eq!(c.get(1, 0, 0), StateId(0));
    // Palette: [air(0), stone(1)]; storage cell 0 = palette id 1 = 0x1.
    assert_eq!(c.get_serialized_size(), 1 + 3 + 256 * 8);
    let bytes = write_to_bytes(&c);
    assert_eq!(&bytes[..4], &[0x04, 0x02, 0x00, 0x01]);
    // raw longs: 256, first long = 0x1 (stone palette id at entry 0), rest zero.
    // Wire writes longs big-endian: value 1 -> [00 00 00 00 00 00 00 01].
    assert_eq!(&bytes[4..12], &[0, 0, 0, 0, 0, 0, 0, 1]);
    assert!(bytes[12..].iter().all(|&b| b == 0));
}

#[test]
fn resize_palette_order_is_air_then_new() {
    // Setting a non-air value first keeps air as palette entry 0 (it was the
    // single value), the new value becomes entry 1.
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set(3, 4, 5, StateId(10)); // dirt
    let mut palette = Vec::new();
    c.for_each_in_palette(|s| palette.push(s));
    assert_eq!(palette, vec![StateId(0), StateId(10)]);
    // entry for (3,4,5) is dirt's palette id 1.
    let index = section_index(3, 4, 5);
    let bytes = write_to_bytes(&c);
    // 4-bit storage: entry at `index` lives in cell `index/16` at nibble
    // `(index % 16) * 4`. Wire longs are big-endian.
    let cell = index / 16;
    let off = (index % 16) * 4;
    let raw: &[u8] = &bytes[4..];
    let long = i64::from_be_bytes(raw[cell * 8..cell * 8 + 8].try_into().unwrap());
    assert_eq!((long >> off) & 0xF, 1);
}

// ---------------------------------------------------------------------------
// Golden superflat section (Java layout, grounded global ids)
// ---------------------------------------------------------------------------

#[test]
fn superflat_section_golden_wire_bytes() {
    // y=0 bedrock, y=1 dirt, y=2 dirt, y=3 grass_block, rest air.
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 0, z, StateId(85)); // bedrock
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 1, z, StateId(10)); // dirt
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 2, z, StateId(10)); // dirt
        }
    }
    for x in 0..16 {
        for z in 0..16 {
            c.set(x, 3, z, StateId(9)); // grass_block default (snowy=false)
        }
    }

    assert_eq!(c.bits_per_entry(), 4);
    let bytes = write_to_bytes(&c);
    assert_eq!(bytes.len(), 2054); // 1 + 5 palette + 2048 storage

    // Header + LinearPalette: size 4, then air=0, bedrock=85, dirt=10, grass=9.
    assert_eq!(&bytes[..6], &[0x04, 0x04, 0x00, 0x55, 0x0A, 0x09]);

    // Storage: 256 longs.
    // longs 0..16 (indices 0..256, y=0): bedrock palette id 1.
    // longs 16..48 (indices 256..768, y=1..2): dirt palette id 2.
    // longs 48..64 (indices 768..1024, y=3): grass palette id 3.
    // longs 64..256: air palette id 0.
    let raw = &bytes[6..];
    for cell in 0..16 {
        assert_eq!(
            &raw[cell * 8..cell * 8 + 8],
            &0x1111_1111_1111_1111i64.to_le_bytes()
        );
    }
    for cell in 16..48 {
        assert_eq!(
            &raw[cell * 8..cell * 8 + 8],
            &0x2222_2222_2222_2222i64.to_le_bytes()
        );
    }
    for cell in 48..64 {
        assert_eq!(
            &raw[cell * 8..cell * 8 + 8],
            &0x3333_3333_3333_3333i64.to_le_bytes()
        );
    }
    for cell in 64..256 {
        assert!(raw[cell * 8..cell * 8 + 8].iter().all(|&b| b == 0));
    }

    // Round-trip: a fresh container reads the same section back.
    let back = read_from_bytes(&bytes);
    assert_eq!(back.get(0, 0, 0), StateId(85));
    assert_eq!(back.get(0, 1, 0), StateId(10));
    assert_eq!(back.get(0, 2, 0), StateId(10));
    assert_eq!(back.get(0, 3, 0), StateId(9));
    assert_eq!(back.get(0, 4, 0), StateId(0));
    assert_eq!(back.bits_per_entry(), 4);
}

// ---------------------------------------------------------------------------
// 15-bit global palette
// ---------------------------------------------------------------------------

#[test]
fn many_states_reaches_global_palette_at_15_bits() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    for i in 1..300 {
        c.set_index(i, StateId(i as u16));
    }
    assert_eq!(c.bits_per_entry(), GLOBAL_PALETTE_BITS as i32); // 15
    assert_eq!(c.get_index(0), StateId(0));
    assert_eq!(c.get_index(250), StateId(250));

    // GlobalPalette writes no palette section: 1 bits byte + 1024 longs.
    let bytes = write_to_bytes(&c);
    assert_eq!(bytes.len(), 1 + 1024 * 8);
    assert_eq!(bytes[0], GLOBAL_PALETTE_BITS);
    // Storage holds global ids directly (15-bit cells, 4 per long).
    assert_eq!(
        i64::from_be_bytes(bytes[1..9].try_into().unwrap()) & 0x7FFF,
        0
    );

    let back = read_from_bytes(&bytes);
    assert_eq!(back.bits_per_entry(), GLOBAL_PALETTE_BITS as i32);
    for i in 0..300 {
        assert_eq!(back.get_index(i), StateId(i as u16), "index {i}");
    }
}

#[test]
fn global_palette_serialized_size_uses_storage_bits() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    for i in 1..300 {
        c.set_index(i, StateId(i as u16));
    }
    // 1 byte bits + 0 palette + 1024 longs * 8.
    assert_eq!(c.get_serialized_size(), 1 + 1024 * 8);
}

// ---------------------------------------------------------------------------
// pack / unpack (NBT codec path) — exercises bits-per-entry transitions
// ---------------------------------------------------------------------------

#[test]
fn pack_unpack_round_trip_small() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set_index(0, StateId(1));
    c.set_index(1, StateId(2));
    c.set_index(2, StateId(3));
    c.set_index(3, StateId(4));
    assert_eq!(c.bits_per_entry(), 4);

    let packed = c.pack();
    // Pack re-encodes into a fresh HashMapPalette in storage first-appearance
    // order (Java reencodeContents iterates the storage): the stored values at
    // indices 0..4 are stone,dirt,grass,water then air.
    assert_eq!(
        packed.palette_entries,
        vec![StateId(1), StateId(2), StateId(3), StateId(4), StateId(0)]
    );
    // palette size 5 -> ceillog2 = 3 -> FOUR_BITS_LINEAR -> bitsOnDisc 4.
    assert_eq!(packed.bits_per_entry, 4);
    assert!(packed.storage.is_some());

    let back = PalettedContainer::unpack(&block_state_strategy(), packed).expect("unpack");
    assert_eq!(back.bits_per_entry(), 4);
    for i in 0..4096 {
        let expect = match i {
            0 => StateId(1),
            1 => StateId(2),
            2 => StateId(3),
            3 => StateId(4),
            _ => StateId(0),
        };
        assert_eq!(back.get_index(i), expect, "index {i}");
    }
}

#[test]
fn pack_unpack_round_trip_global_reencodes() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    for i in 1..300 {
        c.set_index(i, StateId(i as u16));
    }
    assert_eq!(c.bits_per_entry(), GLOBAL_PALETTE_BITS as i32);

    let packed = c.pack();
    // palette size 300 -> ceillog2 = 9 -> Global(15, 9): bitsOnDisc 9, repack path.
    assert_eq!(packed.bits_per_entry, 9);
    assert_eq!(packed.palette_entries.len(), 300);

    let back = PalettedContainer::unpack(&block_state_strategy(), packed).expect("unpack");
    assert_eq!(back.bits_per_entry(), GLOBAL_PALETTE_BITS as i32);
    for i in 0..300 {
        assert_eq!(back.get_index(i), StateId(i as u16), "index {i}");
    }
    assert_eq!(back.get_index(500), StateId(0));
}

#[test]
fn unpack_rejects_wrong_declared_bits() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set_index(0, StateId(1));
    let mut packed = c.pack();
    packed.bits_per_entry = 99;
    let err = PalettedContainer::<StateId>::unpack(&block_state_strategy(), packed)
        .err()
        .expect("expected error");
    assert!(err.contains("Invalid bit count"), "err: {err}");
}

#[test]
fn unpack_accepts_unknown_bits() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set_index(0, StateId(1));
    let packed = c.pack();
    let back =
        PalettedContainer::<StateId>::unpack(&block_state_strategy(), packed).expect("unpack");
    assert_eq!(back.get_index(0), StateId(1));
}

#[test]
fn unpack_missing_storage_errors() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set_index(0, StateId(1));
    let mut packed = c.pack();
    packed.storage = None;
    let err = PalettedContainer::<StateId>::unpack(&block_state_strategy(), packed)
        .err()
        .expect("expected error");
    assert_eq!(err, "Missing values for non-zero storage");
}

// ---------------------------------------------------------------------------
// Mutation / property tests
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::needless_range_loop)] // `idx` is load-bearing: x/y/z derive from it
fn set_get_all_positions_round_trip() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    let mut expect = [0u16; 4096];
    for idx in 0..4096 {
        let x = (idx % 16) as i32;
        let z = ((idx / 16) % 16) as i32;
        let y = (idx / 256) as i32;
        let state = StateId(((idx * 37) % 500) as u16); // < 500, forces global palette
        c.set(x, y, z, state);
        expect[idx] = state.0;
    }
    assert_eq!(c.bits_per_entry(), GLOBAL_PALETTE_BITS as i32);
    for idx in 0..4096 {
        let x = (idx % 16) as i32;
        let z = ((idx / 16) % 16) as i32;
        let y = (idx / 256) as i32;
        assert_eq!(c.get(x, y, z).0, expect[idx], "idx {idx}");
    }
    // Wire round-trip preserves every cell.
    let back = read_from_bytes(&write_to_bytes(&c));
    for idx in 0..4096 {
        let x = (idx % 16) as i32;
        let z = ((idx / 16) % 16) as i32;
        let y = (idx / 256) as i32;
        assert_eq!(back.get(x, y, z).0, expect[idx], "round-trip idx {idx}");
    }
}

#[test]
fn get_and_set_returns_previous() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    assert_eq!(c.get_and_set(0, 0, 0, StateId(85)), StateId(0));
    assert_eq!(c.get_and_set(0, 0, 0, StateId(10)), StateId(85));
    assert_eq!(c.get_and_set(0, 0, 0, StateId(0)), StateId(10));
    assert_eq!(c.get(0, 0, 0), StateId(0));
}

#[test]
fn count_matches_mutation() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set(0, 0, 0, StateId(1));
    c.set(0, 0, 1, StateId(1));
    c.set(0, 0, 2, StateId(85));
    let mut counts = std::collections::BTreeMap::new();
    c.count(|s, n| {
        counts.insert(s.0, n);
    });
    assert_eq!(counts.get(&0), Some(&4093));
    assert_eq!(counts.get(&1), Some(&2));
    assert_eq!(counts.get(&85), Some(&1));
}

#[test]
fn maybe_has_consults_palette() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    assert!(!c.maybe_has(|s| s.0 == 85));
    c.set(0, 0, 0, StateId(85));
    assert!(c.maybe_has(|s| s.0 == 85));
    assert!(!c.maybe_has(|s| s.0 == 99));
}

#[test]
fn copy_and_recreate_are_independent() {
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set(0, 0, 0, StateId(85));
    let copy = c.copy();
    let recreate = c.recreate();

    assert_eq!(copy.get(0, 0, 0), StateId(85));
    assert_eq!(recreate.get(0, 0, 0), StateId(0)); // first palette entry is air

    let mut c2 = copy;
    c2.set(0, 0, 0, StateId(10));
    assert_eq!(c.get(0, 0, 0), StateId(85)); // original untouched
    assert_eq!(c2.get(0, 0, 0), StateId(10));
}

#[test]
fn index_ordering_is_xyz() {
    // Java Strategy.getIndex: (y << 4 | z) << 4 | x — x fastest, then z, then y.
    assert_eq!(section_index(1, 0, 0), 1);
    assert_eq!(section_index(0, 0, 1), 16);
    assert_eq!(section_index(0, 1, 0), 256);
    assert_eq!(section_index(15, 15, 15), 4095);
}

#[test]
fn all_generated_state_ids_are_valid_global_ids() {
    // The global map accepts every generated state id.
    let map = BlockStateGlobalMap;
    assert_eq!(map.size(), BLOCK_STATE_COUNT as i32);
    assert_eq!(map.by_id(0), Some(StateId(0)));
    assert_eq!(
        map.by_id(BLOCK_STATE_COUNT as i32 - 1),
        Some(StateId(BLOCK_STATE_COUNT - 1))
    );
    assert_eq!(map.by_id(BLOCK_STATE_COUNT as i32), None);
    assert_eq!(map.get_id(&StateId(12345)), 12345);
    assert!(is_valid(StateId(BLOCK_STATE_COUNT - 1)));
    assert!(!is_valid(StateId(BLOCK_STATE_COUNT)));
}

// ---------------------------------------------------------------------------
// Anti-Xray presetValues + Moonrise FastPalette snapshot (#216)
// ---------------------------------------------------------------------------

#[test]
fn preset_values_force_palette_growth_on_unpack() {
    // Java: unpackWithPresetValues on a full single-value palette triggers
    // onResize which widens for the presets, then inserts them. Palette [85]
    // (wire values) + presets [1, 10] -> union {85,1,10} -> 3 distinct ->
    // ceillog2(3) = 2, but the configuration ladder maps that to the 4-bit
    // Linear config (bits 1..=4 all map to FOUR_BITS_LINEAR, in-memory 4).
    let data = PackedData::new(vec![StateId(85)], None);
    let c = PalettedContainer::unpack_with_preset_values(
        &block_state_strategy(),
        data,
        StateId(0),
        Some(vec![StateId(1), StateId(10)]),
    )
    .expect("unpack");
    // SingleValue + presets: the single value is not the default, so the
    // Anti-Xray block runs and the palette grows to hold the presets.
    assert_eq!(c.bits_per_entry(), 4);
    let mut palette = Vec::new();
    c.for_each_in_palette(|s| palette.push(s));
    assert_eq!(palette.len(), 3);
    assert!(palette.contains(&StateId(85)));
    assert!(palette.contains(&StateId(1)));
    assert!(palette.contains(&StateId(10)));
    // The storage is zero-width: every stored index resolves to value 85
    // (the single wire entry), and the presets are palette members only.
    assert_eq!(c.get_index(0), StateId(85));
    assert_eq!(c.get_index(4095), StateId(85));
}

#[test]
fn preset_values_skip_when_single_value_matches_default() {
    // Java: a SingleValue config only runs the Anti-Xray block when
    // `valueFor(0) != defaultValue`. With the wire value equal to the default,
    // the presets are NOT inserted.
    let data = PackedData::new(vec![StateId(0)], None);
    let c = PalettedContainer::unpack_with_preset_values(
        &block_state_strategy(),
        data,
        StateId(0),
        Some(vec![StateId(1)]),
    )
    .expect("unpack");
    assert_eq!(c.bits_per_entry(), 0);
    let mut palette = Vec::new();
    c.for_each_in_palette(|s| palette.push(s));
    assert_eq!(palette, vec![StateId(0)]); // presets absent
}

#[test]
fn preset_values_reinserted_after_read() {
    // Java `read()` calls `addPresetValues()` ("inefficient, but this isn't
    // used by the server"): after adopting a fresh palette from the wire, the
    // preset values are re-added to it.
    // The unpacked container (4-bit, palette [85, 1], storage all-85) carries
    // preset 10, so its palette grows to [85, 1, 10].
    let data = PackedData::with_bits(
        vec![StateId(85), StateId(1)],
        Some(vec![0i64; 256]), // 4-bit, 4096 entries, all palette id 0 = 85
        4,
    );
    let mut c = PalettedContainer::unpack_with_preset_values(
        &block_state_strategy(),
        data,
        StateId(0),
        Some(vec![StateId(10)]),
    )
    .expect("unpack");
    let mut palette = Vec::new();
    c.for_each_in_palette(|s| palette.push(s));
    assert_eq!(palette, vec![StateId(85), StateId(1), StateId(10)]);

    // A wire read replaces the palette with the wire's [air, 85, 1] and then
    // re-adds preset 10, restoring [air, 85, 1, 10].
    let mut src = PalettedContainer::new(StateId(0), block_state_strategy());
    src.set_index(0, StateId(85));
    src.set_index(1, StateId(1));
    let bytes = write_to_bytes(&src);
    let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    c.read(&mut buf);
    assert_eq!(c.get_index(0), StateId(85));
    assert_eq!(c.get_index(1), StateId(1));
    let mut palette = Vec::new();
    c.for_each_in_palette(|s| palette.push(s));
    assert_eq!(
        palette,
        vec![StateId(0), StateId(85), StateId(1), StateId(10)]
    );
}

#[test]
fn read_refreshes_snapshot_after_palette_replace() {
    // Java `read()` sets `this.data = newData` then `updateData(this.data)`
    // and `addPresetValues()`. The Moonrise snapshot is a live reference, so
    // the Rust owned snapshot must be refreshed: reading a palette that grew
    // must make the read path see the new entries.
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    let mut src = PalettedContainer::new(StateId(0), block_state_strategy());
    src.set(0, 0, 0, StateId(85));
    src.set(0, 0, 1, StateId(10));
    let bytes = write_to_bytes(&src);
    let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    c.read(&mut buf);
    assert_eq!(c.get(0, 0, 0), StateId(85));
    assert_eq!(c.get(0, 0, 1), StateId(10));
    assert_eq!(c.bits_per_entry(), 4);
}

#[test]
fn snapshot_refreshes_after_set_grows_palette() {
    // Java `onResize` (reached via `idFor` during `set`) calls `updateData`
    // after the preset inserts and the final insert, so the snapshot reflects
    // the grown palette. The Rust port must re-materialize it likewise: after
    // growing, a previously-stored index reads the new value.
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    assert_eq!(c.get(0, 0, 0), StateId(0));
    c.set(0, 0, 0, StateId(85));
    assert_eq!(c.get(0, 0, 0), StateId(85));
    // Set a *new* value on a fresh index: grows to 4-bit, snapshot rebuilt.
    c.set(0, 0, 1, StateId(10));
    assert_eq!(c.get(0, 0, 0), StateId(85));
    assert_eq!(c.get(0, 0, 1), StateId(10));
}

#[test]
fn copy_and_recreate_carry_preset_values() {
    // Java's `copy()` and `recreate()` pass `this.presetValues` through, so
    // resizes on the copy still keep the presets in the palette.
    let data = PackedData::new(vec![StateId(85)], None);
    let c = PalettedContainer::unpack_with_preset_values(
        &block_state_strategy(),
        data,
        StateId(0),
        Some(vec![StateId(1), StateId(10)]),
    )
    .expect("unpack");
    assert_eq!(c.bits_per_entry(), 4);

    // copy preserves preset_values: growing the copy re-adds the presets.
    let mut copy = c.copy();
    copy.set_index(0, StateId(2)); // a third distinct value
    let mut palette = Vec::new();
    copy.for_each_in_palette(|s| palette.push(s));
    assert!(palette.contains(&StateId(1))); // preset survived the resize
    assert!(palette.contains(&StateId(10)));

    // recreate (Java `new PalettedContainer(valueFor(0), strategy, presets)`)
    // starts single-value at valueFor(0) and re-applies the presets on resize.
    let recreate = c.recreate();
    let mut palette = Vec::new();
    recreate.for_each_in_palette(|s| palette.push(s));
    assert_eq!(palette, vec![StateId(85)]);
    let mut recreate = recreate;
    recreate.set_index(0, StateId(2)); // force a resize, presets reappear
    let mut palette = Vec::new();
    recreate.for_each_in_palette(|s| palette.push(s));
    assert!(palette.contains(&StateId(1)));
    assert!(palette.contains(&StateId(10)));
}

#[test]
fn read_palette_snapshot_oob_panics_on_malformed_storage() {
    // Java's `readPalette` throws "Palette index out of bounds" when the
    // snapshot's entry is null. The port reproduces this on a hostile wire
    // buffer whose storage longs contain an index the (small) palette cannot
    // hold: the LinearPalette snapshot covers [air, stone], so a stored index
    // >= 2 reads out of bounds.
    //
    // Build a 4-bit container with palette [air(0), stone(1)] and storage
    // whose entry 0 holds palette index 2 (malformed: no such entry).
    let mut c = PalettedContainer::new(StateId(0), block_state_strategy());
    c.set_index(0, StateId(1)); // palette [0, 1], 4-bit
    let mut bytes = write_to_bytes(&c);
    // Cell 0's long is written big-endian at bytes[4..12]; entry 0 is the low
    // nibble, i.e. the low nibble of bytes[11]. Force it to a palette index
    // the 2-entry palette does not hold.
    bytes[11] = 0x02;
    let mut buf = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
    c.read(&mut buf);
    // `catch_unwind` needs `AssertUnwindSafe`: `c` owns trait objects that
    // cannot prove `UnwindSafe`.
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| c.get_index(0)));
    assert!(err.is_err());
}
