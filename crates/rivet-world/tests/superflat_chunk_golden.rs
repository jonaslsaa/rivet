//! Integration tests for the deterministic single-stone superflat chunk
//! content (issue #100): build the 24-section content with the real generated
//! block-state table, assemble the merged #94
//! `ClientboundLevelChunkWithLightPacket`, and byte-compare every region
//! (buffer / heightmaps / light / body / full) against the committed PR #194
//! capture fixtures in `rivet-protocol/tests/fixtures/`.
//!
//! The fixtures are the canonical spawn chunk (coords -5/-4) from a live
//! single-stone superflat join: 24 sections (section Y=-4 with one stone layer
//! at y=-64, the rest air), three 9-bit heightmaps (all stored offsets 1), and
//! the full-sky light payload. This test proves the pure content builder
//! produces byte-identical output to Paper's `fillFromNoise` + chunk-send path
//! for that capture.

use bytes::BytesMut;
use rivet_protocol::codec::StreamEncoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::protocol::game::clientbound_level_chunk_with_light::ClientboundLevelChunkWithLightPacket;
use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::RegistryAccess;
use rivet_registry::generated::block_states::{BLOCK_STATE_COUNT, StateId};
use rivet_world::chunk::level_chunk_section::LevelChunkSection;
use rivet_world::chunk::palette::GlobalIdMap;
use rivet_world::chunk::paletted_container::PalettedContainer;
use rivet_world::chunk::strategy::Strategy;
use rivet_world::superflat::{BlockFlags, SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y, build_superflat};

const GOLDEN_FULL: &str = include_str!("../../rivet-protocol/tests/fixtures/chunk_golden_full.hex");
const GOLDEN_BODY: &str = include_str!("../../rivet-protocol/tests/fixtures/chunk_golden_body.hex");
const GOLDEN_BUFFER: &str =
    include_str!("../../rivet-protocol/tests/fixtures/chunk_golden_buffer.hex");
const GOLDEN_LIGHT: &str =
    include_str!("../../rivet-protocol/tests/fixtures/chunk_golden_light.hex");
const GOLDEN_HEIGHTMAPS: &str =
    include_str!("../../rivet-protocol/tests/fixtures/chunk_golden_heightmaps.hex");

fn hex(s: &str) -> Vec<u8> {
    let trimmed: String = s.trim().chars().filter(|c| !c.is_whitespace()).collect();
    (0..trimmed.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).unwrap())
        .collect()
}

/// `BiomeId` — a dense biome global-id wrapper (rivet-registry has no biome
/// table yet; the capture's single biome is plains, id 40).
#[derive(Clone, Copy, PartialEq, Debug)]
struct BiomeId(u16);

/// The dense generated block-state global id map (mirrors
/// `tests/paletted_container.rs`'s map).
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

/// A biome global map for the superflat capture: id `n` <-> `BiomeId(n)`.
#[derive(Clone, Copy)]
struct BiomeGlobalMap;

impl GlobalIdMap<BiomeId> for BiomeGlobalMap {
    fn get_id(&self, value: &BiomeId) -> i32 {
        value.0 as i32
    }
    fn by_id_or_throw(&self, id: i32) -> BiomeId {
        assert!(id >= 0, "No value with id {id}");
        BiomeId(id as u16)
    }
    fn size(&self) -> i32 {
        66 // the 26.2 biome registry (plains = 40, alphabetical).
    }
    fn by_id(&self, id: i32) -> Option<BiomeId> {
        (id >= 0).then_some(BiomeId(id as u16))
    }
    fn clone_box(&self) -> Box<dyn GlobalIdMap<BiomeId>> {
        Box::new(*self)
    }
}

fn block_state_strategy() -> Strategy<StateId> {
    Strategy::create_for_block_states(Box::new(BlockStateGlobalMap))
}

fn biome_strategy() -> Strategy<BiomeId> {
    Strategy::create_for_biomes(Box::new(BiomeGlobalMap))
}

/// The predicates for the superflat air + stone content: air is air (not
/// opaque), stone blocks motion, has no fluid, is not leaves.
fn is_air(s: &StateId) -> bool {
    s.0 == 0
}
fn blocks_motion(s: &StateId) -> bool {
    s.0 != 0
}
fn has_fluid(_s: &StateId) -> bool {
    false
}
fn is_leaves(_s: &StateId) -> bool {
    false
}

/// The `BlockFlags` for the superflat content (air + stone).
fn superflat_flags() -> BlockFlags<StateId> {
    BlockFlags {
        is_air: &is_air,
        blocks_motion: &blocks_motion,
        has_fluid: &has_fluid,
        is_leaves: &is_leaves,
    }
}

fn build_packet() -> ClientboundLevelChunkWithLightPacket {
    let content = build_superflat(
        block_state_strategy(),
        biome_strategy(),
        StateId(0),
        StateId(1),
        BiomeId(40),
        superflat_flags(),
    );
    ClientboundLevelChunkWithLightPacket::new(
        -5,
        -4,
        content.chunk_packet_data(),
        content.light_data,
    )
}

fn encode(packet: &ClientboundLevelChunkWithLightPacket) -> Vec<u8> {
    let mut buf = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
    ClientboundLevelChunkWithLightPacket::stream_codec()
        .encode(&mut buf, packet)
        .expect("superflat chunk encodes");
    buf.into_inner().to_vec()
}

#[test]
fn full_packet_bytes_match_fixture() {
    let packet = build_packet();
    assert_eq!(packet.x(), -5);
    assert_eq!(packet.z(), -4);
    assert_eq!(encode(&packet), hex(GOLDEN_FULL));
}

