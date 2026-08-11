//! M2L reconstruction bridge: validated Paper 26.2 `SerializableChunkData` →
//! an owned runtime `LevelChunk` (issue #383).
//!
//! This is the pure composition slice the #339 server-side `ChunkMap` seam will
//! call. It composes the existing per-surface decoders into one ordered,
//! Paper-faithful `LevelChunk` build:
//!
//! - `reconstruct_sections` (#336) decodes each section's palettes and light
//!   into `SectionReconstruction { sections, light_data, diagnostics }`, where
//!   missing in-bounds sections stay `None` for `replaceMissingSections`;
//! - `SerializableChunkData::parse` (#338) extracts the top-level surfaces
//!   (status, heightmaps, upgrade data, post-processing, block entities,
//!   structures, ticks) with the same per-field codecs Paper's `parse` uses;
//! - the `LevelChunk` constructor + `ChunkAccess` load seam (`#337`) installs
//!   sections, stored heightmaps, post-processing, and Starlight nibbles in
//!   Paper's `SerializableChunkData.read` LEVELCHUNK ordering.
//!
//! This module owns none of those files (they are active slices in other
//! worktrees/PRs); it composes them. It deliberately does not add generation,
//! fallback, writes, repair, chunk scheduling, or server boot composition —
//! the caller owns those. Live block entities stay an explicit #341 boundary.
//!
//! Ticks are decoded through the merged `SavedTick` value layer (#370/#381).
//! A FULL chunk carrying a decoded in-chunk `block_ticks`/`fluid_ticks` list
//! reconstructs and carries the typed stored ticks on the result (plus the raw
//! lists); nothing is installed into a runtime container, scheduled, or
//! executed — the `LevelChunkTicks`/`ProtoChunkTicks` execution containers
//! stay deferred with the tick-execution slice.
//!
//! ## Block entities on the FULL path
//!
//! `SerializableChunkData.read`'s `postLoadChunk` materializes unpacked block
//! entities and keeps `keepPacked` ones pending. The block-entity map is not
//! ported (#341), so this slice installs every serialized block entity into the
//! `ChunkAccess.pending_block_entities` authority (#537) instead of
//! materializing. That is Paper-faithful for the `keepPacked` branch and an
//! honest, typed boundary for the unpacked branch: the raw tags are retained
//! exactly, in source order, so a future #341 materialization pass can consume
//! them from the authority. Duplicate corrected positions collapse last-wins in
//! place (one entry per position), exactly like a map-backed runtime. The
//! registry-grounded type outcomes are derived from the authority when a
//! materialization/derivation pass needs them (#520), mirroring Paper's
//! `postLoadChunk` keepPacked/unpacked split.
//!
//! ## Structure references
//!
//! Rivet has no `Structure`/`StructureStart` type yet (#369), so the chunk is
//! keyed by the structure `Identifier` and only `structures.References` is
//! installed: the references decode into [`StructureReference`]s at parse,
//! are filtered by the >8-chunk chessboard rule against the requested position
//! at reconstruction (Paper's `unpackStructureReferences`), and are installed
//! into the chunk's `StructureAccess` map — the runtime authority (#537) — so
//! no duplicate carry field is retained. Non-empty `starts` still surfaces the
//! `UnsupportedStructures` typed boundary (the `StructureStart` load path is
//! not ported).

use crate::block::Block;
use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::storage::section_reconstruction::{
    BiomeId, ChunkReadException, SectionBlockPredicates, SectionReconstruction,
    current_version_container_factory, reconstruct_sections,
};
use crate::chunk::storage::serializable_chunk_data::{
    ChunkParseDiagnostic, SerializableChunkData, SerializableChunkDataError, StructureReference,
    filter_structure_references, reconstruct_heightmaps, reconstruct_lights,
};
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::StateFlags;
use crate::ticks::SavedTick;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_registry::Identifier;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::ChunkPos;
use rivet_registry::fluid_id::FluidId;

/// The canonical behavior predicates for the generated `BlockState` value.
///
/// These are the same five `LevelChunkSection` predicates `section_tag_decode.rs`
/// feeds `reconstruct_sections` for the real-fixture goldens: air/random-tick/
/// fluid flags from the generated behavior table, lava as Paper's only
/// randomly-ticking vanilla fluid, and no large-collision or moving-piston
/// states in the committed fixtures.
pub fn block_state_predicates() -> SectionBlockPredicates {
    SectionBlockPredicates {
        is_air: |state| state.is_air(),
        is_randomly_ticking: |state| state.random_ticking(),
        fluid_is_empty: |state| state.fluid_empty(),
        fluid_is_randomly_ticking: |state| {
            !state.fluid_empty() && state.block().name() == "minecraft:lava"
        },
        is_special_colliding: |_| false,
    }
}

