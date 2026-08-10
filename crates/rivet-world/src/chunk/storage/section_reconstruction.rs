//! Current-version `SerializableChunkData.sections` reconstruction (MC 26.2).
//!
//! This is the section-only seam used by the later top-level chunk loader. It
//! deliberately does not parse light, block entities, heightmaps, or any other
//! `SerializableChunkData` field. The ordering here mirrors Paper's section
//! loop: bounds are checked before either container is decoded, block states
//! are decoded before biomes, and each missing container gets its factory
//! default independently.

use std::fmt;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_utils::read_block_state;
use rivet_nbt::tag::Tag;
use rivet_registry::block_state::BlockState;
use rivet_registry::generated::biomes::{BIOME_BY_NAME, BIOME_COUNT};
use rivet_registry::generated::block_states::{BLOCK_STATE_COUNT, StateId};
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::identifier::Identifier;

use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::palette::GlobalIdMap;
use crate::chunk::paletted_container::{PackedData, PalettedContainer};
use crate::chunk::paletted_container_factory::PalettedContainerFactory;
use crate::chunk::strategy::Strategy;

/// Dense id into the current vanilla biome registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BiomeId(pub u16);

impl BiomeId {
    /// `Biomes.PLAINS` in the pinned generated registry.
    pub const PLAINS: Self = Self(40);
}

#[derive(Clone, Copy)]
struct BlockStateGlobalMap;

impl GlobalIdMap<BlockState> for BlockStateGlobalMap {
    fn get_id(&self, value: &BlockState) -> i32 {
        value.id().0 as i32
    }

    fn by_id_or_throw(&self, id: i32) -> BlockState {
        self.by_id(id)
            .unwrap_or_else(|| panic!("No value with id {id}"))
    }

    fn size(&self) -> i32 {
        BLOCK_STATE_COUNT as i32
    }

    fn by_id(&self, id: i32) -> Option<BlockState> {
        (0..BLOCK_STATE_COUNT as i32)
            .contains(&id)
            .then_some(BlockState::new(StateId(id as u16)))
    }

    fn clone_box(&self) -> Box<dyn GlobalIdMap<BlockState>> {
        Box::new(*self)
    }
}

#[derive(Clone, Copy)]
struct BiomeGlobalMap;

impl GlobalIdMap<BiomeId> for BiomeGlobalMap {
    fn get_id(&self, value: &BiomeId) -> i32 {
        value.0 as i32
    }

    fn by_id_or_throw(&self, id: i32) -> BiomeId {
        self.by_id(id)
            .unwrap_or_else(|| panic!("No value with id {id}"))
    }

    fn size(&self) -> i32 {
        BIOME_COUNT as i32
    }

    fn by_id(&self, id: i32) -> Option<BiomeId> {
        (0..BIOME_COUNT as i32)
            .contains(&id)
            .then_some(BiomeId(id as u16))
    }

    fn clone_box(&self) -> Box<dyn GlobalIdMap<BiomeId>> {
        Box::new(*self)
    }
}

/// Build the registry-derived factory used by Paper 26.2's section codecs.
pub fn current_version_container_factory() -> PalettedContainerFactory<BlockState, BiomeId> {
    let air = BlockState::of(
        BlockId::from_name("minecraft:air").expect("air is in the generated block registry"),
    );
    PalettedContainerFactory::new(
        Strategy::create_for_block_states(Box::new(BlockStateGlobalMap)),
        air,
        Strategy::create_for_biomes(Box::new(BiomeGlobalMap)),
        BiomeId::PLAINS,
    )
}

/// The five `BlockState` predicates consumed by #216's
/// `LevelChunkSection::recalcBlockCounts` port.
///
/// They are passed in rather than obtained from a `Level`, so stored sections
/// can be reconstructed before world boot. Keeping all five explicit also
/// avoids silently approximating fluid random ticks or Moonrise's large-shape
/// collision predicate.
#[derive(Clone, Copy)]
pub struct SectionBlockPredicates {
    /// `BlockState.isAir()`.
    pub is_air: fn(&BlockState) -> bool,
    /// `BlockState.isRandomlyTicking()`.
    pub is_randomly_ticking: fn(&BlockState) -> bool,
    /// `BlockState.getFluidState().isEmpty()`.
    pub fluid_is_empty: fn(&BlockState) -> bool,
    /// `BlockState.getFluidState().isRandomlyTicking()`.
    pub fluid_is_randomly_ticking: fn(&BlockState) -> bool,
    /// Moonrise `CollisionUtil.isSpecialCollidingBlock(BlockState)`.
    pub is_special_colliding: fn(&BlockState) -> bool,
}

/// Paper's `SerializableChunkData.ChunkReadException`, with the section and
/// container retained for the caller's equivalent of `logErrors`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkReadException {
    /// The decoded section Y used by Paper's recoverable-error log.
    pub section_y: i32,
    /// The container whose codec failed (`block_states` or `biomes`).
    pub container: &'static str,
    /// The underlying paletted-container codec error.
    pub message: String,
}

impl fmt::Display for ChunkReadException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ChunkReadException {}

