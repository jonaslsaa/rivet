//! Heightmap/light read-and-carry slice of
//! `net.minecraft.world.level.chunk.storage.SerializableChunkData` (MC 26.2).
//!
//! This intentionally stops below the top-level record/parser: section
//! palettes, block entities, status decoding, chunk construction, region I/O,
//! recomputation, and writes belong to their owning units. Callers supply the
//! already-decoded `heightmapsAfter` set and status predicates.

use crate::chunk::chunk_access::ChunkAccess;
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::Types;
use crate::lighting::swmr_nibble_array::{InitState, SwmrNibbleArray};
use rivet_nbt::compound_tag::CompoundTag;

pub const HEIGHTMAPS_TAG: &str = "Heightmaps";
pub const IS_LIGHT_ON_TAG: &str = "isLightOn";
pub const SECTIONS_TAG: &str = "sections";
pub const BLOCK_LIGHT_TAG: &str = "BlockLight";
pub const SKY_LIGHT_TAG: &str = "SkyLight";
pub const BLOCKLIGHT_STATE_TAG: &str = "starlight.blocklight_state";
pub const SKYLIGHT_STATE_TAG: &str = "starlight.skylight_state";
pub const STARLIGHT_VERSION_TAG: &str = "starlight.light_version";
pub const STARLIGHT_LIGHT_VERSION: i32 = 10;

/// The stored `Map<Heightmap.Types, long[]>`, in enum ordinal order.
pub type StoredHeightmaps = [Option<Vec<i64>>; 6];

/// Parse only the heightmap types allowed by the decoded chunk status.
/// Missing/wrong-tag `Heightmaps`, unknown keys, wrong-tag values, and known
/// keys outside `heightmaps_after` are absent exactly as in Paper.
pub fn parse_heightmaps(chunk_data: &CompoundTag, heightmaps_after: &[Types]) -> StoredHeightmaps {
    let mut out: StoredHeightmaps = std::array::from_fn(|_| None);
    let Some(heightmaps) = chunk_data.get_compound(HEIGHTMAPS_TAG) else {
        return out;
    };

    for key in heightmaps.key_set() {
        if let Some(ty) = Types::from_serialization_key(key)
            && heightmaps_after.contains(&ty)
            && let Some(raw) = heightmaps.get_long_array(key)
        {
            out[ty as usize] = Some(raw.clone());
        }
    }
    out
}

/// Return the absent or wrong-length entries Paper must prime. The malformed
/// stored array remains carried for diagnosis; it is never mistaken for a
/// valid all-zero heightmap.
pub fn heightmaps_to_prime(
    height: i32,
    stored: &StoredHeightmaps,
    heightmaps_after: &[Types],
) -> Vec<Types> {
    let expected_longs = crate::levelgen::heightmap::Heightmap::new(height)
        .get_raw_data()
        .len();
    heightmaps_after
        .iter()
        .copied()
        .filter(|ty| {
            stored[*ty as usize]
                .as_ref()
                .is_none_or(|raw| raw.len() != expected_longs)
        })
        .collect()
}

/// Install valid stored heightmaps and return the exact absent/malformed set
/// Paper passes to `Heightmap.primeHeightmaps`. This slice deliberately does
/// not perform that recomputation.
pub fn reconstruct_heightmaps<T, B, S>(
    chunk: &mut ChunkAccess<T, B, S>,
    stored: &StoredHeightmaps,
    heightmaps_after: &[Types],
) -> Vec<Types>
where
    T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
    S: Eq + std::hash::Hash,
{
    let to_prime = heightmaps_to_prime(chunk.get_height(), stored, heightmaps_after);
    for ty in heightmaps_after {
        if !to_prime.contains(ty)
            && let Some(raw) = &stored[*ty as usize]
        {
            chunk.set_heightmap(*ty, raw);
        }
    }
    to_prime
}

/// The light fields retained from one serialized section. State `-1` means
/// the corresponding state key was absent; bytes remain independently
/// optional, matching `SectionData`'s nullable `DataLayer`s.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionLightData {
    pub y: i32,
    pub block_light: Option<Vec<u8>>,
    pub sky_light: Option<Vec<u8>>,
    pub block_state: i32,
    pub sky_state: i32,
}

