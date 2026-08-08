//! Issue #230 / #183-a — the `chunk.wire` read-view closure DoD: a
//! `SerializableChunkData`-style section tag decodes through the factory +
//! `PalettedContainerRO` read-view layer.
//!
//! Java's `SerializableChunkData` section loop (MC 26.2) reads each `sections`
//! entry's `block_states`/`biomes` compounds through
//! `containerFactory.blockStatesContainerCodec()` /
//! `biomeContainerRWCodec()`. Each codec decodes `{ palette: List<element>,
//! data?: long[] }` into a `PackedData` and runs
//! `PalettedContainer.unpack(strategy, packedData, defaultValue, null)` — the
//! same `unpack` this test drives. Missing compounds fall back to
//! `factory.createForBlockStates()/createForBiomes()` (`orElseGet`).
//!
//! NOTE — the palette is modeled as the **value layer only** here: entries are
//! `IntTag`s (the port's `StateId`/`BiomeId` wire stand-ins), not the
//! `{Name, Properties}` block-state compounds Java's `BlockState.CODEC`
//! decodes. The test pins the container read-view/unpack behavior — the piece
//! this closure owns — not the block-state codec; #202 adds
//! `NbtUtils.readBlockState` and the real compound codecs. The fixture shape
//! (section 0, `[air, stone]` 4-bit Linear palette, stone layer at section-y
//! 0, plains biome) is the committed PR #194 golden chunk's section 0.
//!
//! The test hand-builds that section tag and decodes it, asserting the exact
//! golden values (stone/air layout, 4 bits, serialized size, re-pack identity).

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::tag::Tag;
use rivet_registry::generated::block_states::StateId;
use rivet_world::chunk::palette::GlobalIdMap;
use rivet_world::chunk::paletted_container::{PackedData, PalettedContainer, PalettedContainerRO};
use rivet_world::chunk::paletted_container_factory::PalettedContainerFactory;
use rivet_world::chunk::strategy::Strategy;

/// `BiomeId` — a dense biome global-id wrapper (mirrors the superflat golden
/// test's type; plains = 40).
#[derive(Clone, Copy, PartialEq, Debug)]
struct BiomeId(u16);

/// The dense generated block-state global id map.
#[derive(Clone, Copy)]
struct BlockStateGlobalMap;
impl GlobalIdMap<StateId> for BlockStateGlobalMap {
    fn get_id(&self, value: &StateId) -> i32 {
        value.0 as i32
    }
    fn by_id_or_throw(&self, id: i32) -> StateId {
        assert!(
            (0..rivet_registry::generated::block_states::BLOCK_STATE_COUNT as i32).contains(&id),
            "No value with id {id}"
        );
        StateId(id as u16)
    }
    fn size(&self) -> i32 {
        rivet_registry::generated::block_states::BLOCK_STATE_COUNT as i32
    }
    fn by_id(&self, id: i32) -> Option<StateId> {
        (0..rivet_registry::generated::block_states::BLOCK_STATE_COUNT as i32)
            .contains(&id)
            .then_some(StateId(id as u16))
    }
    fn clone_box(&self) -> Box<dyn GlobalIdMap<StateId>> {
        Box::new(*self)
    }
}

/// `BiomeId` global map (plains = 40, 66-entry registry).
#[derive(Clone, Copy)]
struct BiomeGlobalMap;
impl GlobalIdMap<BiomeId> for BiomeGlobalMap {
    fn get_id(&self, value: &BiomeId) -> i32 {
        value.0 as i32
    }
    fn by_id_or_throw(&self, id: i32) -> BiomeId {
        assert!((0..66).contains(&id), "No value with id {id}");
        BiomeId(id as u16)
    }
    fn size(&self) -> i32 {
        66
    }
    fn by_id(&self, id: i32) -> Option<BiomeId> {
        (0..66).contains(&id).then_some(BiomeId(id as u16))
    }
    fn clone_box(&self) -> Box<dyn GlobalIdMap<BiomeId>> {
        Box::new(*self)
    }
}

fn factory() -> PalettedContainerFactory<StateId, BiomeId> {
    PalettedContainerFactory::new(
        Strategy::create_for_block_states(Box::new(BlockStateGlobalMap)),
        StateId(0),
        Strategy::create_for_biomes(Box::new(BiomeGlobalMap)),
        BiomeId(40),
    )
}

/// The golden section-0 block `data` long array: the 256-entry stone layer at
/// section-y 0 packs to 16 longs of `0x1111…` (4 bits per entry, value 1),
/// the remaining 3840 air entries to 240 zero longs.
fn golden_block_data() -> Vec<i64> {
    let mut data = vec![0x1111_1111_1111_1111i64; 16];
    data.resize(256, 0);
    data
}

