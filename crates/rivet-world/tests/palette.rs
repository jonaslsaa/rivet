//! Palette-level boundary/property tests (#108).
//!
//! Exercises the four palette types against the `Palette` contract: index
//! assignment, resize requests, uninitialized-use panics, and the wire
//! palette-section write/read/size.

use bytes::BytesMut;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_registry::generated::block_states::{BLOCK_STATE_COUNT, StateId};
use rivet_world::chunk::configuration::{Configuration, PaletteFactoryKind};
use rivet_world::chunk::palette::{
    GlobalIdMap, GlobalPalette, HashMapPalette, IdForResult, LinearPalette, Palette,
    SingleValuePalette,
};
use rivet_world::chunk::strategy::Strategy;

#[derive(Clone, Copy)]
struct TestGlobalMap;

impl GlobalIdMap<StateId> for TestGlobalMap {
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
    fn clone_box(&self) -> Box<dyn GlobalIdMap<StateId> + Send + Sync> {
        Box::new(*self)
    }
}

fn strategy() -> Strategy<StateId> {
    Strategy::create_for_block_states(Box::new(TestGlobalMap))
}

// ---------------------------------------------------------------------------
// SingleValuePalette
// ---------------------------------------------------------------------------

#[test]
fn single_value_assigns_index_zero() {
    let mut p = SingleValuePalette::new(Vec::<StateId>::new());
    assert_eq!(p.id_for(&StateId(0)).expect_no_resize(), 0);
    // Same value again keeps id 0.
    assert_eq!(p.id_for(&StateId(0)).expect_no_resize(), 0);
    assert_eq!(p.get_size(), 1);
}

#[test]
fn single_value_requests_resize_on_different_value() {
    let mut p = SingleValuePalette::new(vec![StateId(0)]);
    match p.id_for(&StateId(1)) {
        IdForResult::Resize { bits, value } => {
            assert_eq!(bits, 1);
            assert_eq!(value, StateId(1));
        }
        IdForResult::Id(_) => panic!("expected resize"),
    }
}

#[test]
fn single_value_value_for_out_of_range_panics() {
    let p = SingleValuePalette::new(vec![StateId(0)]);
    let err = std::panic::catch_unwind(|| p.value_for(1));
    // Java `SingleValuePalette.valueFor` message: "Missing Palette entry for id 1."
    let payload = err.unwrap_err();
    let msg = payload.downcast_ref::<String>().map(String::as_str);
    assert_eq!(msg, Some("Missing Palette entry for id 1."));
}

#[test]
fn single_value_uninitialized_write_panics() {
    let p = SingleValuePalette::new(Vec::<StateId>::new());
    let err = std::panic::catch_unwind(|| {
        p.write(&mut FriendlyByteBuf::new(BytesMut::new()), &TestGlobalMap)
    });
    assert!(err.is_err());
}

#[test]
fn single_value_too_many_entries_panics() {
    let err = std::panic::catch_unwind(|| SingleValuePalette::new(vec![StateId(0), StateId(1)]));
    assert!(err.is_err());
}

#[test]
fn single_value_wire_section() {
    let p = SingleValuePalette::new(vec![StateId(85)]);
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    p.write(&mut buf, &TestGlobalMap);
    assert_eq!(buf.into_inner().to_vec(), vec![0x55]); // varint(85)
    assert_eq!(p.get_serialized_size(&TestGlobalMap), 1);

    // Read back.
    let mut q = SingleValuePalette::new(Vec::<StateId>::new());
    let mut buf = FriendlyByteBuf::new(BytesMut::from(&[0x55u8][..]));
    q.read(&mut buf, &TestGlobalMap);
    assert_eq!(q.value_for(0), StateId(85));
}

// ---------------------------------------------------------------------------
// LinearPalette
// ---------------------------------------------------------------------------

#[test]
fn linear_palette_assigns_and_resizes() {
    let mut p = LinearPalette::new(4, Vec::<StateId>::new()); // capacity 16
    for i in 0..16 {
        assert_eq!(p.id_for(&StateId(i)).expect_no_resize(), i as i32);
    }
    assert_eq!(p.get_size(), 16);
    // 17th distinct value requests a resize to 5 bits.
    match p.id_for(&StateId(100)) {
        IdForResult::Resize { bits, value } => {
            assert_eq!(bits, 5);
            assert_eq!(value, StateId(100));
        }
        IdForResult::Id(_) => panic!("expected resize"),
    }
}

#[test]
fn linear_palette_existing_value_reuses_index() {
    let mut p = LinearPalette::new(4, Vec::<StateId>::new());
    assert_eq!(p.id_for(&StateId(7)).expect_no_resize(), 0);
    assert_eq!(p.id_for(&StateId(7)).expect_no_resize(), 0);
    assert_eq!(p.get_size(), 1);
}

#[test]
fn linear_palette_too_many_entries_panics() {
    let err = std::panic::catch_unwind(|| {
        LinearPalette::new(2, (0..=4).map(StateId).collect::<Vec<_>>()) // capacity 4, 5 entries
    });
    assert!(err.is_err());
}