/// Extract the human-readable message from a panic payload, for surfacing a
/// caught section-decode panic as a typed [`ChunkReconstructionError`].
fn panic_payload_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown section-decode panic".to_string()
    }
}

/// The `StateFlags` resolver the `LevelChunk`/`ChunkAccess` constructors store
/// for the heightmap walks (`isOpaque` predicates).
///
/// Air from the generated behavior table; `blocks_motion`/`has_fluid`/
/// `is_leaves` from `BlockState`'s tag-aware queries. Leaves read the
/// `minecraft:leaves` block tag, matching Paper's `state.is(BlockTags.LEAVES)`.
pub fn resolve_state_flags(state: &BlockState) -> StateFlags {
    StateFlags {
        is_air: state.is_air(),
        blocks_motion: state.blocks_motion(),
        has_fluid: !state.fluid_empty(),
        is_leaves: state.is_in_tag("minecraft:leaves"),
    }
}

/// A FULL-status chunk reconstructed into an owned runtime `LevelChunk`.
///
/// The structure key `S` is the structure `Identifier` (the registry key the
/// `structures.References` map is keyed by, #369). Rivet has no `Structure`
/// value type yet, so the chunk holds the reference map keyed by identifier and
/// `starts` remain an `UnsupportedStructures` boundary rather than fabricating
/// starts.
pub type ReconstructedLevelChunk = LevelChunk<BlockState, BiomeId, Identifier>;

/// The products of the reconstruction, mirroring `SerializableChunkData.read`'s
/// LEVELCHUNK branch plus the retained deferred surfaces.
pub struct ChunkReconstruction {
    /// The owned runtime chunk with sections, heightmaps, light, and
    /// post-processing installed.
    pub chunk: ReconstructedLevelChunk,
    /// The recoverable section-palette diagnostics (#336) promoted during the
    /// section loop, for the caller's `logErrors` equivalent.
    pub section_diagnostics:
        Vec<crate::chunk::storage::section_reconstruction::SectionCodecDiagnostic>,
    /// The recoverable parse-time diagnostics, plus the relocated-position
    /// diagnostic, mirroring Paper's `SerializableChunkData.read` (the
    /// `reportMisplacedChunk` report — the chunk is relocated, never rejected).
    pub parse_diagnostics: Vec<ChunkParseDiagnostic>,
    /// The raw `block_ticks` list as it appeared on the wire, preserved for the
    /// future tick installer to consume without rework.
    pub raw_block_ticks: ListTag,
    /// The raw `fluid_ticks` list as it appeared on the wire. See
    /// [`Self::raw_block_ticks`].
    pub raw_fluid_ticks: ListTag,
    /// The typed, per-chunk-filtered stored block ticks (`ChunkAccess.PackedTicks
    /// .blocks()`), faithfully decoded through `SavedTick.codec(...).listOf()`.
    /// Carried on the result — nothing schedules, executes, installs, or writes
    /// them (#370 defers the `LevelChunkTicks`/`ProtoChunkTicks` containers).
    pub stored_block_ticks: Vec<SavedTick<Block>>,
    /// The typed, per-chunk-filtered stored fluid ticks (`ChunkAccess.PackedTicks
    /// .fluids()`). Same carry semantics as [`Self::stored_block_ticks`].
    pub stored_fluid_ticks: Vec<SavedTick<FluidId>>,
}