/// Hand-build the `SerializableChunkData` section tag for the golden section 0
/// (section y -4, block_states/biomes, no light arrays).
fn golden_section_tag() -> CompoundTag {
    let mut block_states = CompoundTag::new();
    block_states.put(
        "palette".into(),
        Tag::List(ListTag::with_list(vec![
            Tag::Int(IntTag::value_of(0)),
            Tag::Int(IntTag::value_of(1)),
        ])),
    );
    block_states.put_long_array("data", golden_block_data());

    let mut biomes = CompoundTag::new();
    biomes.put(
        "palette".into(),
        Tag::List(ListTag::with_list(vec![Tag::Int(IntTag::value_of(40))])),
    );

    let mut section = CompoundTag::new();
    section.put_byte("Y", -4);
    section.put("block_states".into(), Tag::Compound(block_states));
    section.put("biomes".into(), Tag::Compound(biomes));
    section
}

/// Java's `PalettedContainer.codec` decode: read the `palette` int list and
/// optional `data` long array, build a `PackedData` (declared bits unknown),
/// and `unpack` with the factory's strategy.
fn decode_block_states(
    factory: &PalettedContainerFactory<StateId, BiomeId>,
    tag: &CompoundTag,
) -> Result<PalettedContainer<StateId>, String> {
    let Some(block_states) = tag.get_compound("block_states") else {
        return Ok(factory.create_for_block_states());
    };
    let palette: Vec<StateId> = block_states
        .get_list("palette")
        .map(|list| {
            list.iter()
                .map(|entry| {
                    let Tag::Int(id) = entry else {
                        panic!("block_states.palette entries must be ints");
                    };
                    // Java's `BlockState.CODEC` fails on an out-of-range
                    // registry id rather than truncating it.
                    let count = rivet_registry::generated::block_states::BLOCK_STATE_COUNT as i32;
                    if !(0..count).contains(&id.value) {
                        return Err(format!(
                            "block_states.palette id {} out of range 0..{count}",
                            id.value
                        ));
                    }
                    Ok(StateId(id.value as u16))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    let storage = block_states.get_long_array("data").cloned();
    let packed = PackedData::new(palette, storage);
    PalettedContainer::unpack(factory.block_states_strategy(), packed)
}

fn decode_biomes(
    factory: &PalettedContainerFactory<StateId, BiomeId>,
    tag: &CompoundTag,
) -> Result<PalettedContainer<BiomeId>, String> {
    let Some(biomes) = tag.get_compound("biomes") else {
        return Ok(factory.create_for_biomes());
    };
    let palette: Vec<BiomeId> = biomes
        .get_list("palette")
        .map(|list| {
            list.iter()
                .map(|entry| {
                    let Tag::Int(id) = entry else {
                        panic!("biomes.palette entries must be ints");
                    };
                    // Java's biome codec fails on an out-of-range registry id
                    // rather than truncating it (66 biome ids in this port).
                    if !(0..66).contains(&id.value) {
                        return Err(format!("biomes.palette id {} out of range 0..66", id.value));
                    }
                    Ok(BiomeId(id.value as u16))
                })
                .collect::<Result<Vec<_>, String>>()
        })
        .transpose()?
        .unwrap_or_default();
    let storage = biomes.get_long_array("data").cloned();
    let packed = PackedData::new(palette, storage);
    PalettedContainer::unpack(factory.biome_strategy(), packed)
}

#[test]
fn golden_section_tag_decodes_through_read_view() {
    let factory = factory();
    let tag = golden_section_tag();

    let states = decode_block_states(&factory, &tag).expect("block_states decode");
    let biomes = decode_biomes(&factory, &tag).expect("biomes decode");

    // Read through the `PalettedContainerRO` trait (the read-view surface).
    let read: &dyn PalettedContainerRO<StateId> = &states;
    assert_eq!(read.bits_per_entry(), 4);
    // The whole section-y-0 plane is stone; everything above is air.
    for z in 0..16 {
        for x in 0..16 {
            assert_eq!(read.get(x, 0, z), StateId(1), "stone at ({x},0,{z})");
            assert_eq!(read.get(x, 1, z), StateId(0), "air at ({x},1,{z})");
        }
    }
    assert_eq!(read.get_serialized_size(), 1 + 3 + 256 * 8); // bits + palette + raw.

    let biome_read: &dyn PalettedContainerRO<BiomeId> = &biomes;
    assert_eq!(biome_read.bits_per_entry(), 0);
    assert_eq!(biome_read.get(0, 0, 0), BiomeId(40));
}

#[test]
fn unpack_then_pack_reencodes_in_storage_order() {
    // Java's `pack()` re-encodes against a fresh `HashMapPalette`, filling it
    // in storage traversal order: storage index 0 is the stone layer (section
    // y=0), so the re-packed palette is `[stone, air]`, not the original
    // `[air, stone]` — and the re-packed storage re-maps stone to id 0. This
    // pins that exact re-encode, and that the read-view `pack(Strategy)`
    // agrees with the container's own-strategy `pack()`.
    let factory = factory();
    let tag = golden_section_tag();
    let states = decode_block_states(&factory, &tag).expect("decode");

    let repacked = states.pack();
    let read_view_packed = PalettedContainerRO::pack(&states, factory.block_states_strategy());
    // The read-view `pack(Strategy)` must match the container's own-strategy
    // `pack()` field-for-field — comparing only the bit width would let a
    // same-width wrong palette/storage pass.
    assert_eq!(read_view_packed.palette_entries, repacked.palette_entries);
    assert_eq!(read_view_packed.storage, repacked.storage);
    assert_eq!(read_view_packed.bits_per_entry, repacked.bits_per_entry);
    assert_eq!(repacked.palette_entries, vec![StateId(1), StateId(0)]);
    assert_eq!(repacked.bits_per_entry, 4);
    // Stone (id 0 in the re-packed palette) occupies indices 0..255, air
    // (id 1) the rest.
    let mut expected = vec![0i64; 16];
    expected.resize(256, 0x1111_1111_1111_1111i64);
    assert_eq!(repacked.storage.as_deref().unwrap(), &expected);
}

#[test]
fn out_of_range_palette_id_errors_not_truncates() {
    // Java's `BlockState.CODEC` fails on an out-of-range registry id; a
    // truncating `as u16` would silently map 0x20000 to air (id 0) and the
    // golden assertions would pass on the wrong state. The decode helper must
    // error instead.
    let factory = factory();
    let mut tag = golden_section_tag();
    let block_states = tag.get_compound_or_empty_mut("block_states");
    block_states.put(
        "palette".into(),
        Tag::List(ListTag::with_list(vec![Tag::Int(IntTag::value_of(
            0x20000,
        ))])),
    );
    let err = decode_block_states(&factory, &tag)
        .err()
        .expect("out-of-range id must error");
    assert!(err.contains("out of range"), "err: {err}");
}

#[test]
fn missing_block_states_defaults_to_factory_all_air() {
    let factory = factory();
    let mut tag = golden_section_tag();
    tag.remove("block_states");

    let states = decode_block_states(&factory, &tag).expect("default decode");
    // `createForBlockStates()` — all air (default), 0 bits (single value).
    assert_eq!(states.bits_per_entry(), 0);
    assert_eq!(states.get(0, 0, 0), StateId(0));
    assert_eq!(states.get(15, 15, 15), StateId(0));
    assert!(!states.maybe_has(|s| *s != StateId(0)));
}

#[test]
fn malformed_declared_bits_errors_like_java() {
    let factory = factory();
    let tag = golden_section_tag();
    let mut packed = decode_block_states(&factory, &tag).expect("decode").pack();
    // `bitsPerEntry` 99 vs calculated 4 -> `Invalid bit count, calculated 4, but
    // container declared 99`.
    packed.bits_per_entry = 99;
    let err = PalettedContainer::<StateId>::unpack(factory.block_states_strategy(), packed)
        .err()
        .expect("declared-bit mismatch must error");
    assert!(err.contains("Invalid bit count"), "err: {err}");
}

#[test]
fn missing_data_for_nonzero_storage_errors_like_java() {
    let factory = factory();
    let mut tag = golden_section_tag();
    // Drop the `data` long array; the 2-entry palette needs 4-bit storage.
    let block_states = tag.get_compound_or_empty_mut("block_states");
    block_states.remove("data");
    let err = decode_block_states(&factory, &tag)
        .err()
        .expect("must error");
    assert_eq!(err, "Missing values for non-zero storage");
}

#[test]
fn factory_create_for_biomes_holds_plains_default() {
    let factory = factory();
    let biomes = factory.create_for_biomes();
    assert_eq!(biomes.get(0, 0, 0), BiomeId(40));
    assert_eq!(biomes.get(3, 3, 3), BiomeId(40));
    assert_eq!(biomes.bits_per_entry(), 0);
}