#[test]
fn buffer_heightmaps_light_and_body_regions_match() {
    let packet = build_packet();
    let chunk = packet.chunk_data();
    // The opaque sections buffer is the 2242-byte fixture exactly.
    assert_eq!(chunk.buffer(), &hex(GOLDEN_BUFFER)[..]);
    assert!(chunk.block_entities().is_empty());
    // Heightmaps: the three client types in EnumMap order, each the 37-long
    // 9-bit all-ones-offset array.
    let expected_types = [
        HeightmapType::WorldSurface,
        HeightmapType::MotionBlocking,
        HeightmapType::MotionBlockingNoLeaves,
    ];
    assert_eq!(
        chunk
            .heightmaps()
            .iter()
            .map(|(ty, _)| *ty)
            .collect::<Vec<_>>(),
        expected_types.to_vec()
    );
    let expected_raw: Vec<i64> = {
        let mut v = vec![0x0040_2010_0804_0201i64; 36];
        v.push(0x0000_0000_0804_0201i64);
        v
    };
    for (_, raw) in chunk.heightmaps() {
        assert_eq!(raw, &expected_raw);
    }
    // Light: skyYMask=0x06, emptySkyYMask=0x01, emptyBlockYMask=0x07, 2 sky
    // updates of 2048 bytes, no block updates.
    let light = packet.light_data();
    assert_eq!(light.sky_y_mask(), &[0x06]);
    assert!(light.block_y_mask().is_empty());
    assert_eq!(light.empty_sky_y_mask(), &[0x01]);
    assert_eq!(light.empty_block_y_mask(), &[0x07]);
    assert_eq!(light.sky_updates().len(), 2);
    assert_eq!(light.sky_updates()[0].len(), 2048);
    assert_eq!(light.sky_updates()[1].len(), 2048);
    assert!(light.block_updates().is_empty());
    // Body = full[8:], heightmap region = body[0..895], light region =
    // body[3140..] (the last 4130 bytes: after heightmaps 895 + buffer varint
    // 2 + buffer 2242 + block-entity count byte 1).
    let bytes = encode(&packet);
    assert_eq!(bytes[8..].to_vec(), hex(GOLDEN_BODY));
    assert_eq!(bytes[8..8 + 895].to_vec(), hex(GOLDEN_HEIGHTMAPS));
    assert_eq!(bytes[8 + 3140..].to_vec(), hex(GOLDEN_LIGHT));
}

#[test]
fn all_24_section_boundaries_parse_and_reencode_byte_identical() {
    // Walk the opaque sections buffer section-by-section. Section 0 (Y=-4) is
    // the stone container (2058 bytes); sections 1..23 are each 8 bytes:
    // `00 00 00 00` (zero nonEmptyBlockCount/fluidCount), `00 00` (single-value
    // block container: bits 0 + air palette global id 0x00), `00 28` (single-
    // value biome container: bits 0 + plains global id 0x28).
    let buffer = hex(GOLDEN_BUFFER);
    let mut offset = 0usize;
    let mut sections = Vec::new();
    for index in 0..24usize {
        let start = offset;
        let mut buf =
            FriendlyByteBuf::new(BytesMut::from(&buffer[start..][..buffer.len() - start]));
        let mut section = LevelChunkSection::new(
            PalettedContainer::new(StateId(0), block_state_strategy()),
            PalettedContainer::new(BiomeId(40), biome_strategy()),
            is_air,
        );
        // The golden superflat content is air + stone: no special-colliding
        // blocks, so the default predicate matches Java's result.
        section.read(&mut buf, &|_| false);
        let end = buffer.len() - buf.into_inner().len();
        offset = end;
        let size = end - start;
        sections.push((index, start, size, section));
    }
    assert_eq!(
        offset,
        buffer.len(),
        "all sections consume the whole buffer"
    );
    assert_eq!(sections[0].2, 2058, "section 0 (stone) serialized size");
    for (index, _, size, _) in &sections[1..] {
        assert_eq!(*size, 8, "section {index} (air) serialized size");
    }

    // Re-encode each parsed section and check the bytes match the fixture
    // slice it was read from.
    for (index, start, size, section) in &sections {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        section.write(&mut out);
        assert_eq!(
            out.into_inner().to_vec(),
            buffer[*start..start + size],
            "section {index} re-encode must reproduce its fixture bytes"
        );
    }
}

#[test]
fn named_negative_mutation_biome_id_breaks_buffer() {
    // The golden match is not vacuous: a single wrong biome id (41 instead of
    // plains 40) changes every section's `SingleValuePalette` byte and must
    // fail the byte compare on the buffer AND the full packet — while the
    // heightmaps (unaffected by biomes) still match.
    let content = build_superflat(
        block_state_strategy(),
        biome_strategy(),
        StateId(0),
        StateId(1),
        BiomeId(41),
        superflat_flags(),
    );
    let packet = ClientboundLevelChunkWithLightPacket::new(
        -5,
        -4,
        content.chunk_packet_data(),
        content.light_data,
    );
    let bytes = encode(&packet);
    assert_ne!(
        bytes,
        hex(GOLDEN_FULL),
        "biome mutation must change the bytes"
    );
    assert_ne!(
        packet.chunk_data().buffer(),
        &hex(GOLDEN_BUFFER)[..],
        "biome mutation must change the sections buffer"
    );
    // Heightmaps are computed from block states, so a biome-only mutation
    // leaves the heightmap region byte-identical to the fixture.
    assert_eq!(bytes[8..8 + 895].to_vec(), hex(GOLDEN_HEIGHTMAPS));
}

#[test]
fn superflat_geometry_constants_match_the_fixture() {
    // Pins the builder's geometry to the captured world's dimensions.
    assert_eq!(SUPERFLAT_MIN_Y, -64);
    assert_eq!(SUPERFLAT_HEIGHT, 384);
}