/// Why a chunk is not reconstructable into an owned runtime `LevelChunk`.
#[derive(Debug, thiserror::Error)]
pub enum ChunkReconstructionError {
    #[error(transparent)]
    Serializable(#[from] SerializableChunkDataError),
    /// The per-section paletted-container codec failure (#336).
    #[error("section {0}")]
    Section(#[from] ChunkReadException),
    /// A section-decode panic, caught at this boundary and surfaced as a typed
    /// error. The one known source is `decode_section_light` panicking on a
    /// malformed light array (length != 2048), which faithfully mirrors Paper's
    /// unchecked `IllegalArgumentException` from `new DataLayer(byte[])` — but
    /// the public reconstruction API surfaces it as an error, not a crash.
    #[error("section decode panic: {0}")]
    SectionPanic(String),
}

/// Reconstruct one validated FULL chunk into an owned runtime `LevelChunk`.
///
/// `requested_pos` is the position the runtime chunk takes; a mismatch with the
/// stored position surfaces Paper's `reportMisplacedChunk` diagnostic on the
/// returned value (retained in `ChunkReconstruction::parse_diagnostics`), never
/// an error. `has_sky_light` is the dimension's `dimensionType().hasSkyLight()`
/// (the nether is false; the overworld and the end are true).
///
/// Ordering follows Paper's `SerializableChunkData.read` LEVELCHUNK branch:
/// sections and light are decoded from the raw section tags by
/// `reconstruct_sections`, the runtime chunk is constructed with the decoded
/// sections (missing entries replaced with all-air defaults, matching
/// `replaceMissingSections`), then stored heightmaps are installed with the
/// absent/malformed set primed, post-processing packed offsets are added, and
/// Starlight nibbles + `lightCorrect` are installed. `structures.References`
/// are filtered against `requested_pos` and installed into the chunk's
/// `StructureAccess` map; typed stored ticks are carried on the result (nothing
/// is scheduled or executed, #370); block entities are carried as pending NBT,
/// not materialized (#341).
///
/// Boundary: the FULL path rejects (via `SerializableChunkDataError`) blending
/// data, non-empty structure `starts`, `UpgradeData` neighbor tick lists,
/// persistent data, and non-empty entities with the same typed variants
/// `validate_full_for_reconstruction` uses — so the caller can distinguish "chunk was
/// never generated" (proto status) from "chunk carries a deferred surface".
pub fn reconstruct_runtime_chunk(
    requested_pos: ChunkPos,
    mut data: SerializableChunkData,
    height_accessor: SimpleLevelHeightAccessor,
    has_sky_light: bool,
) -> Result<ChunkReconstruction, ChunkReconstructionError> {
    // Mirror the accessor-guard ordering Paper's `SerializableChunkData.read`
    // applies: the reconstruction accessor must agree with the parse-time
    // accessor (or the section Y range / section count would silently
    // misdecode) before the content capabilities are validated, so a mismatched
    // accessor always surfaces as the accessor error regardless of content.
    if height_accessor.get_min_section_y() != data.min_section_y() {
        return Err(SerializableChunkDataError::HeightAccessorMismatch {
            parsed: data.min_section_y(),
            construction: height_accessor.get_min_section_y(),
        }
        .into());
    }
    if height_accessor.get_sections_count() as usize != data.section_count() {
        return Err(
            SerializableChunkDataError::HeightAccessorSectionCountMismatch {
                parsed: data.section_count(),
                construction: height_accessor.get_sections_count() as usize,
            }
            .into(),
        );
    }
    validate_full_for_reconstruction(&data)?;

    let factory = current_version_container_factory();
    let min_section_y = height_accessor.get_min_section_y();
    let max_section_y = height_accessor.get_max_section_y();
    // `reconstruct_sections` surfaces paletted-container failures as a typed
    // `ChunkReadException`, but the section loop's `decode_section_light`
    // panics on a malformed light array (length != 2048) — faithfully mirroring
    // Paper's unchecked `IllegalArgumentException` from `new DataLayer(byte[])`.
    // Catch that panic here so the public reconstruction API surfaces it as a
    // typed error instead of crashing the caller.
    let section_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        reconstruct_sections(
            data.section_tags(),
            min_section_y,
            max_section_y,
            &factory,
            block_state_predicates(),
        )
    }));
    let SectionReconstruction {
        sections,
        light_data,
        diagnostics,
    } = match section_result {
        Ok(Ok(reconstruction)) => reconstruction,
        Ok(Err(error)) => return Err(error.into()),
        Err(payload) => {
            let message = panic_payload_message(&payload);
            return Err(ChunkReconstructionError::SectionPanic(message));
        }
    };
    let sections: Vec<LevelChunkSection<BlockState, BiomeId>> = sections
        .into_iter()
        .map(|section| {
            section.unwrap_or_else(|| {
                LevelChunkSection::new_all_air(
                    factory.create_for_block_states(),
                    factory.create_for_biomes(),
                )
            })
        })
        .collect();

    let mut chunk = LevelChunk::new(
        requested_pos,
        data.upgrade_data().copy(),
        height_accessor,
        &factory,
        data.inhabited_time(),
        Some(sections),
        BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:air")
                .expect("air is in the generated block registry"),
        ),
        &resolve_state_flags,
    );

    install_stored_heightmaps(&mut chunk, &data);
    install_post_processing(&mut chunk, &data);
    install_lights(
        &mut chunk,
        &data,
        height_accessor,
        has_sky_light,
        &light_data,
    );
    // `structures.References` are filtered against the requested position
    // (Paper's `unpackStructureReferences` distance rule) and installed into
    // the chunk's reference map — the runtime authority for structure
    // references (#537), so no separate carry field is retained.
    let (structures_references, structure_diagnostics) =
        filter_structure_references(data.structures_references(), &requested_pos);
    install_structure_references(&mut chunk, &structures_references);

    // The serialized block entities are installed into the chunk's pending map
    // — the runtime authority for loaded block entities (#537) — with
    // duplicate corrected positions collapsing last-wins in place. No duplicate
    // snapshot Vec is retained; the outcome/derivation passes read the map.
    let block_entities = data.take_block_entities();
    install_pending_block_entities(&mut chunk, &block_entities);

    let mut parse_diagnostics = data.diagnostics().to_vec();
    if requested_pos != data.stored_pos() {
        parse_diagnostics.push(ChunkParseDiagnostic::MisplacedChunk {
            stored: data.stored_pos(),
            requested: requested_pos,
        });
    }
    parse_diagnostics.extend(structure_diagnostics);

    Ok(ChunkReconstruction {
        chunk,
        section_diagnostics: diagnostics,
        parse_diagnostics,
        raw_block_ticks: data.raw_block_ticks().clone(),
        raw_fluid_ticks: data.raw_fluid_ticks().clone(),
        stored_block_ticks: data.stored_block_ticks().to_vec(),
        stored_fluid_ticks: data.stored_fluid_ticks().to_vec(),
    })
}

