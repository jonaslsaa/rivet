//! Current-version `SerializableChunkData.sections` reconstruction (MC 26.2).
//!
//! This is the section-list seam used by the later top-level chunk loader. It
//! reconstructs palettes and carries light fields in the same per-tag pass;
//! block entities, heightmaps, and other `SerializableChunkData` fields remain
//! outside this unit. The ordering mirrors Paper's section loop exactly.

use std::fmt;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::block_state::BlockState;
use rivet_registry::generated::biomes::{BIOME_BY_NAME, BIOME_COUNT};
use rivet_registry::generated::block_states::{BLOCK_STATE_COUNT, StateId};
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::identifier::Identifier;
use rivet_registry::state_definition::StateDefinition;

use crate::chunk::level_chunk_section::LevelChunkSection;
use crate::chunk::palette::GlobalIdMap;
use crate::chunk::paletted_container::{PackedData, PalettedContainer};
use crate::chunk::paletted_container_factory::PalettedContainerFactory;
use crate::chunk::storage::serializable_chunk_data::{SectionLightData, decode_section_light};
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

    fn clone_box(&self) -> Box<dyn GlobalIdMap<BlockState> + Send + Sync> {
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

    fn clone_box(&self) -> Box<dyn GlobalIdMap<BiomeId> + Send + Sync> {
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
    /// Structured location within the container codec input.
    pub path: CodecPath,
    /// Recoverable diagnostics promoted before this fatal codec error.
    pub recoverable_diagnostics: Vec<SectionCodecDiagnostic>,
}

/// Location of a paletted-container codec diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecPath {
    /// The required `palette` field itself.
    Palette,
    /// One recoverable element error within `palette`.
    PaletteElement(usize),
    /// Validation while unpacking the decoded palette and optional `data`.
    PackedData,
}

/// Paper's recoverable `promotePartial(logErrors)` payload, retained for the
/// future top-level chunk logger instead of being discarded after fallback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionCodecDiagnostic {
    pub section_y: i32,
    pub container: &'static str,
    pub path: CodecPath,
    pub message: String,
}

/// The products of Paper's single `sections` loop. Keeping chunk sections,
/// light data, and recoverable diagnostics together prevents later callers
/// from reintroducing whole-list passes with different error ordering.
pub struct SectionReconstruction {
    pub sections: Vec<Option<LevelChunkSection<BlockState, BiomeId>>>,
    pub light_data: Vec<SectionLightData>,
    pub diagnostics: Vec<SectionCodecDiagnostic>,
}

impl std::ops::Deref for SectionReconstruction {
    type Target = [Option<LevelChunkSection<BlockState, BiomeId>>];

    fn deref(&self) -> &Self::Target {
        &self.sections
    }
}

