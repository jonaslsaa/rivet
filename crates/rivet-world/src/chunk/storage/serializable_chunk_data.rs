//! Heightmap/light/block-entity read-and-carry slice of
//! `net.minecraft.world.level.chunk.storage.SerializableChunkData` (MC 26.2).
//!
//! This intentionally stops below the top-level record/parser: section
//! palettes, status decoding, chunk construction, live block-entity
//! materialization, region I/O, recomputation, and writes belong to their
//! owning units. Callers supply the already-decoded `heightmapsAfter` set,
//! status predicates, and chunk kind.

use std::sync::Arc;

use crate::chunk::chunk_access::{ChunkAccess, get_pos_from_tag};
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::Types;
use crate::lighting::swmr_nibble_array::{InitState, SwmrNibbleArray};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::block_entity_type::BlockEntityType;
use rivet_registry::core::{BlockPos, ChunkPos};

pub const HEIGHTMAPS_TAG: &str = "Heightmaps";
pub const IS_LIGHT_ON_TAG: &str = "isLightOn";
pub const SECTIONS_TAG: &str = "sections";
pub const BLOCK_LIGHT_TAG: &str = "BlockLight";
pub const SKY_LIGHT_TAG: &str = "SkyLight";
pub const BLOCKLIGHT_STATE_TAG: &str = "starlight.blocklight_state";
pub const SKYLIGHT_STATE_TAG: &str = "starlight.skylight_state";
pub const STARLIGHT_VERSION_TAG: &str = "starlight.light_version";
pub const STARLIGHT_LIGHT_VERSION: i32 = 10;
pub const BLOCK_ENTITIES_TAG: &str = "block_entities";
const KEEP_PACKED_TAG: &str = "keepPacked";
const BLOCK_ENTITY_ID_TAG: &str = "id";

/// The chunk branch that consumes Paper's ordered serialized block entities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockEntityChunkKind {
    /// `LEVELCHUNK`: `keepPacked` controls pending versus type resolution.
    Level,
    /// Any proto chunk: every entry remains pending, irrespective of its tag.
    Proto,
}

/// Why a serialized tag remains opaque pending data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingBlockEntityReason {
    KeepPacked,
    ProtoChunk,
}

/// Opaque serialized data Paper places in `pendingBlockEntities`.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingSerializedBlockEntity {
    pub source_index: usize,
    pub position: BlockPos,
    pub reason: PendingBlockEntityReason,
    pub raw_tag: CompoundTag,
}

/// An unpacked level entry whose type was resolved, but which is deliberately
/// not represented as a live `BlockEntity`.
#[derive(Clone, Debug)]
pub struct ResolvedSerializedBlockEntity {
    pub source_index: usize,
    pub position: BlockPos,
    pub entity_type: Arc<BlockEntityType>,
    pub raw_tag: CompoundTag,
}

/// Entry-local failures from Paper's unpacked level branch. The enclosing
/// failed entry retains the complete tag needed to reproduce diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BlockEntityTypeError {
    MissingId,
    WrongIdType { tag_type: i8 },
    MalformedId { value: String },
    UnknownId { identifier: Identifier },
}

/// A failed unpacked level entry. Position correction has already happened,
/// matching Paper's ordering, and the raw tag remains authoritative.
#[derive(Clone, Debug, PartialEq)]
pub struct FailedSerializedBlockEntity {
    pub source_index: usize,
    pub position: BlockPos,
    pub error: BlockEntityTypeError,
    pub raw_tag: CompoundTag,
}

/// One source-list outcome. Keeping failures in this ordered value stream lets
/// #338 log and continue without inventing a fallback entity type.
#[derive(Clone, Debug)]
pub enum SerializedBlockEntityOutcome {
    Pending(PendingSerializedBlockEntity),
    ResolvedUnpacked(ResolvedSerializedBlockEntity),
    InvalidUnpacked(FailedSerializedBlockEntity),
}

impl SerializedBlockEntityOutcome {
    pub fn source_index(&self) -> usize {
        match self {
            Self::Pending(entry) => entry.source_index,
            Self::ResolvedUnpacked(entry) => entry.source_index,
            Self::InvalidUnpacked(entry) => entry.source_index,
        }
    }

