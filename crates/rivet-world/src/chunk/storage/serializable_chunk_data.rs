//! Current-version extraction half of Paper 26.2's `SerializableChunkData`.
//!
//! Section palette decoding remains at an explicit #336 seam. Region I/O is a
//! caller, not a parser dependency; unsupported runtime surfaces are retained
//! or rejected by name rather than silently discarded.
//!
//! Stored ticks (#370): the top-level `block_ticks`/`fluid_ticks` lists are
//! decoded through the faithful `SavedTick.codec(byNameCodec).listOf()` codecs
//! into typed [`SavedTick<Block>`]/[`SavedTick<FluidId>`] values, filtered to
//! the stored chunk position, and carried as typed stored values on the parse
//! result. A FULL chunk carrying stored ticks now reconstructs, carrying the
//! typed values for the caller's runtime composition — the parser neither
//! executes, schedules, generates, installs, nor writes them (the
//! `LevelChunkTicks`/`ProtoChunkTicks` execution containers stay deferred with
//! the tick-execution slice). `UpgradeData`'s neighbor tick lists remain behind
//! the `UnsupportedUpgradeData` boundary: they are decodable (with the Java
//! `orElse(Blocks.AIR)`/`orElse(Fluids.EMPTY)` asymmetry) but are not yet
//! carried by the `UpgradeData` port.
//!
//! Structures (#369): `structures.References` decodes into ordered typed
//! [`StructureReference`] values (registry-key identifier + packed chunk-long
//! set, in deterministic key-insertion order — not a Paper-observable order,
//! since Java's fastutil hash map iterates nondeterministically). Malformed
//! keys, wrong-type payloads, and construction-time out-of-range references are
//! surfaced as typed [`ChunkParseDiagnostic`]s and discarded, never silently
//! ignored. Non-empty `starts` stays behind `UnsupportedStructures` (the
//! `StructureStart` load path is not ported); a References-only structures
//! compound is now fully carryable.
//!
//! This also carries the ordered serialized block-entity read-and-reconstruct
//! surface (#337): `parse_block_entities` retains the raw compound tags and
//! `reconstruct_block_entities` resolves them per Paper's LEVELCHUNK/proto
//! branch ordering. The top-level parser still defers live materialization.

use std::sync::{Arc, LazyLock};

use crate::block::Block;
use crate::chunk::chunk_access::{ChunkAccess, get_pos_from_tag};
use crate::chunk::registry_codecs::{block_by_name_codec, fluid_by_name_codec};
use crate::chunk::status::ChunkStatus;
use crate::chunk::upgrade_data::UpgradeData;
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::Types;
use crate::lighting::swmr_nibble_array::{InitState, SwmrNibbleArray};
use crate::ticks::{SavedTick, filter_tick_list_for_chunk, saved_tick_codec};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::nbt_utils::CURRENT_DATA_VERSION;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::block_entity_type::BlockEntityType;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_registry::fluid_id::FluidId;
use rivet_serialization::codec::{self, Codec};

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
const BLOCK_TICKS_TAG: &str = "block_ticks";
const FLUID_TICKS_TAG: &str = "fluid_ticks";
const NEIGHBOR_BLOCK_TICKS_TAG: &str = "neighbor_block_ticks";
const NEIGHBOR_FLUID_TICKS_TAG: &str = "neighbor_fluid_ticks";

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

/// One decoded `structures.References` entry: the structure registry-key
/// identifier and the packed chunk-position longs (`ChunkPos.pack`), in the
/// order the reference set was read.
///
/// Ordering: Paper iterates the `References` `CompoundTag` (a fastutil hash
/// map, so its key iteration order is nondeterministic) and reads each entry's
/// `long[]` into a `LongOpenHashSet`. Rivet's insertion-ordered `CompoundTag`
/// yields a deterministic key order, which is not a Paper-observable order —
/// the same divergence the heightmap key-order note below records — so the
/// entry order here is a stable carry, never a byte-order oracle. The reference
/// longs within a key are retained in array order, which the `StructureAccess`
/// port models as first-insertion order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructureReference {
    /// The structure's `Identifier` (registry key), parsed from the tag key.
    pub identifier: Identifier,
    /// The packed `ChunkPos` longs, in the order they appeared (the signed
    /// `long[]` stored in the NBT `LongArrayTag`, matching Java's
    /// `ChunkPos.unpack(long)` input).
    pub references: Vec<i64>,
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
    /// Paper's shared invalid-type diagnostic path for an absent or non-string
    /// `id`. The untouched `raw_tag` retains the underlying tag distinction.
    InvalidIdType,
    MalformedId {
        value: String,
    },
    UnknownId {
        identifier: Identifier,
    },
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

/// Interpret retained tags in source order. Invalid unpacked IDs are
/// entry-local outcomes, including syntactically valid IDs rejected by the
/// codec's Identifier length guard.
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
        None => Err(BlockEntityTypeError::InvalidIdType),
        Some(Tag::String(id)) => {
            let value = id.value.clone();
            // Position was corrected above. Identifier.CODEC catches both
            // invalid syntax and the constructor's length exception, so both
            // remain entry-local invalid-type outcomes on this Paper path.
            let identifier = Identifier::try_parse_result(&value)
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
        Some(_) => Err(BlockEntityTypeError::InvalidIdType),
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

const STATUS_TAG: &str = "Status";
const DATA_VERSION_TAG: &str = "DataVersion";

/// A recoverable codec diagnostic emitted during top-level extraction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChunkParseDiagnostic {
    UnknownStatus(String),
    MisplacedChunk {
        stored: ChunkPos,
        requested: ChunkPos,
    },
    /// A top-level stored-tick list (`block_ticks`/`fluid_ticks`) failed to
    /// fully decode — one or more elements errored (unknown/malformed ids,
    /// missing fields) and the `ListCodec` dropped them, retaining only the
    /// surviving siblings. Paper logs the same failure per element via
    /// `LOGGER.error("Failed to read field ({}={}): {}")`; the codec partial
    /// path drops the per-element messages, so this diagnostic is the port's
    /// explicit trace. The chunk still parses and may be FULL-capable.
    StoredTicksDecodeFailed {
        field: &'static str,
        error: String,
    },
    /// A `structures.References` entry could not be decoded (malformed key,
    /// wrong-type payload, or an over-long identifier that fails Paper's
    /// `Identifier` constructor length guard). Paper discards such an entry —
    /// an unparseable key never reaches the registry, a wrong-type value is
    /// skipped by `asLongArray()` — so this diagnostic is the port's explicit
    /// trace that the entry was dropped, never silently ignored.
    StructureReferenceMalformed {
        key: String,
        reason: String,
    },
    /// A decoded `structures.References` chunk-long points more than 8 chunks
    /// away (chessboard distance) from the chunk being reconstructed. Paper
    /// logs `"Found invalid structure reference ..."` and drops that one
    /// reference; the diagnostic records the drop.
    StructureReferenceOutOfRange {
        identifier: Identifier,
        chunk: ChunkPos,
        chunk_pos: ChunkPos,
    },
}

/// Typed failures at the extraction/construction boundary.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SerializableChunkDataError {
    #[error("chunk DataVersion {found} is newer than supported version {current}")]
    NewerDataVersion { found: i32, current: i32 },
    #[error("section {section_y} {field} has {length} bytes; expected 2048")]
    MalformedDataLayer {
        section_y: i32,
        field: &'static str,
        length: usize,
    },
    #[error("chunk status {status:?} is not constructible until proto-chunk loading lands")]
    UnsupportedChunkStatus { status: ChunkStatus },
    #[error("UpgradeData field {field} requires SavedTick support")]
    UnsupportedUpgradeData { field: &'static str },
    #[error("blending_data requires blending reconstruction (#336)")]
    UnsupportedBlendingData,
    #[error("non-empty entities require post-load entity reconstruction")]
    UnsupportedEntities,
    #[error("non-empty structures require structure reconstruction")]
    UnsupportedStructures,
    #[error("compound ChunkBukkitValues requires an owned PDC carrier")]
    UnsupportedPersistentData,
    #[error("PostProcessing section {index} is outside the {section_count}-section chunk")]
    PostProcessingOutOfBounds { index: usize, section_count: usize },
    #[error("decoded {actual} sections; construction accessor requires {expected}")]
    SectionCountMismatch { expected: usize, actual: usize },
    #[error(
        "parsed minimum section Y {parsed}; construction accessor has minimum section Y {construction}"
    )]
    HeightAccessorMismatch { parsed: i32, construction: i32 },
    #[error(
        "parsed accessor has {parsed} sections; construction accessor has {construction} sections"
    )]
    HeightAccessorSectionCountMismatch { parsed: usize, construction: usize },
}

/// The current-version, top-level extraction result from Paper's
/// `SerializableChunkData.parse`. Section palette decoding remains at the
/// explicit `section_tags` seam until #336 lands.
pub struct SerializableChunkData {
    stored_pos: ChunkPos,
    min_section_y: i32,
    section_count: usize,
    last_update_time: i64,
    inhabited_time: i64,
    status: ChunkStatus,
    diagnostics: Vec<ChunkParseDiagnostic>,
    upgrade_data: UpgradeData,
    raw_upgrade_data: Option<CompoundTag>,
    effective_upgrade_neighbor_block_ticks: bool,
    effective_upgrade_neighbor_fluid_ticks: bool,
    light_correct: bool,
    raw_blending_data: Option<CompoundTag>,
    raw_below_zero_retrogen: Option<CompoundTag>,
    effective_blending_data: bool,
    effective_below_zero_retrogen: bool,
    carving_mask: Option<Vec<i64>>,
    heightmaps: StoredHeightmaps,
    raw_block_ticks: ListTag,
    raw_fluid_ticks: ListTag,
    stored_block_ticks: Vec<SavedTick<Block>>,
    stored_fluid_ticks: Vec<SavedTick<FluidId>>,
    post_processing_sections: Vec<Option<Vec<i16>>>,
    entities: Vec<CompoundTag>,
    block_entities: Vec<CompoundTag>,
    structure_data: CompoundTag,
    structures_references: Vec<StructureReference>,
    section_tags: ListTag,
    persistent_data_container: Option<Tag>,
}

impl SerializableChunkData {
    /// Extract in Paper's observable field order. `Ok(None)` is reserved for
    /// a missing or non-string `Status`, which Paper drops before DataVersion.
    pub fn parse(
        level_height: SimpleLevelHeightAccessor,
        chunk_data: &CompoundTag,
    ) -> Result<Option<Self>, SerializableChunkDataError> {
        let Some(status_name) = chunk_data.get_string(STATUS_TAG) else {
            return Ok(None);
        };

        if let Some(found) = chunk_data.get_int(DATA_VERSION_TAG)
            && found > CURRENT_DATA_VERSION
        {
            return Err(SerializableChunkDataError::NewerDataVersion {
                found,
                current: CURRENT_DATA_VERSION,
            });
        }

        let stored_pos = ChunkPos::new(
            chunk_data.get_int_or("xPos", 0),
            chunk_data.get_int_or("zPos", 0),
        );
        let last_update_time = chunk_data.get_long_or("LastUpdate", 0);
        let inhabited_time = chunk_data.get_long_or("InhabitedTime", 0);
        let (status, mut diagnostics) = match ChunkStatus::from_identifier(status_name) {
            Some(status) => (status, Vec::new()),
            None => (
                ChunkStatus::Empty,
                vec![ChunkParseDiagnostic::UnknownStatus(status_name.clone())],
            ),
        };
        let upgrade_tag = chunk_data.get_compound("UpgradeData");
        let raw_upgrade_data = upgrade_tag.cloned();
        let upgrade_data = upgrade_tag.map_or_else(
            || UpgradeData::empty(level_height.get_sections_count() as usize),
            |tag| UpgradeData::from_tag(tag, level_height.get_sections_count() as usize),
        );
        let effective_upgrade_neighbor_block_ticks = upgrade_tag
            .and_then(|tag| tag.get_list(NEIGHBOR_BLOCK_TICKS_TAG))
            .is_some_and(upgrade_neighbor_block_ticks_decode_non_empty);
        let effective_upgrade_neighbor_fluid_ticks = upgrade_tag
            .and_then(|tag| tag.get_list(NEIGHBOR_FLUID_TICKS_TAG))
            .is_some_and(upgrade_neighbor_fluid_ticks_decode_non_empty);
        let light_correct = parse_light_correct(chunk_data, status.is_or_after(ChunkStatus::Light));
        let raw_blending_data = chunk_data.get_compound("blending_data").cloned();
        let raw_below_zero_retrogen = chunk_data.get_compound("below_zero_retrogen").cloned();
        let effective_blending_data = raw_blending_data
            .as_ref()
            .is_some_and(blending_data_decodes);
        let effective_below_zero_retrogen = raw_below_zero_retrogen
            .as_ref()
            .is_some_and(below_zero_retrogen_decodes);
        let carving_mask = chunk_data.get_long_array("carving_mask").cloned();
        let heightmaps = parse_heightmaps(chunk_data, status.heightmaps_after());
        let raw_block_ticks = chunk_data.get_list_or_empty(BLOCK_TICKS_TAG);
        let raw_fluid_ticks = chunk_data.get_list_or_empty(FLUID_TICKS_TAG);
        let (stored_block_ticks, stored_fluid_ticks, tick_diagnostics) =
            decode_stored_ticks(chunk_data, stored_pos, status);
        diagnostics.extend(tick_diagnostics);
        let post_processing_sections = parse_post_processing(chunk_data);
        let entities = compound_entries(chunk_data.get_list("entities"));
        let block_entities = compound_entries(chunk_data.get_list("block_entities"));
        let structure_data = chunk_data.get_compound_or_empty("structures");
        let (structures_references, structure_diagnostics) =
            parse_structure_references(&structure_data, status);
        diagnostics.extend(structure_diagnostics);
        let section_tags = chunk_data.get_list_or_empty(SECTIONS_TAG);
        let persistent_data_container = chunk_data.get("ChunkBukkitValues").cloned();

        Ok(Some(Self {
            stored_pos,
            min_section_y: level_height.get_min_section_y(),
            section_count: level_height.get_sections_count() as usize,
            last_update_time,
            inhabited_time,
            status,
            diagnostics,
            upgrade_data,
            raw_upgrade_data,
            effective_upgrade_neighbor_block_ticks,
            effective_upgrade_neighbor_fluid_ticks,
            light_correct,
            raw_blending_data,
            raw_below_zero_retrogen,
            effective_blending_data,
            effective_below_zero_retrogen,
            carving_mask,
            heightmaps,
            raw_block_ticks,
            raw_fluid_ticks,
            stored_block_ticks,
            stored_fluid_ticks,
            post_processing_sections,
            entities,
            block_entities,
            structure_data,
            structures_references,
            section_tags,
            persistent_data_container,
        }))
    }