impl fmt::Display for ChunkReadException {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ChunkReadException {}

type PartialDiagnostics = Vec<(usize, String)>;

struct Decoded<T> {
    value: T,
    partials: PartialDiagnostics,
}

struct ContainerCodecError {
    path: CodecPath,
    message: String,
    partials: PartialDiagnostics,
}

fn read_palette<T: Clone>(
    container: &CompoundTag,
    default: T,
    decode: impl Fn(&Tag) -> Result<T, String>,
) -> Result<Decoded<Vec<T>>, ContainerCodecError> {
    // `palette` is `fieldOf`, so it is validated before lenient optional
    // `data`. `orElsePartial(default)` applies to each element independently.
    let palette = match container.get("palette") {
        Some(Tag::List(palette)) => palette,
        Some(tag) => {
            return Err(ContainerCodecError {
                path: CodecPath::Palette,
                message: format!("Not a list: {tag}"),
                partials: Vec::new(),
            });
        }
        None => {
            let entries = NbtOps::instance().map_like(container).entries();
            return Err(ContainerCodecError {
                path: CodecPath::Palette,
                message: format!("No key palette in MapLike[{entries:?}]"),
                partials: Vec::new(),
            });
        }
    };
    let mut values = Vec::with_capacity(palette.size());
    let mut diagnostics = Vec::new();
    for (index, entry) in palette.iter().enumerate() {
        match decode(entry) {
            Ok(value) => values.push(value),
            Err(message) => {
                diagnostics.push((index, format!("({message} -> using default)")));
                values.push(default.clone());
            }
        }
    }
    Ok(Decoded {
        value: values,
        partials: diagnostics,
    })
}

/// Decode one palette element with the same recovery Paper's `BlockState.CODEC`
/// applies (`StateHolder.codec` + `StateDefinition.propertiesCodec`). Only a
/// `Name`-level failure fails the element — a missing, non-string, or unknown
/// `Name` falls through `codecRW(BlockState.CODEC, ..., AIR, ...)`'s
/// `orElsePartial` to the strategy default (air) with a diagnostic. Every
/// property-level malformation instead recovers to the block's default state
/// without a diagnostic, matching `NbtUtils.readBlockState`: the properties
/// codec is a chain over the block's known properties only (unknown keys are
/// ignored), each field is `orElseGet`-wrapped so a missing, invalid, or
/// wrong-typed value recovers to the property's default, and a wrong-typed
/// `Properties` itself is swallowed by `lenientOptionalFieldOf("Properties")`.
fn decode_block_state(entry: &Tag) -> Result<BlockState, String> {
    let Tag::Compound(state) = entry else {
        return Err(format!("Not a map: {entry}"));
    };
    let name_tag = state
        .get("Name")
        .ok_or_else(|| format!("No key Name in MapLike[{state:?}]"))?;
    let Tag::String(name) = name_tag else {
        return Err(format!("Not a string: {name_tag}"));
    };
    let id = Identifier::by_separator_result(&name.value, ':').map_err(|error| {
        // Paper `Identifier.read` errors with `Not a valid resource location:
        // <input> <escaped message>`; that text flows through `orElsePartial`
        // into the element diagnostic and any composed fatal message.
        format!(
            "Not a valid resource location: {} {}",
            name.value,
            error.message()
        )
    })?;
    let block = BlockId::from_name(&id.to_string()).ok_or_else(|| {
        format!("Unknown registry key in ResourceKey[minecraft:root / minecraft:block]: {id}")
    })?;

    let mut result = BlockState::of(block);
    let Some(properties_tag) = state.get("Properties") else {
        return Ok(result);
    };
    let Tag::Compound(properties) = properties_tag else {
        return Ok(result);
    };
    let definition = StateDefinition::for_block(block);
    // Iterate the block's known properties, not the input's keys: Paper's
    // properties codec decodes only fields present in `propertiesByName`, so
    // unknown keys are ignored.
    for property in definition.properties() {
        let Some(value_tag) = properties.get(property.name()) else {
            continue;
        };
        let Tag::String(value) = value_tag else {
            continue;
        };
        let Some(parsed) = property.get_value(&value.value) else {
            continue;
        };
        let Some(index) = definition.value_index(property, parsed) else {
            continue;
        };
        result = result
            .set_property(property.id(), index)
            .expect("validated block-state property is settable");
    }
    Ok(result)
}

fn decode_block_states(
    container: &CompoundTag,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
    preset_values: Option<Vec<BlockState>>,
) -> Result<Decoded<PalettedContainer<BlockState>>, ContainerCodecError> {
    let decoded_palette = read_palette(
        container,
        *factory.default_block_state(),
        decode_block_state,
    )?;
    // `lenientOptionalFieldOf`: a missing or wrong-typed `data` is `None`.
    let packed = PackedData::new(
        decoded_palette.value,
        container.get_long_array("data").cloned(),
    );
    let decoded = match preset_values {
        Some(presets) => PalettedContainer::unpack_with_preset_values(
            factory.block_states_strategy(),
            packed,
            *factory.default_block_state(),
            Some(presets),
        ),
        None => PalettedContainer::unpack(factory.block_states_strategy(), packed),
    }
    .map_err(|message| ContainerCodecError {
        path: CodecPath::PackedData,
        message,
        partials: decoded_palette.partials.clone(),
    })?;
    Ok(Decoded {
        value: decoded,
        partials: decoded_palette.partials,
    })
}

fn decode_biomes(
    container: &CompoundTag,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
) -> Result<Decoded<PalettedContainer<BiomeId>>, ContainerCodecError> {
    let decoded_palette = read_palette(container, *factory.default_biome(), |entry| {
        let Tag::String(name) = entry else {
            return Err(format!("Not a string: {entry}"));
        };
        let id = Identifier::by_separator_result(&name.value, ':').map_err(|error| {
            format!(
                "Not a valid resource location: {} {}",
                name.value,
                error.message()
            )
        })?;
        BIOME_BY_NAME
            .get(id.to_string().as_str())
            .copied()
            .map(BiomeId)
            .ok_or_else(|| {
                format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/biome]: {id}"
                )
            })
    })?;
    let packed = PackedData::new(
        decoded_palette.value,
        container.get_long_array("data").cloned(),
    );
    let decoded =
        PalettedContainer::unpack(factory.biome_strategy(), packed).map_err(|message| {
            ContainerCodecError {
                path: CodecPath::PackedData,
                message,
                partials: decoded_palette.partials.clone(),
            }
        })?;
    Ok(Decoded {
        value: decoded,
        partials: decoded_palette.partials,
    })
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
) -> Result<SectionReconstruction, ChunkReadException> {
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
    preset_values: impl FnMut(i32) -> Option<Vec<BlockState>>,
) -> Result<SectionReconstruction, ChunkReadException> {
    reconstruct_sections_with_presets_and_diagnostics(
        section_tags,
        min_section_y,
        max_section_y,
        factory,
        predicates,
        preset_values,
        |_| {},
    )
}

