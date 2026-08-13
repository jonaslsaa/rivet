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
    fn write_once(root: &CompoundTag) -> CompoundTag {
        let height = height_accessor::create(-64, 384);
        let data = SerializableChunkData::parse(height, root)
            .expect("fixture parses")
            .expect("fixture has a Status");
        assert_eq!(data.validate_full_for_reconstruction(), Ok(()));
        let for_reconstruction = SerializableChunkData::parse(height, root)
            .expect("fixture parses")
            .expect("fixture has a Status");
        let reconstruction =
            reconstruct_runtime_chunk(data.stored_pos(), for_reconstruction, height, true)
                .expect("fixture reconstructs");
        write(&data, &reconstruction.chunk)
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
}