#[test]
fn linear_palette_value_for_missing_panics() {
    let p = LinearPalette::new(4, vec![StateId(0)]);
    let err = std::panic::catch_unwind(|| p.value_for(5));
    assert!(err.is_err());
}

#[test]
fn linear_palette_wire_section() {
    let p = LinearPalette::new(4, vec![StateId(0), StateId(85), StateId(10)]);
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    p.write(&mut buf, &TestGlobalMap);
    // varint(3), varint(0), varint(85), varint(10).
    assert_eq!(buf.into_inner().to_vec(), vec![0x03, 0x00, 0x55, 0x0A]);
    assert_eq!(p.get_serialized_size(&TestGlobalMap), 4);

    let mut q = LinearPalette::new(4, Vec::<StateId>::new());
    let mut buf = FriendlyByteBuf::new(BytesMut::from(&[0x03, 0x00, 0x55, 0x0A][..]));
    q.read(&mut buf, &TestGlobalMap);
    assert_eq!(q.get_size(), 3);
    assert_eq!(q.value_for(0), StateId(0));
    assert_eq!(q.value_for(1), StateId(85));
    assert_eq!(q.value_for(2), StateId(10));
}

// ---------------------------------------------------------------------------
// HashMapPalette
// ---------------------------------------------------------------------------

#[test]
fn hashmap_palette_insertion_order() {
    let mut p = HashMapPalette::new(5, Vec::<StateId>::new());
    for v in [StateId(9), StateId(10), StateId(85), StateId(1)] {
        p.id_for(&v);
    }
    assert_eq!(
        p.get_entries(),
        vec![StateId(9), StateId(10), StateId(85), StateId(1)]
    );
    assert_eq!(p.get_size(), 4);
}

#[test]
fn hashmap_palette_resizes_at_capacity() {
    let mut p = HashMapPalette::new(4, Vec::<StateId>::new()); // capacity 16
    for i in 0..16 {
        p.id_for(&StateId(i));
    }
    match p.id_for(&StateId(200)) {
        IdForResult::Resize { bits, value } => {
            assert_eq!(bits, 5);
            assert_eq!(value, StateId(200));
        }
        IdForResult::Id(_) => panic!("expected resize"),
    }
}

#[test]
fn hashmap_palette_wire_round_trip() {
    let p = HashMapPalette::new(5, vec![StateId(1), StateId(2), StateId(85)]);
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    p.write(&mut buf, &TestGlobalMap);
    assert_eq!(p.get_serialized_size(&TestGlobalMap), 4); // varint(3) + 3 varints

    let mut q = HashMapPalette::new(5, Vec::<StateId>::new());
    let mut buf = FriendlyByteBuf::new(BytesMut::from(&[0x03, 0x01, 0x02, 0x55][..]));
    q.read(&mut buf, &TestGlobalMap);
    assert_eq!(q.get_entries(), vec![StateId(1), StateId(2), StateId(85)]);
}

// ---------------------------------------------------------------------------
// GlobalPalette
// ---------------------------------------------------------------------------

#[test]
fn global_palette_id_is_global_id() {
    let mut p = GlobalPalette::new(Box::new(TestGlobalMap));
    assert_eq!(p.id_for(&StateId(12345)).expect_no_resize(), 12345);
    assert_eq!(p.value_for(12345), StateId(12345));
    assert_eq!(p.get_size(), BLOCK_STATE_COUNT as i32);
}

#[test]
fn global_palette_write_is_empty() {
    let p = GlobalPalette::new(Box::new(TestGlobalMap));
    let mut buf = FriendlyByteBuf::new(BytesMut::new());
    p.write(&mut buf, &TestGlobalMap);
    assert!(buf.into_inner().is_empty());
    assert_eq!(p.get_serialized_size(&TestGlobalMap), 0);
}

// ---------------------------------------------------------------------------
// Moonrise FastPalette snapshot: raw_palette (#216)
// ---------------------------------------------------------------------------

#[test]
fn single_value_raw_palette_snapshots_the_value() {
    let p = SingleValuePalette::new(vec![StateId(85)]);
    assert_eq!(p.raw_palette(), Some(vec![StateId(85)]));
}

#[test]
fn single_value_uninitialized_raw_palette_is_empty() {
    // Java `SingleValuePalette.moonrise$getRawPalette` returns
    // `new Object[] { this.value }` with a null entry when uninitialized; the
    // Rust snapshot models the null as an absent entry (empty Vec), which the
    // container's read path turns into the Java null-entry read panic.
    let p = SingleValuePalette::new(Vec::<StateId>::new());
    assert_eq!(p.raw_palette(), Some(vec![]));
}

#[test]
fn linear_raw_palette_covers_only_occupied_entries() {
    let mut p = LinearPalette::new(4, Vec::<StateId>::new());
    p.id_for(&StateId(0));
    p.id_for(&StateId(85));
    // Java returns the full `1 << bits` array (null beyond `size`); the port
    // returns the occupied prefix mirroring the `value_for` domain.
    assert_eq!(p.raw_palette(), Some(vec![StateId(0), StateId(85)]));
    assert_eq!(p.get_size(), 2);
}