/// Parse the light-only portion of the `sections` list. Non-compound list
/// entries are ignored; absent/wrong-tag arrays remain absent. Explicit arrays
/// are validated at Paper's `DataLayer(byte[])` boundary and therefore panic
/// on a length other than 2048, like Java's `IllegalArgumentException`.
pub fn parse_section_lights(chunk_data: &CompoundTag) -> Vec<SectionLightData> {
    let Some(sections) = chunk_data.get_list(SECTIONS_TAG) else {
        return Vec::new();
    };
    sections
        .list
        .iter()
        .filter_map(|tag| match tag {
            rivet_nbt::tag::Tag::Compound(section) => Some(section),
            _ => None,
        })
        .map(|section| {
            let block_light = section
                .get_byte_array(BLOCK_LIGHT_TAG)
                .map(|bytes| signed_bytes(bytes));
            let sky_light = section
                .get_byte_array(SKY_LIGHT_TAG)
                .map(|bytes| signed_bytes(bytes));
            if let Some(bytes) = &block_light {
                crate::chunk::data_layer::DataLayer::with_data(bytes.clone());
            }
            if let Some(bytes) = &sky_light {
                crate::chunk::data_layer::DataLayer::with_data(bytes.clone());
            }
            SectionLightData {
                y: section.get_byte_or("Y", 0) as i32,
                block_light,
                sky_light,
                block_state: state_or_absent(section, BLOCKLIGHT_STATE_TAG),
                sky_state: state_or_absent(section, SKYLIGHT_STATE_TAG),
            }
        })
        .collect()
}

/// Paper's parsed `lightCorrect` predicate. Status decoding remains outside
/// this slice, so the caller supplies `status_is_or_after_light`.
pub fn parse_light_correct(chunk_data: &CompoundTag, status_is_or_after_light: bool) -> bool {
    status_is_or_after_light
        && chunk_data.contains(IS_LIGHT_ON_TAG)
        && chunk_data.get_int_or(STARLIGHT_VERSION_TAG, -1) == STARLIGHT_LIGHT_VERSION
}

/// Reconstructed Starlight arrays ready to be moved into `ChunkAccess`.
pub struct ReconstructedLightData {
    pub block_nibbles: Vec<SwmrNibbleArray>,
    pub sky_nibbles: Vec<SwmrNibbleArray>,
    pub light_correct: bool,
}

impl ReconstructedLightData {
    /// Carry the reconstructed arrays and final validity flag on the merged
    /// #184 `ChunkAccess` surface.
    pub fn install<T, B, S>(self, chunk: &mut ChunkAccess<T, B, S>)
    where
        T: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        B: Clone + PartialEq + Send + std::fmt::Debug + 'static,
        S: Eq + std::hash::Hash,
    {
        chunk.set_block_nibbles(self.block_nibbles);
        chunk.set_sky_nibbles(self.sky_nibbles);
        chunk.set_light_correct(self.light_correct);
    }
}

/// Rebuild Starlight nibbles without running lighting. Any invalid state,
/// state/data mismatch, or out-of-range section reproduces Paper's caught
/// load failure: all-null arrays are retained and `light_correct` becomes
/// false, with no partially installed data.
pub fn reconstruct_lights(
    height: SimpleLevelHeightAccessor,
    sections: &[SectionLightData],
    light_correct: bool,
    has_sky_light: bool,
) -> ReconstructedLightData {
    let count = height.get_sections_count() as usize + 2;
    let empty = || filled_empty_light(count);
    if !light_correct {
        return ReconstructedLightData {
            block_nibbles: empty(),
            sky_nibbles: empty(),
            light_correct: false,
        };
    }

    let parsed = std::panic::catch_unwind(|| {
        let mut block = empty();
        let mut sky = empty();
        let min_light_section = height.get_min_section_y() - 1;
        for section in sections {
            let index =
                usize::try_from(section.y - min_light_section).expect("light section below world");
            if section.block_state >= 0 {
                block[index] = rebuild_nibble(section.block_light.clone(), section.block_state);
            }
            if section.sky_state >= 0 && has_sky_light {
                sky[index] = rebuild_nibble(section.sky_light.clone(), section.sky_state);
            }
        }
        (block, sky)
    });

    match parsed {
        Ok((block_nibbles, sky_nibbles)) => ReconstructedLightData {
            block_nibbles,
            sky_nibbles,
            light_correct: true,
        },
        Err(_) => ReconstructedLightData {
            block_nibbles: empty(),
            sky_nibbles: empty(),
            light_correct: false,
        },
    }
}

fn rebuild_nibble(bytes: Option<Vec<u8>>, state: i32) -> SwmrNibbleArray {
    SwmrNibbleArray::new_with_state(bytes, InitState::from_i32(state))
}

fn state_or_absent(section: &CompoundTag, key: &str) -> i32 {
    if section.contains(key) {
        section.get_int_or(key, 0)
    } else {
        -1
    }
}

fn signed_bytes(bytes: &[i8]) -> Vec<u8> {
    bytes.iter().map(|byte| *byte as u8).collect()
}