/// As [`reconstruct_sections_with_presets`], while delivering each recoverable
/// palette diagnostic at Paper's `promotePartial(logErrors)` point. The result
/// also retains the diagnostics; the callback matters when a later light-array
/// validation panics before a result can be returned.
pub fn reconstruct_sections_with_presets_and_diagnostics(
    section_tags: &ListTag,
    min_section_y: i32,
    max_section_y: i32,
    factory: &PalettedContainerFactory<BlockState, BiomeId>,
    predicates: SectionBlockPredicates,
    mut preset_values: impl FnMut(i32) -> Option<Vec<BlockState>>,
    mut on_diagnostic: impl FnMut(&SectionCodecDiagnostic),
) -> Result<SectionReconstruction, ChunkReadException> {
    let section_count = if max_section_y < min_section_y {
        0
    } else {
        max_section_y.wrapping_sub(min_section_y).wrapping_add(1) as usize
    };
    let mut sections: Vec<Option<LevelChunkSection<BlockState, BiomeId>>> =
        (0..section_count).map(|_| None).collect();
    let mut light_data = Vec::with_capacity(section_tags.size());
    let mut diagnostics = Vec::new();

    for section_tag in section_tags.compound_stream() {
        let y = section_tag.get_byte_or("Y", 0) as i32;
        if y >= min_section_y && y <= max_section_y {
            // Paper selects presets, decodes blocks, then biomes before this
            // same tag's BlockLight and SkyLight validation.
            let presets = preset_values(y);
            let blocks = match section_tag.get_compound("block_states") {
                Some(container) => {
                    let decoded = match decode_block_states(container, factory, presets) {
                        Ok(decoded) => decoded,
                        Err(error) => {
                            let message = compose_fatal_message(&error.partials, &error.message);
                            record_diagnostics(
                                &mut diagnostics,
                                &mut on_diagnostic,
                                y,
                                "block_states",
                                error.partials,
                            );
                            return Err(ChunkReadException {
                                section_y: y,
                                container: "block_states",
                                message,
                                path: error.path,
                                recoverable_diagnostics: diagnostics,
                            });
                        }
                    };
                    record_diagnostics(
                        &mut diagnostics,
                        &mut on_diagnostic,
                        y,
                        "block_states",
                        decoded.partials,
                    );
                    decoded.value
                }
                None => factory.create_for_block_states(),
            };
            let biomes = match section_tag.get_compound("biomes") {
                Some(container) => {
                    let decoded = match decode_biomes(container, factory) {
                        Ok(decoded) => decoded,
                        Err(error) => {
                            let message = compose_fatal_message(&error.partials, &error.message);
                            record_diagnostics(
                                &mut diagnostics,
                                &mut on_diagnostic,
                                y,
                                "biomes",
                                error.partials,
                            );
                            return Err(ChunkReadException {
                                section_y: y,
                                container: "biomes",
                                message,
                                path: error.path,
                                recoverable_diagnostics: diagnostics,
                            });
                        }
                    };
                    record_diagnostics(
                        &mut diagnostics,
                        &mut on_diagnostic,
                        y,
                        "biomes",
                        decoded.partials,
                    );
                    decoded.value
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

        light_data
            .push(decode_section_light(section_tag).unwrap_or_else(|error| panic!("{error}")));
    }

    Ok(SectionReconstruction {
        sections,
        light_data,
        diagnostics,
    })
}

fn record_diagnostics(
    diagnostics: &mut Vec<SectionCodecDiagnostic>,
    on_diagnostic: &mut impl FnMut(&SectionCodecDiagnostic),
    section_y: i32,
    container: &'static str,
    partials: PartialDiagnostics,
) {
    for (index, message) in partials {
        let diagnostic = SectionCodecDiagnostic {
            section_y,
            container,
            path: CodecPath::PaletteElement(index),
            message,
        };
        on_diagnostic(&diagnostic);
        diagnostics.push(diagnostic);
    }
}

/// Paper/DFU's fatal message after `promotePartial`/`getOrThrow`: palette-element
/// partial errors compose with the fatal codec error via
/// `DataResult.appendMessages(first, second)` = `first + "; " + second`, but the
/// list decoder accumulates them by *prepending* each new element error
/// (`Error.ap` on the accumulator). Empirically verified against the pinned
/// Paper 26.2 + DFU 10.0.21 jars: with a palette `[a, b, c]` whose elements all
/// fail and a fatal unpack, the message is `Err[c]; Err[b]; Err[a]; Fatal` —
/// the failing elements appear in reverse decode order, then the fatal error.
/// `ChunkReadException` is constructed from that combined `message()`, so the
/// fatal packed-data error never drops the preceding element diagnostics. Empty
/// partials (a palette-field failure, or a packed-data failure with a clean
/// palette) leave the fatal message unchanged.
fn compose_fatal_message(partials: &[(usize, String)], fatal: &str) -> String {
    if partials.is_empty() {
        return fatal.to_string();
    }
    let mut message = String::new();
    for (_, partial) in partials.iter().rev() {
        if !message.is_empty() {
            message.push_str("; ");
        }
        message.push_str(partial);
    }
    message.push_str("; ");
    message.push_str(fatal);
    message
}
