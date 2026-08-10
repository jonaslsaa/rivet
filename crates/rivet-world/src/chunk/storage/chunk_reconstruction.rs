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
//! the caller owns those. Structures stay an explicit #369 boundary; live block
//! entities stay an explicit #341 boundary. Ticks are decoded through the
//! merged `SavedTick` value layer (#370/#381), but stay deferred: a FULL chunk
//! carrying a decoded non-empty `block_ticks`/`fluid_ticks` list is a typed
//! `UnsupportedTicks` error, and the API carries the raw lists so the
//! tick-execution slice (`LevelChunkTicks`/`ProtoChunkTicks`) can install
//! them without rework.
//!
//! ## Block entities on the FULL path
//!
//! `SerializableChunkData.read`'s `postLoadChunk` materializes unpacked block
//! entities and keeps `keepPacked` ones pending. The block-entity map is not
//! ported (#341), so this slice carries every serialized block entity as
//! pending NBT (the `ChunkAccess.pending_block_entities` carrier) instead of
//! materializing. That is Paper-faithful for the `keepPacked` branch and an
//! honest, typed boundary for the unpacked branch: the raw tags are retained
//! exactly, in source order, so a future #341 materialization pass can consume
//! them. This is deliberately NOT `UnsupportedBlockEntities` — the clean FULL
//! fixture carries no block entities, and a chunk that does is still fully
//! reconstructable up to that deferred surface.
//!
//! ## Structure key
//!
//! Rivet has no `Structure` type (#369), so the chunk is instantiated with the
//! unit structure key `()`. A FULL chunk whose `structures` starts/References
//! are non-empty surfaces the existing `UnsupportedStructures` typed boundary
//! from `SerializableChunkDataError`; an empty `structures` compound carries
//! nothing.

use crate::chunk::level_chunk::LevelChunk;
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::storage::section_reconstruction::{
    BiomeId, ChunkReadException, SectionBlockPredicates, SectionReconstruction,
    current_version_container_factory, reconstruct_sections,
};
use crate::chunk::storage::serializable_chunk_data::{
    ChunkParseDiagnostic, SerializableChunkData, SerializableChunkDataError,
    reconstruct_heightmaps, reconstruct_lights,
};
use crate::level::height_accessor::{LevelHeightAccessor, SimpleLevelHeightAccessor};
use crate::levelgen::heightmap::StateFlags;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::ChunkPos;

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
/// The structure key `S` is the caller's structure type; this slice instantiates
/// the chunk with the unit key `()` (no `Structure` type yet, #369) so a
/// structure-bearing chunk fails at the typed `UnsupportedStructures` boundary
/// instead of fabricating starts.
pub type ReconstructedLevelChunk = LevelChunk<BlockState, BiomeId, ()>;

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
    /// diagnostic, mirroring `SerializableChunkData::construct_full` (Paper's
    /// `reportMisplacedChunk` — the chunk is relocated, never rejected).
    pub parse_diagnostics: Vec<ChunkParseDiagnostic>,
    /// The raw `block_ticks` list, retained for the tick-execution installer.
    pub raw_block_ticks: ListTag,
    /// The raw `fluid_ticks` list, retained for the tick-execution installer.
    pub raw_fluid_ticks: ListTag,
    /// The serialized block-entity compounds, retained in source order for the
    /// #341 materialization pass.
    pub block_entities: Vec<CompoundTag>,
}