    pub fn position(&self) -> BlockPos {
        match self {
            Self::Pending(entry) => entry.position,
            Self::ResolvedUnpacked(entry) => entry.position,
            Self::InvalidUnpacked(entry) => entry.position,
        }
    }

    pub fn raw_tag(&self) -> &CompoundTag {
        match self {
            Self::Pending(entry) => &entry.raw_tag,
            Self::ResolvedUnpacked(entry) => &entry.raw_tag,
            Self::InvalidUnpacked(entry) => &entry.raw_tag,
        }
    }
}

/// `getList("block_entities").stream().flatMap(ListTag::compoundStream)`.
/// Missing/wrong containers and non-compound elements are silently ignored;
/// retained compounds are deep-copied without inspecting or reshaping them.
pub fn parse_block_entities(chunk_data: &CompoundTag) -> Vec<CompoundTag> {
    chunk_data
        .get_list(BLOCK_ENTITIES_TAG)
        .into_iter()
        .flat_map(|list| &list.list)
        .filter_map(|tag| match tag {
            Tag::Compound(compound) => Some(compound.clone()),
            _ => None,
        })
        .collect()
}

/// Interpret all retained tags in source order. Every entry produces one
/// outcome; malformed unpacked IDs never abort later entries.
pub fn reconstruct_block_entities(
    chunk_pos: &ChunkPos,
    block_entities: &[CompoundTag],
    chunk_kind: BlockEntityChunkKind,
) -> Vec<SerializedBlockEntityOutcome> {
    block_entities
        .iter()
        .cloned()
        .enumerate()
        .map(|(source_index, raw_tag)| {
            reconstruct_block_entity(chunk_pos, source_index, raw_tag, chunk_kind)
        })
        .collect()
}

