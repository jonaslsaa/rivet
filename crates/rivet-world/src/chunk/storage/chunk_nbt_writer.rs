//! Current-version chunk NBT write half of Paper 26.2's
//! `SerializableChunkData.write()` (issue #231 / the #54 chunk-hash gate).
//!
//! This is the write counterpart to the parse surface in
//! [`super::serializable_chunk_data`]: given the extracted
//! [`SerializableChunkData`] (the auxiliary fields `SerializableChunkData.copyOf`
//! snapshots off the live chunk) and the reconstructed runtime
//! [`ReconstructedLevelChunk`] (the sections and Starlight nibbles the parse
//! installed), [`write`] produces the exact wire `CompoundTag` Paper's
//! `write()` emits.
//!
//! Field order and omission rules mirror `SerializableChunkData.write()`
//! exactly: `DataVersion` (first, via `NbtUtils.addCurrentDataVersion`), then
//! `xPos`/`yPos`(=minSectionY)/`zPos`/`LastUpdate`/`InhabitedTime`/`Status`,
//! `blending_data`/`below_zero_retrogen` when the raw carried compound is
//! present, `UpgradeData` only when non-empty, the `sections` list (each entry
//! carrying `block_states`/`biomes` when the block section is present, plain
//! `BlockLight`/`SkyLight` byte arrays when the saved nibble carried data, the
//! Starlight state INTs only when > 0, and `Y` added last only on a non-empty
//! section tag), `isLightOn` when light-correct, `block_entities`, the
//! proto-only `entities`/`carving_mask`, `block_ticks`/`fluid_ticks` via the
//! faithful tick codecs, `PostProcessing` via `packOffsets`, `Heightmaps`
//! (ordinal order — the `compound_key_order` divergence, see
//! [`super::serializable_chunk_data::write_heightmaps`]), `structures`, the
//! `ChunkBukkitValues` compound when a non-empty PDC is carried, and the
//! Starlight tail (`isLightOn` clobbered false + `starlight.light_version`).
//!
//! The paletted-container encoding reuses the exact `pack()`-produced
//! `PackedData` the parse consumes: the `palette` list is the packed
//! `palette_entries` (blocks via `NbtUtils.writeBlockState`, biomes as plain
//! registry-name strings, the `holderByNameCodec` wire form), and the `data`
//! long array is omitted exactly when `storage` is `None` (bits-on-disc == 0,
//! single-value palette).
//!
//! Honest boundaries, retained not fabricated:
//! - `blending_data`/`below_zero_retrogen` are written verbatim from the raw
//!   parse-carried compound instead of re-encoded through
//!   `BlendingData.Packed.CODEC`/`BelowZeroRetrogen.CODEC`, which are not
//!   ported (the #336 blending value layer). A genuine FULL chunk carries these
//!   only when they failed to decode (effective blending data is rejected by
//!   `validate_full_for_reconstruction`), so the verbatim write preserves the
//!   wire bytes rather than fabricating a re-encode.
//! - `entities`/`carving_mask` (proto-only) are written for a proto status
//!   from the carried values, but the reconstruction accepts only FULL chunks,
//!   so a reconstructable chunk never reaches that branch.
//! - The `ChunkBukkitValues` compound is written only when non-empty, matching
//!   `copyOf`'s `persistentDataContainer.isEmpty()` null-out.
//!
//! The input split mirrors `copyOf`: the auxiliary fields live on
//! [`SerializableChunkData`]; the live sections and Starlight nibbles live on
//! the reconstructed chunk. [`reconstruct_runtime_chunk`] consumes its
//! [`SerializableChunkData`], so a round-trip caller keeps a separate parse
//! alive for [`write`] (the tests do exactly this).

use std::sync::{Arc, LazyLock};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::nbt_utils::{add_current_data_version, write_block_state};
use rivet_nbt::short_tag::ShortTag;
use rivet_nbt::string_tag::StringTag;
use rivet_nbt::tag::Tag;
use rivet_registry::block_state::BlockState;
use rivet_serialization::codec::{self, Codec};

use crate::block::Block;
use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::paletted_container::PackedData;
use crate::chunk::registry_codecs::{block_by_name_codec, fluid_by_name_codec};
use crate::chunk::status::{ChunkStatus, ChunkType};
use crate::chunk::storage::chunk_reconstruction::ReconstructedLevelChunk;
use crate::chunk::storage::section_reconstruction::BiomeId;
use crate::chunk::storage::serializable_chunk_data::{
    BLOCK_LIGHT_TAG, BLOCKLIGHT_STATE_TAG, SKY_LIGHT_TAG, SKYLIGHT_STATE_TAG,
    STARLIGHT_LIGHT_VERSION, STARLIGHT_VERSION_TAG, SerializableChunkData, write_heightmaps,
};
use crate::level::height_accessor::LevelHeightAccessor;
use crate::ticks::{SavedTick, saved_tick_codec};
use rivet_registry::fluid_id::FluidId;