/// Why a chunk is not reconstructable into an owned runtime `LevelChunk`.
#[derive(Debug, thiserror::Error)]
pub enum ChunkReconstructionError {
    #[error(transparent)]
    Serializable(#[from] SerializableChunkDataError),
    /// The per-section paletted-container codec failure (#336).
    #[error("section {0}")]
    Section(#[from] ChunkReadException),
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
/// Starlight nibbles + `lightCorrect` are installed. Block entities are carried
/// as pending NBT, not materialized (#341).
///
/// Boundary: the FULL path rejects (via `SerializableChunkDataError`) blending
/// data, non-empty structures, non-empty ticks, persistent data, and non-empty
/// entities with the same typed variants `construct_full` uses — so the caller
/// can distinguish "chunk was never generated" (proto status) from
/// "chunk carries a deferred surface".
pub fn reconstruct_runtime_chunk(
    requested_pos: ChunkPos,
    data: SerializableChunkData,
    height_accessor: SimpleLevelHeightAccessor,
    has_sky_light: bool,
) -> Result<ChunkReconstruction, ChunkReconstructionError> {
    // Mirror `SerializableChunkData::construct_full` exactly, including its
    // guard order: the reconstruction accessor must agree with the parse-time
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
    validate_full_capabilities(&data)?;

    let factory = current_version_container_factory();
    let min_section_y = height_accessor.get_min_section_y();
    let max_section_y = height_accessor.get_max_section_y();
    let SectionReconstruction {
        sections,
        light_data,
        diagnostics,
    } = reconstruct_sections(
        data.section_tags(),
        min_section_y,
        max_section_y,
        &factory,
        block_state_predicates(),
    )?;
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
    install_pending_block_entities(&mut chunk, &data);

    let mut parse_diagnostics = data.diagnostics().to_vec();
    if requested_pos != data.stored_pos() {
        parse_diagnostics.push(ChunkParseDiagnostic::MisplacedChunk {
            stored: data.stored_pos(),
            requested: requested_pos,
        });
    }

    Ok(ChunkReconstruction {
        chunk,
        section_diagnostics: diagnostics,
        parse_diagnostics,
        raw_block_ticks: data.raw_block_ticks().clone(),
        raw_fluid_ticks: data.raw_fluid_ticks().clone(),
        block_entities: data.block_entities().to_vec(),
    })
}

/// The capabilities the FULL runtime reconstruction requires, delegating to
/// the `SerializableChunkData` seam that skips only the block-entity rejection
/// (block entities are carried pending on this path, see the module docs).
fn validate_full_capabilities(
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

/// Carry every serialized block entity as pending NBT in source order
/// (`ChunkAccess.setBlockEntityNbt`). Materialization is the #341 boundary;
/// the raw tags stay available for that pass. Java's `postLoadChunk` only keeps
/// `keepPacked` entries pending and materializes the rest, but the block-entity
/// map is not ported — carrying all of them pending is the honest boundary.
fn install_pending_block_entities(
    chunk: &mut ReconstructedLevelChunk,
    data: &SerializableChunkData,
) {
    for entity_tag in data.block_entities() {
        chunk.set_block_entity_nbt(entity_tag.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::status::ChunkStatus;
    use crate::level::height_accessor;
    use crate::levelgen::heightmap::Types;
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
        // The clean fixture carries no ticks and no block entities.
        assert!(reconstructed.raw_block_ticks.list.is_empty());
        assert!(reconstructed.raw_fluid_ticks.list.is_empty());
        assert!(reconstructed.block_entities.is_empty());
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
        // accessor that disagrees: mirror `construct_full`'s guard instead of
        // silently misdecoding the section Y range.
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
    fn nether_full_fixture_carries_lava_ticks_as_typed_boundary() {
        let data = parse_fixture("the_nether", 0, 256);
        // The nether 0.0 fixture carries real lava `fluid_ticks`, which stay
        // behind the tick-execution installer — a typed, honest error, not a
        // silent drop.
        let error =
            reconstruct_runtime_chunk(ChunkPos::ZERO, data, height_accessor::create(0, 256), false)
                .err()
                .expect("nether fixture carries ticks");
        assert!(matches!(
            error,
            ChunkReconstructionError::Serializable(SerializableChunkDataError::UnsupportedTicks {
                field: "fluid_ticks"
            })
        ));
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
    fn non_empty_block_entities_reconstruct_with_pending_nbt_carrier() {
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
        .expect("block entities are carried pending, not rejected");
        assert_eq!(reconstructed.block_entities.len(), 1);
        assert_eq!(reconstructed.block_entities[0], entity);
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