fn read_palette<T: Clone>(
    container: &CompoundTag,
    default: T,
    decode: impl Fn(&Tag) -> Option<T>,
) -> Result<Vec<T>, String> {
    // `palette` is `fieldOf`, so it is validated before lenient optional
    // `data`. `orElsePartial(default)` applies to each element independently.
    let palette = container
        .get_list("palette")
        .ok_or_else(|| "Missing required field: palette".to_string())?;
    Ok(palette
        .iter()
        .map(|entry| decode(entry).unwrap_or_else(|| default.clone()))
        .collect())
}

fn decode_block_states(
    container: &CompoundTag,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
    preset_values: Option<Vec<BlockState>>,
) -> Result<PalettedContainer<BlockState>, String> {
    let palette_entries = read_palette(container, *factory.default_block_state(), |entry| {
        let Tag::Compound(state) = entry else {
            return None;
        };
        Some(read_block_state(state))
    })?;
    // `lenientOptionalFieldOf`: a missing or wrong-typed `data` is `None`.
    let packed = PackedData::new(palette_entries, container.get_long_array("data").cloned());
    match preset_values {
        Some(presets) => PalettedContainer::unpack_with_preset_values(
            factory.block_states_strategy(),
            packed,
            *factory.default_block_state(),
            Some(presets),
        ),
        None => PalettedContainer::unpack(factory.block_states_strategy(), packed),
    }
}

fn decode_biomes(
    container: &CompoundTag,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
) -> Result<PalettedContainer<BiomeId>, String> {
    let palette_entries = read_palette(container, *factory.default_biome(), |entry| {
        let Tag::String(name) = entry else {
            return None;
        };
        let id = Identifier::by_separator_result(&name.value, ':').ok()?;
        BIOME_BY_NAME
            .get(id.to_string().as_str())
            .copied()
            .map(BiomeId)
    })?;
    let packed = PackedData::new(palette_entries, container.get_long_array("data").cloned());
    PalettedContainer::unpack(factory.biome_strategy(), packed)
}

/// Reconstruct the in-bounds block-section array from a current-version
/// `sections` list. The returned index is exactly `sectionY - minSectionY`;
/// absent entries remain `None` for the later `replaceMissingSections` step.
pub fn reconstruct_sections(
    section_tags: &ListTag,
    min_section_y: i32,
    max_section_y: i32,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
    predicates: SectionBlockPredicates,
) -> Result<Vec<Option<LevelChunkSection<BlockState, BiomeId>>>, ChunkReadException> {
    reconstruct_sections_with_presets(
        section_tags,
        min_section_y,
        max_section_y,
        factory,
        predicates,
        |_| None,
    )
}

/// As [`reconstruct_sections`], with Paper Anti-Xray preset values selected per
/// in-bounds section Y. The callback is deliberately not evaluated for an
/// out-of-bounds or non-compound entry, matching Paper's parse loop.
pub fn reconstruct_sections_with_presets(
    section_tags: &ListTag,
    min_section_y: i32,
    max_section_y: i32,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
    predicates: SectionBlockPredicates,
    mut preset_values: impl FnMut(i32) -> Option<Vec<BlockState>>,
) -> Result<Vec<Option<LevelChunkSection<BlockState, BiomeId>>>, ChunkReadException> {
    let section_count = if max_section_y < min_section_y {
        0
    } else {
        max_section_y.wrapping_sub(min_section_y).wrapping_add(1) as usize
    };
    let mut sections: Vec<Option<LevelChunkSection<BlockState, BiomeId>>> =
        (0..section_count).map(|_| None).collect();

    for section_tag in section_tags.compound_stream() {
        let y = section_tag.get_byte_or("Y", 0) as i32;
        if y < min_section_y || y > max_section_y {
            continue;
        }

        // Keep Java's evaluation/default/error order exactly: blocks first,
        // then biomes, then `LevelChunkSection` count reconstruction.
        // Paper selects Anti-Xray presets before checking whether the
        // `block_states` compound exists; the missing fallback does not use
        // them, but the callback ordering remains observable.
        let presets = preset_values(y);
        let blocks = match section_tag.get_compound("block_states") {
            Some(container) => {
                decode_block_states(container, factory, presets).map_err(|message| {
                    ChunkReadException {
                        section_y: y,
                        container: "block_states",
                        message,
                    }
                })?
            }
            None => factory.create_for_block_states(),
        };
        let biomes = match section_tag.get_compound("biomes") {
            Some(container) => {
                decode_biomes(container, factory).map_err(|message| ChunkReadException {
                    section_y: y,
                    container: "biomes",
                    message,
                })?
            }
            None => factory.create_for_biomes(),
        };
        let section = LevelChunkSection::new(
            blocks,
            biomes,
            predicates.is_air,
            predicates.is_randomly_ticking,
            predicates.fluid_is_empty,
            predicates.fluid_is_randomly_ticking,
            predicates.is_special_colliding,
        );
        sections[y.wrapping_sub(min_section_y) as usize] = Some(section);
    }

    Ok(sections)
}