const BLOCK_TICKS_TAG: &str = "block_ticks";
const FLUID_TICKS_TAG: &str = "fluid_ticks";
const POST_PROCESSING_TAG: &str = "PostProcessing";
const STRUCTURES_TAG: &str = "structures";
const CHUNK_BUKKIT_VALUES_TAG: &str = "ChunkBukkitValues";

/// The cached top-level tick-list codecs, built once and shared (Paper's
/// `BLOCK_TICKS_CODEC` / `FLUID_TICKS_CODEC` are `static final` in
/// `SerializableChunkData`).
static BLOCK_TICK_LIST_CODEC: LazyLock<Arc<dyn Codec<Vec<SavedTick<Block>>, NbtOps>>> =
    LazyLock::new(|| codec::list(saved_tick_codec::<Block, NbtOps>(block_by_name_codec())));
static FLUID_TICK_LIST_CODEC: LazyLock<Arc<dyn Codec<Vec<SavedTick<FluidId>>, NbtOps>>> =
    LazyLock::new(|| codec::list(saved_tick_codec::<FluidId, NbtOps>(fluid_by_name_codec())));

/// Encode one paletted-container's `PackedData` into the wire `CompoundTag`
/// (`PalettedContainer.codec` encode: `palette` list + optional `data`).
///
/// The `palette` list uses the element codec's encode form: `write_block_state`
/// for block states (`Name` + name-sorted `Properties`; a singleton state omits
/// `Properties`), and the plain registry-name string for biomes
/// (`Registry.holderByNameCodec` encodes a `Holder.Reference` to its key's
/// identifier). `data` is omitted exactly when the pack produced no storage
/// (bits-on-disc == 0, single-value palette) — `Codec.LONG_STREAM
/// .lenientOptionalFieldOf("data")` omits the field on an empty `Optional`.
pub fn encode_paletted_container<T>(
    packed: &PackedData<T>,
    encode_element: impl Fn(&T) -> Tag,
) -> CompoundTag {
    let mut container = CompoundTag::new();
    container.put(
        "palette".to_string(),
        Tag::List(ListTag::with_list(
            packed.palette_entries.iter().map(&encode_element).collect(),
        )),
    );
    if let Some(storage) = &packed.storage {
        container.put_long_array("data", storage.clone());
    }
    container
}

/// `NbtUtils.writeBlockState`-based palette element encoder for a block-state
/// container.
fn block_state_element(state: &BlockState) -> Tag {
    Tag::Compound(write_block_state(*state))
}

/// `Registry.holderByNameCodec` wire form for a biome container element: the
/// biome registry key as a plain string. The reconstructed `BiomeId` is a dense
/// registry index (see `section_reconstruction`), decoded from the same
/// `BIOME_BY_NAME` table, so the reverse lookup is the id-indexed name table.
fn biome_element(biome: &BiomeId) -> Tag {
    Tag::String(StringTag::value_of(
        rivet_registry::generated::biomes::BIOME_BY_ID[biome.0 as usize].to_string(),
    ))
}

/// Write a block-section's `block_states`/`biomes` into `section_tag`. The
/// section's containers pack with their own strategies — `pack()` re-encodes
/// the storage against a fresh `HashMapPalette`, the exact inverse of the
/// parse's `unpack`, so a genuine section round-trips byte-identically.
fn store_section_containers(
    section_tag: &mut CompoundTag,
    section: &LevelChunkSection<BlockState, BiomeId>,
) {
    section_tag.put(
        "block_states".to_string(),
        Tag::Compound(encode_paletted_container(
            &section.states().pack(),
            block_state_element,
        )),
    );
    section_tag.put(
        "biomes".to_string(),
        Tag::Compound(encode_paletted_container(
            &section.biomes().pack(),
            biome_element,
        )),
    );
}

/// Mirror `SerializableChunkData.write()`'s `packOffsets`: a `null` (absent)
/// section becomes an empty list; a non-empty `short[]` becomes a `ShortTag`
/// list. The parse's `post_processing_sections` already normalizes an empty
/// wire list to `None` (Java's `!shorts.isEmpty() ? ... : null`).
fn pack_offsets(post_processing: &[Option<Vec<i16>>]) -> ListTag {
    let mut list = ListTag::new();
    for offsets in post_processing {
        let mut offsets_tag = ListTag::new();
        if let Some(offsets) = offsets {
            for offset in offsets {
                offsets_tag.add(Tag::Short(ShortTag::value_of(*offset)));
            }
        }
        list.add(Tag::List(offsets_tag));
    }
    list
}