#[test]
fn hashmap_raw_palette_is_by_id_order() {
    let mut p = HashMapPalette::new(5, Vec::<StateId>::new());
    for v in [StateId(9), StateId(10), StateId(85)] {
        p.id_for(&v);
    }
    // Java's `moonrise$getRawPalette` forwards to the identity map's `byId`
    // array — dense id order, exactly insertion order for the Rust port.
    assert_eq!(
        p.raw_palette(),
        Some(vec![StateId(9), StateId(10), StateId(85)])
    );
}

#[test]
fn global_raw_palette_is_none() {
    // Java `GlobalPalette` does not implement the materialized snapshot; its
    // `moonrise$getRawPalette` is the interface default `null`. The container
    // falls back to `value_for`.
    let p = GlobalPalette::new(Box::new(TestGlobalMap));
    assert_eq!(p.raw_palette(), None);
}

// ---------------------------------------------------------------------------
// Strategy ladder (exact bits-per-entry transitions)
// ---------------------------------------------------------------------------

#[test]
fn block_states_ladder_exact() {
    let s = strategy();
    assert_eq!(s.global_palette_bits_in_memory(), 15); // ceillog2(32366)
    assert_eq!(s.entry_count(), 4096);

    let c0 = s.configuration_for_bit_count(0);
    assert_eq!(c0.bits_in_memory(), 0);
    assert!(matches!(
        c0,
        Configuration::Simple {
            factory: PaletteFactoryKind::SingleValue,
            bits: 0
        }
    ));

    for bits in 1..=4 {
        let c = s.configuration_for_bit_count(bits);
        assert_eq!(c.bits_in_memory(), 4, "bits {bits} -> in-memory 4");
        assert!(matches!(
            c,
            Configuration::Simple {
                factory: PaletteFactoryKind::Linear,
                bits: 4
            }
        ));
    }
    for bits in 5..=8 {
        let c = s.configuration_for_bit_count(bits);
        assert_eq!(c.bits_in_memory(), bits, "bits {bits} -> in-memory {bits}");
        assert!(matches!(
            c,
            Configuration::Simple {
                factory: PaletteFactoryKind::HashMap,
                ..
            }
        ));
    }
    for bits in [9, 12, 15] {
        let c = s.configuration_for_bit_count(bits);
        assert!(
            matches!(c, Configuration::Global { bits_in_memory: 15, bits_in_storage: b } if b == bits)
        );
        assert!(c.always_repack());
    }
}

#[test]
fn biomes_ladder_exact() {
    let s = Strategy::create_for_biomes(Box::new(TestGlobalMap));
    assert_eq!(s.entry_count(), 64);
    assert_eq!(s.global_palette_bits_in_memory(), 15);

    let c0 = s.configuration_for_bit_count(0);
    assert!(matches!(
        c0,
        Configuration::Simple {
            factory: PaletteFactoryKind::SingleValue,
            bits: 0
        }
    ));
    for (bits, in_memory) in [(1, 1), (2, 2), (3, 3)] {
        let c = s.configuration_for_bit_count(bits);
        assert_eq!(c.bits_in_memory(), in_memory);
        assert!(matches!(
            c,
            Configuration::Simple {
                factory: PaletteFactoryKind::Linear,
                ..
            }
        ));
    }
    let c4 = s.configuration_for_bit_count(4);
    assert!(matches!(
        c4,
        Configuration::Global {
            bits_in_memory: 15,
            bits_in_storage: 4
        }
    ));
}

#[test]
fn configuration_for_palette_size_uses_ceillog2() {
    let s = strategy();
    // palette size 1 -> ceillog2(1) = 0 -> zero bits.
    assert_eq!(s.configuration_for_palette_size(1).bits_in_memory(), 0);
    // palette size 16 -> 4 -> FOUR_BITS_LINEAR.
    assert_eq!(s.configuration_for_palette_size(16).bits_in_memory(), 4);
    // palette size 17 -> 5 -> FIVE_BITS_HASHMAP.
    assert_eq!(s.configuration_for_palette_size(17).bits_in_memory(), 5);
    // palette size 256 -> 8 -> EIGHT_BITS_HASHMAP.
    assert_eq!(s.configuration_for_palette_size(256).bits_in_memory(), 8);
    // palette size 300 -> 9 -> Global.
    assert!(s.configuration_for_palette_size(300).always_repack());
}

#[test]
fn strategy_get_index_xyzw() {
    let s = strategy();
    assert_eq!(s.get_index(1, 0, 0), 1);
    assert_eq!(s.get_index(0, 0, 1), 16);
    assert_eq!(s.get_index(0, 1, 0), 256);
    assert_eq!(s.get_index(15, 15, 15), 4095);
    assert_eq!(s.get_index(0, 0, 0), 0);
}