fn reconstruct_block_entity(
    chunk_pos: &ChunkPos,
    source_index: usize,
    raw_tag: CompoundTag,
    chunk_kind: BlockEntityChunkKind,
) -> SerializedBlockEntityOutcome {
    if chunk_kind == BlockEntityChunkKind::Proto {
        return SerializedBlockEntityOutcome::Pending(PendingSerializedBlockEntity {
            source_index,
            position: get_pos_from_tag(Some(chunk_pos), &raw_tag),
            reason: PendingBlockEntityReason::ProtoChunk,
            raw_tag,
        });
    }

    // Paper reads this before position or id on the LEVELCHUNK branch.
    let keep_packed = raw_tag.get_boolean_or(KEEP_PACKED_TAG, false);
    let position = get_pos_from_tag(Some(chunk_pos), &raw_tag);
    if keep_packed {
        return SerializedBlockEntityOutcome::Pending(PendingSerializedBlockEntity {
            source_index,
            position,
            reason: PendingBlockEntityReason::KeepPacked,
            raw_tag,
        });
    }

    let resolved = match raw_tag.tags.get(BLOCK_ENTITY_ID_TAG) {
        None => Err(BlockEntityTypeError::MissingId),
        Some(Tag::String(id)) => {
            let value = id.value.clone();
            // `try_parse` includes Paper's default minecraft namespace. Its
            // length guard is an unchecked Java exception, so contain it as an
            // entry-local malformed codec result rather than losing the list.
            let identifier = std::panic::catch_unwind(|| Identifier::try_parse(&value))
                .ok()
                .flatten()
                .ok_or_else(|| BlockEntityTypeError::MalformedId {
                    value: value.clone(),
                });
            identifier.and_then(|identifier| {
                BlockEntityType::from_identifier(&identifier)
                    .ok_or(BlockEntityTypeError::UnknownId { identifier })
            })
        }
        Some(tag) => Err(BlockEntityTypeError::WrongIdType { tag_type: tag.id() }),
    };

    match resolved {
        Ok(entity_type) => {
            SerializedBlockEntityOutcome::ResolvedUnpacked(ResolvedSerializedBlockEntity {
                source_index,
                position,
                entity_type,
                raw_tag,
            })
        }
        Err(error) => SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
            source_index,
            position,
            error,
            raw_tag,
        }),
    }
}

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
    use rivet_nbt::long_tag::LongTag;
    use rivet_nbt::nbt_accounter::NbtAccounter;
    use rivet_nbt::nbt_io;
    use rivet_nbt::tag::Tag;
    use rivet_registry::registries::BLOCK_ENTITY_TYPE;
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

    fn block_entity_fixture() -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/block-entities/chunk-0-0.nbt");
        let bytes = std::fs::read(path).expect("Paper 26.2 block-entity fixture");
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

    fn block_entity(id: Option<&str>, x: i32, y: i32, z: i32) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_int("x", x);
        tag.put_int("y", y);
        tag.put_int("z", z);
        if let Some(id) = id {
            tag.put_string("id", id);
        }
        tag
    }

    fn one_level(tag: CompoundTag) -> SerializedBlockEntityOutcome {
        reconstruct_block_entities(&ChunkPos::new(2, -3), &[tag], BlockEntityChunkKind::Level)
            .pop()
            .expect("one outcome")
    }

    #[test]
    fn real_26_2_fixture_resolves_two_lossless_canonical_block_entities() {
        let fixture = block_entity_fixture();
        assert_eq!(fixture.get_int_or("DataVersion", -1), 4903);
        let raw = parse_block_entities(&fixture);
        assert_eq!(raw.len(), 2);
        assert_eq!(
            raw.iter()
                .map(|tag| tag.get_string("id").map(String::as_str))
                .collect::<Vec<_>>(),
            vec![Some("minecraft:chest"), Some("minecraft:furnace")]
        );

        let outcomes =
            reconstruct_block_entities(&ChunkPos::new(0, 0), &raw, BlockEntityChunkKind::Level);
        assert_eq!(outcomes.len(), 2);
        for (index, (outcome, expected)) in outcomes
            .iter()
            .zip([
                (BlockPos::new(1, 65, 1), "minecraft:chest"),
                (BlockPos::new(2, 65, 1), "minecraft:furnace"),
            ])
            .enumerate()
        {
            let SerializedBlockEntityOutcome::ResolvedUnpacked(entry) = outcome else {
                panic!("fixture entry {index} was not resolved: {outcome:?}");
            };
            assert_eq!(entry.source_index, index);
            assert_eq!(entry.position, expected.0);
            assert_eq!(entry.entity_type.name(), expected.1);
            assert!(Arc::ptr_eq(
                &entry.entity_type,
                &BlockEntityType::from_name(expected.1).unwrap()
            ));
            assert_eq!(&entry.raw_tag, &raw[index]);
            assert!(
                entry
                    .raw_tag
                    .get_list("Items")
                    .is_some_and(|items| !items.is_empty())
            );
            assert!(entry.raw_tag.get_compound("PublicBukkitValues").is_some());
        }

        let chest = outcomes[0].raw_tag();
        assert!(
            chest
                .get_compound("components")
                .is_some_and(|components| !components.is_empty())
        );
        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        for outcome in outcomes {
            let SerializedBlockEntityOutcome::ResolvedUnpacked(entry) = outcome else {
                unreachable!()
            };
            assert_eq!(
                registry.get_id(&entry.entity_type),
                entry.entity_type.id() as i32
            );
        }
    }

    #[test]
    fn block_entity_parse_filters_only_compounds_and_preserves_order_and_raw_tags() {
        let mut first = block_entity(Some("minecraft:chest"), 32, 64, -48);
        first.put_string("unknown_first", "opaque");
        let mut nested = CompoundTag::new();
        nested.put_string("example:component", "untouched");
        first.put("components".to_string(), Tag::Compound(nested));

        let mut second = block_entity(Some("minecraft:furnace"), 33, 65, -47);
        second.put_int("CookTime", 7);
        let source = vec![
            Tag::Int(IntTag::value_of(99)),
            Tag::Compound(first.clone()),
            Tag::String(rivet_nbt::string_tag::StringTag::value_of(
                "ignored".to_string(),
            )),
            Tag::Compound(second.clone()),
        ];
        let mut chunk = CompoundTag::new();
        chunk.put(
            BLOCK_ENTITIES_TAG.to_string(),
            Tag::List(ListTag::with_list(source)),
        );

        assert_eq!(parse_block_entities(&chunk), vec![first, second]);
        assert!(parse_block_entities(&CompoundTag::new()).is_empty());
        chunk.put_int(BLOCK_ENTITIES_TAG, 1);
        assert!(parse_block_entities(&chunk).is_empty());
    }

    #[test]
    fn keep_packed_uses_numeric_low_byte_and_missing_or_wrong_type_is_false() {
        let cases = [
            (None, false),
            (Some(Tag::Long(LongTag::value_of(0))), false),
            (Some(Tag::Long(LongTag::value_of(256))), false),
            (Some(Tag::Long(LongTag::value_of(257))), true),
            (
                Some(Tag::String(rivet_nbt::string_tag::StringTag::value_of(
                    "true".to_string(),
                ))),
                false,
            ),
        ];
        for (keep_packed, expected_pending) in cases {
            let mut tag = block_entity(Some("minecraft:chest"), 32, 64, -48);
            if let Some(keep_packed) = keep_packed {
                tag.put(KEEP_PACKED_TAG.to_string(), keep_packed);
            }
            assert_eq!(
                matches!(one_level(tag), SerializedBlockEntityOutcome::Pending(_)),
                expected_pending
            );
        }

        let mut packed_without_id = block_entity(None, 32, 64, -48);
        packed_without_id.put_int(KEEP_PACKED_TAG, -1);
        assert!(matches!(
            one_level(packed_without_id),
            SerializedBlockEntityOutcome::Pending(PendingSerializedBlockEntity {
                reason: PendingBlockEntityReason::KeepPacked,
                ..
            })
        ));

        for id in ["bad id", "example:unknown"] {
            let mut packed = block_entity(Some(id), 32, 64, -48);
            packed.put_int(KEEP_PACKED_TAG, 1);
            assert!(matches!(
                one_level(packed),
                SerializedBlockEntityOutcome::Pending(_)
            ));
        }
    }

    #[test]
    fn proto_entries_are_always_opaque_without_id_validation() {
        let mut malformed = block_entity(Some("not valid"), 32, 64, -48);
        malformed.put_boolean(KEEP_PACKED_TAG, false);
        let missing = block_entity(None, 33, 65, -47);
        let outcomes = reconstruct_block_entities(
            &ChunkPos::new(2, -3),
            &[malformed.clone(), missing.clone()],
            BlockEntityChunkKind::Proto,
        );
        assert_eq!(outcomes.len(), 2);
        for (index, outcome) in outcomes.iter().enumerate() {
            assert_eq!(outcome.source_index(), index);
            assert!(matches!(
                outcome,
                SerializedBlockEntityOutcome::Pending(PendingSerializedBlockEntity {
                    reason: PendingBlockEntityReason::ProtoChunk,
                    ..
                })
            ));
        }
        assert_eq!(outcomes[0].raw_tag(), &malformed);
        assert_eq!(outcomes[1].raw_tag(), &missing);
    }

    #[test]
    fn unpacked_position_is_corrected_before_ordered_entry_local_id_results() {
        let tags = vec![
            block_entity(None, 5, 70, -21),
            {
                let mut tag = block_entity(None, 6, 71, -20);
                tag.put_int("id", 1);
                tag
            },
            block_entity(Some("bad id"), 7, 72, -19),
            block_entity(Some("example:unknown"), 8, 73, -18),
            block_entity(Some("chest"), 9, 74, -17),
            block_entity(Some("minecraft:furnace"), 10, 75, -16),
        ];
        let outcomes =
            reconstruct_block_entities(&ChunkPos::new(2, -3), &tags, BlockEntityChunkKind::Level);

        assert_eq!(outcomes.len(), tags.len());
        assert_eq!(
            outcomes
                .iter()
                .map(|entry| entry.source_index())
                .collect::<Vec<_>>(),
            (0..tags.len()).collect::<Vec<_>>()
        );
        assert_eq!(outcomes[0].position(), BlockPos::new(37, 70, -37));
        assert_eq!(outcomes[1].position(), BlockPos::new(38, 71, -36));
        assert!(matches!(
            &outcomes[0],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                error: BlockEntityTypeError::MissingId,
                ..
            })
        ));
        assert!(matches!(
            &outcomes[1],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                error: BlockEntityTypeError::WrongIdType { tag_type: 3 },
                ..
            })
        ));
        assert!(matches!(
            &outcomes[2],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                error: BlockEntityTypeError::MalformedId { value },
                ..
            }) if value == "bad id"
        ));
        assert!(matches!(
            &outcomes[3],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                error: BlockEntityTypeError::UnknownId { identifier },
                ..
            }) if identifier.to_string() == "example:unknown"
        ));

        let chest = match &outcomes[4] {
            SerializedBlockEntityOutcome::ResolvedUnpacked(entry) => entry,
            other => panic!("expected resolved chest, got {other:?}"),
        };
        assert_eq!(chest.entity_type.name(), "minecraft:chest");
        assert!(Arc::ptr_eq(
            &chest.entity_type,
            &BlockEntityType::from_name("minecraft:chest").unwrap()
        ));
        let furnace = match &outcomes[5] {
            SerializedBlockEntityOutcome::ResolvedUnpacked(entry) => entry,
            other => panic!("expected resolved furnace, got {other:?}"),
        };
        assert_eq!(furnace.entity_type.name(), "minecraft:furnace");

        let access = BlockEntityType::built_in_registry_access();
        let registry = access.lookup(&BLOCK_ENTITY_TYPE).unwrap();
        assert_eq!(registry.get_id(&chest.entity_type), 1);
        assert_eq!(registry.get_id(&furnace.entity_type), 0);
    }

    #[test]
    fn position_fields_use_numeric_conversion_and_zero_defaults() {
        let mut tag = CompoundTag::new();
        tag.put_long("x", 37);
        tag.put_string("y", "wrong numeric type");
        tag.put_string("id", "minecraft:chest");
        // Missing z defaults to zero. Because z is outside chunk (2, -3),
        // Paper reanchors both horizontal coordinates; x keeps its low bits.
        let outcome = one_level(tag.clone());
        assert_eq!(outcome.position(), BlockPos::new(37, 0, -48));
        assert_eq!(outcome.raw_tag(), &tag);
        assert!(matches!(
            outcome,
            SerializedBlockEntityOutcome::ResolvedUnpacked(_)
        ));
    }

    #[test]
    fn duplicate_corrected_positions_remain_distinct_ordered_outcomes() {
        let first = block_entity(Some("minecraft:chest"), 5, 64, -21);
        let second = block_entity(Some("minecraft:furnace"), 37, 64, -37);
        let outcomes = reconstruct_block_entities(
            &ChunkPos::new(2, -3),
            &[first.clone(), second.clone()],
            BlockEntityChunkKind::Level,
        );

        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].position(), BlockPos::new(37, 64, -37));
        assert_eq!(outcomes[1].position(), BlockPos::new(37, 64, -37));
        assert_eq!(outcomes[0].raw_tag(), &first);
        assert_eq!(outcomes[1].raw_tag(), &second);
        assert_eq!(outcomes[0].source_index(), 0);
        assert_eq!(outcomes[1].source_index(), 1);
    }

    #[test]
    fn raw_components_public_bukkit_values_and_unknown_fields_stay_opaque() {
        let mut tag = block_entity(Some("minecraft:chest"), 32, 64, -48);
        let mut components = CompoundTag::new();
        components.put_int("unknown_component", 17);
        tag.put("components".to_string(), Tag::Compound(components));
        tag.put_string("PublicBukkitValues", "wrong type but preserved");
        tag.put_long("plugin_custom", i64::MAX);
        let original = tag.clone();

        let outcome = one_level(tag);
        assert!(matches!(
            &outcome,
            SerializedBlockEntityOutcome::ResolvedUnpacked(_)
        ));
        assert_eq!(outcome.raw_tag(), &original);
        assert_eq!(
            outcome
                .raw_tag()
                .key_set()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            original.key_set().map(String::as_str).collect::<Vec<_>>()
        );
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
