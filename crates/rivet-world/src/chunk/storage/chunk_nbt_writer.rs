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
//! `xPos`/`yPos`(=minSectionY)/`zPos`/`LastUpdate`/`InhabitedTime`/`Status`
//! (`blending_data` is never written and `below_zero_retrogen` only on the
//! proto-only branch — see the honest boundaries below), `UpgradeData` only
//! when non-empty, the `sections` list (each entry
//! carrying `block_states`/`biomes` when the block section is present, plain
//! `BlockLight`/`SkyLight` byte arrays and the Starlight state INTs only for a
//! light-correct chunk when the saved nibble carried data / a state > 0 —
//! Paper's `copyOf` snapshots these from the live nibbles, which are all-Null
//! on a `!lightCorrect` chunk, so a non-light-correct write emits neither —
//! and `Y` added last only on a non-empty section tag), `isLightOn` when
//! light-correct, `block_entities`, the
//! proto-only `entities`/`carving_mask`, `block_ticks`/`fluid_ticks` via the
//! faithful tick codecs, `PostProcessing` via `packOffsets`, `Heightmaps`
//! (ordinal order — the `compound_key_order` divergence, see
//! [`super::serializable_chunk_data::write_heightmaps`]), `structures`, the
//! `ChunkBukkitValues` compound when a non-empty PDC is carried, and the
//! Starlight tail (`isLightOn` clobbered false + `starlight.light_version`).
//!
//! The typed block/fluid tick lists are intentionally written from the parsed
//! pending values verbatim. Reconstruction carries [`SavedTick`] values but
//! does not install authoritative `LevelChunkTicks`/`ProtoChunkTicks` runtime
//! containers, so this writer must not invent a rebase from serialized
//! `LastUpdate`. Paper preserves pending ticks unchanged and only repacks live
//! queued ticks against the level's current game time. The live-save seam for
//! #522/#231 must eventually snapshot those runtime containers before calling
//! this writer, so newly scheduled, removed, or unpacked ticks are included.
//!
//! The paletted-container encoding reuses the exact `pack()`-produced
//! `PackedData` the parse consumes: the `palette` list is the packed
//! `palette_entries` (blocks via `NbtUtils.writeBlockState`, biomes as plain
//! registry-name strings, the `holderByNameCodec` wire form), and the `data`
//! long array is omitted exactly when `storage` is `None` (bits-on-disc == 0,
//! single-value palette).
//!
//! Honest boundaries, retained not fabricated:
//! - `blending_data` is never written. Paper's `parse` decodes it through
//!   `BlendingData.Packed.CODEC` (`orElse(null)` on failure), `read` unpacks it
//!   into the chunk, and `copyOf` re-packs + `write` re-encodes only a
//!   decodable value. A decodable one is rejected before reconstruction by
//!   `validate_full_for_reconstruction` (the #336 blending layer); an
//!   undecodable one decodes to null on parse and is dropped — so neither
//!   reaches this writer, matching Paper's re-save.
//! - `below_zero_retrogen` is written only on the proto-only branch. Paper's
//!   `read` installs it only on the proto branch
//!   (`protoChunk.setBelowZeroRetrogen`); the LEVELCHUNK branch never does, so
//!   `copyOf`'s `chunk.getBelowZeroRetrogen()` is null for a FULL chunk and
//!   `write` omits it — even when it decoded. On the proto branch it is written
//!   verbatim from the raw carried compound (the `BelowZeroRetrogen.CODEC` is
//!   not ported, #336), but the FULL reconstruction this writer serves never
//!   reaches that branch.
//! - `entities`/`carving_mask` (proto-only) are written for a proto status
//!   from the carried values, but the reconstruction accepts only FULL chunks,
//!   so a reconstructable chunk never reaches that branch.
//! - The `ChunkBukkitValues` compound is written only when non-empty, matching
//!   `copyOf`'s `persistentDataContainer.isEmpty()` null-out.
//!
//! The input split mirrors `copyOf`: the auxiliary fields live on
//! [`SerializableChunkData`]; the live sections, heightmaps, and Starlight
//! nibbles live on the reconstructed chunk (the heightmaps especially: Paper's
//! `read` primes missing ones before `copyOf`, so the write must read them from
//! the chunk — the authoritative primed result — not the stored parse map).
//! [`reconstruct_runtime_chunk`] consumes its [`SerializableChunkData`], so a
//! round-trip caller keeps a separate parse alive for [`write`] (the tests do
//! exactly this).

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
    STARLIGHT_LIGHT_VERSION, STARLIGHT_VERSION_TAG, SerializableChunkData, StoredHeightmaps,
    write_heightmaps,
};
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::heightmap::Types;
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