/// The capabilities the FULL runtime reconstruction requires. Serialized block
/// entities are carried pending on this path (see the module docs), so the
/// validation does not reject them; the remaining unsupported surfaces surface
/// their typed errors.
fn validate_full_for_reconstruction(
    data: &SerializableChunkData,
) -> Result<(), ChunkReconstructionError> {
    Ok(data.validate_full_for_reconstruction()?)
}

/// Install stored heightmaps and prime the absent/malformed set, exactly like
/// `SerializableChunkData.read`'s heightmap loop (`chunk.setHeightmap(type,
/// heightmap)` for stored, `toPrime.add(type)` for missing, then
/// `Heightmap.primeHeightmaps`).
fn install_stored_heightmaps(chunk: &mut ReconstructedLevelChunk, data: &SerializableChunkData) {
    let heightmaps_after = data.status().heightmaps_after();
    let to_prime = reconstruct_heightmaps(chunk.base_mut(), data.heightmaps(), heightmaps_after);
    chunk.prime_heightmaps(&to_prime);
}

/// Install the packed post-processing offsets, mirroring
/// `SerializableChunkData.read`'s `addPackedPostProcess` loop. The validation
/// above already rejected an out-of-bounds non-empty section.
fn install_post_processing(chunk: &mut ReconstructedLevelChunk, data: &SerializableChunkData) {
    for (index, offsets) in data.post_processing_sections().iter().enumerate() {
        if let Some(offsets) = offsets {
            chunk.add_packed_post_process(offsets, index);
        }
    }
}

/// Install the Starlight nibble arrays and `lightCorrect` from the decoded
/// per-section light data, exactly like `SerializableChunkData.read`'s
/// `loadStarlightLightData` (`ret.setLightCorrect(false)` + filled-empty on
/// failure, else the reconstructed arrays).
fn install_lights(
    chunk: &mut ReconstructedLevelChunk,
    data: &SerializableChunkData,
    height_accessor: SimpleLevelHeightAccessor,
    has_sky_light: bool,
    light_data: &[crate::chunk::storage::serializable_chunk_data::SectionLightData],
) {
    let light = reconstruct_lights(
        height_accessor,
        light_data,
        data.light_correct(),
        has_sky_light,
    );
    chunk.set_block_nibbles(light.block_nibbles);
    chunk.set_sky_nibbles(light.sky_nibbles);
    chunk.set_light_correct(light.light_correct);
}

/// Install every serialized block entity into the chunk's pending-NBT
/// authority (`ChunkAccess.setBlockEntityNbt`) — the runtime single source of
/// truth for loaded block entities (#537). Materialization is the #341
/// boundary; the raw tags stay available in the authority for that pass.
/// Java's `postLoadChunk` only keeps `keepPacked` entries pending and
/// materializes the rest, but the block-entity map is not ported — carrying
/// all of them pending is the honest boundary.
///
/// Serialized entries whose corrected position collides in the pending map
/// collapse with the later tag winning, in place: `set_block_entity_nbt` omits
/// Paper's `containsKey` first-tag-wins guard (#216), so the chunk's pending
/// map keeps exactly one entry per position, first-insertion ordered for the
/// survivors. The collapsed duplicates are intentionally not retained — the
/// map IS the authority, and a packet materialization reflects its current
/// state.
fn install_pending_block_entities(
    chunk: &mut ReconstructedLevelChunk,
    block_entities: &[CompoundTag],
) {
    for entity_tag in block_entities {
        chunk.set_block_entity_nbt(entity_tag.clone());
    }
}