/// Write the full current-version chunk `CompoundTag` from the extracted
/// [`SerializableChunkData`] and the reconstructed runtime chunk — the write
/// counterpart to [`SerializableChunkData::parse`] +
/// [`reconstruct_runtime_chunk`], mirroring `SerializableChunkData.write()`
/// field-for-field.
///
/// The input split mirrors `copyOf`'s: the auxiliary fields (position, times,
/// status, upgrade data, raw blending/below-zero compounds, block entities,
/// post-processing, structures, persistent data) live on
/// [`SerializableChunkData`]; the live sections and Starlight nibbles live on
/// the reconstructed [`ReconstructedLevelChunk`]. `LastUpdate` is the parsed
/// carry (a live save would pass the current game time, exactly what `copyOf`
/// reads from `level.getGameTime()`).
pub fn write(data: &SerializableChunkData, chunk: &ReconstructedLevelChunk) -> CompoundTag {
    let mut tag = CompoundTag::new();
    add_current_data_version(&mut tag);
    // Paper's `write()` emits `this.chunkPos`/`this.minSectionY`, which `copyOf`
    // snapshots from the live chunk (`chunk.getPos()`/`chunk.getMinSectionY()`).
    // The reconstructed chunk is that authority here, so the position comes from
    // the chunk — not the parsed `stored_pos`, which may differ when a chunk was
    // re-parked at a different coordinate.
    let pos = chunk.get_pos();
    tag.put_int("xPos", pos.x());
    tag.put_int("yPos", chunk.height_accessor().get_min_section_y());
    tag.put_int("zPos", pos.z());
    tag.put_long("LastUpdate", data.last_update_time());
    tag.put_long("InhabitedTime", chunk.get_inhabited_time());
    tag.put_string("Status", data.status().serialization_name());
    // `blending_data` / `below_zero_retrogen` are written verbatim from the
    // raw parse-carried compound. Paper re-encodes through
    // `BlendingData.Packed.CODEC` / `BelowZeroRetrogen.CODEC`, which are not
    // ported (the #336 blending value layer); a genuine FULL chunk reaches
    // write only without decodable blending data, so the verbatim compound is
    // the honest preservation of the wire bytes.
    if let Some(raw) = data.raw_blending_data() {
        tag.put("blending_data".to_string(), Tag::Compound(raw.clone()));
    }
    if let Some(raw) = data.raw_below_zero_retrogen() {
        tag.put(
            "below_zero_retrogen".to_string(),
            Tag::Compound(raw.clone()),
        );
    }
    if !data.upgrade_data().is_empty() {
        tag.put(
            "UpgradeData".to_string(),
            Tag::Compound(data.upgrade_data().write()),
        );
    }

    let min_section_y = data.min_section_y();
    let max_section_y = chunk.height_accessor().get_max_section_y();
    let sections = chunk.get_sections();
    let block_nibbles = chunk.block_nibbles();
    let sky_nibbles = chunk.sky_nibbles();

    let mut section_tags = ListTag::new();
    // The Starlight loop bounds: min light section = minSection - 1, max light
    // section = maxSection + 1 (`WorldUtil.getMin/MaxLightSection`). The block
    // section index is `lightSection - minBlockSection` (Java's
    // `blockSectionIdx >= 0 && blockSectionIdx < chunkSections.length` guard
    // becomes an out-of-bounds `get` -> `None`); the nibble index is
    // `lightSection - minLightSection`.
    for light_section in min_section_y - 1..=max_section_y + 1 {
        let light_index = (light_section - (min_section_y - 1)) as usize;
        let block_index = light_section - min_section_y;
        let chunk_section = sections.get(block_index as usize);
        let block_nibble = block_nibbles
            .get(light_index)
            .and_then(|n| n.get_save_state());
        let sky_nibble = sky_nibbles
            .get(light_index)
            .and_then(|n| n.get_save_state());

        if chunk_section.is_none() && block_nibble.is_none() && sky_nibble.is_none() {
            continue;
        }

        let mut section_tag = CompoundTag::new();
        if let Some(chunk_section) = chunk_section {
            store_section_containers(&mut section_tag, chunk_section);
        }
        // `SaveUtil` writes `BlockLight`/`SkyLight` only when the saved state
        // carried data (a `null` data array becomes no byte array).
        if let Some(state) = &block_nibble
            && let Some(bytes) = &state.data
        {
            section_tag.put_byte_array(
                BLOCK_LIGHT_TAG,
                bytes.iter().map(|byte| *byte as i8).collect(),
            );
        }
        if let Some(state) = &sky_nibble
            && let Some(bytes) = &state.data
        {
            section_tag.put_byte_array(
                SKY_LIGHT_TAG,
                bytes.iter().map(|byte| *byte as i8).collect(),
            );
        }
        // Starlight state INTs are written only when > 0 (a Null nibble has no
        // state at all — `get_save_state` returns `None` — and an absent state
        // stays absent, matching `getSaveState`'s `> 0` gate).
        if let Some(state) = &block_nibble
            && state.state.to_i32() > 0
        {
            section_tag.put_int(BLOCKLIGHT_STATE_TAG, state.state.to_i32());
        }
        if let Some(state) = &sky_nibble
            && state.state.to_i32() > 0
        {
            section_tag.put_int(SKYLIGHT_STATE_TAG, state.state.to_i32());
        }

        if !section_tag.is_empty() {
            section_tag.put_byte("Y", light_section as i8);
            section_tags.add(Tag::Compound(section_tag));
        }
    }

    tag.put("sections".to_string(), Tag::List(section_tags));

    // Paper gates `isLightOn` on `this.lightCorrect`, which `copyOf` snapshots
    // from the live chunk's `isLightCorrect()`. The reconstructed chunk is that
    // authority here — `install_lights` may have lowered it (a light-array
    // validation panic forces the chunk flag to false, exactly like Paper's
    // `loadStarlightLightData` catch).
    let light_correct = chunk.is_light_correct();
    if light_correct {
        tag.put_boolean("isLightOn", true);
    }

    let mut block_entity_tags = ListTag::new();
    for entity in data.block_entities() {
        block_entity_tags.add(Tag::Compound(entity.clone()));
    }
    tag.put("block_entities".to_string(), Tag::List(block_entity_tags));

    // Proto-only fields, mirroring `write()`'s `PROTOCHUNK` branch. The
    // reconstruction accepts only FULL chunks, so a reconstructable chunk never
    // reaches this branch — retained for exact parity and guarded by the status
    // so a proto status cannot silently drop them.
    if data.status().chunk_type() == ChunkType::ProtoChunk {
        let mut entity_tags = ListTag::new();
        for entity in data.entities() {
            entity_tags.add(Tag::Compound(entity.clone()));
        }
        tag.put("entities".to_string(), Tag::List(entity_tags));
        if let Some(mask) = data.carving_mask() {
            tag.put_long_array("carving_mask", mask.to_vec());
        }
    }

    save_ticks(&mut tag, data);

    tag.put(
        POST_PROCESSING_TAG.to_string(),
        Tag::List(pack_offsets(data.post_processing_sections())),
    );

    tag.put(
        "Heightmaps".to_string(),
        Tag::Compound(write_heightmaps(data.heightmaps().clone())),
    );

    tag.put(
        STRUCTURES_TAG.to_string(),
        Tag::Compound(data.structure_data().clone()),
    );

    // `copyOf` nulls the PDC when `persistentDataContainer.isEmpty()`, so a
    // present-but-empty wire compound is not re-written (`!isEmpty()` gate).
    if let Some(Tag::Compound(container)) = data.persistent_data_container()
        && !container.is_empty()
    {
        tag.put(
            CHUNK_BUKKIT_VALUES_TAG.to_string(),
            Tag::Compound(container.clone()),
        );
    }

    // Starlight tail: a light-correct chunk at-or-after LIGHT clobbers
    // `isLightOn` to false and stamps its light version
    // (`!isBefore(LIGHT)` is `is_or_after(LIGHT)`). The gate reuses the same
    // chunk light-correct flag the `isLightOn` write used, matching Paper's
    // `this.lightCorrect` in both.
    if light_correct && data.status().is_or_after(ChunkStatus::Light) {
        tag.put_boolean("isLightOn", false);
        tag.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
    }

    tag
}

/// `SerializableChunkData.saveTicks` — store the typed stored tick lists
/// through the faithful `SavedTick.codec(byNameCodec).listOf()` codecs (the
/// exact `BLOCK_TICKS_CODEC`/`FLUID_TICKS_CODEC` Paper caches). The parse
/// already filtered the typed values to the stored chunk (Paper's
/// `filterTickListForChunk`), so this reproduces what `copyOf`'s
/// `chunk.getTicksForSerialization(level.getGameTime())` would hand to
/// `write()`.
fn save_ticks(tag: &mut CompoundTag, data: &SerializableChunkData) {
    // The accessors expose the stored ticks as slices; `CompoundTag::store`
    // encodes through the `Vec`-shaped tick-list codec, so the owned value is
    // collected once per save.
    tag.store(
        BLOCK_TICKS_TAG,
        &BLOCK_TICK_LIST_CODEC,
        &data.stored_block_ticks().to_vec(),
    );
    tag.store(
        FLUID_TICKS_TAG,
        &FLUID_TICK_LIST_CODEC,
        &data.stored_fluid_ticks().to_vec(),
    );
}
