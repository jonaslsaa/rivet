//! Paper 26.2 `SerializableChunkData.sections` reconstruction fixtures and
//! hostile counterexamples (#336).

use std::io::Cursor;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::nbt_utils::write_block_state;
use rivet_nbt::string_tag::StringTag;
use rivet_nbt::tag::Tag;
use rivet_registry::block_state::BlockState;
use rivet_registry::generated::blocks::BlockId;
use rivet_util::DataInputStream;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId, SectionBlockPredicates, current_version_container_factory, reconstruct_sections,
    reconstruct_sections_with_presets,
};

fn predicates() -> SectionBlockPredicates {
    SectionBlockPredicates {
        is_air: |state| state.is_air(),
        is_randomly_ticking: |state| state.random_ticking(),
        fluid_is_empty: |state| state.fluid_empty(),
        // Lava is Paper's only randomly-ticking vanilla fluid; water (including
        // waterlogged states) is not randomly ticking.
        fluid_is_randomly_ticking: |state| {
            !state.fluid_empty() && state.block().name() == "minecraft:lava"
        },
        // The real-fixture assertions do not contain large collision shapes or
        // moving pistons. The callback seam is tested separately below.
        is_special_colliding: |_| false,
    }
}

fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).expect("fixture block exists"))
}

fn state_tag(name: &str) -> Tag {
    Tag::Compound(write_block_state(block(name)))
}

fn container(palette: Vec<Tag>, data: Option<Vec<i64>>) -> CompoundTag {
    let mut tag = CompoundTag::new();
    tag.put("palette".into(), Tag::List(ListTag::with_list(palette)));
    if let Some(data) = data {
        tag.put_long_array("data", data);
    }
    tag
}

fn section(y: i8, states: Option<CompoundTag>, biomes: Option<CompoundTag>) -> CompoundTag {
    let mut tag = CompoundTag::new();
    tag.put_byte("Y", y);
    if let Some(states) = states {
        tag.put("block_states".into(), Tag::Compound(states));
    }
    if let Some(biomes) = biomes {
        tag.put("biomes".into(), Tag::Compound(biomes));
    }
    tag
}

fn plains() -> CompoundTag {
    container(
        vec![Tag::String(StringTag::value_of("minecraft:plains".into()))],
        None,
    )
}

fn list(tags: Vec<CompoundTag>) -> ListTag {
    ListTag::with_list(tags.into_iter().map(Tag::Compound).collect())
}

fn read_fixture(bytes: &[u8]) -> CompoundTag {
    let mut input = DataInputStream::new(Cursor::new(bytes));
    nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("26.2 fixture parses")
}

#[test]
fn real_26_2_spawn_fixtures_reconstruct_negative_y_sections() {
    const FIXTURES: [&[u8]; 2] = [
        include_bytes!("../../../tools/rivet-oracle/fixtures/chunk/overworld/0.0/0.0.nbt"),
        include_bytes!("../../../tools/rivet-oracle/fixtures/chunk/overworld/0.0/0.1.nbt"),
    ];
    let factory = current_version_container_factory();

    for bytes in FIXTURES {
        let root = read_fixture(bytes);
        let section_tags = root.get_list("sections").expect("Paper sections list");
        let sections = reconstruct_sections(section_tags, -4, 19, &factory, predicates())
            .expect("real Paper section reconstruction");

        assert_eq!(sections.len(), 24);
        assert!(sections.iter().all(Option::is_some));
        // The pinned overworld fixture carries the negative lower block bound.
        // Hostile -5/20 padding-boundary exclusions are pinned separately.
        assert!(
            section_tags
                .compound_stream()
                .any(|tag| tag.get_byte_or("Y", 0) == -4)
        );
        assert_eq!(
            sections[0].as_ref().unwrap().get_noise_biome(0, 0, 0),
            BiomeId::PLAINS
        );
    }
}

#[test]
fn y_mapping_defaults_to_zero_skips_bounds_and_last_duplicate_wins() {
    let factory = current_version_container_factory();
    let malformed = container(
        vec![state_tag("minecraft:stone"), state_tag("minecraft:dirt")],
        None,
    );
    let mut default_y = section(
        7,
        Some(container(vec![state_tag("minecraft:stone")], None)),
        Some(plains()),
    );
    default_y.remove("Y"); // `getByteOr("Y", 0)`.
    let tags = list(vec![
        // These malformed containers must not be evaluated outside the bounds.
        section(-5, Some(malformed.clone()), Some(plains())),
        section(20, Some(malformed), Some(plains())),
        section(
            -4,
            Some(container(vec![state_tag("minecraft:stone")], None)),
            Some(plains()),
        ),
        default_y,
        // Duplicate Y=0: Java's later array assignment wins.
        section(
            0,
            Some(container(vec![state_tag("minecraft:dirt")], None)),
            Some(plains()),
        ),
    ]);

    let sections = reconstruct_sections(&tags, -4, 19, &factory, predicates()).unwrap();
    assert_eq!(sections.len(), 24);
    assert_eq!(
        sections[0].as_ref().unwrap().get_block_state(0, 0, 0),
        block("minecraft:stone")
    );
    assert_eq!(
        sections[4].as_ref().unwrap().get_block_state(0, 0, 0),
        block("minecraft:dirt")
    );
    assert_eq!(sections.iter().filter(|s| s.is_some()).count(), 2);
}