/// Install the filtered `structures.References` into the chunk's
/// `StructureAccess` reference map, keyed by the structure `Identifier`
/// (Paper's `chunk.setAllReferences(unpackStructureReferences(...))`). The
/// `markUnsaved` side effect is omitted with the chunk dirty-tracking unit.
fn install_structure_references(
    chunk: &mut ReconstructedLevelChunk,
    references: &[StructureReference],
) {
    if references.is_empty() {
        return;
    }
    let mut data = std::collections::HashMap::with_capacity(references.len());
    for entry in references {
        // The `StructureAccess` reference map models the packed longs as their
        // raw `u64` bit patterns (`IndexSet<u64>`); the parsed NBT carries the
        // signed `i64` wire form, so the cast is the install-boundary only.
        let packed: Vec<u64> = entry
            .references
            .iter()
            .map(|reference| *reference as u64)
            .collect();
        data.insert(entry.identifier.clone(), packed);
    }
    chunk.set_all_references(data);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::status::ChunkStatus;
    use crate::chunk::storage::serializable_chunk_data::SerializedBlockEntityOutcome;
    use crate::level::height_accessor;
    use crate::levelgen::heightmap::Types;
    use crate::ticks::TickPriority;
    use rivet_nbt::nbt_accounter::NbtAccounter;
    use rivet_nbt::nbt_io;
    use rivet_registry::core::BlockPos;
    use rivet_util::DataInputStream;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn fixture(dimension: &str) -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/chunk")
            .join(dimension)
            .join("0.0")
            .join("0.0.nbt");
        let bytes = std::fs::read(path).expect("Paper 26.2 chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    fn parse_fixture(dimension: &str, min_y: i32, height: i32) -> SerializableChunkData {
        let root = fixture(dimension);
        SerializableChunkData::parse(height_accessor::create(min_y, height), &root)
            .expect("fixture parses")
            .expect("fixture has a Status")
    }

    /// A radius-1 loaded-world auxiliary-data fixture (issue #371).
    fn loaded_world_fixture(name: &str) -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk")
            .join(name);
        let bytes = std::fs::read(path).expect("Paper 26.2 loaded-world chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    fn parse_loaded_world(name: &str) -> SerializableChunkData {
        let root = loaded_world_fixture(name);
        SerializableChunkData::parse(height_accessor::create(-64, 384), &root)
            .expect("fixture parses")
            .expect("fixture has a Status")
    }

    #[test]
    fn clean_overworld_full_fixture_reconstructs_into_owned_level_chunk() {
        let data = parse_fixture("overworld", -64, 384);
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("clean overworld FULL fixture reconstructs");

        let chunk = &reconstructed.chunk;
        assert_eq!(chunk.get_pos(), ChunkPos::ZERO);
        assert_eq!(chunk.get_min_y(), -64);
        assert_eq!(chunk.get_height(), 384);
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Full);
        assert_eq!(chunk.get_sections().len(), 24);
        // The bedrock layer at absolute y=-64 (section 0, local (0,0,0)).
        let bedrock = chunk.get_block_state(0, -64, 0);
        assert_eq!(bedrock.block().name(), "minecraft:bedrock");
        let grass = chunk.get_block_state(0, -61, 0);
        assert_eq!(grass.block().name(), "minecraft:grass_block");
        // The stored heightmaps are installed (all four FINAL types), not just
        // primed.
        assert!(chunk.heightmaps()[Types::WorldSurface as usize].is_some());
        assert!(chunk.heightmaps()[Types::MotionBlocking as usize].is_some());
        // The clean fixture carries no ticks and no block entities (the
        // pending authority is empty).
        assert!(reconstructed.raw_block_ticks.list.is_empty());
        assert!(reconstructed.raw_fluid_ticks.list.is_empty());
        assert!(chunk.pending_block_entities().is_empty());
        // Light is carried (lightCorrect true on the fixture).
        assert!(chunk.is_light_correct());
        assert_eq!(chunk.block_nibbles().len(), 26);
        assert_eq!(chunk.sky_nibbles().len(), 26);
    }

    #[test]
    fn clean_end_full_fixture_reconstructs_with_sky_light_carried() {
        // The End dimension type (`the_end.json`) has `has_skylight: true`, so
        // the sky gate passes and the fixture's stored sky states install.
        let data = parse_fixture("the_end", 0, 256);
        let reconstructed =
            reconstruct_runtime_chunk(ChunkPos::ZERO, data, height_accessor::create(0, 256), true)
                .expect("clean end FULL fixture reconstructs");
        assert_eq!(reconstructed.chunk.get_sections().len(), 16);
        assert!(reconstructed.chunk.is_light_correct());
        // The fixture carries a stored skylight state for its first section,
        // so the sky nibble array is not empty (the overworld fixture does too).
        assert!(
            reconstructed
                .chunk
                .sky_nibbles()
                .iter()
                .any(|nibble| nibble.get_save_state().is_some())
        );
    }

    #[test]
    fn mismatched_reconstruction_accessor_is_a_typed_error() {
        // Parsed for the overworld (-64/384) but reconstructed with an
        // accessor that disagrees: mirror `SerializableChunkData.read`'s guard
        // instead of silently misdecoding the section Y range.
        let data = parse_fixture("overworld", -64, 384);
        let wrong_min = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-80, 384),
            true,
        )
        .err()
        .expect("min section Y mismatch is typed");
        assert!(matches!(
            wrong_min,
            ChunkReconstructionError::Serializable(
                SerializableChunkDataError::HeightAccessorMismatch {
                    parsed: -4,
                    construction: -5
                }
            )
        ));

        let data = parse_fixture("overworld", -64, 384);
        let wrong_count = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 400),
            true,
        )
        .err()
        .expect("section count mismatch is typed");
        assert!(matches!(
            wrong_count,
            ChunkReconstructionError::Serializable(
                SerializableChunkDataError::HeightAccessorSectionCountMismatch {
                    parsed: 24,
                    construction: 25
                }
            )
        ));
    }

    #[test]
    fn misplaced_chunk_reconstructs_at_requested_pos_with_diagnostic() {
        // Paper `SerializableChunkData.read` relocates a mismatched stored
        // position to the requested chunk, reporting it; the reconstruction
        // keeps that diagnostic on the result rather than failing.
        let data = parse_fixture("overworld", -64, 384);
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::new(3, -7),
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("misplaced chunk still reconstructs");
        assert_eq!(reconstructed.chunk.get_pos(), ChunkPos::new(3, -7));
        assert_eq!(
            reconstructed.parse_diagnostics,
            vec![ChunkParseDiagnostic::MisplacedChunk {
                stored: ChunkPos::ZERO,
                requested: ChunkPos::new(3, -7),
            }]
        );
    }

    #[test]
    fn nether_full_fixture_reconstructs_carrying_lava_ticks() {
        // The nether 0.0 fixture carries 13 real lava `fluid_ticks`; the FULL
        // chunk reconstructs and carries the typed stored ticks on the result
        // (plus the raw list). Nothing schedules or executes them — the
        // `LevelChunkTicks`/`ProtoChunkTicks` containers stay deferred (#370).
        let data = parse_fixture("the_nether", 0, 256);
        let reconstructed =
            reconstruct_runtime_chunk(ChunkPos::ZERO, data, height_accessor::create(0, 256), false)
                .expect("nether FULL fixture reconstructs");
        assert_eq!(reconstructed.stored_fluid_ticks.len(), 13);
        assert!(reconstructed.stored_block_ticks.is_empty());
        // The raw wire list is preserved for a future installer.
        assert_eq!(reconstructed.raw_fluid_ticks.list.len(), 13);
        assert!(reconstructed.raw_block_ticks.list.is_empty());
    }

    #[test]
    fn mineshaft_fixture_reconstructs_installing_ordered_references() {
        // The radius-1 loaded-world `0.-4.nbt` fixture carries a single
        // `structures.References` entry (mineshaft -> chunk (5,-6), distance 5).
        // The FULL chunk reconstructs, the reference is filtered in-range and
        // installed into the chunk's `StructureAccess` map (keyed by the
        // structure `Identifier`) — the runtime authority (#537), so the
        // installed map is the observable carry.
        let data = parse_loaded_world("0.-4.nbt");
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::new(0, -4),
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("mineshaft FULL fixture reconstructs");

        assert!(reconstructed.parse_diagnostics.is_empty());
        let installed = reconstructed.chunk.get_all_references();
        assert_eq!(installed.len(), 1);
        let (identifier, mine) = installed.iter().next().expect("one installed ref");
        assert_eq!(identifier.namespace(), "minecraft");
        assert_eq!(identifier.path(), "mineshaft");
        assert_eq!(
            mine.iter().copied().collect::<Vec<_>>(),
            vec![-25769803771i64 as u64]
        );
    }

    #[test]
    fn block_tick_fixture_reconstructs_carrying_exact_stored_tick() {
        // The radius-1 loaded-world `-17.-19.nbt` fixture carries one sand
        // `block_ticks` entry. The FULL chunk reconstructs carrying the typed
        // stored tick with exact position/delay/priority and the raw list;
        // nothing schedules or executes it (#370).
        let data = parse_loaded_world("-17.-19.nbt");
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::new(-17, -19),
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("block-tick FULL fixture reconstructs");
        assert_eq!(
            reconstructed.stored_block_ticks,
            vec![SavedTick::new(
                Block::from_name("minecraft:sand").unwrap(),
                BlockPos::new(-268, 61, -302),
                -59,
                TickPriority::Normal,
            )]
        );
        assert!(reconstructed.stored_fluid_ticks.is_empty());
        assert_eq!(reconstructed.raw_block_ticks.list.len(), 1);
        assert!(reconstructed.raw_fluid_ticks.list.is_empty());
    }

    #[test]
    fn fluid_tick_fixture_reconstructs_carrying_exact_stored_tick() {
        // The radius-1 loaded-world `-2.-2.nbt` fixture carries one water
        // `fluid_ticks` entry; the FULL chunk reconstructs carrying it with
        // exact position/delay/priority.
        let data = parse_loaded_world("-2.-2.nbt");
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::new(-2, -2),
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("fluid-tick FULL fixture reconstructs");
        assert_eq!(
            reconstructed.stored_fluid_ticks,
            vec![SavedTick::new(
                FluidId::WATER,
                BlockPos::new(-27, 59, -17),
                2,
                TickPriority::Normal,
            )]
        );
        assert!(reconstructed.stored_block_ticks.is_empty());
        assert_eq!(reconstructed.raw_fluid_ticks.list.len(), 1);
    }

    #[test]
    fn chest_fixture_reconstructs_installing_pending_block_entity_authority() {
        // The radius-1 loaded-world `-19.-21.nbt` fixture carries one unpacked
        // chest. The FULL chunk reconstructs and installs the tag into the
        // chunk's pending-block-entity authority (the #341/#537 boundary); the
        // position-keyed map is the observable carry, and the resolved type is
        // derivable from it.
        let data = parse_loaded_world("-19.-21.nbt");
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::new(-19, -21),
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("chest FULL fixture reconstructs");
        let pending = reconstructed.chunk.pending_block_entities();
        assert_eq!(pending.len(), 1);
        let (pos, tag) = pending.iter().next().expect("one installed chest");
        // `keepPacked` is byte 0 on the fixture, so the level branch resolves
        // the unpacked type (Paper's postLoadChunk keepPacked check).
        assert_eq!(tag.get_byte_or("keepPacked", -1), 0);
        assert_eq!(
            tag.get_string("id").map(String::as_str),
            Some("minecraft:chest")
        );
        assert_eq!(*pos, BlockPos::new(-299, -51, -321));
        let resolved = crate::chunk::storage::serializable_chunk_data::reconstruct_block_entities(
            &ChunkPos::new(-19, -21),
            std::slice::from_ref(tag),
            crate::chunk::storage::serializable_chunk_data::BlockEntityChunkKind::Level,
        );
        let SerializedBlockEntityOutcome::ResolvedUnpacked(entry) = &resolved[0] else {
            panic!("fixture chest was not resolved");
        };
        assert_eq!(entry.entity_type.name(), "minecraft:chest");
    }

    #[test]
    fn proto_status_is_a_typed_no_generation_boundary() {
        let mut chunk = CompoundTag::new();
        chunk.put_string("Status", "minecraft:noise");
        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &chunk)
            .unwrap()
            .unwrap();
        let error = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .err()
        .expect("proto chunk is not a runtime LevelChunk");
        assert!(matches!(
            error,
            ChunkReconstructionError::Serializable(
                SerializableChunkDataError::UnsupportedChunkStatus {
                    status: ChunkStatus::Noise
                }
            )
        ));
    }

    #[test]
    fn missing_status_is_a_typed_absent_boundary() {
        let empty = CompoundTag::new();
        assert!(
            SerializableChunkData::parse(height_accessor::create(-64, 384), &empty)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn corrupt_section_is_a_typed_chunk_read_exception() {
        // A `block_states` compound whose `palette` is a wrong-typed tag is a
        // fatal paletted-container codec error (Paper `getOrThrow`), surfaced as
        // the #336 `ChunkReadException`.
        let mut block_states = CompoundTag::new();
        block_states.put(
            "palette".to_string(),
            rivet_nbt::tag::Tag::Int(rivet_nbt::int_tag::IntTag::value_of(7)),
        );
        let mut section = CompoundTag::new();
        section.put_byte("Y", -4);
        section.put(
            "block_states".to_string(),
            rivet_nbt::tag::Tag::Compound(block_states),
        );
        let mut chunk = CompoundTag::new();
        chunk.put_string("Status", "minecraft:full");
        chunk.put(
            "sections".to_string(),
            rivet_nbt::tag::Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                rivet_nbt::tag::Tag::Compound(section),
            ])),
        );
        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &chunk)
            .unwrap()
            .unwrap();
        let error = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .err()
        .expect("corrupt section is a typed read error");
        assert!(matches!(error, ChunkReconstructionError::Section(_)));
    }

    #[test]
    fn malformed_light_array_is_a_caught_section_panic() {
        // `decode_section_light` panics on a BlockLight byte array whose length
        // is not 2048 (faithfully mirroring Paper's unchecked
        // `IllegalArgumentException` from `new DataLayer(byte[])`). The
        // reconstruction catches that panic and surfaces it as a typed
        // `SectionPanic` instead of crashing the caller.
        // A valid single-entry `block_states` palette so the section decodes
        // past the container and reaches the light validation.
        let stone = rivet_nbt::nbt_utils::write_block_state(BlockState::of(
            rivet_registry::generated::blocks::BlockId::from_name("minecraft:stone")
                .expect("stone is in the generated block registry"),
        ));
        let mut block_states = CompoundTag::new();
        block_states.put(
            "palette".to_string(),
            rivet_nbt::tag::Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                rivet_nbt::tag::Tag::Compound(stone),
            ])),
        );
        let mut section = CompoundTag::new();
        section.put_byte("Y", 0);
        section.put(
            "block_states".to_string(),
            rivet_nbt::tag::Tag::Compound(block_states),
        );
        section.put_byte_array("BlockLight", vec![0i8; 17]);
        let mut chunk = CompoundTag::new();
        chunk.put_string("Status", "minecraft:full");
        chunk.put(
            "sections".to_string(),
            rivet_nbt::tag::Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                rivet_nbt::tag::Tag::Compound(section),
            ])),
        );
        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &chunk)
            .unwrap()
            .unwrap();
        let error = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .err()
        .expect("malformed light array is a caught panic, not a crash");
        assert!(matches!(
            error,
            ChunkReconstructionError::SectionPanic(message)
                if message.contains("BlockLight") && message.contains("2048")
        ));
    }

    #[test]
    fn non_empty_block_entities_reconstruct_into_pending_authority() {
        let mut entity = CompoundTag::new();
        entity.put_string("id", "minecraft:chest");
        entity.put_int("x", 1);
        entity.put_int("y", 64);
        entity.put_int("z", 1);
        let mut chunk = CompoundTag::new();
        chunk.put_string("Status", "minecraft:full");
        chunk.put(
            "block_entities".to_string(),
            rivet_nbt::tag::Tag::List(rivet_nbt::list_tag::ListTag::with_list(vec![
                rivet_nbt::tag::Tag::Compound(entity.clone()),
            ])),
        );
        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &chunk)
            .unwrap()
            .unwrap();
        let reconstructed = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .expect("block entities are installed pending, not rejected");
        assert_eq!(reconstructed.chunk.pending_block_entities().len(), 1);
        let pos = BlockPos::new(1, 64, 1);
        assert!(reconstructed.chunk.get_block_entity_nbt(&pos).is_some());
        assert!(
            reconstructed
                .chunk
                .get_block_entity_nbt_for_saving(&pos)
                .is_some()
        );
    }

    #[test]
    fn non_empty_structures_surface_a_typed_boundary() {
        let mut starts = CompoundTag::new();
        starts.put_int("minecraft:village", 1);
        let mut structures = CompoundTag::new();
        structures.put("starts".to_string(), rivet_nbt::tag::Tag::Compound(starts));
        let mut chunk = CompoundTag::new();
        chunk.put_string("Status", "minecraft:full");
        chunk.put(
            "structures".to_string(),
            rivet_nbt::tag::Tag::Compound(structures),
        );
        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &chunk)
            .unwrap()
            .unwrap();
        let error = reconstruct_runtime_chunk(
            ChunkPos::ZERO,
            data,
            height_accessor::create(-64, 384),
            true,
        )
        .err()
        .expect("non-empty structures are an explicit #369 boundary");
        assert!(matches!(
            error,
            ChunkReconstructionError::Serializable(
                SerializableChunkDataError::UnsupportedStructures
            )
        ));
    }
}