/// `copyOf`'s heightmap snapshot: the live chunk's `heightmapsAfter()` types,
/// cloned raw data. Paper's `read` primes missing heightmaps
/// (`Heightmap.primeHeightmaps`) before `copyOf`, so a wire chunk whose
/// `Heightmaps` omitted a type (or carried a wrong-length array) re-saves with
/// the primed value, not a dropped key — the reconstructed chunk is the
/// authoritative result. `write_heightmaps` then emits ordinal order, matching
/// Paper's `write` iterating the `EnumMap`.
fn chunk_heightmaps(
    chunk: &ReconstructedLevelChunk,
    heightmaps_after: &[Types],
) -> StoredHeightmaps {
    let mut stored: StoredHeightmaps = std::array::from_fn(|_| None);
    for ty in heightmaps_after {
        let index = *ty as usize;
        if let Some(heightmap) = &chunk.heightmaps()[index] {
            stored[index] = Some(heightmap.get_raw_data().to_vec());
        }
    }
    stored
}

/// Write the full current-version chunk `CompoundTag` from the extracted
/// [`SerializableChunkData`] and the reconstructed runtime chunk — the write
/// counterpart to [`SerializableChunkData::parse`] +
/// [`reconstruct_runtime_chunk`], mirroring `SerializableChunkData.write()`
/// field-for-field.
///
/// The input split mirrors `copyOf`'s: the auxiliary fields (position, times,
/// status, upgrade data, below-zero retrogen on the proto-only branch, block
/// entities, post-processing, structures, persistent data) live on
/// [`SerializableChunkData`]; the live sections, heightmaps, and Starlight
/// nibbles live on the reconstructed [`ReconstructedLevelChunk`]. `LastUpdate`
/// comes from the explicit `game_time` argument, matching the value
/// `SerializableChunkData.copyOf` snapshots from `level.getGameTime()`.
///
/// `InhabitedTime` remains sourced from the reconstructed chunk, exactly as
/// `copyOf` snapshots it from `chunk.getInhabitedTime()`.
pub fn write(
    data: &SerializableChunkData,
    chunk: &ReconstructedLevelChunk,
    game_time: i64,
) -> CompoundTag {
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
    tag.put_long("LastUpdate", game_time);
    tag.put_long("InhabitedTime", chunk.get_inhabited_time());
    tag.put_string("Status", data.status().serialization_name());
    // `blending_data` is never written: Paper's `parse` decodes it through
    // `BlendingData.Packed.CODEC` (`orElse(null)` on failure), `read` unpacks
    // it into the chunk, and `copyOf` re-packs + `write` re-encodes only a
    // decodable value. A decodable one is rejected before reconstruction
    // (`validate_full_for_reconstruction`, #336); an undecodable one decodes to
    // null and is dropped — matching Paper's re-save.
    //
    // `below_zero_retrogen` mirrors Paper's `read` branch split: only the proto
    // branch installs it (`protoChunk.setBelowZeroRetrogen`), so `copyOf`'s
    // `chunk.getBelowZeroRetrogen()` is null for a FULL chunk and `write` omits
    // it — even when it decoded. The proto branch writes it verbatim (the
    // `BelowZeroRetrogen.CODEC` is not ported, #336); the FULL reconstruction
    // this writer serves never reaches that branch.
    if data.status().chunk_type() == ChunkType::ProtoChunk
        && let Some(raw) = data.raw_below_zero_retrogen()
    {
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

    // `this.lightCorrect`, which `copyOf` snapshots from the live chunk's
    // `isLightCorrect()`. Hoisted before the section loop: it also gates the
    // per-section light-array/state-INT emission (see below), not just
    // `isLightOn` and the Starlight tail.
    let light_correct = chunk.is_light_correct();

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
        // Light arrays and Starlight state INTs are written only for a
        // light-correct chunk. Paper's `copyOf` snapshots both from the live
        // nibbles' `getSaveState()`, and a `!lightCorrect` chunk always carries
        // all-Null nibbles (`loadStarlightLightData` installs
        // `getFilledEmptyLight` and returns early), so its `write()` emits
        // neither the `BlockLight`/`SkyLight` arrays nor the state INTs. Rivet's
        // reconstruction installs vanilla-format plain arrays as Initialised
        // nibbles for the send path (issue #531), so the writer must not leak
        // those onto disk: gate the emission on `light_correct` to reproduce
        // Paper's re-save (the vanilla light is dropped, and the chunk is
        // re-lit by Starlight).
        if light_correct {
            // `SaveUtil` writes `BlockLight`/`SkyLight` only when the saved
            // state carried data (a `null` data array becomes no byte array).
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
            // Starlight state INTs are written only when > 0 (a Null nibble has
            // no state at all — `get_save_state` returns `None` — and an absent
            // state stays absent, matching `getSaveState`'s `> 0` gate).
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
    // `loadStarlightLightData` catch). The flag was hoisted before the section
    // loop above (it also gates the per-section light emission).
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

    // `copyOf` snapshots the live chunk's `heightmapsAfter()` set — which after
    // `read`'s `Heightmap.primeHeightmaps` includes the primed missing entries —
    // so the write reads from the reconstructed chunk, not the stored parse
    // map, normalizing a wire chunk that omitted (or carried a wrong-length)
    // heightmap instead of dropping the key.
    tag.put(
        "Heightmaps".to_string(),
        Tag::Compound(write_heightmaps(chunk_heightmaps(
            chunk,
            data.status().heightmaps_after(),
        ))),
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
/// `filterTickListForChunk`). These values remain pending in the current
/// reconstruction, and Paper's `LevelChunkTicks.pack` / `ProtoChunkTicks.pack`
/// preserve pending values unchanged; only live queued runtime ticks are
/// repacked against the current game time.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::storage::chunk_reconstruction::reconstruct_runtime_chunk;
    use crate::chunk::storage::section_reconstruction::current_version_container_factory;
    use crate::level::height_accessor;
    use crate::lighting::swmr_nibble_array::InitState;
    use rivet_nbt::nbt_accounter::NbtAccounter;
    use rivet_nbt::nbt_io;
    use rivet_util::DataInputStream;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn fixture(name: &str) -> CompoundTag {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/rivet-oracle/fixtures/loaded-world/chunk")
            .join(name);
        let bytes = std::fs::read(path).expect("Paper 26.2 loaded-world chunk fixture");
        let mut input = DataInputStream::new(Cursor::new(bytes));
        nbt_io::read(&mut input, &mut NbtAccounter::unlimited_heap()).expect("valid fixture")
    }

    /// Recursively assert two tags carry the same content, ignoring compound
    /// key order. Rivet's insertion-ordered `CompoundTag` writes heightmap and
    /// section keys in a different order than Paper's fastutil hash map (the
    /// `compound_key_order` divergence), so byte identity is never the oracle
    /// here — structural equality is.
    fn assert_equivalent(actual: &Tag, expected: &Tag, path: &str) {
        match (actual, expected) {
            (Tag::Compound(a), Tag::Compound(e)) => {
                assert_eq!(a.size(), e.size(), "compound {path} size");
                for key in a.key_set() {
                    let expected_tag = e
                        .get(key.as_str())
                        .unwrap_or_else(|| panic!("missing key {path}.{key}"));
                    assert_equivalent(
                        a.get(key.as_str()).unwrap(),
                        expected_tag,
                        &format!("{path}.{key}"),
                    );
                }
            }
            (Tag::List(a), Tag::List(e)) => {
                assert_eq!(a.size(), e.size(), "list {path} size");
                for (index, (actual_tag, expected_tag)) in
                    a.list.iter().zip(e.list.iter()).enumerate()
                {
                    assert_equivalent(actual_tag, expected_tag, &format!("{path}[{index}]"));
                }
            }
            _ => assert_eq!(actual, expected, "tag {path}"),
        }
    }

    /// Parse -> reconstruct -> write once, returning the written payload. The
    /// reconstruction consumes its [`SerializableChunkData`], so the caller
    /// keeps a separate parse alive for `write` (the writer's input split is
    /// data + chunk, mirroring `copyOf`'s split of auxiliary fields and live
    /// sections).
    /// Preserve the parse/write fixed point by passing the parsed
    /// `LastUpdate` as the explicit game-time argument. A live save will pass
    /// the level's current game time instead.
    fn write_once(root: &CompoundTag) -> CompoundTag {
        write_once_with_game_time(root, |data| data.last_update_time())
    }

    fn write_once_with_game_time(
        root: &CompoundTag,
        game_time: impl FnOnce(&SerializableChunkData) -> i64,
    ) -> CompoundTag {
        let height = height_accessor::create(-64, 384);
        let data = SerializableChunkData::parse(height, root)
            .expect("fixture parses")
            .expect("fixture has a Status");
        assert_eq!(data.validate_full_for_reconstruction(), Ok(()));
        let game_time = game_time(&data);
        let for_reconstruction = SerializableChunkData::parse(height, root)
            .expect("fixture parses")
            .expect("fixture has a Status");
        let reconstruction =
            reconstruct_runtime_chunk(data.stored_pos(), for_reconstruction, height, true)
                .expect("fixture reconstructs");
        write(&data, &reconstruction.chunk, game_time)
    }

    /// The loaded-world fixtures are vanilla-format writes: `isLightOn` present
    /// but no `starlight.light_version`, so `light_correct` is false and Paper's
    /// own re-save would normalize them (drop `isLightOn`, stamp Starlight state
    /// INTs). They cannot be compared byte- or structure-identical to a fresh
    /// write of the same chunk. This helper re-stamps them into a genuine
    /// Starlight-lit save so the writer's fixed point is well-defined: every
    /// in-bounds section carries the `getFilledEmptyLight` Uninitialised block
    /// and sky states, the two sky-lit sections (Y=4,5) carry the Initialised
    /// sky state that matches their byte arrays, and the chunk carries the
    /// Starlight light version.
    fn make_starlight_lit(mut root: CompoundTag) -> CompoundTag {
        root.put_int(STARLIGHT_VERSION_TAG, STARLIGHT_LIGHT_VERSION);
        let mut sky_lit = 0;
        root.for_each(|key, value| {
            if key != "sections" {
                return;
            }
            let Tag::List(sections) = value else {
                return;
            };
            for section in sections.list.iter_mut() {
                let Tag::Compound(section) = section else {
                    continue;
                };
                let y = section.get_byte_or("Y", 0);
                section.put_int(BLOCKLIGHT_STATE_TAG, InitState::Uninitialised.to_i32());
                let sky_state = if y == 4 || y == 5 {
                    sky_lit += 1;
                    InitState::Initialised.to_i32()
                } else {
                    InitState::Uninitialised.to_i32()
                };
                section.put_int(SKYLIGHT_STATE_TAG, sky_state);
            }
        });
        assert_eq!(sky_lit, 2, "the two sky-lit fixture sections are stamped");
        root
    }

    /// The writer's fixed-point oracle on a genuine Starlight-lit chunk:
    /// `write` must be a fixed point of `parse` given a light-correct chunk,
    /// because the reconstruction installs the light state INTs exactly as the
    /// write reads them back.
    fn assert_write_idempotent(root: &CompoundTag) {
        let first = write_once(root);
        let second = write_once(&first);
        assert_equivalent(&Tag::Compound(first), &Tag::Compound(second), "chunk");
    }

    #[test]
    fn paletted_container_single_value_omits_data() {
        let factory = current_version_container_factory();
        // A single-value (all-air) block-state container packs with no storage.
        let single = factory.create_for_block_states();
        let encoded = encode_paletted_container(&single.pack(), block_state_element);
        assert!(
            encoded.get_long_array("data").is_none(),
            "single-value palette omits data"
        );
        let Tag::List(palette) = encoded.get("palette").expect("palette is a list") else {
            panic!("palette must be a list");
        };
        assert_eq!(palette.size(), 1);
        let Tag::Compound(first) = &palette.list[0] else {
            panic!("block-state element is a compound");
        };
        assert_eq!(
            first.get_string("Name").map(String::as_str),
            Some("minecraft:air")
        );
        assert!(
            first.get("Properties").is_none(),
            "singleton state omits Properties"
        );
    }

    #[test]
    fn paletted_container_multi_value_writes_data_and_name_sorted_properties() {
        let factory = current_version_container_factory();
        let mut states = factory.create_for_block_states();
        // Two distinct states force a non-zero bits-on-disc storage.
        let stone = crate::block::Block::from_name("minecraft:stone")
            .unwrap()
            .default_block_state();
        let grass = crate::block::Block::from_name("minecraft:grass_block")
            .unwrap()
            .default_block_state();
        states.set(0, 0, 0, stone);
        states.set(1, 0, 0, grass);
        let packed = states.pack();
        assert!(
            packed.storage.is_some(),
            "multi-value container has storage"
        );
        assert!(
            packed.bits_per_entry > 0,
            "multi-value container has non-zero bits"
        );
        let encoded = encode_paletted_container(&packed, block_state_element);
        assert!(
            encoded.get_long_array("data").is_some(),
            "data written for non-zero storage"
        );
        let Tag::List(palette) = encoded.get("palette").unwrap() else {
            panic!("palette must be a list");
        };
        // `pack_with_strategy` re-encodes against a fresh HashMapPalette seeded
        // empty, so the packed entries are the distinct storage values in
        // index-scan order (slot 0 = stone, slot 1 = grass, the remaining air
        // slots) — size 3, not the two hand-set states.
        assert_eq!(palette.size(), 3);
        let Tag::Compound(stone_tag) = &palette.list[0] else {
            panic!()
        };
        assert_eq!(
            stone_tag.get_string("Name").map(String::as_str),
            Some("minecraft:stone")
        );
        assert!(
            stone_tag.get("Properties").is_none(),
            "singleton stone omits Properties"
        );
        let Tag::Compound(grass_tag) = &palette.list[1] else {
            panic!()
        };
        assert_eq!(
            grass_tag.get_string("Name").map(String::as_str),
            Some("minecraft:grass_block")
        );
        // grass_block is a multi-state block: Properties present, name-sorted.
        let properties = grass_tag
            .get_compound("Properties")
            .expect("multi-state block has Properties");
        assert_eq!(
            properties.key_set().map(String::as_str).collect::<Vec<_>>(),
            vec!["snowy"]
        );
        let Tag::Compound(air_tag) = &palette.list[2] else {
            panic!()
        };
        assert_eq!(
            air_tag.get_string("Name").map(String::as_str),
            Some("minecraft:air")
        );
    }

    #[test]
    fn biome_container_palette_uses_plain_registry_name_strings() {
        let factory = current_version_container_factory();
        let biomes = factory.create_for_biomes();
        let encoded = encode_paletted_container(&biomes.pack(), biome_element);
        assert!(encoded.get_long_array("data").is_none());
        let Tag::List(palette) = encoded.get("palette").unwrap() else {
            panic!("palette must be a list");
        };
        assert_eq!(palette.size(), 1);
        assert_eq!(
            palette.list[0],
            Tag::String(StringTag::value_of("minecraft:plains".to_string()))
        );
    }

    #[test]
    fn starlight_lit_fixture_write_is_a_fixed_point() {
        // Re-stamp the genuine FULL loaded-world fixture as a Starlight-lit
        // save (the exact `parse_light_correct` predicate) and assert the
        // writer's fixed point: parsing the first write back and writing again
        // reproduces it structurally. This is the real parity contract — Paper
        // `copyOf`+`write` on a light-correct chunk round-trips the section
        // contents, light states, `isLightOn` (clobbered false) and
        // `starlight.light_version`.
        let lit = make_starlight_lit(fixture("-1.-3.nbt"));
        assert_write_idempotent(&lit);
    }

    #[test]
    fn starlight_tail_writes_clobbered_is_light_on_and_light_version() {
        // The tail's `isLightOn` clobber + `starlight.light_version` stamp are
        // asserted directly on a light-correct write: `isLightOn` must be
        // present-as-false and the light version stamped.
        let lit = make_starlight_lit(fixture("-1.-3.nbt"));
        let written = write_once(&lit);
        assert_eq!(
            written.get_int(STARLIGHT_VERSION_TAG),
            Some(STARLIGHT_LIGHT_VERSION),
            "light-correct write stamps the Starlight light version"
        );
        assert_eq!(
            written.get_boolean("isLightOn"),
            Some(false),
            "Starlight tail clobbers isLightOn to false"
        );
        assert_eq!(STARLIGHT_LIGHT_VERSION, 10);
    }

    #[test]
    fn vanilla_fixture_write_drops_is_light_on_and_light_arrays() {
        // The unmodified fixture is a vanilla-format save: `light_correct`
        // false, so Paper's own re-save writes no `isLightOn`, no `BlockLight`/
        // `SkyLight` arrays, and no Starlight state INTs — its
        // `loadStarlightLightData` installs all-Null nibbles for a
        // `!lightCorrect` chunk, and `copyOf`/`write` emit nothing from Null
        // nibbles. The reconstruction installs the vanilla arrays as Initialised
        // nibbles for the send path (issue #531), but the writer must not leak
        // them onto disk: without a light version the write also must not stamp
        // the Starlight tail.
        let written = write_once(&fixture("-1.-3.nbt"));
        assert!(
            written.get("isLightOn").is_none(),
            "non-light-correct write omits isLightOn"
        );
        assert!(
            written.get(STARLIGHT_VERSION_TAG).is_none(),
            "non-light-correct write omits starlight.light_version"
        );
        let Tag::List(sections) = written.get("sections").unwrap() else {
            panic!("sections is a list");
        };
        let sky_arrays = sections
            .compound_stream()
            .filter(|section| section.get_byte_array("SkyLight").is_some())
            .count();
        assert_eq!(
            sky_arrays, 0,
            "non-light-correct write drops the vanilla SkyLight arrays"
        );
        let block_arrays = sections
            .compound_stream()
            .filter(|section| section.get_byte_array("BlockLight").is_some())
            .count();
        assert_eq!(block_arrays, 0, "non-light-correct write drops BlockLight");
        let state_ints = sections
            .compound_stream()
            .filter(|section| {
                section.get_int(BLOCKLIGHT_STATE_TAG).is_some()
                    || section.get_int(SKYLIGHT_STATE_TAG).is_some()
            })
            .count();
        assert_eq!(
            state_ints, 0,
            "non-light-correct write emits no Starlight state INTs"
        );
        // The block sections themselves survive (block_states are still
        // written), so the write is not lossy beyond Paper's light drop.
        let block_sections = sections
            .compound_stream()
            .filter(|section| section.get_compound("block_states").is_some())
            .count();
        assert!(
            block_sections > 0,
            "block_states survive a non-light-correct write"
        );
    }

    #[test]
    fn vanilla_fixture_write_is_a_fixed_point() {
        // The vanilla-format path must also be a fixed point: the first write
        // drops the light arrays/states, and re-parsing that output must
        // reproduce it structurally (the write adds no Starlight state side
        // effects that a second pass would erase).
        assert_write_idempotent(&fixture("-1.-3.nbt"));
    }

    #[test]
    fn explicit_game_time_overrides_parsed_last_update_at_i64_boundaries() {
        // `copyOf` snapshots LastUpdate from the live level's game time, not
        // from the parsed chunk payload. Exercise both signed i64 endpoints
        // while keeping the parsed value distinct, and verify InhabitedTime
        // still comes from the reconstructed chunk.
        let mut root = fixture("-1.-3.nbt");
        root.put_long("LastUpdate", 0);
        let expected_inhabited_time = root
            .get_long("InhabitedTime")
            .expect("fixture has InhabitedTime");
        assert_eq!(root.get_long("LastUpdate"), Some(0));

        for game_time in [i64::MIN, i64::MAX] {
            assert_ne!(game_time, root.get_long("LastUpdate").unwrap());
            let written = write_once_with_game_time(&root, |_| game_time);
            assert_eq!(
                written.get_long("LastUpdate"),
                Some(game_time),
                "explicit game time wins over parsed LastUpdate"
            );
            assert_eq!(
                written.get_long("InhabitedTime"),
                Some(expected_inhabited_time),
                "InhabitedTime remains sourced from the chunk"
            );
        }
    }

    fn stored_tick(id: &str, x: i32, y: i32, z: i32, delay: i32) -> CompoundTag {
        let mut tick = CompoundTag::new();
        tick.put_string("i", id);
        tick.put_int("x", x);
        tick.put_int("y", y);
        tick.put_int("z", z);
        tick.put_int("t", delay);
        tick.put_int("p", 0);
        tick
    }

    fn put_single_stored_tick(
        root: &mut CompoundTag,
        field: &str,
        id: &str,
        x: i32,
        y: i32,
        z: i32,
        delay: i32,
    ) {
        root.put(
            field.to_string(),
            Tag::List(ListTag::with_list(vec![Tag::Compound(stored_tick(
                id, x, y, z, delay,
            ))])),
        );
    }

    fn written_tick_delay(root: &CompoundTag, field: &str) -> i32 {
        let Tag::List(ticks) = root.get(field).expect("tick list written") else {
            panic!("{field} must be a list");
        };
        let Tag::Compound(tick) = &ticks.list[0] else {
            panic!("{field} entry must be a compound");
        };
        tick.get_int("t").expect("tick delay written")
    }

    #[test]
    fn pending_block_and_fluid_ticks_keep_delays_when_game_time_changes() {
        // Paper's LevelChunkTicks/ProtoChunkTicks preserve pending SavedTicks
        // unchanged. Only queued runtime ticks are unpacked and repacked against
        // the current game time, and reconstruction has not installed those
        // authoritative runtime containers yet.
        let mut root = fixture("-1.-3.nbt");
        let parsed_game_time = 41;
        let current_game_time = 100;
        root.put_long("LastUpdate", parsed_game_time);
        put_single_stored_tick(&mut root, "block_ticks", "minecraft:sand", -1, 64, -33, 7);
        put_single_stored_tick(&mut root, "fluid_ticks", "minecraft:water", -2, 63, -34, 7);

        assert_eq!(root.get_long("LastUpdate"), Some(parsed_game_time));
        let written = write_once_with_game_time(&root, |_| current_game_time);
        assert_eq!(written.get_long("LastUpdate"), Some(current_game_time));
        assert_eq!(written_tick_delay(&written, "block_ticks"), 7);
        assert_eq!(written_tick_delay(&written, "fluid_ticks"), 7);
    }

    #[test]
    fn root_fields_preserve_paper_field_order_and_status() {
        // Field order mirrors `SerializableChunkData.write()` exactly: the
        // DataVersion prefix, position, times, status, sections, `isLightOn`
        // (light-correct), the aux compounds, then the Starlight tail. On this
        // genuine FULL fixture the conditional fields are absent (no
        // blending_data/below_zero_retrogen, empty UpgradeData, no PDC), so the
        // assertion pins the complete fixture-specific sequence — a regression
        // reordering any field would fail the prefix check.
        let lit = make_starlight_lit(fixture("-1.-3.nbt"));
        let written = write_once(&lit);
        let keys: Vec<String> = written.key_set().cloned().collect();
        let expected = [
            "DataVersion",
            "xPos",
            "yPos",
            "zPos",
            "LastUpdate",
            "InhabitedTime",
            "Status",
            "sections",
            "isLightOn",
            "block_entities",
            "block_ticks",
            "fluid_ticks",
            "PostProcessing",
            "Heightmaps",
            "structures",
            "starlight.light_version",
        ];
        assert_eq!(
            keys, expected,
            "root fields follow SerializableChunkData.write() insertion order"
        );
        // `isLightOn` sits at its step-12 slot: the Starlight tail's
        // `putBoolean("isLightOn", false)` updates the existing key in place
        // (Paper's NbtAccounter `put` semantics) rather than re-appending, with
        // `starlight.light_version` appended last.
        assert_eq!(
            written.get_boolean("isLightOn"),
            Some(false),
            "the tail clobber keeps the key at its original slot"
        );
        assert_eq!(
            written.get_string("Status").map(String::as_str),
            Some("minecraft:full")
        );
        assert_eq!(written.get_int("xPos"), Some(-1));
        assert_eq!(written.get_int("zPos"), Some(-3));
        assert_eq!(written.get_int("yPos"), Some(-4), "yPos is minSectionY");
    }

    #[test]
    fn full_write_drops_decodable_below_zero_retrogen() {
        // Paper `read`'s LEVELCHUNK branch never installs `below_zero_retrogen`
        // onto the chunk (only the proto branch does), so `copyOf` reads null
        // and `write` omits it — even when the value decoded. The FULL writer
        // must drop the carried compound, not write it verbatim.
        let mut root = fixture("-1.-3.nbt");
        let mut retrogen = CompoundTag::new();
        retrogen.put_string("target_status", "minecraft:noise");
        root.put("below_zero_retrogen".to_string(), Tag::Compound(retrogen));

        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &root)
            .expect("fixture parses")
            .expect("fixture has a Status");
        assert!(
            data.effective_below_zero_retrogen(),
            "the injected retrogen decodes"
        );
        assert_eq!(data.validate_full_for_reconstruction(), Ok(()));

        let written = write_once(&root);
        assert!(
            written.get("below_zero_retrogen").is_none(),
            "FULL re-save drops a decodable below_zero_retrogen"
        );
    }

    #[test]
    fn full_write_drops_undecodable_retrogen_and_blending_data() {
        // Hostile inputs: an undecodable `below_zero_retrogen` and an
        // undecodable `blending_data` both decode to null on Paper's parse
        // (`CODEC` `orElse(null)`), so `copyOf` snapshots null and `write`
        // omits them — the carried raw compounds must not be written verbatim.
        let mut root = fixture("-1.-3.nbt");
        let mut retrogen = CompoundTag::new();
        retrogen.put_string("target_status", "minecraft:not_a_status");
        root.put("below_zero_retrogen".to_string(), Tag::Compound(retrogen));
        let mut blending = CompoundTag::new();
        blending.put_int("min_section", -4); // missing max_section -> undecodable
        root.put("blending_data".to_string(), Tag::Compound(blending));

        let data = SerializableChunkData::parse(height_accessor::create(-64, 384), &root)
            .expect("fixture parses")
            .expect("fixture has a Status");
        assert!(!data.effective_below_zero_retrogen());
        assert_eq!(data.validate_full_for_reconstruction(), Ok(()));

        let written = write_once(&root);
        assert!(written.get("below_zero_retrogen").is_none());
        assert!(written.get("blending_data").is_none());
    }

    #[test]
    fn missing_wire_heightmap_is_primed_and_written_not_dropped() {
        // Paper's `read` primes a missing heightmap (`toPrime` +
        // `Heightmap.primeHeightmaps`) before `copyOf`, which snapshots the
        // live chunk's complete `heightmapsAfter()` set. Removing one stored
        // key from the wire must produce a re-save that writes the primed
        // value, not a dropped key.
        let original = fixture("-1.-3.nbt");
        let stored_blocking = original
            .get_compound("Heightmaps")
            .and_then(|maps| maps.get_long_array("MOTION_BLOCKING"))
            .cloned()
            .expect("fixture carries MOTION_BLOCKING");
        let mut root = original.clone();
        root.get_compound_or_empty_mut("Heightmaps")
            .remove("MOTION_BLOCKING");

        let written = write_once(&root);
        let written_maps = written
            .get_compound("Heightmaps")
            .expect("Heightmaps written");
        assert!(
            written_maps.contains("MOTION_BLOCKING_NO_LEAVES"),
            "the remaining FINAL types are all still written"
        );
        let blocking = written_maps
            .get_long_array("MOTION_BLOCKING")
            .expect("missing heightmap is primed and written, not dropped");
        assert_eq!(
            blocking.len(),
            stored_blocking.len(),
            "primed MOTION_BLOCKING is a full-length array"
        );
        assert_eq!(
            *blocking, stored_blocking,
            "the primed value matches the stored one Paper computed from the same blocks"
        );
    }

    #[test]
    fn wrong_length_wire_heightmap_is_primed_and_normalized() {
        // A wrong-length stored heightmap triggers Paper's `setRawData` re-prime
        // path (warn + `primeHeightmaps`), so `copyOf` writes the primed
        // full-length value, not the malformed array.
        let original = fixture("-1.-3.nbt");
        let stored_blocking = original
            .get_compound("Heightmaps")
            .and_then(|maps| maps.get_long_array("MOTION_BLOCKING"))
            .cloned()
            .expect("fixture carries MOTION_BLOCKING");
        let mut root = original;
        root.get_compound_or_empty_mut("Heightmaps")
            .put_long_array("MOTION_BLOCKING", vec![7; 1]);

        let written = write_once(&root);
        let blocking = written
            .get_compound("Heightmaps")
            .expect("Heightmaps written")
            .get_long_array("MOTION_BLOCKING")
            .expect("wrong-length heightmap is primed and written");
        assert_eq!(
            blocking.len(),
            stored_blocking.len(),
            "the wrong-length stored array is replaced by a full-length prime"
        );
    }

    #[test]
    fn empty_persistent_data_container_is_not_written() {
        // Paper's `copyOf` nulls the PDC when `persistentDataContainer.isEmpty()`
        // (and `read`'s `putAll` of an empty wire compound leaves it empty), so
        // a present-but-empty `ChunkBukkitValues` is not re-written.
        let mut root = fixture("-1.-3.nbt");
        root.put(
            "ChunkBukkitValues".to_string(),
            Tag::Compound(CompoundTag::new()),
        );
        let written = write_once(&root);
        assert!(written.get("ChunkBukkitValues").is_none());
    }

    #[test]
    fn stored_block_tick_fixture_writes_typed_tick_back() {
        // The `-17.-19.nbt` fixture carries one sand `block_ticks` entry; the
        // write reproduces it through the faithful `SavedTick.codec` list codec
        // (Paper's `saveTicks`).
        let written = write_once(&fixture("-17.-19.nbt"));
        let Tag::List(ticks) = written.get("block_ticks").expect("block_ticks written") else {
            panic!("block_ticks must be a list");
        };
        assert_eq!(ticks.list.len(), 1);
        let Tag::Compound(tick) = &ticks.list[0] else {
            panic!("tick is a compound");
        };
        assert_eq!(
            tick.get_string("i").map(String::as_str),
            Some("minecraft:sand")
        );
        assert_eq!(tick.get_int("x"), Some(-268));
        assert_eq!(tick.get_int("y"), Some(61));
        assert_eq!(tick.get_int("z"), Some(-302));
        assert_eq!(tick.get_int("t"), Some(-59));
    }
}