    pub fn stored_pos(&self) -> ChunkPos {
        self.stored_pos
    }
    pub fn min_section_y(&self) -> i32 {
        self.min_section_y
    }
    /// The parse-time accessor's section count (the #383 reconstruction's
    /// accessor-mismatch guard).
    pub(crate) fn section_count(&self) -> usize {
        self.section_count
    }
    pub fn last_update_time(&self) -> i64 {
        self.last_update_time
    }
    pub fn inhabited_time(&self) -> i64 {
        self.inhabited_time
    }
    pub fn status(&self) -> ChunkStatus {
        self.status
    }
    pub fn diagnostics(&self) -> &[ChunkParseDiagnostic] {
        &self.diagnostics
    }
    pub fn upgrade_data(&self) -> &UpgradeData {
        &self.upgrade_data
    }
    pub fn raw_upgrade_data(&self) -> Option<&CompoundTag> {
        self.raw_upgrade_data.as_ref()
    }
    pub fn light_correct(&self) -> bool {
        self.light_correct
    }
    pub fn carving_mask(&self) -> Option<&[i64]> {
        self.carving_mask.as_deref()
    }
    pub fn heightmaps(&self) -> &StoredHeightmaps {
        &self.heightmaps
    }
    pub fn post_processing_sections(&self) -> &[Option<Vec<i16>>] {
        &self.post_processing_sections
    }
    pub fn entities(&self) -> &[CompoundTag] {
        &self.entities
    }
    pub fn block_entities(&self) -> &[CompoundTag] {
        &self.block_entities
    }
    /// Consume the serialized block-entity compounds, source order, leaving an
    /// empty list in place. The #383 reconstruction uses this to carry the
    /// tags into the runtime chunk's pending map and the returned field without
    /// cloning the list twice.
    pub(crate) fn take_block_entities(&mut self) -> Vec<CompoundTag> {
        std::mem::take(&mut self.block_entities)
    }
    pub fn structure_data(&self) -> &CompoundTag {
        &self.structure_data
    }
    /// The decoded `structures.References` entries, in deterministic
    /// key-insertion order (a stable carry, not a Paper-observable order). The
    /// chunk position is not consulted until reconstruction (the >8-chunk
    /// filter).
    pub fn structures_references(&self) -> &[StructureReference] {
        &self.structures_references
    }
    pub fn section_tags(&self) -> &ListTag {
        &self.section_tags
    }
    pub fn raw_block_ticks(&self) -> &ListTag {
        &self.raw_block_ticks
    }
    pub fn raw_fluid_ticks(&self) -> &ListTag {
        &self.raw_fluid_ticks
    }
    /// The typed, per-chunk-filtered stored block ticks (`ChunkAccess.PackedTicks
    /// .blocks()`), faithfully decoded through `SavedTick.codec(...).listOf()`.
    /// Only populated for a FULL chunk — every other status skips the typed
    /// decode and carries an empty list (proto paths never claim tick support;
    /// see [`Self::validate_full_for_reconstruction`]). Carried as stored values
    /// only — the reconstruction consumes them off the parse result ([`Self`]
    /// installs them into no runtime container; the `LevelChunkTicks`/
    /// `ProtoChunkTicks` execution containers defer with the tick-execution
    /// slice (#370)).
    pub fn stored_block_ticks(&self) -> &[SavedTick<Block>] {
        &self.stored_block_ticks
    }
    /// The typed, per-chunk-filtered stored fluid ticks (`ChunkAccess.PackedTicks
    /// .fluids()`). Only populated for a FULL chunk (proto paths skip the typed
    /// decode); carried as stored values only. Same boundary as
    /// [`Self::stored_block_ticks`].
    pub fn stored_fluid_ticks(&self) -> &[SavedTick<FluidId>] {
        &self.stored_fluid_ticks
    }
    pub fn raw_blending_data(&self) -> Option<&CompoundTag> {
        self.raw_blending_data.as_ref()
    }
    pub fn raw_below_zero_retrogen(&self) -> Option<&CompoundTag> {
        self.raw_below_zero_retrogen.as_ref()
    }
    pub fn effective_below_zero_retrogen(&self) -> bool {
        self.effective_below_zero_retrogen
    }
    pub fn persistent_data_container(&self) -> Option<&Tag> {
        self.persistent_data_container.as_ref()
    }

    /// Validate every capability the FULL reconstruction requires. This is the
    /// single reconstruction capability boundary: `reconstruct_runtime_chunk`
    /// consults it before composing, and region-backed boot uses it as the
    /// pre-composition gate so the preflight agrees with what reconstruction
    /// will accept.
    ///
    /// Serialized block entities are NOT rejected — the reconstruction carries
    /// them as pending NBT (materialization defers with #341). Stored ticks are
    /// carried as typed values, never rejected (the `LevelChunkTicks`/
    /// `ProtoChunkTicks` execution containers defer with the tick-execution
    /// slice, #370). The remaining unsupported surfaces (proto status,
    /// `UpgradeData` neighbor ticks, blending data, persistent data, structure
    /// `starts`, non-empty entities, out-of-bounds post-processing) surface
    /// their typed errors here.
    pub fn validate_full_for_reconstruction(&self) -> Result<(), SerializableChunkDataError> {
        self.validate_full_construction(self.section_count)
    }

    fn validate_full_construction(
        &self,
        section_count: usize,
    ) -> Result<(), SerializableChunkDataError> {
        if self.status != ChunkStatus::Full {
            return Err(SerializableChunkDataError::UnsupportedChunkStatus {
                status: self.status,
            });
        }
        if self.effective_upgrade_neighbor_block_ticks {
            return Err(SerializableChunkDataError::UnsupportedUpgradeData {
                field: "neighbor_block_ticks",
            });
        }
        if self.effective_upgrade_neighbor_fluid_ticks {
            return Err(SerializableChunkDataError::UnsupportedUpgradeData {
                field: "neighbor_fluid_ticks",
            });
        }
        // Stored `block_ticks`/`fluid_ticks` decode into typed stored values on
        // parse and are carried. The runtime tick containers
        // (`LevelChunkTicks`/`ProtoChunkTicks`) defer with the tick-execution
        // slice, so a FULL chunk with stored ticks now reconstructs with the
        // values carried — nothing is scheduled, generated, installed, or
        // written (#370).
        // `below_zero_retrogen` is deliberately not checked here: Paper's
        // LEVELCHUNK branch of `SerializableChunkData.read` never consults it
        // (only the proto branch does), so a FULL chunk carrying one loads as-is.
        if self.effective_blending_data {
            return Err(SerializableChunkDataError::UnsupportedBlendingData);
        }
        if matches!(
            self.persistent_data_container,
            Some(Tag::Compound(ref compound)) if !compound.is_empty()
        ) {
            return Err(SerializableChunkDataError::UnsupportedPersistentData);
        }
        // `structures.References` decodes into carried [`StructureReference`]s
        // and no longer blocks construction. Non-empty `starts` remains an
        // unsupported surface (the `StructureStart` load path is not ported),
        // so a starts-bearing structures compound still fails here.
        if structures_starts_are_non_empty(&self.structure_data) {
            return Err(SerializableChunkDataError::UnsupportedStructures);
        }
        if let Some(index) =
            self.post_processing_sections
                .iter()
                .enumerate()
                .find_map(|(index, offsets)| {
                    (index >= section_count && offsets.is_some()).then_some(index)
                })
        {
            return Err(SerializableChunkDataError::PostProcessingOutOfBounds {
                index,
                section_count,
            });
        }
        if !self.entities.is_empty() {
            return Err(SerializableChunkDataError::UnsupportedEntities);
        }
        Ok(())
    }
}