#[test]
fn non_compounds_are_skipped_and_inverted_bounds_are_empty() {
    let factory = current_version_container_factory();
    let tags = ListTag::with_list(vec![Tag::Int(IntTag::value_of(7))]);
    let sections = reconstruct_sections(&tags, -4, 19, &factory, predicates()).unwrap();
    assert!(sections.iter().all(Option::is_none));
    assert!(
        reconstruct_sections(&tags, 1, 0, &factory, predicates())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn missing_containers_default_independently_and_blocks_fail_first() {
    let factory = current_version_container_factory();
    let missing_both = list(vec![section(0, None, None)]);
    let sections = reconstruct_sections(&missing_both, 0, 0, &factory, predicates()).unwrap();
    let decoded = sections[0].as_ref().unwrap();
    assert_eq!(decoded.get_block_state(15, 15, 15), block("minecraft:air"));
    assert_eq!(decoded.get_noise_biome(3, 3, 3), BiomeId::PLAINS);
    assert_eq!(decoded.non_empty_block_count(), 0);

    let malformed = CompoundTag::new(); // missing required `palette`
    let tags = list(vec![section(0, Some(malformed.clone()), Some(malformed))]);
    let err = reconstruct_sections(&tags, 0, 0, &factory, predicates())
        .err()
        .expect("block-state failure");
    assert_eq!(err.container, "block_states");
    assert_eq!(err.section_y, 0);
    assert_eq!(err.message, "Missing required field: palette");

    let tags = list(vec![section(0, None, Some(CompoundTag::new()))]);
    let err = reconstruct_sections(&tags, 0, 0, &factory, predicates())
        .err()
        .expect("biome failure");
    assert_eq!(err.container, "biomes");
}

#[test]
fn unknown_block_properties_and_biomes_use_codec_defaults() {
    let factory = current_version_container_factory();
    let mut unknown = CompoundTag::new();
    unknown.put_string("Name", "minecraft:not_a_block");

    let mut properties = CompoundTag::new();
    properties.put_string("bogus", "x");
    properties.put_string("axis", "sideways");
    let mut oak_log = CompoundTag::new();
    oak_log.put_string("Name", "minecraft:oak_log");
    oak_log.put("Properties".into(), Tag::Compound(properties));

    let unknown_biome = container(
        vec![Tag::String(StringTag::value_of(
            "minecraft:not_a_biome".into(),
        ))],
        None,
    );
    let tags = list(vec![
        section(
            0,
            Some(container(vec![Tag::Compound(unknown)], None)),
            Some(unknown_biome),
        ),
        section(
            1,
            Some(container(vec![Tag::Compound(oak_log)], None)),
            Some(plains()),
        ),
    ]);
    let sections = reconstruct_sections(&tags, 0, 1, &factory, predicates()).unwrap();

    let defaults = sections[0].as_ref().unwrap();
    assert_eq!(defaults.get_block_state(0, 0, 0), block("minecraft:air"));
    assert_eq!(defaults.get_noise_biome(0, 0, 0), BiomeId::PLAINS);
    let log = sections[1].as_ref().unwrap();
    assert_eq!(log.get_block_state(0, 0, 0), block("minecraft:oak_log"));
    assert_eq!(log.non_empty_block_count(), 4096);
}

#[test]
fn malformed_palette_entries_degrade_before_storage_validation() {
    let factory = current_version_container_factory();
    let wrong_entry = Tag::Int(IntTag::value_of(123));
    // A wrong singleton element becomes air and needs no `data`.
    let tags = list(vec![section(
        0,
        Some(container(vec![wrong_entry.clone()], None)),
        Some(plains()),
    )]);
    let sections = reconstruct_sections(&tags, 0, 0, &factory, predicates()).unwrap();
    assert_eq!(
        sections[0].as_ref().unwrap().get_block_state(0, 0, 0),
        block("minecraft:air")
    );

    // Two entries select non-zero storage only after both element fallbacks;
    // absent data then fails at the unpack phase.
    let tags = list(vec![section(
        0,
        Some(container(vec![wrong_entry.clone(), wrong_entry], None)),
        Some(plains()),
    )]);
    let err = reconstruct_sections(&tags, 0, 0, &factory, predicates())
        .err()
        .expect("missing packed data");
    assert_eq!(err.message, "Missing values for non-zero storage");
}

#[test]
fn singleton_ignores_absent_or_malformed_optional_data() {
    let factory = current_version_container_factory();
    let singleton = container(vec![state_tag("minecraft:stone")], None);
    let mut malformed_data = singleton.clone();
    malformed_data.put_int("data", 12); // lenient optional -> absent
    let mut extra_data = singleton.clone();
    extra_data.put_long_array("data", vec![123]); // zero-bit path ignores it

    for states in [singleton, malformed_data, extra_data] {
        let tags = list(vec![section(0, Some(states), Some(plains()))]);
        let sections = reconstruct_sections(&tags, 0, 0, &factory, predicates()).unwrap();
        let decoded = sections[0].as_ref().unwrap();
        assert_eq!(decoded.states().bits_per_entry(), 0);
        assert_eq!(decoded.non_empty_block_count(), 4096);
    }
}

#[test]
fn packed_storage_length_errors_are_container_scoped() {
    let factory = current_version_container_factory();
    let states = container(
        vec![state_tag("minecraft:air"), state_tag("minecraft:stone")],
        Some(vec![0; 255]), // 4 bits * 4096 entries requires 256 longs
    );
    let tags = list(vec![section(0, Some(states), Some(plains()))]);
    let err = reconstruct_sections(&tags, 0, 0, &factory, predicates())
        .err()
        .expect("malformed packed data");
    assert_eq!(err.container, "block_states");
    assert_eq!(
        err.message,
        "Failed to read PalettedContainer: Invalid length given for storage, got: 255 but expected: 256"
    );
}

#[test]
fn block_and_biome_palette_width_boundaries_match_26_2_strategies() {
    let factory = current_version_container_factory();
    let block_palette = |count: usize| {
        (0..count)
            .map(|id| {
                let state =
                    BlockState::new(rivet_registry::generated::block_states::StateId(id as u16));
                Tag::Compound(write_block_state(state))
            })
            .collect::<Vec<_>>()
    };
    let biome_palette = |count: usize| {
        rivet_registry::generated::biomes::BIOME_BY_ID[..count]
            .iter()
            .map(|name| Tag::String(StringTag::value_of((*name).into())))
            .collect::<Vec<_>>()
    };

    // Block states: 16 entries stay at the forced four-bit linear palette;
    // 17 entries cross to the five-bit hashmap palette.
    for (count, longs, bits) in [(16, 256, 4), (17, 342, 5)] {
        let tags = list(vec![section(
            0,
            Some(container(block_palette(count), Some(vec![0; longs]))),
            Some(plains()),
        )]);
        let sections = reconstruct_sections(&tags, 0, 0, &factory, predicates()).unwrap();
        assert_eq!(
            sections[0].as_ref().unwrap().states().bits_per_entry(),
            bits
        );
    }

    // Biomes: 8 entries use the local three-bit palette; 9 entries select
    // global-on-memory storage (7 bits for the 66-entry registry) after the
    // four-bit on-disc data is repacked.
    for (count, disc_longs, memory_bits) in [(8, 4, 3), (9, 4, 7)] {
        let tags = list(vec![section(
            0,
            Some(container(vec![state_tag("minecraft:air")], None)),
            Some(container(biome_palette(count), Some(vec![0; disc_longs]))),
        )]);
        let sections = reconstruct_sections(&tags, 0, 0, &factory, predicates()).unwrap();
        assert_eq!(
            sections[0].as_ref().unwrap().biomes().bits_per_entry(),
            memory_bits
        );
    }
}

#[test]
fn presets_are_selected_after_bounds_and_feed_unpack() {
    let factory = current_version_container_factory();
    let tags = list(vec![
        section(-1, Some(CompoundTag::new()), Some(plains())),
        section(
            0,
            Some(container(vec![state_tag("minecraft:stone")], None)),
            Some(plains()),
        ),
        section(1, None, Some(plains())),
    ]);
    let mut calls = Vec::new();
    let sections = reconstruct_sections_with_presets(&tags, 0, 1, &factory, predicates(), |y| {
        calls.push(y);
        Some(vec![block("minecraft:dirt")])
    })
    .unwrap();
    assert_eq!(calls, [0, 1]);
    let states = sections[0].as_ref().unwrap().states();
    let mut palette = Vec::new();
    states.for_each_in_palette(|state| palette.push(state));
    assert!(palette.contains(&block("minecraft:stone")));
    assert!(palette.contains(&block("minecraft:dirt")));
}

#[test]
fn reconstructed_counts_use_all_supplied_216_predicates() {
    let factory = current_version_container_factory();
    let tags = list(vec![section(
        0,
        Some(container(vec![state_tag("minecraft:stone")], None)),
        Some(plains()),
    )]);
    let sections = reconstruct_sections(
        &tags,
        0,
        0,
        &factory,
        SectionBlockPredicates {
            is_air: |_| false,
            is_randomly_ticking: |_| true,
            fluid_is_empty: |_| false,
            fluid_is_randomly_ticking: |_| true,
            is_special_colliding: |_| true,
        },
    )
    .unwrap();
    let decoded = sections[0].as_ref().unwrap();
    assert_eq!(decoded.non_empty_block_count(), 4096);
    assert_eq!(decoded.fluid_count(), 4096);
    assert_eq!(decoded.ticking_block_count(), 4096);
    assert_eq!(decoded.ticking_fluid_count(), 4096);
    assert!(decoded.has_special_colliding_blocks());
    assert_eq!(decoded.ticking_blocks().size(), 4096);
}