fn filled_empty_light(count: usize) -> Vec<SwmrNibbleArray> {
    (0..count)
        .map(|_| SwmrNibbleArray::new_with_bytes_and_null(None, true))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor;
    use crate::lighting::swmr_nibble_array::ARRAY_SIZE;
    use rivet_nbt::int_tag::IntTag;
    use rivet_nbt::list_tag::ListTag;
    use rivet_nbt::nbt_accounter::NbtAccounter;
    use rivet_nbt::nbt_io;
    use rivet_nbt::tag::Tag;
    use rivet_util::DataInputStream;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn fixture() -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/chunk/overworld/0.0/0.0.nbt");
        let bytes = std::fs::read(path).expect("Paper 26.2 chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    fn section_tag(y: i8) -> CompoundTag {
        let mut section = CompoundTag::new();
        section.put_byte("Y", y);
        section
    }

    fn chunk_with_sections(sections: Vec<CompoundTag>) -> CompoundTag {
        let mut chunk = CompoundTag::new();
        chunk.put(
            SECTIONS_TAG.to_string(),
            Tag::List(ListTag::with_list(
                sections.into_iter().map(Tag::Compound).collect(),
            )),
        );
        chunk
    }

    #[test]
    fn real_26_2_fixture_carries_heightmaps_and_light() {
        let chunk = fixture();
        let stored = parse_heightmaps(&chunk, &crate::levelgen::heightmap::FINAL_HEIGHTMAPS);
        for ty in crate::levelgen::heightmap::FINAL_HEIGHTMAPS {
            assert_eq!(stored[ty as usize].as_ref().expect("stored").len(), 37);
        }
        assert!(stored[Types::WorldSurfaceWg as usize].is_none());
        assert!(stored[Types::OceanFloorWg as usize].is_none());

        assert!(parse_light_correct(&chunk, true));
        assert!(!parse_light_correct(&chunk, false));
        let sections = parse_section_lights(&chunk);
        assert_eq!(sections.len(), 25);
        assert_eq!(sections[0].y, -5);
        assert_eq!(sections[0].block_state, InitState::Uninitialised.to_i32());
        assert_eq!(sections[0].sky_state, InitState::Uninitialised.to_i32());
        assert!(sections[0].block_light.is_none());
        assert!(sections[0].sky_light.is_none());
        assert_eq!(
            sections[1]
                .sky_light
                .as_ref()
                .expect("stored skylight")
                .len(),
            ARRAY_SIZE
        );

        let rebuilt = reconstruct_lights(height_accessor::create(-64, 384), &sections, true, true);
        assert!(rebuilt.light_correct);
        assert_eq!(rebuilt.block_nibbles.len(), 26);
        assert_eq!(rebuilt.sky_nibbles.len(), 26);
        assert_eq!(
            rebuilt.block_nibbles[0]
                .get_save_state()
                .expect("uninitialised")
                .state,
            InitState::Uninitialised
        );
        assert_eq!(
            rebuilt.sky_nibbles[1]
                .get_save_state()
                .expect("stored sky")
                .data,
            sections[1].sky_light
        );
    }

    #[test]
    fn heightmap_lookup_is_exact_and_wrong_tags_are_absent() {
        let mut maps = CompoundTag::new();
        maps.put_long_array("WORLD_SURFACE", vec![7; 37]);
        maps.put_long_array("world_surface", vec![8; 37]);
        maps.put_int("MOTION_BLOCKING", 1);
        maps.put_long_array("UNKNOWN", vec![9; 37]);
        let mut chunk = CompoundTag::new();
        chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(maps));

        let stored = parse_heightmaps(
            &chunk,
            &[
                Types::WorldSurface,
                Types::MotionBlocking,
                Types::OceanFloor,
            ],
        );
        assert_eq!(stored[Types::WorldSurface as usize], Some(vec![7; 37]));
        assert!(stored[Types::MotionBlocking as usize].is_none());
        assert!(stored[Types::OceanFloor as usize].is_none());
        assert!(stored[Types::WorldSurfaceWg as usize].is_none());

        let mut wrong_container = CompoundTag::new();
        wrong_container.put_int(HEIGHTMAPS_TAG, 1);
        assert!(
            parse_heightmaps(&wrong_container, &[Types::WorldSurface])
                .iter()
                .all(Option::is_none)
        );
    }

    #[test]
    fn missing_and_wrong_length_heightmaps_are_marked_for_priming() {
        let mut maps = CompoundTag::new();
        maps.put_long_array("WORLD_SURFACE", vec![7; 37]);
        maps.put_long_array("MOTION_BLOCKING", vec![8]);
        let mut chunk = CompoundTag::new();
        chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(maps));
        let after = [
            Types::WorldSurface,
            Types::OceanFloor,
            Types::MotionBlocking,
        ];
        let stored = parse_heightmaps(&chunk, &after);

        assert_eq!(stored[Types::MotionBlocking as usize], Some(vec![8]));
        assert_eq!(
            heightmaps_to_prime(384, &stored, &after),
            vec![Types::OceanFloor, Types::MotionBlocking]
        );
    }

    #[test]
    fn absent_light_state_does_not_install_present_vanilla_bytes() {
        let mut section = section_tag(-4);
        section.put_byte_array(BLOCK_LIGHT_TAG, vec![0x11; ARRAY_SIZE]);
        let sections = parse_section_lights(&chunk_with_sections(vec![section]));
        assert_eq!(sections[0].block_state, -1);
        assert!(sections[0].block_light.is_some());

        let rebuilt = reconstruct_lights(height_accessor::create(-64, 384), &sections, true, true);
        assert!(rebuilt.light_correct);
        assert!(rebuilt.block_nibbles[1].get_save_state().is_none());
    }

    #[test]
    fn arbitrary_raw_light_state_with_data_is_retained() {
        let raw = SectionLightData {
            y: -4,
            block_light: Some(vec![0x11; ARRAY_SIZE]),
            sky_light: None,
            block_state: 4,
            sky_state: -1,
        };
        let rebuilt = reconstruct_lights(height_accessor::create(-64, 384), &[raw], true, true);
        assert!(rebuilt.light_correct);
        let save = rebuilt.block_nibbles[1]
            .get_save_state()
            .expect("nonzero unknown state is saved");
        assert_eq!(save.state, InitState::Other(4));
        assert_eq!(save.data, Some(vec![0x11; ARRAY_SIZE]));
    }

    #[test]
    fn initialised_state_without_data_or_bad_position_invalidates_the_whole_payload() {
        for state in [InitState::Initialised, InitState::Hidden] {
            let invalid = SectionLightData {
                y: -4,
                block_light: None,
                sky_light: None,
                block_state: state.to_i32(),
                sky_state: -1,
            };
            let rebuilt =
                reconstruct_lights(height_accessor::create(-64, 384), &[invalid], true, true);
            assert!(!rebuilt.light_correct);
            assert!(
                rebuilt
                    .block_nibbles
                    .iter()
                    .all(|nibble| nibble.get_save_state().is_none())
            );
        }

        let out_of_range = SectionLightData {
            y: 100,
            block_state: InitState::Uninitialised.to_i32(),
            block_light: None,
            sky_light: None,
            sky_state: -1,
        };
        let rebuilt = reconstruct_lights(
            height_accessor::create(-64, 384),
            &[out_of_range],
            true,
            true,
        );
        assert!(!rebuilt.light_correct);
    }

    #[test]
    fn malformed_byte_array_panics_at_the_data_layer_boundary() {
        let mut section = section_tag(-4);
        section.put_byte_array(BLOCK_LIGHT_TAG, vec![0; ARRAY_SIZE - 1]);
        let chunk = chunk_with_sections(vec![section]);
        assert!(std::panic::catch_unwind(|| parse_section_lights(&chunk)).is_err());
    }

    #[test]
    fn light_defaults_and_dimension_sky_gate_match_paper() {
        let mut section = section_tag(-4);
        section.put_int(BLOCKLIGHT_STATE_TAG, InitState::Uninitialised.to_i32());
        section.put_int(SKYLIGHT_STATE_TAG, InitState::Uninitialised.to_i32());
        let sections = parse_section_lights(&chunk_with_sections(vec![section]));

        let unlit = reconstruct_lights(height_accessor::create(-64, 384), &sections, false, true);
        assert!(!unlit.light_correct);
        assert!(
            unlit
                .block_nibbles
                .iter()
                .all(|nibble| nibble.get_save_state().is_none())
        );

        let no_sky = reconstruct_lights(height_accessor::create(-64, 384), &sections, true, false);
        assert!(no_sky.light_correct);
        assert_eq!(
            no_sky.block_nibbles[1]
                .get_save_state()
                .expect("block state")
                .state,
            InitState::Uninitialised
        );
        assert!(no_sky.sky_nibbles[1].get_save_state().is_none());
    }

    #[test]
    fn light_correct_requires_presence_not_truth_of_is_light_on() {
        let mut chunk = CompoundTag::new();
        chunk.put_boolean(IS_LIGHT_ON_TAG, false);
        chunk.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
        assert!(parse_light_correct(&chunk, true));
        chunk.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION - 1);
        assert!(!parse_light_correct(&chunk, true));

        let mut wrong_type = CompoundTag::new();
        wrong_type.put_string(IS_LIGHT_ON_TAG, "still present");
        wrong_type.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
        assert!(parse_light_correct(&wrong_type, true));

        let mut numeric_version = CompoundTag::new();
        numeric_version.put_boolean(IS_LIGHT_ON_TAG, true);
        numeric_version.put(
            STARLIGHT_VERSION_TAG.to_string(),
            Tag::Int(IntTag::value_of(STARLIGHT_LIGHT_VERSION)),
        );
        assert!(parse_light_correct(&numeric_version, true));
    }
}