fn parse_post_processing(chunk_data: &CompoundTag) -> Vec<Option<Vec<i16>>> {
    chunk_data
        .get_list_or_empty("PostProcessing")
        .list
        .iter()
        .map(|entry| match entry {
            Tag::List(offsets) if !offsets.list.is_empty() => Some(
                (0..offsets.list.len())
                    .map(|index| offsets.get_short_or(index, 0))
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

fn compound_entries(list: Option<&ListTag>) -> Vec<CompoundTag> {
    list.into_iter()
        .flat_map(|list| &list.list)
        .filter_map(|entry| match entry {
            Tag::Compound(compound) => Some(compound.clone()),
            _ => None,
        })
        .collect()
}

/// Whether the `structures.starts` compound carries any entries. References
/// alone no longer blocks FULL construction (#369); starts remain unsupported
/// until the `StructureStart` load path is ported.
fn structures_starts_are_non_empty(structures: &CompoundTag) -> bool {
    // `get_compound` borrows (Paper's `getCompoundOrEmpty` returns a live
    // reference); the absent case is just an empty compound, so there is no
    // need to clone the starts compound just to test emptiness.
    structures
        .get_compound("starts")
        .is_some_and(|starts| !starts.is_empty())
}

/// Decode `structures.References` into ordered typed entries, mirroring
/// Paper's `unpackStructureReferences` read phase (the per-chunk distance filter
/// is a reconstruction-time concern — Paper passes `pos` to the unpack, which
/// this slice defers to construction so the stored position and the requested
/// position can differ). Only a FULL chunk decodes, mirroring the tick gate's
/// FULL-only decode: the runtime reconstruction this feeds accepts only FULL
/// chunks, and proto chunks are rejected before any of these surfaces are
/// consulted, so decoding (and surfacing malformed-key diagnostics) on a
/// non-FULL chunk would be dead work Paper's observable behavior never reaches.
/// (Paper itself calls `setAllReferences(unpackStructureReferences(...))`
/// unconditionally in `read` for every chunk type; the FULL gate here mirrors
/// the pre-existing `decode_stored_ticks` boundary rather than Paper's exact
/// call site.)
///
/// An absent `References` tag -> no entries (normal). A wrong-typed `References`
/// container drops every entry and surfaces a `StructureReferenceMalformed`
/// diagnostic — Paper tolerates the wrong type silently (`getCompoundOrEmpty`
/// returns an empty compound), the port surfaces the drop per the never-silent
/// requirement, mirroring how per-key wrong-type values already surface. Each
/// key is parsed with `Identifier::try_parse_result`
/// (invalid characters -> dropped, like Paper's `Identifier.tryParse` returning
/// null; an over-long identifier -> surfaced as a `StructureReferenceMalformed`
/// diagnostic and dropped — Paper's `Identifier` constructor throws an
/// unchecked `IdentifierException` for an over-long id that propagates up and
/// aborts the whole chunk read, so the port deliberately degrades that crash
/// into an entry-local diagnostic instead of dropping the entire chunk). A
/// wrong-type value (anything but a `LongArray`) is skipped exactly like Java's
/// `entry.asLongArray()` empty-check, and surfaced as a diagnostic. The packed
/// chunk longs are retained in array order.
///
// RivetTodo(#369): Paper's STRUCTURE-registry membership discard is deferred.
// `unpackStructureReferences` looks up each key through
// `registryAccess.lookupOrThrow(Registries.STRUCTURE).getValue(identifier)` and
// warns+discards the entry when it is absent (an unregistered structure id).
// Rivet has no `Structure` type or STRUCTURE registry yet, so a syntactically
// valid key is carried keyed by its `Identifier` and installed regardless of
// membership; once the registry lands, this decode must drop unregistered keys
// (typed diagnostic) to match Paper's observable map. The >8-chunk distance
// filter still runs at reconstruction (see `filter_structure_references`).
pub fn parse_structure_references(
    structures: &CompoundTag,
    status: ChunkStatus,
) -> (Vec<StructureReference>, Vec<ChunkParseDiagnostic>) {
    let mut references = Vec::new();
    let mut diagnostics = Vec::new();
    // Decode/carry only for FULL (mirroring the tick gate): the reconstruction
    // this feeds accepts only FULL chunks, so a malformed `References` key on a
    // non-FULL chunk would never reach a Paper-observable path.
    if status != ChunkStatus::Full {
        return (references, diagnostics);
    }
    let Some(references_tag) = structures.get("References") else {
        // Absent `References` is normal (Paper's `getCompoundOrEmpty`).
        return (references, diagnostics);
    };
    let Tag::Compound(references_tag) = references_tag else {
        // A wrong-typed `References` container drops every entry; Paper
        // tolerates it silently, the port surfaces the drop per the
        // never-silent requirement.
        diagnostics.push(ChunkParseDiagnostic::StructureReferenceMalformed {
            key: "References".to_string(),
            reason: format!(
                "expected compound container, found tag type {:?}",
                references_tag.get_type()
            ),
        });
        return (references, diagnostics);
    };

    for key in references_tag.key_set() {
        let Some(tag) = references_tag.get(key) else {
            continue;
        };
        let identifier = match Identifier::try_parse_result(key) {
            Ok(Some(identifier)) => identifier,
            Ok(None) | Err(_) => {
                diagnostics.push(ChunkParseDiagnostic::StructureReferenceMalformed {
                    key: key.clone(),
                    reason: "invalid identifier".to_string(),
                });
                continue;
            }
        };
        let Tag::LongArray(longs) = tag else {
            diagnostics.push(ChunkParseDiagnostic::StructureReferenceMalformed {
                key: key.clone(),
                reason: format!("expected long array, found tag type {:?}", tag.get_type()),
            });
            continue;
        };
        references.push(StructureReference {
            identifier,
            references: longs.data.clone(),
        });
    }
    (references, diagnostics)
}

/// Reconstruct-time `unpackStructureReferences` filter: keep only the packed
/// chunk-longs whose chessboard distance from the chunk being reconstructed is
/// <= 8 (Paper logs `"Found invalid structure reference ..."` and drops the
/// rest). A dropped reference is recorded as a [`ChunkParseDiagnostic`] so the
/// discard is never silent. Duplicate in-range references deduplicate silently,
/// preserving first-insertion order — Paper builds the `LongOpenHashSet` from
/// the filtered array, so the carried and installed sets agree.
///
/// The map shape mirrors Paper's `outmap`: a key is preserved even when every
/// reference filters out — Paper's
/// `outmap.put(structureType, new LongOpenHashSet(filtered...))` keeps the key
/// with an empty set, and `setAllReferences` installs that empty entry. That
/// includes a key whose wire `long[]` was already empty: Paper's guard is
/// `!longArray.isEmpty()` on the `Optional<long[]>` from
/// `LongArrayTag.asLongArray()`, which is always present for a `LongArrayTag`
/// regardless of array length, so the key still enters the map with an empty
/// set.
pub fn filter_structure_references(
    references: &[StructureReference],
    chunk_pos: &ChunkPos,
) -> (Vec<StructureReference>, Vec<ChunkParseDiagnostic>) {
    let mut kept = Vec::new();
    let mut diagnostics = Vec::new();
    for entry in references {
        let mut filtered: Vec<i64> = Vec::with_capacity(entry.references.len());
        // O(n) dedup mirroring Paper's `LongOpenHashSet`: the membership set
        // is transient and only guards first-insertion order (the `Vec` is the
        // carried value). A hostile `long[]` must not degrade to an O(n^2)
        // linear scan.
        let mut seen: std::collections::HashSet<i64> =
            std::collections::HashSet::with_capacity(entry.references.len());
        for reference in entry.references.iter().copied() {
            let ref_pos = ChunkPos::unpack(reference);
            if ref_pos.get_chessboard_distance(chunk_pos) > 8 {
                diagnostics.push(ChunkParseDiagnostic::StructureReferenceOutOfRange {
                    identifier: entry.identifier.clone(),
                    chunk: *chunk_pos,
                    chunk_pos: ref_pos,
                });
            } else if seen.insert(reference) {
                // The `LongOpenHashSet` dedup is silent; only the out-of-range
                // drop surfaces a diagnostic (Paper warns on that one).
                filtered.push(reference);
            }
        }
        // Paper puts every key whose `References` value was a `LongArrayTag`
        // (the `Optional` from `asLongArray` is always present), so an
        // already-empty wire array still enters the map with an empty set.
        kept.push(StructureReference {
            identifier: entry.identifier.clone(),
            references: filtered,
        });
    }
    (kept, diagnostics)
}

fn blending_data_decodes(tag: &CompoundTag) -> bool {
    const CELL_COLUMN_COUNT: usize = 16;

    tag.get_int("min_section").is_some()
        && tag.get_int("max_section").is_some()
        && tag.get("heights").is_none_or(|heights| {
            let decoded_length = match heights {
                Tag::List(list) if list.list.iter().all(|height| height.as_number().is_some()) => {
                    Some(list.list.len())
                }
                Tag::ByteArray(heights) => Some(heights.data.len()),
                Tag::IntArray(heights) => Some(heights.data.len()),
                Tag::LongArray(heights) => Some(heights.data.len()),
                // `lenientOptionalFieldOf` turns any field-codec error,
                // including a list's partial error, into an absent height
                // array while preserving the surrounding packed value.
                _ => return true,
            };
            decoded_length == Some(CELL_COLUMN_COUNT)
        })
}

fn below_zero_retrogen_decodes(tag: &CompoundTag) -> bool {
    tag.get_string("target_status")
        .and_then(|status| ChunkStatus::from_identifier(status))
        .is_some_and(|status| status != ChunkStatus::Empty)
}

/// Decode the non-registry portion of `SavedTick.CODEC`. Callers deliberately
/// decode list elements independently, matching `ListCodec` partial results.
fn decode_saved_tick_position(tick: &CompoundTag) -> Option<ChunkPos> {
    let (x, _y, z, _delay, _priority) = (
        tick.get_int("x")?,
        tick.get_int("y")?,
        tick.get_int("z")?,
        tick.get_int("t")?,
        tick.get_int("p")?,
    );
    Some(ChunkPos::new(x >> 4, z >> 4))
}

/// Paper's cached top-level tick-list codecs (`BLOCK_TICKS_CODEC` /
/// `FLUID_TICKS_CODEC` are `static final` in `SerializableChunkData`).
///
/// The graphs are immutable and ops-pinned to `NbtOps`, so they are built once
/// and shared across every chunk parse.
static BLOCK_TICK_LIST_CODEC: LazyLock<Arc<dyn Codec<Vec<SavedTick<Block>>, NbtOps>>> =
    LazyLock::new(|| codec::list(saved_tick_codec::<Block, NbtOps>(block_by_name_codec())));
static FLUID_TICK_LIST_CODEC: LazyLock<Arc<dyn Codec<Vec<SavedTick<FluidId>>, NbtOps>>> =
    LazyLock::new(|| codec::list(saved_tick_codec::<FluidId, NbtOps>(fluid_by_name_codec())));

/// Decode Paper's top-level stored-tick lists into typed, per-chunk-filtered
/// values (#370).
///
/// Java:
/// `SavedTick.filterTickListForChunk(chunkData.read("block_ticks",
/// BLOCK_TICKS_CODEC).orElse(List.of()), chunkPos)` — the `read` decodes the
/// whole list through `SavedTick.codec(byNameCodec).listOf()` (a `ListCodec`
/// that retains successful siblings and a partial on element errors), then
/// the filter keeps only ticks packing to the stored chunk. The port uses the
/// same faithful codec factory over `NbtOps` directly on the borrowed list
/// tag, so unknown/malformed element errors keep flowing through the
/// codec-result partial path (Paper's `LOGGER.error("Failed to read field
/// ...")`), then `filter_tick_list_for_chunk`.
///
/// The typed decode is only meaningful for a FULL chunk: every other status is
/// rejected at the `UnsupportedChunkStatus` capability boundary before ticks
/// are consulted, so decoding them would be wasted work and would not claim
/// tick support on proto paths.
///
/// The borrowed `raw_*` lists are decoded in place (no extra clone — the
/// retained raw `ListTag` values are the caller's clones); a decode that drops
/// elements (an error partial) surfaces a [`ChunkParseDiagnostic`] so a stored
/// tick list that fails to decode any element is not silently empty.
fn decode_stored_ticks(
    chunk_data: &CompoundTag,
    stored_pos: ChunkPos,
    status: ChunkStatus,
) -> (
    Vec<SavedTick<Block>>,
    Vec<SavedTick<FluidId>>,
    Vec<ChunkParseDiagnostic>,
) {
    let mut diagnostics = Vec::new();
    if status != ChunkStatus::Full {
        return (Vec::new(), Vec::new(), diagnostics);
    }
    let blocks = decode_tick_list(
        chunk_data,
        BLOCK_TICKS_TAG,
        &BLOCK_TICK_LIST_CODEC,
        &mut diagnostics,
    );
    let fluids = decode_tick_list(
        chunk_data,
        FLUID_TICKS_TAG,
        &FLUID_TICK_LIST_CODEC,
        &mut diagnostics,
    );
    (
        filter_tick_list_for_chunk(&blocks, &stored_pos),
        filter_tick_list_for_chunk(&fluids, &stored_pos),
        diagnostics,
    )
}

/// Decode one stored-tick list from its borrowed compound tag, appending a
/// [`ChunkParseDiagnostic`] when the `ListCodec` drops any element (a failed
/// sibling). A `ListCodec` returns a partial (surviving) value on element
/// errors, so the survivors are carried while the failure is surfaced.
fn decode_tick_list<T>(
    chunk_data: &CompoundTag,
    field: &'static str,
    codec: &Arc<dyn Codec<Vec<SavedTick<T>>, NbtOps>>,
    diagnostics: &mut Vec<ChunkParseDiagnostic>,
) -> Vec<SavedTick<T>>
where
    T: 'static + Clone + Send + Sync,
{
    let Some(tag) = chunk_data.get(field) else {
        return Vec::new();
    };
    let ops = NbtOps::instance();
    let result = codec.parse(&ops, tag);
    if let Some(error) = result.error_ref() {
        diagnostics.push(ChunkParseDiagnostic::StoredTicksDecodeFailed {
            field,
            error: error.message().to_string(),
        });
    }
    result.result_or_partial_silent().unwrap_or_default()
}

/// `UpgradeData` uses the block registry codec with `.orElse(Blocks.AIR)`.
/// A present `i` therefore always decodes (unknown/malformed/wrong-type ids
/// fall back to air); missing fields and malformed siblings remain
/// partial-list failures.
fn upgrade_neighbor_block_ticks_decode_non_empty(list: &ListTag) -> bool {
    list.list.iter().any(|entry| {
        let Tag::Compound(tick) = entry else {
            return false;
        };
        tick.get("i").is_some() && decode_saved_tick_position(tick).is_some()
    })
}

/// `UpgradeData` uses the fluid registry codec with `.orElse(Fluids.EMPTY)`.
fn upgrade_neighbor_fluid_ticks_decode_non_empty(list: &ListTag) -> bool {
    list.list.iter().any(|entry| {
        let Tag::Compound(tick) = entry else {
            return false;
        };
        tick.get("i").is_some() && decode_saved_tick_position(tick).is_some()
    })
}

/// The stored `Map<Heightmap.Types, long[]>`, in enum ordinal order.
///
/// Indexed by `Types` discriminant; the compile-time assertion below keeps the
/// two in lockstep so a seventh `Types` variant fails to build instead of
/// panicking an out-of-bounds index at runtime.
pub type StoredHeightmaps = [Option<Vec<i64>>; 6];

const _: () = assert!(
    Types::all().len()
        == std::mem::size_of::<StoredHeightmaps>() / std::mem::size_of::<Option<Vec<i64>>>()
);

/// Parse only the heightmap types allowed by the decoded chunk status.
/// Missing/wrong-tag `Heightmaps`, unknown keys, wrong-tag values, and known
/// keys outside `heightmaps_after` are absent exactly as in Paper.
pub fn parse_heightmaps(chunk_data: &CompoundTag, heightmaps_after: &[Types]) -> StoredHeightmaps {
    let mut out: StoredHeightmaps = std::array::from_fn(|_| None);
    let Some(heightmaps) = chunk_data.get_compound(HEIGHTMAPS_TAG) else {
        return out;
    };

    for ty in heightmaps_after {
        if let Some(raw) = heightmaps.get_long_array(ty.serialization_key()) {
            out[*ty as usize] = Some(raw.clone());
        }
    }
    out
}

/// The `write()` heightmaps half: build the `Heightmaps` compound from stored
/// columns, keyed by `Types.getSerializationKey()`.
///
/// Mirrors `SerializableChunkData.write()`: it iterates the already-filtered
/// `EnumMap` in ordinal (declaration) order and passes each raw `long[]` into a
/// `LongArrayTag`. Java shares the array reference — `copyOf`'s single
/// `data.clone()` has already happened at the stored-build boundary, so this
/// move is the one copy after parse, matching Java's copy count on the
/// live-chunk → disk path. Stored columns are consumed, never cloned again:
/// `put_long_array` takes the `Vec<i64>` by value, and Java's `write` passes
/// the array into the tag without copying. (On the disk → tag read-back path
/// Java makes zero copies — the tag's array flows straight into the map by
/// reference — while Rust's ownership model still requires the single clone in
/// `parse_heightmaps`; `write` adds no second copy on either path.)
///
/// The `copyOf` filter — keep only types the persisted status's
/// `heightmapsAfter()` allows — lives at the stored-build boundary
/// ([`parse_heightmaps`], which is passed the status slice), not here. `write`
/// emits whatever the map holds, exactly as Java's `write` takes no status
/// argument.
///
/// The key order this produces is NOT the order Paper's own chunk files store
/// heightmap keys in: Java's `CompoundTag` is a fastutil hash map, so the
/// on-disk fixture order (`MOTION_BLOCKING, MOTION_BLOCKING_NO_LEAVES,
/// WORLD_SURFACE, OCEAN_FLOOR`) reflects hash order, not the `EnumMap`
/// iteration order. Rivet's insertion-ordered `CompoundTag` therefore writes
/// ordinal order — the `compound_key_order` divergence counted in PARITY.md —
/// so the fixture's key order is never a byte-order oracle here.
pub fn write_heightmaps(mut stored: StoredHeightmaps) -> CompoundTag {
    let mut out = CompoundTag::new();
    for ty in Types::all() {
        // `.get_mut` + `.take()` moves each column out so `put_long_array` gets
        // it by value (the one copy after parse, matching Java's `copyOf`/`write`
        // share). The `StoredHeightmaps` lockstep assertion above already bounds
        // the index; `get_mut` keeps this an Option-handled read regardless.
        if let Some(raw) = stored.get_mut(ty as usize).and_then(Option::take) {
            out.put_long_array(ty.serialization_key(), raw);
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

/// Decode light only after the caller has decoded this section's block-state
/// and biome palettes. This narrow operation lets #336 preserve Paper's
/// per-section hard-error order without top-level parsing touching light.
pub fn decode_section_light(
    section: &CompoundTag,
) -> Result<SectionLightData, SerializableChunkDataError> {
    let y = section.get_byte_or("Y", 0) as i32;
    let block_light = section
        .get_byte_array(BLOCK_LIGHT_TAG)
        .map(|bytes| signed_bytes(bytes));
    let sky_light = section
        .get_byte_array(SKY_LIGHT_TAG)
        .map(|bytes| signed_bytes(bytes));
    if let Some(bytes) = &block_light
        && bytes.len() != 2048
    {
        return Err(SerializableChunkDataError::MalformedDataLayer {
            section_y: y,
            field: BLOCK_LIGHT_TAG,
            length: bytes.len(),
        });
    }
    if let Some(bytes) = &sky_light
        && bytes.len() != 2048
    {
        return Err(SerializableChunkDataError::MalformedDataLayer {
            section_y: y,
            field: SKY_LIGHT_TAG,
            length: bytes.len(),
        });
    }
    Ok(SectionLightData {
        y,
        block_light,
        sky_light,
        block_state: state_or_absent(section, BLOCKLIGHT_STATE_TAG),
        sky_state: state_or_absent(section, SKYLIGHT_STATE_TAG),
    })
}

/// Parse the light-only portion of the `sections` list. Non-compound list
/// entries are ignored; absent/wrong-tag arrays remain absent. Explicit arrays
/// are validated at Paper's `DataLayer(byte[])` boundary and therefore panic
/// on a length other than 2048, like Java's `IllegalArgumentException`.
pub fn parse_section_lights(chunk_data: &CompoundTag) -> Vec<SectionLightData> {
    let Some(sections) = chunk_data.get_list(SECTIONS_TAG) else {
        return Vec::new();
    };
    try_parse_section_lights_from_list(sections).unwrap_or_else(|error| panic!("{error}"))
}

fn try_parse_section_lights_from_list(
    sections: &ListTag,
) -> Result<Vec<SectionLightData>, SerializableChunkDataError> {
    sections
        .list
        .iter()
        .filter_map(|tag| match tag {
            rivet_nbt::tag::Tag::Compound(section) => Some(section),
            _ => None,
        })
        .map(decode_section_light)
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

/// Rebuild the Starlight nibbles from the decoded per-section light, without
/// running lighting. `light_correct` distinguishes the two ingest paths Paper
/// has for a serialized chunk's light:
///
/// - `true` (Starlight save): every `SectionLightData` carries the persisted
///   `starlight.*light_state` INTs (`SaveUtil.loadLightHookReal`), so each
///   present byte array is rebuilt with its raw state — the section stays
///   absent unless it has data (an absent state INT defaults to `Null`, so a
///   bytes-but-no-state section is skipped, matching Paper).
/// - `false` (any save that failed the Starlight predicate): if the save is
///   genuinely vanilla-format — no `starlight.*light_state` INT on any section
///   — a present plain `BlockLight`/`SkyLight` array is the light vanilla
///   `SerializableChunkData` read as a `DataLayer` and would queue for the send
///   (`new DataLayer(byte[])` is never empty, so it always becomes an update
///   mask + bytes); each such array is installed as an `Initialised` nibble
///   (issue #531). A Starlight save that merely failed the predicate still
///   carries those state INTs and installs nothing, matching Paper. Paper
///   itself would drop these and relight; Rivet has no lighting engine (#184),
///   so the faithful packet is the persisted array (issue #531).
///
/// The vanilla-format fallback is gated on both of Paper's conditions for it:
/// the save must be genuinely vanilla-format — *no* section carries a
/// `starlight.*light_state` INT (`SaveUtil` writes one for every data-carrying
/// section) — and `light_correct` must be false. Paper's `loadStarlightLightData`
/// returns all-null nibbles without touching the plain arrays when `lightCorrect`
/// is false, and its `blockState >= 0` guard skips any section whose state INT
/// is absent on a light-correct chunk, so a Starlight save that merely failed
/// the predicate (version mismatch / missing `isLightOn` / status below Light)
/// still carries those INTs and must not be reinterpreted as vanilla.
///
/// Any invalid state, state/data mismatch, or out-of-range section reproduces
/// Paper's caught load failure: all-null arrays are retained and
/// `light_correct` becomes false, with no partially installed data.
pub fn reconstruct_lights(
    height: SimpleLevelHeightAccessor,
    sections: &[SectionLightData],
    light_correct: bool,
    has_sky_light: bool,
) -> ReconstructedLightData {
    let count = height.get_sections_count() as usize + 2;
    let empty = || filled_empty_light(count);
    // Vanilla-format saves carry no per-section Starlight state INTs; a
    // Starlight save always writes at least one for a data-carrying section.
    let vanilla_format = sections
        .iter()
        .all(|section| section.block_state < 0 && section.sky_state < 0);
    let parsed = std::panic::catch_unwind(|| {
        let mut block = empty();
        let mut sky = empty();
        let min_light_section = height.get_min_section_y() - 1;
        for section in sections {
            let index =
                usize::try_from(section.y - min_light_section).expect("light section below world");
            if light_correct && section.block_state >= 0 {
                block[index] = rebuild_nibble(section.block_light.clone(), section.block_state);
            } else if !light_correct
                && vanilla_format
                && let Some(bytes) = &section.block_light
            {
                // Genuine vanilla-format save, not Starlight-lit: a present plain
                // `BlockLight` array is the light, installed as an `Initialised`
                // nibble exactly like the vanilla `new DataLayer(byte[])` the
                // send would carry (issue #531).
                block[index] = SwmrNibbleArray::new_with_bytes(bytes.clone());
            }
            if light_correct && section.sky_state >= 0 && has_sky_light {
                sky[index] = rebuild_nibble(section.sky_light.clone(), section.sky_state);
            } else if !light_correct
                && vanilla_format
                && has_sky_light
                && let Some(bytes) = &section.sky_light
            {
                sky[index] = SwmrNibbleArray::new_with_bytes(bytes.clone());
            }
        }
        (block, sky)
    });

    match parsed {
        Ok((block_nibbles, sky_nibbles)) => ReconstructedLightData {
            block_nibbles,
            sky_nibbles,
            light_correct,
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
    use crate::chunk::data_layer::DataLayer;
    use crate::level::height_accessor;
    use crate::levelgen::heightmap::{FINAL_HEIGHTMAPS, WORLDGEN_HEIGHTMAPS};
    use crate::lighting::light_update_data::build_light_update_data;
    use crate::lighting::swmr_nibble_array::ARRAY_SIZE;
    use crate::ticks::TickPriority;
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

    fn named_fixture(dimension: &str, region: &str, chunk: &str) -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/chunk")
            .join(dimension)
            .join(region)
            .join(chunk);
        let bytes = std::fs::read(path).expect("Paper 26.2 chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    /// A radius-1 loaded-world auxiliary-data fixture (issue #371) — the
    /// committed `fixtures/loaded-world/chunk/` corpus captured from the
    /// disposable New World copy, each named for its role (`mineshaft-structure-refs`,
    /// `block-ticks`, `fluid-ticks`, `chest-block-entity`, `clean-spawn`).
    fn loaded_world_fixture(name: &str) -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk")
            .join(name);
        let bytes = std::fs::read(path).expect("Paper 26.2 loaded-world chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    fn top_level(status: &str) -> CompoundTag {
        let mut chunk = CompoundTag::new();
        chunk.put_string(STATUS_TAG, status);
        chunk.put_int(DATA_VERSION_TAG, CURRENT_DATA_VERSION);
        chunk
    }

    fn saved_tick_with_id(id: &str, x: i32, z: i32) -> CompoundTag {
        let mut tick = CompoundTag::new();
        tick.put_string("i", id);
        tick.put_int("x", x);
        tick.put_int("y", 0);
        tick.put_int("z", z);
        tick.put_int("t", 1);
        tick.put_int("p", 0);
        tick
    }

    fn block_tick(x: i32, z: i32) -> CompoundTag {
        saved_tick_with_id("minecraft:stone", x, z)
    }

    fn fluid_tick(x: i32, z: i32) -> CompoundTag {
        saved_tick_with_id("minecraft:water", x, z)
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
                error: BlockEntityTypeError::InvalidIdType,
                ..
            })
        ));
        assert!(matches!(
            &outcomes[1],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                error: BlockEntityTypeError::InvalidIdType,
                ..
            })
        ));
        let invalid_type_errors = [&outcomes[0], &outcomes[1]].map(|outcome| match outcome {
            SerializedBlockEntityOutcome::InvalidUnpacked(entry) => &entry.error,
            other => panic!("expected invalid-type outcome, got {other:?}"),
        });
        assert_eq!(invalid_type_errors[0], invalid_type_errors[1]);
        assert!(outcomes[0].raw_tag().tags.get("id").is_none());
        assert!(matches!(
            outcomes[1].raw_tag().tags.get("id"),
            Some(Tag::Int(_))
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
    fn identifier_length_boundary_is_exact_for_default_namespace() {
        // No explicit namespace adds `minecraft:` in Paper. A path of 21,835
        // UTF-16 units therefore reaches the effective 21,845-unit limit.
        let at_limit = "a".repeat(21_835);
        let outcome = one_level(block_entity(Some(&at_limit), 5, 70, -21));
        assert!(matches!(
            outcome,
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                error: BlockEntityTypeError::UnknownId { identifier },
                ..
            }) if identifier.path() == at_limit
        ));

        let over_limit = "a".repeat(21_836);
        let tag = block_entity(Some(&over_limit), 5, 70, -21);
        let outcome = one_level(tag.clone());
        assert!(matches!(
            outcome,
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                source_index: 0,
                position,
                error: BlockEntityTypeError::MalformedId { value },
                raw_tag,
            }) if position == BlockPos::new(37, 70, -37)
                && value == over_limit
                && raw_tag == tag
        ));
    }

    #[test]
    fn overlong_middle_entry_retains_raw_tag_and_later_sources_continue() {
        let first = block_entity(Some("bad id"), 5, 70, -21);
        let over_limit = "a".repeat(21_836);
        let middle = block_entity(Some(&over_limit), 6, 71, -20);
        let later = block_entity(Some("minecraft:chest"), 7, 72, -19);
        let outcomes = reconstruct_block_entities(
            &ChunkPos::new(2, -3),
            &[first.clone(), middle.clone(), later.clone()],
            BlockEntityChunkKind::Level,
        );

        assert_eq!(outcomes.len(), 3);
        assert!(matches!(
            &outcomes[0],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                source_index: 0,
                error: BlockEntityTypeError::MalformedId { value },
                raw_tag,
                ..
            }) if value == "bad id" && raw_tag == &first
        ));
        assert!(matches!(
            &outcomes[1],
            SerializedBlockEntityOutcome::InvalidUnpacked(FailedSerializedBlockEntity {
                source_index: 1,
                position,
                error: BlockEntityTypeError::MalformedId { value },
                raw_tag,
            }) if *position == BlockPos::new(38, 71, -36)
                && value == &over_limit
                && raw_tag == &middle
        ));
        assert!(matches!(
            &outcomes[2],
            SerializedBlockEntityOutcome::ResolvedUnpacked(ResolvedSerializedBlockEntity {
                source_index: 2,
                position,
                entity_type,
                raw_tag,
            }) if *position == BlockPos::new(39, 72, -35)
                && entity_type.name() == "minecraft:chest"
                && raw_tag == &later
        ));
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
    fn status_type_precheck_precedes_newer_data_version() {
        let height = height_accessor::create(-64, 384);
        let mut missing = CompoundTag::new();
        missing.put_long(DATA_VERSION_TAG, i64::from(CURRENT_DATA_VERSION + 1));
        assert!(
            SerializableChunkData::parse(height, &missing)
                .unwrap()
                .is_none()
        );

        missing.put_int(STATUS_TAG, 7);
        assert!(
            SerializableChunkData::parse(height, &missing)
                .unwrap()
                .is_none()
        );

        missing.put_string(STATUS_TAG, "minecraft:full");
        assert!(matches!(
            SerializableChunkData::parse(height, &missing),
            Err(SerializableChunkDataError::NewerDataVersion {
                found,
                current,
            }) if found == CURRENT_DATA_VERSION + 1 && current == CURRENT_DATA_VERSION
        ));
    }

    #[test]
    fn numeric_fields_coerce_and_default_and_y_pos_is_ignored() {
        let height = height_accessor::create(-64, 384);
        let mut chunk = top_level("full");
        chunk.put_double("xPos", 3.75);
        chunk.put_short("zPos", -7);
        chunk.put_byte("LastUpdate", 12);
        chunk.put_float("InhabitedTime", 41.9);
        chunk.put_string("yPos", "deliberately wrong");
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.stored_pos(), ChunkPos::new(3, -7));
        assert_eq!(parsed.last_update_time(), 12);
        assert_eq!(parsed.inhabited_time(), 41);
        assert_eq!(parsed.min_section_y(), -4);

        let mut defaults = top_level("minecraft:full");
        defaults.put_string("xPos", "wrong");
        defaults.put_string("LastUpdate", "wrong");
        let parsed = SerializableChunkData::parse(height, &defaults)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.stored_pos(), ChunkPos::new(0, 0));
        assert_eq!(parsed.last_update_time(), 0);
        assert_eq!(parsed.inhabited_time(), 0);
    }

    #[test]
    fn empty_and_unknown_status_diagnose_then_fall_back_to_empty() {
        let height = height_accessor::create(-64, 384);
        for name in ["", "minecraft:not_a_status", "other:full"] {
            let parsed = SerializableChunkData::parse(height, &top_level(name))
                .unwrap()
                .unwrap();
            assert_eq!(parsed.status(), ChunkStatus::Empty);
            assert_eq!(
                parsed.diagnostics(),
                &[ChunkParseDiagnostic::UnknownStatus(name.to_string())]
            );
        }

        let mut malformed = top_level("");
        let mut section = section_tag(4);
        section.put_byte_array(BLOCK_LIGHT_TAG, vec![0; 3]);
        malformed.put(
            SECTIONS_TAG.into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(section)])),
        );
        let parsed = SerializableChunkData::parse(height, &malformed)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.section_tags().list.len(), 1);
        let section = parsed.section_tags().get_compound(0).unwrap();
        assert!(matches!(
            decode_section_light(section),
            Err(SerializableChunkDataError::MalformedDataLayer { section_y: 4, .. })
        ));
    }

    #[test]
    fn heightmap_keys_are_status_filtered_typed_and_case_sensitive() {
        let height = height_accessor::create(-64, 384);
        let mut chunk = top_level("minecraft:full");
        let mut heightmaps = CompoundTag::new();
        heightmaps.put_long_array("WORLD_SURFACE", vec![1]);
        heightmaps.put_long_array("OCEAN_FLOOR", vec![2]);
        heightmaps.put_int_array("MOTION_BLOCKING", vec![3]);
        heightmaps.put_long_array("motion_blocking_no_leaves", vec![4]);
        heightmaps.put_long_array("WORLD_SURFACE_WG", vec![5]);
        chunk.put("Heightmaps".into(), Tag::Compound(heightmaps));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(
            parsed.heightmaps()[Types::WorldSurface as usize],
            Some(vec![1])
        );
        assert_eq!(
            parsed.heightmaps()[Types::OceanFloor as usize],
            Some(vec![2])
        );
        assert!(parsed.heightmaps()[Types::MotionBlocking as usize].is_none());
        assert!(parsed.heightmaps()[Types::MotionBlockingNoLeaves as usize].is_none());
        assert!(parsed.heightmaps()[Types::WorldSurfaceWg as usize].is_none());
    }

    #[test]
    fn light_correct_uses_is_light_on_presence_not_value_or_type() {
        let height = height_accessor::create(-64, 384);
        for tag in [
            Tag::Byte(rivet_nbt::byte_tag::ByteTag::value_of(0)),
            Tag::String(rivet_nbt::string_tag::StringTag::value_of("false".into())),
        ] {
            let mut chunk = top_level("minecraft:light");
            chunk.put(IS_LIGHT_ON_TAG.into(), tag);
            chunk.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
            assert!(
                SerializableChunkData::parse(height, &chunk)
                    .unwrap()
                    .unwrap()
                    .light_correct()
            );
        }
        let mut absent = top_level("minecraft:light");
        absent.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
        assert!(
            !SerializableChunkData::parse(height, &absent)
                .unwrap()
                .unwrap()
                .light_correct()
        );
        let mut early = top_level("minecraft:initialize_light");
        early.put_int(IS_LIGHT_ON_TAG, 1);
        early.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
        assert!(
            !SerializableChunkData::parse(height, &early)
                .unwrap()
                .unwrap()
                .light_correct()
        );
    }

    #[test]
    fn post_processing_preserves_outer_shape_and_numeric_coercion() {
        let height = height_accessor::create(-64, 384);
        let mut chunk = top_level("minecraft:full");
        let offsets = ListTag::with_list(vec![
            Tag::Int(IntTag::value_of(7)),
            Tag::Long(LongTag::value_of(65_537)),
            Tag::String(rivet_nbt::string_tag::StringTag::value_of("wrong".into())),
        ]);
        chunk.put(
            "PostProcessing".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::List(offsets),
                Tag::List(ListTag::new()),
                Tag::Int(IntTag::value_of(1)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(
            parsed.post_processing_sections(),
            &[Some(vec![7, 1, 0]), None, None]
        );
    }

    #[test]
    fn entity_lists_filter_non_compounds_without_reordering() {
        let height = height_accessor::create(-64, 384);
        let mut first = CompoundTag::new();
        first.put_int("order", 1);
        let mut second = CompoundTag::new();
        second.put_int("order", 2);
        let mixed = ListTag::with_list(vec![
            Tag::Compound(first),
            Tag::Int(IntTag::value_of(99)),
            Tag::Compound(second),
        ]);
        let mut chunk = top_level("minecraft:full");
        chunk.put("entities".into(), Tag::List(mixed.clone()));
        chunk.put("block_entities".into(), Tag::List(mixed));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        for compounds in [parsed.entities(), parsed.block_entities()] {
            assert_eq!(compounds.len(), 2);
            assert_eq!(compounds[0].get_int_or("order", 0), 1);
            assert_eq!(compounds[1].get_int_or("order", 0), 2);
        }
    }

    #[test]
    fn ancillary_surfaces_have_named_unsupported_markers() {
        let height = height_accessor::create(-64, 384);
        let cases: Vec<(&str, Tag, SerializableChunkDataError)> = vec![
            (
                "blending_data",
                Tag::Compound({
                    let mut tag = CompoundTag::new();
                    tag.put_int("min_section", -4);
                    tag.put_int("max_section", 19);
                    tag
                }),
                SerializableChunkDataError::UnsupportedBlendingData,
            ),
            (
                "entities",
                Tag::List(ListTag::with_list(vec![Tag::Compound(CompoundTag::new())])),
                SerializableChunkDataError::UnsupportedEntities,
            ),
            (
                "ChunkBukkitValues",
                Tag::Compound({
                    let mut tag = CompoundTag::new();
                    tag.put_int("plugin:value", 1);
                    tag
                }),
                SerializableChunkDataError::UnsupportedPersistentData,
            ),
        ];
        for (field, value, expected) in cases {
            let mut chunk = top_level("minecraft:full");
            chunk.put(field.into(), value);
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(
                parsed.validate_full_for_reconstruction(),
                Err(expected),
                "{field}"
            );
        }

        // A non-empty `block_entities` list is carried (not rejected): the
        // reconstruction installs the serialized tags as pending NBT and
        // materialization defers with #341.
        let mut block_entity_chunk = top_level("minecraft:full");
        block_entity_chunk.put(
            "block_entities".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(CompoundTag::new())])),
        );
        let parsed = SerializableChunkData::parse(height, &block_entity_chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.block_entities().len(), 1);

        // Stored block/fluid tick lists decode into typed stored values and are
        // carried; a FULL chunk carrying a non-empty stored list is now fully
        // capable (the values are carried for the caller's runtime composition,
        // nothing schedules or installs them, #370).
        let mut tick_chunk = top_level("minecraft:full");
        tick_chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(block_tick(0, 0))])),
        );
        let parsed = SerializableChunkData::parse(height, &tick_chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.stored_block_ticks().len(), 1);
        let parsed_ticks = parsed.stored_block_ticks()[0];
        assert_eq!(parsed_ticks.pos, BlockPos::new(0, 0, 0));
        assert_eq!(parsed_ticks.delay, 1);
        assert_eq!(
            parsed_ticks.r#type,
            Block::from_name("minecraft:stone").unwrap()
        );

        let mut tick_chunk = top_level("minecraft:full");
        tick_chunk.put(
            "fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(fluid_tick(0, 0))])),
        );
        let parsed = SerializableChunkData::parse(height, &tick_chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.stored_fluid_ticks().len(), 1);
        assert_eq!(
            parsed.stored_fluid_ticks()[0].r#type,
            FluidId::from_name("minecraft:water").unwrap()
        );

        let mut structures = CompoundTag::new();
        let mut starts = CompoundTag::new();
        starts.put_int("minecraft:village", 1);
        structures.put("starts".into(), Tag::Compound(starts));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));
        assert_eq!(
            SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap()
                .validate_full_for_reconstruction(),
            Err(SerializableChunkDataError::UnsupportedStructures)
        );

        for (field, tick) in [
            ("neighbor_block_ticks", block_tick(0, 0)),
            ("neighbor_fluid_ticks", fluid_tick(0, 0)),
        ] {
            let mut upgrade = CompoundTag::new();
            upgrade.put(
                field.into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(tick)])),
            );
            let mut chunk = top_level("minecraft:full");
            chunk.put("UpgradeData".into(), Tag::Compound(upgrade));
            assert_eq!(
                SerializableChunkData::parse(height, &chunk)
                    .unwrap()
                    .unwrap()
                    .validate_full_for_reconstruction(),
                Err(SerializableChunkDataError::UnsupportedUpgradeData { field })
            );
        }

        let mut chunk = top_level("minecraft:full");
        chunk.put_long_array("carving_mask", vec![1, 2, 3]);
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.carving_mask(), Some(&[1, 2, 3][..]));
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
    }

    #[test]
    fn malformed_defaulted_and_relocated_optional_data_is_effectively_empty() {
        let height = height_accessor::create(-64, 384);
        let mut chunk = top_level("minecraft:full");
        chunk.put_int("xPos", 3);
        chunk.put_int("zPos", -2);
        chunk.put("blending_data".into(), Tag::Compound(CompoundTag::new()));
        chunk.put(
            "below_zero_retrogen".into(),
            Tag::Compound(CompoundTag::new()),
        );
        chunk.put(
            "ChunkBukkitValues".into(),
            Tag::Compound(CompoundTag::new()),
        );
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(CompoundTag::new())])),
        );
        chunk.put(
            "fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(CompoundTag::new())])),
        );
        let mut upgrade = CompoundTag::new();
        upgrade.put(
            "neighbor_block_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(CompoundTag::new())])),
        );
        chunk.put("UpgradeData".into(), Tag::Compound(upgrade));

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.raw_block_ticks().list.len(), 1);
        assert_eq!(parsed.raw_fluid_ticks().list.len(), 1);
        // Both tick lists are entirely malformed, so the ListCodec drops every
        // element (Paper logs each) and the chunk carries no stored ticks.
        assert!(parsed.stored_block_ticks().is_empty());
        assert!(parsed.stored_fluid_ticks().is_empty());
        assert!(parsed.raw_blending_data().is_some());
        assert!(parsed.raw_below_zero_retrogen().is_some());
        assert!(parsed.raw_upgrade_data().is_some());
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
    }

    #[test]
    fn top_level_tick_codecs_keep_valid_partial_siblings_and_validate_registries() {
        let height = height_accessor::create(-64, 384);

        // A `ListCodec` retains only successful siblings; an empty compound
        // (missing `i`) and an unknown/malformed id fail their element, and the
        // valid sibling survives. The typed stored list carries exactly that
        // valid tick (filtered to the stored chunk (0,0)); a FULL chunk carrying
        // stored ticks now reconstructs (the typed values are carried, nothing
        // schedules them, #370).
        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Compound(CompoundTag::new()),
                Tag::Compound(saved_tick_with_id("minecraft:not_a_block", 0, 0)),
                Tag::Compound(block_tick(0, 0)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.stored_block_ticks().len(), 1);
        assert_eq!(parsed.stored_block_ticks()[0].pos, BlockPos::new(0, 0, 0));
        assert_eq!(parsed.stored_block_ticks()[0].delay, 1);
        assert_eq!(
            parsed.stored_block_ticks()[0].r#type,
            Block::from_name("minecraft:stone").unwrap()
        );
        assert_eq!(parsed.stored_fluid_ticks(), &[]);

        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Compound(CompoundTag::new()),
                Tag::Compound(block_tick(0, 0)), // block id in the fluid registry -> unknown
                Tag::Compound(fluid_tick(0, 0)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.stored_fluid_ticks().len(), 1);
        assert_eq!(
            parsed.stored_fluid_ticks()[0].r#type,
            FluidId::from_name("minecraft:water").unwrap()
        );
        assert_eq!(parsed.stored_block_ticks(), &[]);

        // Relocated ticks decode but are filtered out by the per-chunk filter,
        // so the stored lists are empty and construction still succeeds.
        for (field, relocated) in [
            ("block_ticks", block_tick(16, 0)),
            ("fluid_ticks", fluid_tick(0, -16)),
        ] {
            let mut chunk = top_level("minecraft:full");
            chunk.put(
                field.into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(relocated)])),
            );
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()), "{field}");
            if field == "block_ticks" {
                assert_eq!(parsed.stored_block_ticks(), &[]);
            } else {
                assert_eq!(parsed.stored_fluid_ticks(), &[]);
            }
        }

        // Unknown-only lists decode to an empty partial -> stored empty, Ok.
        for (field, tick) in [
            (
                "block_ticks",
                saved_tick_with_id("minecraft:not_a_block", 0, 0),
            ),
            (
                "fluid_ticks",
                saved_tick_with_id("minecraft:not_a_fluid", 0, 0),
            ),
            ("fluid_ticks", block_tick(0, 0)),
        ] {
            let mut chunk = top_level("minecraft:full");
            chunk.put(
                field.into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(tick)])),
            );
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()), "{field}");
            if field == "block_ticks" {
                assert_eq!(parsed.stored_block_ticks(), &[]);
            } else {
                assert_eq!(parsed.stored_fluid_ticks(), &[]);
            }
        }
    }

    #[test]
    fn top_level_tick_codecs_normalize_unqualified_identifiers() {
        let height = height_accessor::create(-64, 384);

        // Unqualified ids normalize through `Identifier` before the registry
        // lookup ("stone" -> "minecraft:stone"), and a malformed id fails only
        // its element.
        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Compound(saved_tick_with_id("not valid", 0, 0)),
                Tag::Compound(saved_tick_with_id("stone", 0, 0)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.stored_block_ticks().len(), 1);
        assert_eq!(
            parsed.stored_block_ticks()[0].r#type,
            Block::from_name("minecraft:stone").unwrap()
        );

        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Compound(saved_tick_with_id("not valid", 0, 0)),
                Tag::Compound(saved_tick_with_id("water", 0, 0)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.stored_fluid_ticks().len(), 1);
        assert_eq!(
            parsed.stored_fluid_ticks()[0].r#type,
            FluidId::from_name("minecraft:water").unwrap()
        );

        // Relocated normalized ticks decode but are filtered out.
        for (field, relocated) in [
            ("block_ticks", saved_tick_with_id("stone", 16, 0)),
            ("fluid_ticks", saved_tick_with_id("water", 0, -16)),
        ] {
            let mut chunk = top_level("minecraft:full");
            chunk.put(
                field.into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(relocated)])),
            );
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()), "{field}");
            if field == "block_ticks" {
                assert_eq!(parsed.stored_block_ticks(), &[]);
            } else {
                assert_eq!(parsed.stored_fluid_ticks(), &[]);
            }
        }

        // Unqualified unknown ids are still unknown after normalization.
        for (field, unknown) in [
            ("block_ticks", saved_tick_with_id("not_a_block", 0, 0)),
            ("fluid_ticks", saved_tick_with_id("not_a_fluid", 0, 0)),
        ] {
            let mut chunk = top_level("minecraft:full");
            chunk.put(
                field.into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(unknown)])),
            );
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()), "{field}");
            if field == "block_ticks" {
                assert_eq!(parsed.stored_block_ticks(), &[]);
            } else {
                assert_eq!(parsed.stored_fluid_ticks(), &[]);
            }
        }
    }

    /// A tracked 26.2 nether chunk with 13 stored lava `fluid_ticks` decodes
    /// through the real `SerializableChunkData::parse` path into exact typed
    /// values (positions, delay, priority) — the full stored-value surface, not
    /// the synthetic JsonOps round-trip the codec unit tests cover. This is the
    /// decode/carry layer only: the parse carries the typed ticks as stored
    /// values, nothing schedules or executes them, and the chunk is
    /// full-capable (#370 remains open for the deferred
    /// `LevelChunkTicks`/`ProtoChunkTicks` containers).
    #[test]
    fn real_26_2_nether_fixture_decodes_stored_fluid_ticks_exactly() {
        let fixture = named_fixture("the_nether", "0.0", "0.0.nbt");
        assert_eq!(fixture.get_int_or("DataVersion", -1), 4903);
        assert_eq!(
            fixture.get_string("Status").map(String::as_str),
            Some("minecraft:full")
        );

        let parsed = SerializableChunkData::parse(height_accessor::create(-64, 384), &fixture)
            .unwrap()
            .expect("nether fixture has a Status");
        assert_eq!(parsed.stored_pos(), ChunkPos::new(0, 0));
        // The chunk decodes its 13 stored lava ticks faithfully and is
        // full-capable: the typed ticks are carried as stored values, not
        // scheduled (the `LevelChunkTicks`/`ProtoChunkTicks` containers stay
        // deferred to #370).
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        // The nether chunk stores fluid ticks only; its `block_ticks` list is
        // empty.
        assert!(parsed.stored_block_ticks().is_empty());
        assert_eq!(parsed.stored_fluid_ticks().len(), 13);

        let lava = FluidId::LAVA;
        let normal = TickPriority::Normal;
        let expected = vec![
            SavedTick::new(lava, BlockPos::new(15, 101, 5), 0, normal),
            SavedTick::new(lava, BlockPos::new(10, 39, 12), 0, normal),
            SavedTick::new(lava, BlockPos::new(14, 90, 11), 0, normal),
            SavedTick::new(lava, BlockPos::new(0, 10, 5), 0, normal),
            SavedTick::new(lava, BlockPos::new(8, 64, 14), 0, normal),
            SavedTick::new(lava, BlockPos::new(3, 16, 5), 0, normal),
            SavedTick::new(lava, BlockPos::new(3, 88, 2), 0, normal),
            SavedTick::new(lava, BlockPos::new(14, 87, 1), 0, normal),
            SavedTick::new(lava, BlockPos::new(2, 100, 10), 0, normal),
            SavedTick::new(lava, BlockPos::new(11, 90, 5), 0, normal),
            SavedTick::new(lava, BlockPos::new(14, 89, 9), 0, normal),
            SavedTick::new(lava, BlockPos::new(1, 104, 8), 0, normal),
            SavedTick::new(lava, BlockPos::new(5, 51, 6), 0, normal),
        ];
        assert_eq!(parsed.stored_fluid_ticks(), expected.as_slice());
    }

    /// Radius-1 loaded-world fixture `0.-4.nbt` (role `mineshaft-structure-refs`):
    /// a single `structures.References` entry decodes into an ordered
    /// [`StructureReference`] (registry-keyed + packed chunk-long), stays behind
    /// no boundary, and survives the reconstruct-time >8-chunk filter against
    /// both the stored and the requested position (the reference packs to chunk
    /// (5,-6), chessboard distance 5 from (0,-4)).
    #[test]
    fn real_26_2_mineshaft_fixture_carries_ordered_structure_reference() {
        let fixture = loaded_world_fixture("0.-4.nbt");
        assert_eq!(fixture.get_int_or("DataVersion", -1), 4903);
        assert_eq!(
            fixture.get_string("Status").map(String::as_str),
            Some("minecraft:full")
        );

        let parsed = SerializableChunkData::parse(height_accessor::create(-64, 384), &fixture)
            .unwrap()
            .expect("loaded-world fixture has a Status");
        assert_eq!(parsed.stored_pos(), ChunkPos::new(0, -4));
        // References-only structures never block FULL construction (#369).
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.structures_references().len(), 1);
        let entry = &parsed.structures_references()[0];
        assert_eq!(entry.identifier.namespace(), "minecraft");
        assert_eq!(entry.identifier.path(), "mineshaft");
        assert_eq!(entry.references, vec![-25769803771]);
        assert_eq!(ChunkPos::unpack(entry.references[0]), ChunkPos::new(5, -6));
        assert_eq!(parsed.diagnostics(), &[]);

        // The reconstruct-time filter keeps it (distance <= 8) against the
        // stored position, with no diagnostic.
        let (kept, diagnostics) =
            filter_structure_references(parsed.structures_references(), &parsed.stored_pos());
        assert_eq!(kept, parsed.structures_references().to_vec());
        assert!(diagnostics.is_empty());
    }

    /// Radius-1 loaded-world fixture `-17.-19.nbt` (role `block-ticks`): a
    /// single stored `block_ticks` entry decodes into an exact typed
    /// [`SavedTick<Block>`] (sand at the packed chunk, delay -59, normal
    /// priority) and the FULL chunk stays capable — nothing is scheduled or
    /// generated, the value is carried (#370).
    #[test]
    fn real_26_2_block_tick_fixture_carries_typed_stored_tick() {
        let fixture = loaded_world_fixture("-17.-19.nbt");
        assert_eq!(fixture.get_int_or("DataVersion", -1), 4903);

        let parsed = SerializableChunkData::parse(height_accessor::create(-64, 384), &fixture)
            .unwrap()
            .expect("loaded-world fixture has a Status");
        assert_eq!(parsed.stored_pos(), ChunkPos::new(-17, -19));
        assert_eq!(
            parsed.stored_block_ticks(),
            &[SavedTick::new(
                Block::from_name("minecraft:sand").unwrap(),
                BlockPos::new(-268, 61, -302),
                -59,
                TickPriority::Normal,
            )]
        );
        assert!(parsed.stored_fluid_ticks().is_empty());
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
    }

    /// Radius-1 loaded-world fixture `-2.-2.nbt` (role `fluid-ticks`): a single
    /// stored `fluid_ticks` entry decodes into an exact typed
    /// [`SavedTick<FluidId>`] (water at the packed chunk, delay 2, normal
    /// priority) and is carried, not scheduled.
    #[test]
    fn real_26_2_fluid_tick_fixture_carries_typed_stored_tick() {
        let fixture = loaded_world_fixture("-2.-2.nbt");
        assert_eq!(fixture.get_int_or("DataVersion", -1), 4903);

        let parsed = SerializableChunkData::parse(height_accessor::create(-64, 384), &fixture)
            .unwrap()
            .expect("loaded-world fixture has a Status");
        assert_eq!(parsed.stored_pos(), ChunkPos::new(-2, -2));
        assert_eq!(
            parsed.stored_fluid_ticks(),
            &[SavedTick::new(
                FluidId::WATER,
                BlockPos::new(-27, 59, -17),
                2,
                TickPriority::Normal,
            )]
        );
        assert!(parsed.stored_block_ticks().is_empty());
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
    }

    /// Radius-1 loaded-world fixture `-19.-21.nbt` (role `chest-block-entity`):
    /// the unpacked chest resolves registry-grounded (`minecraft:chest`) through
    /// Paper's `postLoadChunk` level branch and the raw tag is retained exactly,
    /// in source order, for the #341 materialization pass.
    #[test]
    fn real_26_2_chest_fixture_resolves_unpacked_block_entity() {
        let fixture = loaded_world_fixture("-19.-21.nbt");
        assert_eq!(fixture.get_int_or("DataVersion", -1), 4903);

        let parsed = SerializableChunkData::parse(height_accessor::create(-64, 384), &fixture)
            .unwrap()
            .expect("loaded-world fixture has a Status");
        assert_eq!(parsed.stored_pos(), ChunkPos::new(-19, -21));
        assert_eq!(parsed.block_entities().len(), 1);
        assert_eq!(
            parsed.block_entities()[0]
                .get_string("id")
                .map(String::as_str),
            Some("minecraft:chest")
        );
        // `keepPacked` is byte 0 on the fixture, so the level branch resolves
        // the unpacked entity type through the built-in registry.
        let outcomes = reconstruct_block_entities(
            &ChunkPos::new(-19, -21),
            parsed.block_entities(),
            BlockEntityChunkKind::Level,
        );
        assert_eq!(outcomes.len(), 1);
        let SerializedBlockEntityOutcome::ResolvedUnpacked(entry) = &outcomes[0] else {
            panic!("fixture chest was not resolved: {:?}", outcomes[0]);
        };
        assert_eq!(entry.source_index, 0);
        assert_eq!(entry.position, BlockPos::new(-299, -51, -321));
        assert_eq!(entry.entity_type.name(), "minecraft:chest");
        assert_eq!(&entry.raw_tag, &parsed.block_entities()[0]);
    }

    /// The public full-capability surface: a FULL chunk carrying decoded stored
    /// ticks is full-capable. The typed values decode and are carried as stored
    /// values (nothing schedules or executes them, #370); the tick presence no
    /// longer rejects the chunk.
    #[test]
    fn stored_tick_chunks_decode_typed_and_are_full_capable() {
        let height = height_accessor::create(-64, 384);

        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(block_tick(0, 0))])),
        );
        chunk.put(
            "fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(fluid_tick(0, 0))])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.stored_block_ticks().len(), 1);
        assert_eq!(
            parsed.stored_block_ticks()[0].r#type,
            Block::from_name("minecraft:stone").unwrap()
        );
        assert_eq!(parsed.stored_fluid_ticks().len(), 1);
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));

        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(fluid_tick(0, 0))])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.stored_block_ticks().is_empty());
        assert_eq!(parsed.stored_fluid_ticks().len(), 1);
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
    }

    /// Malformed tick entries are not swallowed into stored values: the
    /// `ListCodec` drops each failing element (Paper's `read` observes the
    /// error) and keeps successful siblings.
    #[test]
    fn malformed_tick_entries_are_dropped_and_survivors_carried() {
        let height = height_accessor::create(-64, 384);

        // A malformed element alongside a valid one: the valid tick survives
        // and is carried; the FULL chunk reconstructs (nothing schedules the
        // carried tick, #370).
        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Compound(CompoundTag::new()),
                Tag::Compound(saved_tick_with_id("not valid", 0, 0)),
                Tag::Compound(block_tick(0, 0)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.stored_block_ticks().len(), 1);
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        // The two failing elements (empty compound + malformed id) are
        // surfaced as a decode diagnostic even though a sibling survived.
        assert_eq!(parsed.diagnostics().len(), 1);
        assert!(matches!(
            parsed.diagnostics()[0],
            ChunkParseDiagnostic::StoredTicksDecodeFailed {
                field: "block_ticks",
                ref error,
            } if !error.is_empty()
        ));

        // An all-malformed list decodes to an empty partial (Paper drops the
        // elements after logging), so the chunk carries no stored ticks and is
        // full-capable — but the failure is not silent: a diagnostic records it.
        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![
                Tag::Compound(CompoundTag::new()),
                Tag::Compound(saved_tick_with_id("not valid", 0, 0)),
            ])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.stored_block_ticks().is_empty());
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.diagnostics().len(), 1);
        assert!(matches!(
            parsed.diagnostics()[0],
            ChunkParseDiagnostic::StoredTicksDecodeFailed {
                field: "block_ticks",
                ref error,
            } if !error.is_empty()
        ));
    }

    /// Proto paths never claim tick support: the typed decode is skipped for
    /// statuses that cannot be FULL-capable, so the stored lists stay empty and
    /// the capability boundary reports the status, never ticks.
    #[test]
    fn proto_paths_do_not_decode_or_claim_tick_support() {
        let height = height_accessor::create(-64, 384);
        for status in ["minecraft:empty", "minecraft:noise", "minecraft:light"] {
            let mut chunk = top_level(status);
            chunk.put(
                "block_ticks".into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(block_tick(0, 0))])),
            );
            chunk.put(
                "fluid_ticks".into(),
                Tag::List(ListTag::with_list(vec![Tag::Compound(fluid_tick(0, 0))])),
            );
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert!(parsed.stored_block_ticks().is_empty(), "{status}");
            assert!(parsed.stored_fluid_ticks().is_empty(), "{status}");
            assert_eq!(
                parsed.validate_full_for_reconstruction(),
                Err(SerializableChunkDataError::UnsupportedChunkStatus {
                    status: ChunkStatus::from_identifier(status).unwrap()
                })
            );
        }
    }

    #[test]
    fn upgrade_tick_codecs_apply_block_and_fluid_fallbacks_per_element() {
        let height = height_accessor::create(-64, 384);
        for (field, tick) in [
            (
                "neighbor_block_ticks",
                saved_tick_with_id("minecraft:not_a_block", 0, 0),
            ),
            ("neighbor_fluid_ticks", block_tick(0, 0)),
        ] {
            let mut upgrade = CompoundTag::new();
            upgrade.put(
                field.into(),
                Tag::List(ListTag::with_list(vec![
                    Tag::Compound(CompoundTag::new()),
                    Tag::Compound(tick),
                ])),
            );
            let mut chunk = top_level("minecraft:full");
            chunk.put("UpgradeData".into(), Tag::Compound(upgrade));
            assert_eq!(
                SerializableChunkData::parse(height, &chunk)
                    .unwrap()
                    .unwrap()
                    .validate_full_for_reconstruction(),
                Err(SerializableChunkDataError::UnsupportedUpgradeData { field })
            );
        }

        let mut missing_position = CompoundTag::new();
        missing_position.put_string("i", "minecraft:not_a_fluid");
        missing_position.put_int("y", 0);
        missing_position.put_int("z", 0);
        missing_position.put_int("t", 1);
        missing_position.put_int("p", 0);
        let mut upgrade = CompoundTag::new();
        upgrade.put(
            "neighbor_fluid_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(missing_position)])),
        );
        let mut chunk = top_level("minecraft:full");
        chunk.put("UpgradeData".into(), Tag::Compound(upgrade));
        assert_eq!(
            SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap()
                .validate_full_for_reconstruction(),
            Ok(())
        );
    }

    #[test]
    fn blending_heights_match_lenient_optional_field_codec() {
        let height = height_accessor::create(-64, 384);
        for heights in [
            Tag::String(rivet_nbt::string_tag::StringTag::value_of(
                "wrong type".into(),
            )),
            Tag::List(ListTag::with_list(vec![Tag::String(
                rivet_nbt::string_tag::StringTag::value_of("invalid".into()),
            )])),
            Tag::List(ListTag::with_list(vec![
                Tag::Int(IntTag::value_of(1)),
                Tag::String(rivet_nbt::string_tag::StringTag::value_of("invalid".into())),
            ])),
        ] {
            let mut blending = CompoundTag::new();
            blending.put_int("min_section", -4);
            blending.put_int("max_section", 19);
            blending.put("heights".into(), heights.clone());
            let mut chunk = top_level("minecraft:full");
            chunk.put("blending_data".into(), Tag::Compound(blending));
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(
                parsed.raw_blending_data().unwrap().get("heights"),
                Some(&heights)
            );
            assert!(parsed.effective_blending_data);
            assert_eq!(
                parsed.validate_full_for_reconstruction(),
                Err(SerializableChunkDataError::UnsupportedBlendingData)
            );
        }

        let mut wrong_length = CompoundTag::new();
        wrong_length.put_int("min_section", -4);
        wrong_length.put_int("max_section", 19);
        wrong_length.put(
            "heights".into(),
            Tag::List(ListTag::with_list(vec![Tag::Int(IntTag::value_of(1))])),
        );
        let mut chunk = top_level("minecraft:full");
        chunk.put("blending_data".into(), Tag::Compound(wrong_length));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.raw_blending_data().is_some());
        assert!(!parsed.effective_blending_data);
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));

        let mut absent = CompoundTag::new();
        absent.put_int("min_section", -4);
        absent.put_int("max_section", 19);
        let mut chunk = top_level("minecraft:full");
        chunk.put("blending_data".into(), Tag::Compound(absent));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.raw_blending_data().is_some());
        assert!(parsed.effective_blending_data);
        assert_eq!(
            parsed.validate_full_for_reconstruction(),
            Err(SerializableChunkDataError::UnsupportedBlendingData)
        );

        let mut blending = CompoundTag::new();
        blending.put_int("min_section", -4);
        blending.put_int("max_section", 19);
        blending.put(
            "heights".into(),
            Tag::List(ListTag::with_list(
                (0..16)
                    .map(|height| Tag::Int(IntTag::value_of(height)))
                    .collect(),
            )),
        );
        let mut chunk = top_level("minecraft:full");
        chunk.put("blending_data".into(), Tag::Compound(blending));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.effective_blending_data);
        assert_eq!(
            parsed.validate_full_for_reconstruction(),
            Err(SerializableChunkDataError::UnsupportedBlendingData)
        );
    }

    #[test]
    fn leading_colon_statuses_decode_across_full_and_partial_data() {
        let height = height_accessor::create(-64, 384);

        let full = SerializableChunkData::parse(height, &top_level(":full"))
            .unwrap()
            .unwrap();
        assert_eq!(full.status(), ChunkStatus::Full);
        assert!(full.diagnostics().is_empty());
        assert_eq!(full.validate_full_for_reconstruction(), Ok(()));

        let partial = SerializableChunkData::parse(height, &top_level(":noise"))
            .unwrap()
            .unwrap();
        assert_eq!(partial.status(), ChunkStatus::Noise);
        assert!(partial.diagnostics().is_empty());
        assert_eq!(
            partial.validate_full_for_reconstruction(),
            Err(SerializableChunkDataError::UnsupportedChunkStatus {
                status: ChunkStatus::Noise,
            })
        );

        let mut retrogen = CompoundTag::new();
        retrogen.put_string("target_status", ":noise");
        let mut chunk = top_level(":full");
        chunk.put("below_zero_retrogen".into(), Tag::Compound(retrogen));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.status(), ChunkStatus::Full);
        assert!(parsed.effective_below_zero_retrogen());
        assert!(parsed.raw_below_zero_retrogen().is_some());
    }

    #[test]
    fn structures_only_consider_non_empty_known_compounds() {
        let height = height_accessor::create(-64, 384);
        for structures in [
            {
                let mut structures = CompoundTag::new();
                structures.put_int("metadata", 1);
                structures
            },
            {
                let mut structures = CompoundTag::new();
                structures.put_int("starts", 1);
                structures.put_string("References", "wrong type");
                structures
            },
            // References-only structures (all refs in range of the stored pos)
            // are fully supported: they decode and are carried, so they no
            // longer reject the FULL chunk.
            {
                let mut references = CompoundTag::new();
                references.put_long_array("minecraft:village", vec![0]);
                let mut structures = CompoundTag::new();
                structures.put("References".into(), Tag::Compound(references));
                structures
            },
            // A `starts` compound present but empty is not "non-empty": the
            // guard is `!starts.is_empty()`, so an empty starts compound (like
            // an absent one) must not reject the FULL chunk. Distinct from the
            // wrong-typed `starts` int above, which `get_compound` treats as
            // absent.
            {
                let mut structures = CompoundTag::new();
                structures.put("starts".into(), Tag::Compound(CompoundTag::new()));
                structures
            },
        ] {
            let mut chunk = top_level("minecraft:full");
            chunk.put("structures".into(), Tag::Compound(structures));
            let parsed = SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap();
            assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        }

        // A non-empty `starts` compound is the one structures surface still
        // unsupported: structure starts (StructureStart) are not ported, so the
        // FULL boundary stays.
        let mut starts = CompoundTag::new();
        starts.put_string("minecraft:village", "pending");
        let mut structures = CompoundTag::new();
        structures.put("starts".into(), Tag::Compound(starts));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));
        assert_eq!(
            SerializableChunkData::parse(height, &chunk)
                .unwrap()
                .unwrap()
                .validate_full_for_reconstruction(),
            Err(SerializableChunkDataError::UnsupportedStructures)
        );
    }

    /// A malformed `structures.References` key is dropped with a typed
    /// diagnostic (Paper's `Identifier.tryParse` returns null for it) — never
    /// silently ignored, and the chunk still carries any valid siblings.
    #[test]
    fn malformed_structure_reference_key_is_dropped_with_diagnostic() {
        let height = height_accessor::create(-64, 384);
        let mut references = CompoundTag::new();
        references.put_long_array("not a valid : key", vec![0]);
        references.put_long_array("minecraft:valid", vec![ChunkPos::new(0, 0).pack()]);
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert_eq!(parsed.structures_references().len(), 1);
        assert_eq!(parsed.structures_references()[0].identifier.path(), "valid");
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            ChunkParseDiagnostic::StructureReferenceMalformed { key, reason }
                if key == "not a valid : key"
                    && !reason.is_empty()
        )));
    }

    /// A wrong-type `structures.References` payload is dropped with a typed
    /// diagnostic (Paper's `entry.asLongArray()` skips it), so a chunk whose
    /// only reference entry is malformed carries nothing but surfaces the drop.
    #[test]
    fn wrong_type_structure_reference_payload_is_dropped_with_diagnostic() {
        let height = height_accessor::create(-64, 384);
        let mut references = CompoundTag::new();
        references.put_string("minecraft:village", "not an array");
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.structures_references().is_empty());
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            ChunkParseDiagnostic::StructureReferenceMalformed { key, reason }
                if key == "minecraft:village" && !reason.is_empty()
        )));
    }

    /// A wrong-typed `structures.References` container drops every entry with a
    /// typed diagnostic on a FULL chunk. Paper tolerates the wrong type
    /// silently (`getCompoundOrEmpty` returns an empty compound); the port
    /// surfaces the drop per the never-silent requirement, exactly like the
    /// per-key wrong-type payload above. An absent `References` tag remains a
    /// silent no-op (that is the normal, empty case).
    #[test]
    fn wrong_type_structure_references_container_is_dropped_with_diagnostic() {
        let height = height_accessor::create(-64, 384);
        let mut structures = CompoundTag::new();
        structures.put_string("References", "not a compound");
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.structures_references().is_empty());
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            ChunkParseDiagnostic::StructureReferenceMalformed { key, reason }
                if key == "References" && !reason.is_empty()
        )));

        let no_references = top_level("minecraft:full");
        let parsed = SerializableChunkData::parse(height, &no_references)
            .unwrap()
            .unwrap();
        assert!(parsed.structures_references().is_empty());
        assert!(!parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic,
            ChunkParseDiagnostic::StructureReferenceMalformed { .. }
        )));
    }

    /// A non-FULL (proto) chunk never decodes `structures.References` in the
    /// port: the runtime reconstruction this feeds accepts only FULL chunks, so
    /// decoding on a proto chunk would be dead work the FULL gate already
    /// rejects. This is the same boundary the tick decode uses — not Paper's
    /// exact call site, since Paper's `read` invokes
    /// `setAllReferences(unpackStructureReferences(...))` unconditionally for
    /// every chunk type (see [`parse_structure_references`]).
    #[test]
    fn proto_chunk_ignores_malformed_structure_references() {
        let height = height_accessor::create(-64, 384);
        let mut references = CompoundTag::new();
        references.put_string("minecraft:village", "not an array");
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:noise");
        chunk.put("structures".into(), Tag::Compound(structures));

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.structures_references().is_empty());
        assert!(parsed.diagnostics().is_empty());
    }

    /// An out-of-range `structures.References` chunk-long (>8 chessboard
    /// distance) is dropped by the reconstruct-time filter with a typed
    /// diagnostic — Paper's `"Found invalid structure reference"` log — and the
    /// surviving filtered entry is carried.
    #[test]
    fn out_of_range_structure_reference_is_filtered_with_diagnostic() {
        let height = height_accessor::create(-64, 384);
        let in_range = ChunkPos::new(0, 0).pack();
        // Chunk (30, 0) is 30 away from (0, 0) > 8.
        let out_of_range = ChunkPos::new(30, 0).pack();
        let mut references = CompoundTag::new();
        references.put_long_array("minecraft:village", vec![out_of_range, in_range]);
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();

        let (kept, diagnostics) =
            filter_structure_references(parsed.structures_references(), &ChunkPos::new(0, 0));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].references, vec![in_range]);
        assert_eq!(diagnostics.len(), 1);
        assert!(matches!(
            &diagnostics[0],
            ChunkParseDiagnostic::StructureReferenceOutOfRange {
                identifier,
                chunk,
                chunk_pos,
            } if identifier.path() == "village"
                && *chunk == ChunkPos::new(0, 0)
                && *chunk_pos == ChunkPos::new(30, 0)
        ));
    }

    /// A structure key whose every reference is out of range is still preserved
    /// in the filtered map with an empty reference set, mirroring Paper's
    /// `outmap.put(structureType, new LongOpenHashSet(filtered...))` — the key
    /// survives with an empty `LongSet`. That holds even for a key whose wire
    /// `long[]` was already empty: Paper's guard is `!longArray.isEmpty()` on
    /// the `Optional<long[]>` from `LongArrayTag.asLongArray()`, which is
    /// always present for a `LongArrayTag` regardless of array length, so the
    /// key still enters the map with an empty set. The out-of-range discard is
    /// still surfaced as a diagnostic.
    #[test]
    fn filtered_out_structure_key_is_preserved_with_empty_reference_set() {
        let height = height_accessor::create(-64, 384);
        // Two out-of-range refs, none in range: the key survives, empty.
        let out = ChunkPos::new(30, 0).pack();
        let mut references = CompoundTag::new();
        references.put_long_array("minecraft:village", vec![out, out]);
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();

        let (kept, diagnostics) =
            filter_structure_references(parsed.structures_references(), &ChunkPos::new(0, 0));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].identifier.path(), "village");
        assert!(kept[0].references.is_empty());
        // Two refs dropped -> two diagnostics, never silent.
        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics.iter().all(|diagnostic| matches!(
            diagnostic,
            ChunkParseDiagnostic::StructureReferenceOutOfRange { .. }
        )));

        // An already-empty wire `long[]` still enters the map with an empty
        // set: Paper's guard is `!longArray.isEmpty()` on the `Optional` from
        // `LongArrayTag.asLongArray()`, which is always present regardless of
        // array length, so the key is put and installed with no reference set
        // and no diagnostic.
        let mut references = CompoundTag::new();
        references.put_long_array("minecraft:village", Vec::<i64>::new());
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        let (kept, diagnostics) =
            filter_structure_references(parsed.structures_references(), &ChunkPos::new(0, 0));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].identifier.path(), "village");
        assert!(kept[0].references.is_empty());
        assert!(diagnostics.is_empty());
    }

    /// A hostile `References` `long[]` of in-range duplicates deduplicates in
    /// O(n): a huge, nearly-all-duplicate array must collapse to the distinct
    /// entries while preserving first-insertion order. This is a regression
    /// guard against a return to the linear `Vec::contains` dedup — the wire
    /// carries every in-range position Paper's chessboard filter can produce
    /// (289) followed by millions of repeats of the last one, so a
    /// `Vec::contains` revert rescans the entire surviving set for every
    /// repeat (~6e8 comparisons) while the `HashSet` dedup stays O(n). The
    /// dedup itself is silent (Paper's `LongOpenHashSet`); the size of the
    /// surviving set is asserted directly, so the test fails fast if the dedup
    /// is wrong without relying on wall-clock timing.
    #[test]
    fn hostile_duplicate_references_dedup_in_linear_time_and_preserve_order() {
        let height = height_accessor::create(-64, 384);
        // Every position within chessboard distance 8 of (0,0) — the full
        // in-range set Paper's filter can produce — so the surviving-set scan a
        // `Vec::contains` revert would do per repeat is genuinely large (289
        // distinct, not a handful).
        let mut distinct: Vec<i64> = Vec::new();
        for z in -8..=8 {
            for x in -8..=8 {
                distinct.push(ChunkPos::new(x, z).pack());
            }
        }
        assert_eq!(distinct.len(), 289);
        const REPEATS: usize = 2_000_000;
        let mut wire = Vec::with_capacity(distinct.len() + REPEATS);
        wire.extend_from_slice(&distinct);
        // Repeat the LAST distinct position so every duplicate would scan the
        // whole surviving set under `Vec::contains` (the worst case): the dedup
        // must collapse the wire to the distinct entries in first-insertion
        // order (Paper's `LongOpenHashSet` keeps first-insertion order).
        wire.resize(wire.len() + REPEATS, *distinct.last().unwrap());
        let mut references = CompoundTag::new();
        references.put_long_array("minecraft:village", wire);
        let mut structures = CompoundTag::new();
        structures.put("References".into(), Tag::Compound(references));
        let mut chunk = top_level("minecraft:full");
        chunk.put("structures".into(), Tag::Compound(structures));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();

        let (kept, diagnostics) =
            filter_structure_references(parsed.structures_references(), &ChunkPos::new(0, 0));
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].references, distinct,
            "the dedup must collapse to the distinct in-range references in first-insertion order"
        );
        assert!(
            diagnostics.is_empty(),
            "every wire reference is in range; the silent dedup must not emit diagnostics"
        );
    }

    /// A stored-tick entry whose id does not resolve through the block registry
    /// is dropped by the codec's partial path and surfaced as a
    /// [`StoredTicksDecodeFailed`](ChunkParseDiagnostic::StoredTicksDecodeFailed)
    /// diagnostic — the chunk stays parseable, never silently empty.
    #[test]
    fn unknown_block_id_tick_is_dropped_with_diagnostic() {
        let height = height_accessor::create(-64, 384);
        let mut chunk = top_level("minecraft:full");
        chunk.put(
            "block_ticks".into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(saved_tick_with_id(
                "minecraft:no_such_block",
                0,
                0,
            ))])),
        );
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.stored_block_ticks().is_empty());
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
        assert_eq!(parsed.diagnostics().len(), 1);
        assert!(matches!(
            parsed.diagnostics()[0],
            ChunkParseDiagnostic::StoredTicksDecodeFailed {
                field: "block_ticks",
                ..
            }
        ));
    }

    #[test]
    fn full_construction_retains_but_ignores_below_zero_retrogen() {
        let height = height_accessor::create(-64, 384);
        let mut retrogen = CompoundTag::new();
        retrogen.put_string("target_status", "minecraft:noise");
        let mut chunk = top_level("minecraft:full");
        chunk.put("below_zero_retrogen".into(), Tag::Compound(retrogen));
        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(parsed.raw_below_zero_retrogen().is_some());
        assert!(parsed.effective_below_zero_retrogen());
        assert_eq!(parsed.validate_full_for_reconstruction(), Ok(()));
    }

    #[test]
    fn top_level_parse_defers_mixed_palette_and_light_failures() {
        let height = height_accessor::create(-64, 384);
        let mut section = section_tag(-4);
        section.put_int("block_states", 7);
        section.put_byte_array(BLOCK_LIGHT_TAG, vec![0; 3]);
        let mut chunk = top_level("minecraft:full");
        chunk.put(
            SECTIONS_TAG.into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(section)])),
        );

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        let raw = parsed.section_tags().get_compound(0).unwrap();
        assert_eq!(raw.get_int_or("block_states", 0), 7);
        assert_eq!(raw.get_byte_array(BLOCK_LIGHT_TAG).unwrap().len(), 3);
    }

    #[test]
    fn real_full_and_partial_fixture_metadata_is_pinned() {
        let full = [
            (
                "overworld",
                height_accessor::create(-64, 384),
                25usize,
                24usize,
            ),
            ("the_nether", height_accessor::create(0, 256), 17, 16),
            ("the_end", height_accessor::create(0, 256), 17, 16),
        ];
        for (dimension, height, section_tags, post_sections) in full {
            let root = named_fixture(dimension, "0.0", "0.0.nbt");
            assert_eq!(root.get_int_or(DATA_VERSION_TAG, -1), CURRENT_DATA_VERSION);
            let parsed = SerializableChunkData::parse(height, &root)
                .unwrap()
                .unwrap();
            assert_eq!(parsed.stored_pos(), ChunkPos::new(0, 0), "{dimension}");
            assert_eq!(parsed.status(), ChunkStatus::Full, "{dimension}");
            assert_eq!(
                parsed.section_tags().list.len(),
                section_tags,
                "{dimension}"
            );
            assert_eq!(
                parsed.post_processing_sections().len(),
                post_sections,
                "{dimension}"
            );
            assert_eq!(
                parsed
                    .status()
                    .heightmaps_after()
                    .iter()
                    .filter(|ty| parsed.heightmaps()[**ty as usize].is_some())
                    .count(),
                4,
                "{dimension}"
            );
            assert!(parsed.entities().is_empty(), "{dimension}");
            assert!(parsed.block_entities().is_empty(), "{dimension}");
        }

        let partial_root = named_fixture("overworld", "0.0", "0.1.nbt");
        let partial =
            SerializableChunkData::parse(height_accessor::create(-64, 384), &partial_root)
                .unwrap()
                .unwrap();
        assert_eq!(partial.status(), ChunkStatus::InitializeLight);
        assert_eq!(partial.section_tags().list.len(), 24);
        assert_eq!(
            partial.validate_full_for_reconstruction(),
            Err(SerializableChunkDataError::UnsupportedChunkStatus {
                status: ChunkStatus::InitializeLight,
            })
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

    /// The #371 loaded-world spawn fixture `-1.-3.nbt` is a vanilla-format
    /// save: `isLightOn` present but no `starlight.light_version`, plain
    /// `SkyLight`/`BlockLight` arrays, no per-section state INTs. Paper would
    /// drop these and relight; Rivet has no lighting engine (#184), so
    /// `reconstruct_lights` installs each present array as an `Initialised`
    /// nibble at its exact light-section index (issue #531) — the payload the
    /// vanilla `new DataLayer(byte[])` send would carry.
    #[test]
    fn loaded_world_vanilla_sky_arrays_install_at_exact_section_indices() {
        let chunk = loaded_world_fixture("-1.-3.nbt");
        assert!(
            !parse_light_correct(&chunk, true),
            "a vanilla-format save is not Starlight-lit"
        );
        let sections = parse_section_lights(&chunk);
        let rebuilt = reconstruct_lights(height_accessor::create(-64, 384), &sections, false, true);
        // Paper marks a vanilla-format save unlit; the plain arrays are still
        // carried (Rivet cannot relight them).
        assert!(!rebuilt.light_correct);

        // minLightSection = -64/16 - 1 = -5; sky Y=4 -> index 9, Y=5 -> index 10.
        let sky4 = sections
            .iter()
            .find(|s| s.y == 4)
            .expect("stored sky at Y=4")
            .sky_light
            .clone()
            .expect("plain sky array");
        let sky5 = sections
            .iter()
            .find(|s| s.y == 5)
            .expect("stored sky at Y=5")
            .sky_light
            .clone()
            .expect("plain sky array");
        assert_eq!(
            rebuilt.sky_nibbles[9]
                .to_vanilla_nibble()
                .unwrap()
                .get_data(),
            sky4
        );
        assert_eq!(
            rebuilt.sky_nibbles[10]
                .to_vanilla_nibble()
                .unwrap()
                .get_data(),
            sky5
        );
        // No light above/below the stored sections, and no block light at all.
        assert!(rebuilt.sky_nibbles[8].to_vanilla_nibble().is_none());
        assert!(rebuilt.sky_nibbles[11].to_vanilla_nibble().is_none());
        assert!(
            rebuilt
                .block_nibbles
                .iter()
                .all(|nibble| nibble.to_vanilla_nibble().is_none())
        );

        // The #184 send seam folds the nibbles into the packet payload: the two
        // sky updates set the update mask at bits 9 and 10 (0x600), in ascending
        // section order; nothing sets the empty masks or block masks.
        let sky_layers: Vec<Option<DataLayer>> = rebuilt
            .sky_nibbles
            .iter()
            .map(|nibble| nibble.to_vanilla_nibble())
            .collect();
        let block_layers: Vec<Option<DataLayer>> = rebuilt
            .block_nibbles
            .iter()
            .map(|nibble| nibble.to_vanilla_nibble())
            .collect();
        let payload = build_light_update_data(&sky_layers, &block_layers);
        assert_eq!(payload.sky_y_mask(), &[0x600]);
        assert!(payload.block_y_mask().is_empty());
        assert!(payload.empty_sky_y_mask().is_empty());
        assert!(payload.empty_block_y_mask().is_empty());
        assert_eq!(payload.sky_updates(), &[sky4, sky5]);
        assert!(payload.block_updates().is_empty());
    }

    /// The #371 loaded-world fixture `-2.-2.nbt` carries both plain `BlockLight`
    /// (Y=-4..=-1) and plain `SkyLight` (Y=3..=5). Both install at the exact
    /// light-section indices, producing a block update mask at bits 1..=4 and a
    /// sky update mask at bits 8..=10 — wrong masks, array order, or section
    /// offsets would fail these assertions.
    #[test]
    fn loaded_world_vanilla_block_and_sky_arrays_mask_offsets() {
        let chunk = loaded_world_fixture("-2.-2.nbt");
        assert!(!parse_light_correct(&chunk, true));
        let sections = parse_section_lights(&chunk);
        let rebuilt = reconstruct_lights(height_accessor::create(-64, 384), &sections, false, true);
        assert!(!rebuilt.light_correct);

        // minLightSection = -5: block Y=-4..=-1 -> indices 1..=4.
        let block = |y: i32| {
            sections
                .iter()
                .find(|s| s.y == y)
                .unwrap_or_else(|| panic!("stored block at Y={y}"))
                .block_light
                .clone()
                .expect("plain block array")
        };
        let expected_block = (1..=4)
            .map(|index| {
                rebuilt.block_nibbles[index]
                    .to_vanilla_nibble()
                    .unwrap()
                    .get_data()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            expected_block,
            vec![block(-4), block(-3), block(-2), block(-1)]
        );
        assert!(rebuilt.block_nibbles[0].to_vanilla_nibble().is_none());
        assert!(rebuilt.block_nibbles[5].to_vanilla_nibble().is_none());

        // sky Y=3..=5 -> indices 8..=10.
        let sky = |y: i32| {
            sections
                .iter()
                .find(|s| s.y == y)
                .unwrap_or_else(|| panic!("stored sky at Y={y}"))
                .sky_light
                .clone()
                .expect("plain sky array")
        };
        let expected_sky = (8..=10)
            .map(|index| {
                rebuilt.sky_nibbles[index]
                    .to_vanilla_nibble()
                    .unwrap()
                    .get_data()
            })
            .collect::<Vec<_>>();
        assert_eq!(expected_sky, vec![sky(3), sky(4), sky(5)]);

        let sky_layers: Vec<Option<DataLayer>> = rebuilt
            .sky_nibbles
            .iter()
            .map(|nibble| nibble.to_vanilla_nibble())
            .collect();
        let block_layers: Vec<Option<DataLayer>> = rebuilt
            .block_nibbles
            .iter()
            .map(|nibble| nibble.to_vanilla_nibble())
            .collect();
        let payload = build_light_update_data(&sky_layers, &block_layers);
        assert_eq!(payload.block_y_mask(), &[0b11110]);
        assert_eq!(payload.sky_y_mask(), &[0b111 << 8]);
        assert_eq!(
            payload.block_updates(),
            &[block(-4), block(-3), block(-2), block(-1)]
        );
        assert_eq!(payload.sky_updates(), &[sky(3), sky(4), sky(5)]);
    }

    /// Paper's vanilla `canReadSky` gate (`dimensionType().hasSkyLight()`)
    /// applies to the vanilla-format path too: a plain `SkyLight` array in a
    /// sky-less dimension is dropped, while a plain `BlockLight` array is
    /// retained.
    #[test]
    fn vanilla_format_plain_sky_arrays_respect_the_dimension_sky_gate() {
        let mut section = section_tag(-4);
        section.put_byte_array(BLOCK_LIGHT_TAG, vec![0x22; ARRAY_SIZE]);
        section.put_byte_array(SKY_LIGHT_TAG, vec![0x33; ARRAY_SIZE]);
        let sections = parse_section_lights(&chunk_with_sections(vec![section]));

        let no_sky = reconstruct_lights(height_accessor::create(-64, 384), &sections, false, false);
        assert!(!no_sky.light_correct);
        assert_eq!(
            no_sky.block_nibbles[1]
                .to_vanilla_nibble()
                .unwrap()
                .get_data(),
            vec![0x22; ARRAY_SIZE]
        );
        assert!(no_sky.sky_nibbles[1].to_vanilla_nibble().is_none());

        let with_sky =
            reconstruct_lights(height_accessor::create(-64, 384), &sections, false, true);
        assert_eq!(
            with_sky.sky_nibbles[1]
                .to_vanilla_nibble()
                .unwrap()
                .get_data(),
            vec![0x33; ARRAY_SIZE]
        );
    }

    /// A Starlight save that merely failed the light predicate (mismatched
    /// `starlight.light_version` / missing `isLightOn` / status below Light)
    /// still carries per-section `starlight.*light_state` INTs, so it is not
    /// vanilla-format. Paper's `lit &&` load loop never runs for it and its
    /// plain arrays stay Null — they must not be installed as authoritative
    /// vanilla updates. Both persisted hostile states — `Null` (0) and
    /// `Uninitialised` (1) — carry the INT, so neither counts as vanilla-format.
    #[test]
    fn failed_predicate_starlight_save_does_not_install_vanilla_bytes() {
        for state in [InitState::Null, InitState::Uninitialised] {
            let mut section = section_tag(-4);
            section.put_byte_array(BLOCK_LIGHT_TAG, vec![0x22; ARRAY_SIZE]);
            section.put_byte_array(SKY_LIGHT_TAG, vec![0x33; ARRAY_SIZE]);
            section.put_int(BLOCKLIGHT_STATE_TAG, state.to_i32());
            section.put_int(SKYLIGHT_STATE_TAG, state.to_i32());
            let sections = parse_section_lights(&chunk_with_sections(vec![section]));
            assert_eq!(sections[0].block_state, state.to_i32());

            let rebuilt =
                reconstruct_lights(height_accessor::create(-64, 384), &sections, false, true);
            assert!(!rebuilt.light_correct);
            assert!(
                rebuilt
                    .block_nibbles
                    .iter()
                    .all(|nibble| nibble.to_vanilla_nibble().is_none())
            );
            assert!(
                rebuilt
                    .sky_nibbles
                    .iter()
                    .all(|nibble| nibble.to_vanilla_nibble().is_none())
            );
        }
    }

    /// End-to-end hostile regression: a Starlight save whose
    /// `starlight.light_version` no longer matches (so `parse_light_correct`
    /// is false) still carries per-section `starlight.*light_state` INTs — here
    /// with the `Null` state (0) — alongside plain light arrays. Paper's `lit
    /// &&` load loop never runs, so those arrays must stay Null through the
    /// parse → reconstruct path; the vanilla-format fallback must not install
    /// them as authoritative updates just because `light_correct` failed.
    #[test]
    fn version_mismatched_starlight_save_with_null_states_installs_nothing() {
        let height = height_accessor::create(-64, 384);
        let mut chunk = top_level("minecraft:light");
        chunk.put_boolean(IS_LIGHT_ON_TAG, true);
        chunk.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION + 1);
        let mut section = section_tag(-4);
        section.put_byte_array(BLOCK_LIGHT_TAG, vec![0x22; ARRAY_SIZE]);
        section.put_byte_array(SKY_LIGHT_TAG, vec![0x33; ARRAY_SIZE]);
        section.put_int(BLOCKLIGHT_STATE_TAG, InitState::Null.to_i32());
        section.put_int(SKYLIGHT_STATE_TAG, InitState::Null.to_i32());
        chunk.put(
            SECTIONS_TAG.into(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(section)])),
        );

        let parsed = SerializableChunkData::parse(height, &chunk)
            .unwrap()
            .unwrap();
        assert!(!parsed.light_correct());

        let sections = parse_section_lights(&chunk);
        assert_eq!(sections[0].block_state, InitState::Null.to_i32());
        let rebuilt = reconstruct_lights(height, &sections, parsed.light_correct(), true);
        assert!(!rebuilt.light_correct);
        assert!(
            rebuilt
                .block_nibbles
                .iter()
                .all(|nibble| nibble.to_vanilla_nibble().is_none())
        );
        assert!(
            rebuilt
                .sky_nibbles
                .iter()
                .all(|nibble| nibble.to_vanilla_nibble().is_none())
        );
    }

    /// A vanilla-format section outside the world's light-section range fails
    /// the whole payload exactly like the Starlight path: Paper's caught load
    /// failure keeps all-null arrays and `light_correct` false.
    #[test]
    fn vanilla_format_out_of_range_section_invalidates_the_whole_payload() {
        let out_of_range = SectionLightData {
            y: 100,
            block_light: Some(vec![0x11; ARRAY_SIZE]),
            sky_light: None,
            block_state: -1,
            sky_state: -1,
        };
        let rebuilt = reconstruct_lights(
            height_accessor::create(-64, 384),
            &[out_of_range],
            false,
            true,
        );
        assert!(!rebuilt.light_correct);
        assert!(
            rebuilt
                .block_nibbles
                .iter()
                .all(|nibble| nibble.to_vanilla_nibble().is_none())
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
    fn write_heightmaps_emits_full_four_keys_in_ordinal_order() {
        // A FULL chunk's stored map (parse-filtered by FINAL_HEIGHTMAPS) emits
        // the four types in ordinal (declaration) order: WORLD_SURFACE,
        // OCEAN_FLOOR, MOTION_BLOCKING, MOTION_BLOCKING_NO_LEAVES. This is the
        // storage key order, NOT the fixture's fastutil hash order.
        let stored = {
            let mut maps = CompoundTag::new();
            maps.put_long_array("WORLD_SURFACE", vec![1; 37]);
            maps.put_long_array("OCEAN_FLOOR", vec![2; 37]);
            maps.put_long_array("MOTION_BLOCKING", vec![3; 37]);
            maps.put_long_array("MOTION_BLOCKING_NO_LEAVES", vec![4; 37]);
            let mut chunk = CompoundTag::new();
            chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(maps));
            parse_heightmaps(&chunk, &FINAL_HEIGHTMAPS)
        };
        let written = write_heightmaps(stored);
        let keys: Vec<&String> = written.key_set().collect();
        assert_eq!(
            keys,
            vec![
                &"WORLD_SURFACE".to_string(),
                &"OCEAN_FLOOR".to_string(),
                &"MOTION_BLOCKING".to_string(),
                &"MOTION_BLOCKING_NO_LEAVES".to_string(),
            ]
        );
        assert_eq!(written.get_long_array("WORLD_SURFACE"), Some(&vec![1; 37]));
        assert_eq!(written.get_long_array("OCEAN_FLOOR"), Some(&vec![2; 37]));
        assert_eq!(
            written.get_long_array("MOTION_BLOCKING"),
            Some(&vec![3; 37])
        );
        assert_eq!(
            written.get_long_array("MOTION_BLOCKING_NO_LEAVES"),
            Some(&vec![4; 37])
        );
    }

    #[test]
    fn write_heightmaps_omits_missing_entries() {
        // A chunk holding only WORLD_SURFACE writes only that key; the absent
        // FINAL types stay absent.
        let stored = {
            let mut maps = CompoundTag::new();
            maps.put_long_array("WORLD_SURFACE", vec![1; 37]);
            let mut chunk = CompoundTag::new();
            chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(maps));
            parse_heightmaps(&chunk, &FINAL_HEIGHTMAPS)
        };
        let written = write_heightmaps(stored);
        let keys: Vec<&String> = written.key_set().collect();
        assert_eq!(keys, vec![&"WORLD_SURFACE".to_string()]);
    }

    #[test]
    fn write_heightmaps_writes_raw_data_exactly() {
        // The stored column lands in the tag byte-for-byte (Java's `write`
        // passes the `copyOf`-cloned array reference into the `LongArrayTag` —
        // this move is that same single copy, no second clone and no
        // truncation).
        let stored = {
            let mut maps = CompoundTag::new();
            let raw: Vec<i64> = (0..37).map(|i| i64::from(i).wrapping_mul(7)).collect();
            maps.put_long_array("MOTION_BLOCKING_NO_LEAVES", raw);
            let mut chunk = CompoundTag::new();
            chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(maps));
            parse_heightmaps(&chunk, &FINAL_HEIGHTMAPS)
        };
        let written = write_heightmaps(stored);
        let expected: Vec<i64> = (0..37).map(|i| i64::from(i).wrapping_mul(7)).collect();
        assert_eq!(
            written.get_long_array("MOTION_BLOCKING_NO_LEAVES"),
            Some(&expected)
        );
    }

    #[test]
    fn write_heightmaps_emits_only_worldgen_status_columns() {
        // A pre-CARVERS status's `heightmapsAfter()` is `WORLDGEN_HEIGHTMAPS`
        // (WORLD_SURFACE_WG, OCEAN_FLOOR_WG). `parse_heightmaps` filters by
        // that slice (copyOf's job), so the stored map carries only the two WG
        // columns even though the tag also holds WORLD_SURFACE; `write` emits
        // exactly those.
        let mut maps = CompoundTag::new();
        maps.put_long_array("WORLD_SURFACE_WG", vec![9; 37]);
        maps.put_long_array("OCEAN_FLOOR_WG", vec![10; 37]);
        maps.put_long_array("WORLD_SURFACE", vec![1; 37]);
        let mut chunk = CompoundTag::new();
        chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(maps));
        let stored = parse_heightmaps(&chunk, &WORLDGEN_HEIGHTMAPS);
        assert!(stored[Types::WorldSurface as usize].is_none());
        let written = write_heightmaps(stored);
        let keys: Vec<&String> = written.key_set().collect();
        assert_eq!(
            keys,
            vec![
                &"WORLD_SURFACE_WG".to_string(),
                &"OCEAN_FLOOR_WG".to_string()
            ]
        );
        assert_eq!(
            written.get_long_array("WORLD_SURFACE_WG"),
            Some(&vec![9; 37])
        );
        assert_eq!(
            written.get_long_array("OCEAN_FLOOR_WG"),
            Some(&vec![10; 37])
        );
    }

    #[test]
    fn write_heightmaps_round_trips_real_fixture_values() {
        // Parse the pinned Paper 26.2 FULL chunk, then write it back and
        // re-parse: every column the fixture carries must survive exactly.
        // The key order the writer emits is ordinal order (not the fixture's
        // fastutil hash order), so this compares by value, never by key order.
        let chunk = fixture();
        let stored = parse_heightmaps(&chunk, &FINAL_HEIGHTMAPS);
        let written = write_heightmaps(stored.clone());
        // The writer's insertion order is the EnumMap ordinal order.
        assert_eq!(
            written.key_set().collect::<Vec<_>>(),
            vec![
                &"WORLD_SURFACE".to_string(),
                &"OCEAN_FLOOR".to_string(),
                &"MOTION_BLOCKING".to_string(),
                &"MOTION_BLOCKING_NO_LEAVES".to_string(),
            ]
        );
        let mut re_chunk = CompoundTag::new();
        re_chunk.put(HEIGHTMAPS_TAG.to_string(), Tag::Compound(written));
        let re_parsed = parse_heightmaps(&re_chunk, &FINAL_HEIGHTMAPS);
        for ty in FINAL_HEIGHTMAPS {
            assert_eq!(re_parsed[ty as usize], stored[ty as usize], "{:?}", ty);
            assert_eq!(re_parsed[ty as usize].as_ref().expect("stored").len(), 37);
        }
        assert!(re_parsed[Types::WorldSurfaceWg as usize].is_none());
        assert!(re_parsed[Types::OceanFloorWg as usize].is_none());
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
