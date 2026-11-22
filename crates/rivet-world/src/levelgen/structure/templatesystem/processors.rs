//! Built-in Paper 26.2 `StructureProcessor` implementations:
//! - `NopProcessor` (`minecraft:nop`)
//! - `BlockIgnoreProcessor` (`minecraft:block_ignore`)
//! - `BlockRotProcessor` (`minecraft:block_rot`)
//! - `ProtectedBlockProcessor` (`minecraft:protected_blocks`)
//! - `RuleProcessor` (`minecraft:rule`)
//! - `GravityProcessor` (`minecraft:gravity`)
//! - `JigsawReplacementProcessor` (`minecraft:jigsaw_replacement`)
//! - `LavaSubmergedBlockProcessor` (`minecraft:lava_submerged_block`)
//! - `BlackstoneReplaceProcessor` (`minecraft:blackstone_replace`)
//! - `BlockAgeProcessor` (`minecraft:block_age`)
//! - `CappedProcessor` (`minecraft:capped`)

use super::processor_rule::{ProcessorRule, processor_rule_codec};
use super::structure_place_settings::StructurePlaceSettings;
use super::structure_processor::{
    ErasedStructureProcessor, StructureProcessor, StructureProcessorTypeId,
    StructureProcessorTypes, erased_process_block,
};
use super::structure_template::StructureBlockInfo;
use crate::level::world_gen_level::WorldGenLevel;
use crate::levelgen::heightmap::{Types as HeightmapType, types_codec as heightmap_types_codec};
use indexmap::IndexMap;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_codec::block_state_codec;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::BlockPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::identifier::Identifier;
use rivet_registry::registries::{self, BlockType};
use rivet_registry::registry_file_codec::{HolderSetCodec, RegistryFixedCodec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::state_definition::StateDefinition;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::mth;
use rivet_util::random::{LegacyRandomSource, PositionalRandomFactory};
use rivet_util::valueproviders::int_provider::{IntProvider, positive_int_provider_codec};
use std::any::Any;
use std::sync::Arc;

fn block_id(name: &str) -> BlockId {
    BlockId::from_name(name).unwrap_or_else(|| panic!("unknown block {name}"))
}

fn default_state(name: &str) -> BlockState {
    BlockState::of(block_id(name))
}

/// `Block.withPropertiesOf(BlockState)` — copy every shared property value from
/// `source` onto `target_block`'s default state.
fn with_properties_of(target_block: BlockId, source: BlockState) -> BlockState {
    let mut result = BlockState::of(target_block);
    for prop in StateDefinition::for_block(source.block()).properties() {
        if let Some(val) = source.get_value(prop)
            && let Ok(next) = result.try_set_value(prop, val)
        {
            result = next;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// 1. NopProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.NopProcessor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NopProcessor;

impl StructureProcessor for NopProcessor {
    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::NOP
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn nop_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<NopProcessor, Ops>> {
    map_codec::unit(NopProcessor)
}

// ---------------------------------------------------------------------------
// 2. BlockIgnoreProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.BlockIgnoreProcessor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockIgnoreProcessor {
    to_ignore: Vec<BlockId>,
}

impl BlockIgnoreProcessor {
    pub fn new(blocks: Vec<BlockId>) -> Self {
        Self { to_ignore: blocks }
    }

    pub fn from_states(states: Vec<BlockState>) -> Self {
        Self {
            to_ignore: states.into_iter().map(|s| s.block()).collect(),
        }
    }

    pub fn structure_block() -> Self {
        Self::new(vec![block_id("minecraft:structure_block")])
    }

    pub fn air() -> Self {
        Self::new(vec![block_id("minecraft:air")])
    }

    pub fn structure_and_air() -> Self {
        Self::new(vec![
            block_id("minecraft:air"),
            block_id("minecraft:structure_block"),
        ])
    }

    pub fn to_ignore(&self) -> &[BlockId] {
        &self.to_ignore
    }
}

impl StructureProcessor for BlockIgnoreProcessor {
    fn process_block(
        &self,
        _level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        if self.to_ignore.contains(&processed_block_info.state.block()) {
            None
        } else {
            Some(processed_block_info)
        }
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::BLOCK_IGNORE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn block_ignore_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BlockIgnoreProcessor, Ops>> {
    let block_list_codec = codec::list(codec::xmap(
        block_state_codec::<Ops>(),
        Arc::new(|s: &BlockState| s.block()),
        Arc::new(|id: &BlockId| BlockState::of(*id)),
    ));
    map_codec::xmap(
        codec::field_of(block_list_codec, "blocks".to_string()),
        Arc::new(|blocks: &Vec<BlockId>| BlockIgnoreProcessor::new(blocks.clone())),
        Arc::new(|p: &BlockIgnoreProcessor| p.to_ignore.clone()),
    )
}

// ---------------------------------------------------------------------------
// 3. BlockRotProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.BlockRotProcessor`.
#[derive(Clone, Debug)]
pub struct BlockRotProcessor {
    rottable_blocks: Option<HolderSet<BlockType>>,
    integrity: f32,
}

impl BlockRotProcessor {
    pub fn new(integrity: f32) -> Self {
        Self {
            rottable_blocks: None,
            integrity,
        }
    }

    pub fn with_rottable_blocks(
        rottable_blocks: Option<HolderSet<BlockType>>,
        integrity: f32,
    ) -> Self {
        Self {
            rottable_blocks,
            integrity,
        }
    }

    pub fn integrity(&self) -> f32 {
        self.integrity
    }

    pub fn rottable_blocks(&self) -> Option<&HolderSet<BlockType>> {
        self.rottable_blocks.as_ref()
    }
}

impl StructureProcessor for BlockRotProcessor {
    fn process_block(
        &self,
        _level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        if let Some(rottable) = &self.rottable_blocks
            && !rottable.contains_id(processed_block_info.state.block().id() as u32)
        {
            return Some(processed_block_info);
        }
        let mut random = settings.get_random(Some(processed_block_info.pos));
        if self.integrity < 1.0 && random.next_float() > self.integrity {
            None
        } else {
            Some(processed_block_info)
        }
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::BLOCK_ROT
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn block_holder_set_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<rivet_registry::holder::Holder<BlockType>, Ops>> =
        Arc::new(RegistryFixedCodec::create(&registries::BLOCK));
    #[allow(clippy::arc_with_non_send_sync)]
    Arc::new(HolderSetCodec::create(&registries::BLOCK, element, false))
}

pub fn block_rot_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<BlockRotProcessor, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &BlockRotProcessor| p.rottable_blocks.clone()),
                codec::optional_field(
                    "rottable_blocks".to_string(),
                    block_holder_set_codec::<Ops>(),
                    false,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &BlockRotProcessor| p.integrity),
                codec::field_of(codec::float_range::<Ops>(0.0, 1.0), "integrity".to_string()),
            ))
            .apply(
                instance,
                Arc::new(|rottable, integrity| {
                    BlockRotProcessor::with_rottable_blocks(rottable, integrity)
                }),
            )
    })
}

// ---------------------------------------------------------------------------
// 4. ProtectedBlockProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.ProtectedBlockProcessor`.
#[derive(Clone, Debug)]
pub struct ProtectedBlockProcessor {
    cannot_replace: HolderSet<BlockType>,
}

impl ProtectedBlockProcessor {
    pub fn new(cannot_replace: HolderSet<BlockType>) -> Self {
        Self { cannot_replace }
    }

    pub fn cannot_replace(&self) -> &HolderSet<BlockType> {
        &self.cannot_replace
    }
}

impl StructureProcessor for ProtectedBlockProcessor {
    fn process_block(
        &self,
        level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        let existing = level.get_block_state(&processed_block_info.pos);
        if self
            .cannot_replace
            .contains_id(existing.block().id() as u32)
        {
            None
        } else {
            Some(processed_block_info)
        }
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::PROTECTED_BLOCKS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn protected_block_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<ProtectedBlockProcessor, Ops>> {
    map_codec::xmap(
        codec::field_of(block_holder_set_codec::<Ops>(), "value".to_string()),
        Arc::new(|hs: &HolderSet<BlockType>| ProtectedBlockProcessor::new(hs.clone())),
        Arc::new(|p: &ProtectedBlockProcessor| p.cannot_replace.clone()),
    )
}

// ---------------------------------------------------------------------------
// 5. RuleProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.RuleProcessor`.
#[derive(Clone, Debug)]
pub struct RuleProcessor {
    rules: Vec<ProcessorRule>,
}

impl RuleProcessor {
    pub fn new(rules: Vec<ProcessorRule>) -> Self {
        Self { rules }
    }

    pub fn rules(&self) -> &[ProcessorRule] {
        &self.rules
    }
}

impl StructureProcessor for RuleProcessor {
    fn process_block(
        &self,
        level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        reference_pos: BlockPos,
        template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        let mut random = LegacyRandomSource::new(mth::get_seed(
            processed_block_info.pos.get_x(),
            processed_block_info.pos.get_y(),
            processed_block_info.pos.get_z(),
        ));
        for rule in &self.rules {
            if rule.test(
                level,
                &processed_block_info.state,
                &template_relative_pos,
                &processed_block_info.pos,
                &reference_pos,
                &mut random,
            ) {
                let output_nbt =
                    rule.get_output_tag(&mut random, processed_block_info.nbt.as_ref());
                return Some(StructureBlockInfo::new(
                    processed_block_info.pos,
                    rule.get_output_state(),
                    output_nbt,
                ));
            }
        }
        Some(processed_block_info)
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::RULE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn rule_processor_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<RuleProcessor, Ops>>
{
    map_codec::xmap(
        codec::field_of(
            codec::list(processor_rule_codec::<Ops>()),
            "rules".to_string(),
        ),
        Arc::new(|rules: &Vec<ProcessorRule>| RuleProcessor::new(rules.clone())),
        Arc::new(|p: &RuleProcessor| p.rules.clone()),
    )
}

// ---------------------------------------------------------------------------
// 6. GravityProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.GravityProcessor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GravityProcessor {
    heightmap: HeightmapType,
    offset: i32,
}

impl GravityProcessor {
    pub fn new(heightmap: HeightmapType, offset: i32) -> Self {
        Self { heightmap, offset }
    }

    pub fn heightmap(&self) -> HeightmapType {
        self.heightmap
    }

    pub fn offset(&self) -> i32 {
        self.offset
    }
}

impl StructureProcessor for GravityProcessor {
    fn process_block(
        &self,
        level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        let pos = processed_block_info.pos;
        let height = level
            .get_height_at(self.heightmap, pos.get_x(), pos.get_z())
            .wrapping_add(self.offset);
        let delta = template_relative_pos.get_y();
        Some(StructureBlockInfo::new(
            BlockPos::new(pos.get_x(), height.wrapping_add(delta), pos.get_z()),
            processed_block_info.state,
            processed_block_info.nbt,
        ))
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::GRAVITY
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn gravity_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<GravityProcessor, Ops>> {
    let hm_field = map_codec::xmap(
        codec::optional_field(
            "heightmap".to_string(),
            Arc::new(heightmap_types_codec::<Ops>()),
            false,
        ),
        Arc::new(|opt: &Option<HeightmapType>| opt.unwrap_or(HeightmapType::WorldSurfaceWg)),
        Arc::new(|v: &HeightmapType| {
            if *v == HeightmapType::WorldSurfaceWg {
                None
            } else {
                Some(*v)
            }
        }),
    );
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|p: &GravityProcessor| p.heightmap),
                hm_field,
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|p: &GravityProcessor| p.offset),
                codec::optional_field_of("offset", codec::int_codec::<Ops>(), 0),
            ))
            .apply(instance, Arc::new(GravityProcessor::new))
    })
}

// ---------------------------------------------------------------------------
// 7. JigsawReplacementProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.JigsawReplacementProcessor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JigsawReplacementProcessor;

impl StructureProcessor for JigsawReplacementProcessor {
    fn process_block(
        &self,
        _level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        if processed_block_info.state.block() != block_id("minecraft:jigsaw") {
            return Some(processed_block_info);
        }
        let Some(nbt) = &processed_block_info.nbt else {
            return Some(processed_block_info);
        };
        let state_string = nbt.get_string_or("final_state", "minecraft:air");
        // Parse base block identifier (strip optional `[props]` suffix until full
        // BlockStateParser is ported).
        let block_name = state_string.split('[').next().unwrap_or(state_string);
        let id = Identifier::try_parse(block_name)?;
        let block = BlockId::from_name(&id.to_string())?;
        let state = BlockState::of(block);
        if state.block() == block_id("minecraft:structure_void") {
            None
        } else {
            Some(StructureBlockInfo::new(
                processed_block_info.pos,
                state,
                None,
            ))
        }
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::JIGSAW_REPLACEMENT
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn jigsaw_replacement_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<JigsawReplacementProcessor, Ops>> {
    map_codec::unit(JigsawReplacementProcessor)
}

// ---------------------------------------------------------------------------
// 8. LavaSubmergedBlockProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.LavaSubmergedBlockProcessor`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LavaSubmergedBlockProcessor;

impl StructureProcessor for LavaSubmergedBlockProcessor {
    fn process_block(
        &self,
        level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        let pos = processed_block_info.pos;
        let lava_id = block_id("minecraft:lava");
        let was_lava_before = level.get_block_state(&pos).block() == lava_id;
        // Paper: !Block.isShapeFullBlock(state.getShape(level, pos)).
        // All 6 faces full (0b0011_1111) identifies full-cube shapes (e.g. stone, glass, double
        // slabs) and excludes partial blocks such as stairs, single slabs, walls, and air.
        const ALL_SIX_FACES_FULL: u8 = 0b0011_1111;
        let is_full_block = processed_block_info.state.face_sturdy_mask() == ALL_SIX_FACES_FULL;
        if was_lava_before && !is_full_block {
            Some(StructureBlockInfo::new(
                pos,
                BlockState::of(lava_id),
                processed_block_info.nbt,
            ))
        } else {
            Some(processed_block_info)
        }
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::LAVA_SUBMERGED_BLOCK
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn lava_submerged_block_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<LavaSubmergedBlockProcessor, Ops>> {
    map_codec::unit(LavaSubmergedBlockProcessor)
}

// ---------------------------------------------------------------------------
// 9. BlackstoneReplaceProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.BlackstoneReplaceProcessor`.
#[derive(Clone, Debug)]
pub struct BlackstoneReplaceProcessor {
    replacements: IndexMap<BlockId, BlockId>,
}

impl Default for BlackstoneReplaceProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl BlackstoneReplaceProcessor {
    pub fn new() -> Self {
        let mut replacements = IndexMap::new();
        let pairs = [
            ("minecraft:cobblestone", "minecraft:blackstone"),
            ("minecraft:mossy_cobblestone", "minecraft:blackstone"),
            ("minecraft:stone", "minecraft:polished_blackstone"),
            (
                "minecraft:stone_bricks",
                "minecraft:polished_blackstone_bricks",
            ),
            (
                "minecraft:mossy_stone_bricks",
                "minecraft:polished_blackstone_bricks",
            ),
            (
                "minecraft:cobblestone_stairs",
                "minecraft:blackstone_stairs",
            ),
            (
                "minecraft:mossy_cobblestone_stairs",
                "minecraft:blackstone_stairs",
            ),
            (
                "minecraft:stone_stairs",
                "minecraft:polished_blackstone_stairs",
            ),
            (
                "minecraft:stone_brick_stairs",
                "minecraft:polished_blackstone_brick_stairs",
            ),
            (
                "minecraft:mossy_stone_brick_stairs",
                "minecraft:polished_blackstone_brick_stairs",
            ),
            ("minecraft:cobblestone_slab", "minecraft:blackstone_slab"),
            (
                "minecraft:mossy_cobblestone_slab",
                "minecraft:blackstone_slab",
            ),
            (
                "minecraft:smooth_stone_slab",
                "minecraft:polished_blackstone_slab",
            ),
            ("minecraft:stone_slab", "minecraft:polished_blackstone_slab"),
            (
                "minecraft:stone_brick_slab",
                "minecraft:polished_blackstone_brick_slab",
            ),
            (
                "minecraft:mossy_stone_brick_slab",
                "minecraft:polished_blackstone_brick_slab",
            ),
            (
                "minecraft:stone_brick_wall",
                "minecraft:polished_blackstone_brick_wall",
            ),
            (
                "minecraft:mossy_stone_brick_wall",
                "minecraft:polished_blackstone_brick_wall",
            ),
            ("minecraft:cobblestone_wall", "minecraft:blackstone_wall"),
            (
                "minecraft:mossy_cobblestone_wall",
                "minecraft:blackstone_wall",
            ),
            (
                "minecraft:chiseled_stone_bricks",
                "minecraft:chiseled_polished_blackstone",
            ),
            (
                "minecraft:cracked_stone_bricks",
                "minecraft:cracked_polished_blackstone_bricks",
            ),
            ("minecraft:iron_bars", "minecraft:iron_chain"),
        ];
        for (from, to) in pairs {
            replacements.insert(block_id(from), block_id(to));
        }
        Self { replacements }
    }
}

impl StructureProcessor for BlackstoneReplaceProcessor {
    fn process_block(
        &self,
        _level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        _settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        let Some(&new_block) = self.replacements.get(&processed_block_info.state.block()) else {
            return Some(processed_block_info);
        };
        let old_state = processed_block_info.state;
        let mut new_state = BlockState::of(new_block);
        if let Some(facing) = old_state.get_value(BlockStateProperties::HORIZONTAL_FACING)
            && let Ok(next) = new_state.set_value(BlockStateProperties::HORIZONTAL_FACING, facing)
        {
            new_state = next;
        }
        if let Some(half) = old_state.get_value(BlockStateProperties::HALF)
            && let Ok(next) = new_state.set_value(BlockStateProperties::HALF, half)
        {
            new_state = next;
        }
        if let Some(slab_type) = old_state.get_value(BlockStateProperties::SLAB_TYPE)
            && let Ok(next) = new_state.set_value(BlockStateProperties::SLAB_TYPE, slab_type)
        {
            new_state = next;
        }
        Some(StructureBlockInfo::new(
            processed_block_info.pos,
            new_state,
            processed_block_info.nbt,
        ))
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::BLACKSTONE_REPLACE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn blackstone_replace_map_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn MapCodec<BlackstoneReplaceProcessor, Ops>> {
    map_codec::unit(BlackstoneReplaceProcessor::new())
}

// ---------------------------------------------------------------------------
// 10. BlockAgeProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.BlockAgeProcessor`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockAgeProcessor {
    mossiness: f32,
}

impl BlockAgeProcessor {
    const PROBABILITY_OF_REPLACING_FULL_BLOCK: f32 = 0.5;
    const PROBABILITY_OF_REPLACING_STAIRS: f32 = 0.5;
    const PROBABILITY_OF_REPLACING_OBSIDIAN: f32 = 0.15;

    pub fn new(mossiness: f32) -> Self {
        Self { mossiness }
    }

    pub fn mossiness(&self) -> f32 {
        self.mossiness
    }

    fn pick_random_block<R: RandomSource>(
        &self,
        random: &mut R,
        normal: &[BlockState],
        mossy: &[BlockState],
    ) -> BlockState {
        if random.next_float() < self.mossiness {
            mossy[random.next_int_bound(mossy.len() as i32) as usize]
        } else {
            normal[random.next_int_bound(normal.len() as i32) as usize]
        }
    }

    /// `BlockAgeProcessor.getRandomFacingStairs(RandomSource, Block)`.
    fn get_random_facing_stairs<R: RandomSource>(
        random: &mut R,
        stair_block: BlockId,
    ) -> BlockState {
        // Paper: Direction.Plane.HORIZONTAL = [NORTH, EAST, SOUTH, WEST], Half.values() = [TOP, BOTTOM].
        const HORIZONTAL_DIRECTIONS: [&str; 4] = ["north", "east", "south", "west"];
        const HALVES: [&str; 2] = ["top", "bottom"];
        let facing = HORIZONTAL_DIRECTIONS[random.next_int_bound(4) as usize];
        let half = HALVES[random.next_int_bound(2) as usize];
        let state = BlockState::of(stair_block);
        let state = state
            .set_value(
                BlockStateProperties::HORIZONTAL_FACING,
                PropertyValue::Enum(facing),
            )
            .unwrap_or(state);
        state
            .set_value(BlockStateProperties::HALF, PropertyValue::Enum(half))
            .unwrap_or(state)
    }
}

impl StructureProcessor for BlockAgeProcessor {
    fn process_block(
        &self,
        _level: &mut dyn WorldGenLevel,
        _target_position: BlockPos,
        _reference_pos: BlockPos,
        _template_relative_pos: BlockPos,
        processed_block_info: StructureBlockInfo,
        settings: &StructurePlaceSettings,
    ) -> Option<StructureBlockInfo> {
        let mut random = settings.get_random(Some(processed_block_info.pos));
        let state = processed_block_info.state;
        let block = state.block();

        let replacement = if block == block_id("minecraft:stone_bricks")
            || block == block_id("minecraft:stone")
            || block == block_id("minecraft:chiseled_stone_bricks")
        {
            if random.next_float() >= Self::PROBABILITY_OF_REPLACING_FULL_BLOCK {
                None
            } else {
                let non_mossy = [
                    default_state("minecraft:cracked_stone_bricks"),
                    Self::get_random_facing_stairs(
                        &mut random,
                        block_id("minecraft:stone_brick_stairs"),
                    ),
                ];
                let mossy = [
                    default_state("minecraft:mossy_stone_bricks"),
                    Self::get_random_facing_stairs(
                        &mut random,
                        block_id("minecraft:mossy_stone_brick_stairs"),
                    ),
                ];
                Some(self.pick_random_block(&mut random, &non_mossy, &mossy))
            }
        } else if state.is_in_tag("minecraft:stairs") {
            if random.next_float() >= Self::PROBABILITY_OF_REPLACING_STAIRS {
                None
            } else {
                let non_mossy = [
                    default_state("minecraft:stone_slab"),
                    default_state("minecraft:stone_brick_slab"),
                ];
                let mossy = [
                    with_properties_of(block_id("minecraft:mossy_stone_brick_stairs"), state),
                    default_state("minecraft:mossy_stone_brick_slab"),
                ];
                Some(self.pick_random_block(&mut random, &non_mossy, &mossy))
            }
        } else if state.is_in_tag("minecraft:slabs") {
            if random.next_float() < self.mossiness {
                Some(with_properties_of(
                    block_id("minecraft:mossy_stone_brick_slab"),
                    state,
                ))
            } else {
                None
            }
        } else if state.is_in_tag("minecraft:walls") {
            if random.next_float() < self.mossiness {
                Some(with_properties_of(
                    block_id("minecraft:mossy_stone_brick_wall"),
                    state,
                ))
            } else {
                None
            }
        } else if block == block_id("minecraft:obsidian") {
            if random.next_float() < Self::PROBABILITY_OF_REPLACING_OBSIDIAN {
                Some(default_state("minecraft:crying_obsidian"))
            } else {
                None
            }
        } else {
            None
        };

        match replacement {
            Some(new_state) => Some(StructureBlockInfo::new(
                processed_block_info.pos,
                new_state,
                processed_block_info.nbt,
            )),
            None => Some(processed_block_info),
        }
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::BLOCK_AGE
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn block_age_map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<BlockAgeProcessor, Ops>>
{
    map_codec::xmap(
        codec::field_of(codec::float_codec::<Ops>(), "mossiness".to_string()),
        Arc::new(|m: &f32| BlockAgeProcessor::new(*m)),
        Arc::new(|p: &BlockAgeProcessor| p.mossiness),
    )
}

// ---------------------------------------------------------------------------
// 11. CappedProcessor
// ---------------------------------------------------------------------------

/// `net.minecraft.world.level.levelgen.structure.templatesystem.CappedProcessor`.
#[derive(Clone, Debug)]
pub struct CappedProcessor {
    delegate: Arc<dyn ErasedStructureProcessor>,
    limit: IntProvider,
}

impl CappedProcessor {
    pub fn new(delegate: Arc<dyn ErasedStructureProcessor>, limit: IntProvider) -> Self {
        Self { delegate, limit }
    }

    pub fn delegate(&self) -> &Arc<dyn ErasedStructureProcessor> {
        &self.delegate
    }

    pub fn limit(&self) -> &IntProvider {
        &self.limit
    }
}

impl StructureProcessor for CappedProcessor {
    fn evaluates_entire_piece_state(&self) -> bool {
        true
    }

    fn finalize_processing(
        &self,
        level: &mut dyn WorldGenLevel,
        position: BlockPos,
        reference_pos: BlockPos,
        original_block_info_list: &[StructureBlockInfo],
        mut processed_block_info_list: Vec<StructureBlockInfo>,
        settings: &StructurePlaceSettings,
    ) -> Vec<StructureBlockInfo> {
        if self.limit.max_inclusive() == 0 || processed_block_info_list.is_empty() {
            return processed_block_info_list;
        }
        if original_block_info_list.len() != processed_block_info_list.len() {
            return processed_block_info_list;
        }

        // Paper: RandomSource.createThreadLocalInstance(level.getLevel().getSeed()).forkPositional().at(position)
        let mut random = LegacyRandomSource::new(level.get_seed())
            .fork_positional()
            .at(position.get_x(), position.get_y(), position.get_z());
        let sampled = self.limit.sample(&mut random);
        let max_to_replace = sampled.min(processed_block_info_list.len() as i32);
        if max_to_replace < 1 {
            return processed_block_info_list;
        }

        // Paper's `Util.toShuffledList(IntStream.range(0, n), random)` uses
        // Fisher-Yates from the end (`for i in (1..n).rev() { swap(i, random.nextInt(i + 1)) }`).
        let mut indices: Vec<usize> = (0..processed_block_info_list.len()).collect();
        for i in (1..indices.len()).rev() {
            let j = random.next_int_bound((i + 1) as i32) as usize;
            indices.swap(i, j);
        }

        let mut replaced = 0i32;
        for index in indices {
            if replaced >= max_to_replace {
                break;
            }
            let original = &original_block_info_list[index];
            let processed = &processed_block_info_list[index];
            if let Some(altered) = erased_process_block(
                &self.delegate,
                level,
                position,
                reference_pos,
                original.pos,
                processed.clone(),
                settings,
            ) && altered != *processed
            {
                replaced = replaced.wrapping_add(1);
                processed_block_info_list[index] = altered;
            }
        }

        processed_block_info_list
    }

    fn type_id(&self) -> StructureProcessorTypeId {
        StructureProcessorTypes::CAPPED
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn capped_map_codec<Ops: DynamicOps + 'static>(
    single_codec: Arc<dyn Codec<Arc<dyn ErasedStructureProcessor>, Ops>>,
) -> Arc<dyn MapCodec<CappedProcessor, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &CappedProcessor| Arc::clone(&c.delegate)),
                codec::field_of(single_codec, "delegate".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &CappedProcessor| c.limit.clone()),
                codec::field_of(positive_int_provider_codec::<Ops>(), "limit".to_string()),
            ))
            .apply(instance, Arc::new(CappedProcessor::new))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::block::Block;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::structure::templatesystem::always_true_test::AlwaysTrueTest;
    use crate::levelgen::structure::templatesystem::block_match_test::BlockMatchTest;
    use crate::levelgen::structure::templatesystem::pos_always_true_test::PosAlwaysTrueTest;
    use crate::levelgen::structure::templatesystem::rule_block_entity_modifier::AppendStatic;
    use crate::levelgen::structure::templatesystem::structure_template::StructureTemplate;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_registry::holder::Holder;
    use rivet_util::valueproviders::constant_int::ConstantInt;

    struct MockLevel {
        seed: i64,
        blocks: HashMap<BlockPos, BlockState>,
        heights: HashMap<(i32, i32), i32>,
    }

    impl MockLevel {
        fn new(seed: i64) -> Self {
            Self {
                seed,
                blocks: HashMap::new(),
                heights: HashMap::new(),
            }
        }

        fn set_block(&mut self, pos: BlockPos, state: BlockState) {
            self.blocks.insert(pos, state);
        }

        fn set_height(&mut self, x: i32, z: i32, y: i32) {
            self.heights.insert((x, z), y);
        }
    }

    impl LevelHeightAccessor for MockLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for MockLevel {
        fn get_seed(&self) -> i64 {
            self.seed
        }

        fn get_block_state(&self, pos: &BlockPos) -> BlockState {
            self.blocks
                .get(pos)
                .copied()
                .unwrap_or_else(|| state_of("minecraft:air"))
        }

        fn get_height_at(&mut self, _ty: HeightmapType, x: i32, z: i32) -> i32 {
            self.heights.get(&(x, z)).copied().unwrap_or(64)
        }
    }

    fn state_of(name: &str) -> BlockState {
        BlockState::of(id_of(name))
    }

    fn id_of(name: &str) -> BlockId {
        BlockId::from_name(name).expect("valid block")
    }

    #[test]
    fn block_ignore_drops_matching_and_keeps_others() {
        let mut level = MockLevel::new(0);
        let settings = StructurePlaceSettings::new();
        let proc = BlockIgnoreProcessor::structure_and_air();

        let air = StructureBlockInfo::new(BlockPos::new(0, 0, 0), state_of("minecraft:air"), None);
        let sb = StructureBlockInfo::new(
            BlockPos::new(1, 0, 0),
            state_of("minecraft:structure_block"),
            None,
        );
        let stone =
            StructureBlockInfo::new(BlockPos::new(2, 0, 0), state_of("minecraft:stone"), None);

        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                air,
                &settings
            )
            .is_none()
        );
        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                sb,
                &settings
            )
            .is_none()
        );
        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                stone,
                &settings
            )
            .is_some()
        );
    }

    #[test]
    fn block_rot_respects_integrity_and_rottable_filter() {
        let mut level = MockLevel::new(0);
        let settings = StructurePlaceSettings::new();
        let stone_holder = Holder::reference(
            rivet_registry::RegistryId(0),
            id_of("minecraft:stone").id() as u32,
        );
        let proc = BlockRotProcessor::with_rottable_blocks(
            Some(HolderSet::direct(vec![stone_holder])),
            0.0,
        );

        let stone =
            StructureBlockInfo::new(BlockPos::new(0, 10, 0), state_of("minecraft:stone"), None);
        let dirt =
            StructureBlockInfo::new(BlockPos::new(1, 10, 0), state_of("minecraft:dirt"), None);

        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                stone,
                &settings
            )
            .is_none()
        );
        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                dirt,
                &settings
            )
            .is_some()
        );
    }

    #[test]
    fn protected_block_drops_when_world_block_is_protected() {
        let mut level = MockLevel::new(0);
        let pos = BlockPos::new(5, 64, 5);
        level.set_block(pos, state_of("minecraft:bedrock"));

        let bedrock_holder = Holder::reference(
            rivet_registry::RegistryId(0),
            id_of("minecraft:bedrock").id() as u32,
        );
        let proc = ProtectedBlockProcessor::new(HolderSet::direct(vec![bedrock_holder]));
        let settings = StructurePlaceSettings::new();

        let info = StructureBlockInfo::new(pos, state_of("minecraft:stone"), None);
        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                info,
                &settings
            )
            .is_none()
        );

        let other =
            StructureBlockInfo::new(BlockPos::new(6, 64, 5), state_of("minecraft:stone"), None);
        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                other,
                &settings
            )
            .is_some()
        );
    }

    #[test]
    fn rule_processor_first_matching_rule_wins_and_applies_modifier() {
        let mut level = MockLevel::new(0);
        let mut extra = CompoundTag::new();
        extra.put_int("tagged", 1);

        let rule1 = ProcessorRule::with_modifier(
            Arc::new(BlockMatchTest::new(Block::new(id_of("minecraft:stone")))),
            Arc::new(AlwaysTrueTest::INSTANCE),
            Arc::new(PosAlwaysTrueTest::INSTANCE),
            state_of("minecraft:deepslate"),
            Arc::new(AppendStatic::new(extra)),
        );
        let rule2 = ProcessorRule::new(
            Arc::new(BlockMatchTest::new(Block::new(id_of("minecraft:stone")))),
            Arc::new(AlwaysTrueTest::INSTANCE),
            state_of("minecraft:granite"),
        );

        let proc = RuleProcessor::new(vec![rule1, rule2]);
        let settings = StructurePlaceSettings::new();
        let pos = BlockPos::new(3, 4, 5);
        let input = StructureBlockInfo::new(pos, state_of("minecraft:stone"), None);
        let out = proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                input,
                &settings,
            )
            .expect("rule should produce output");

        assert_eq!(out.state, state_of("minecraft:deepslate"));
        assert_eq!(out.nbt.as_ref().and_then(|t| t.get_int("tagged")), Some(1));
    }

    #[test]
    fn gravity_processor_shifts_y_by_heightmap_offset_and_relative_y() {
        let mut level = MockLevel::new(0);
        level.set_height(10, 20, 70);

        let proc = GravityProcessor::new(HeightmapType::WorldSurfaceWg, -2);
        let settings = StructurePlaceSettings::new();
        let input =
            StructureBlockInfo::new(BlockPos::new(10, 5, 20), state_of("minecraft:stone"), None);
        let out = proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::new(0, 3, 0),
                input,
                &settings,
            )
            .expect("gravity keeps block");

        // height(70) + offset(-2) + relative_y(3) = 71
        assert_eq!(out.pos, BlockPos::new(10, 71, 20));
    }

    #[test]
    fn jigsaw_replacement_parses_final_state_and_drops_structure_void() {
        let mut level = MockLevel::new(0);
        let settings = StructurePlaceSettings::new();
        let proc = JigsawReplacementProcessor;

        let mut nbt = CompoundTag::new();
        nbt.put_string("final_state", "minecraft:cobblestone");
        let jigsaw = StructureBlockInfo::new(
            BlockPos::new(1, 2, 3),
            state_of("minecraft:jigsaw"),
            Some(nbt),
        );
        let replaced = proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                jigsaw,
                &settings,
            )
            .expect("cobblestone is kept");
        assert_eq!(replaced.state, state_of("minecraft:cobblestone"));
        assert!(replaced.nbt.is_none());

        let mut void_nbt = CompoundTag::new();
        void_nbt.put_string("final_state", "minecraft:structure_void");
        let jigsaw_void = StructureBlockInfo::new(
            BlockPos::new(1, 2, 3),
            state_of("minecraft:jigsaw"),
            Some(void_nbt),
        );
        assert!(
            proc.process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                jigsaw_void,
                &settings,
            )
            .is_none()
        );
    }

    #[test]
    fn lava_submerged_and_blackstone_replace_work_as_expected() {
        let mut level = MockLevel::new(0);
        let pos = BlockPos::new(4, 30, 4);
        level.set_block(pos, state_of("minecraft:lava"));
        let settings = StructurePlaceSettings::new();

        let lava_proc = LavaSubmergedBlockProcessor;
        let air = StructureBlockInfo::new(pos, state_of("minecraft:air"), None);
        let out = lava_proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                air,
                &settings,
            )
            .expect("lava submerged replaces air with lava");
        assert_eq!(out.state, state_of("minecraft:lava"));

        // Partial legacy-solid blocks (stairs, single slabs) are also replaced by lava,
        // while full-cube blocks (stone, glass) displace lava.
        let stairs = StructureBlockInfo::new(pos, state_of("minecraft:stone_brick_stairs"), None);
        let out_stairs = lava_proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                stairs,
                &settings,
            )
            .unwrap();
        assert_eq!(out_stairs.state, state_of("minecraft:lava"));

        let stone = StructureBlockInfo::new(pos, state_of("minecraft:stone"), None);
        let out_stone = lava_proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                stone,
                &settings,
            )
            .unwrap();
        assert_eq!(out_stone.state, state_of("minecraft:stone"));

        let bs_proc = BlackstoneReplaceProcessor::new();
        let cobble = StructureBlockInfo::new(pos, state_of("minecraft:cobblestone"), None);
        let out_bs = bs_proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                cobble,
                &settings,
            )
            .expect("blackstone replace produces blackstone");
        assert_eq!(out_bs.state, state_of("minecraft:blackstone"));

        // Preserves stair facing/half and slab type (Paper BlackstoneReplaceProcessor).
        let top_west_stairs = state_of("minecraft:cobblestone_stairs")
            .set_value(
                BlockStateProperties::HORIZONTAL_FACING,
                PropertyValue::Enum("west"),
            )
            .unwrap()
            .set_value(BlockStateProperties::HALF, PropertyValue::Enum("top"))
            .unwrap();
        let out_stairs = bs_proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                StructureBlockInfo::new(pos, top_west_stairs, None),
                &settings,
            )
            .unwrap();
        assert_eq!(
            out_stairs.state.block(),
            block_id("minecraft:blackstone_stairs")
        );
        assert_eq!(
            out_stairs
                .state
                .get_value(BlockStateProperties::HORIZONTAL_FACING),
            Some(PropertyValue::Enum("west"))
        );
        assert_eq!(
            out_stairs.state.get_value(BlockStateProperties::HALF),
            Some(PropertyValue::Enum("top"))
        );

        let double_slab = state_of("minecraft:cobblestone_slab")
            .set_value(
                BlockStateProperties::SLAB_TYPE,
                PropertyValue::Enum("double"),
            )
            .unwrap();
        let out_slab = bs_proc
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                StructureBlockInfo::new(pos, double_slab, None),
                &settings,
            )
            .unwrap();
        assert_eq!(
            out_slab.state.block(),
            block_id("minecraft:blackstone_slab")
        );
        assert_eq!(
            out_slab.state.get_value(BlockStateProperties::SLAB_TYPE),
            Some(PropertyValue::Enum("double"))
        );
    }

    #[test]
    fn capped_processor_and_template_pipeline_enforce_limit_deterministically() {
        let mut level = MockLevel::new(42);
        let delegate: Arc<dyn ErasedStructureProcessor> =
            Arc::new(BlackstoneReplaceProcessor::new());
        let capped = CappedProcessor::new(delegate, IntProvider::Constant(ConstantInt::of(2)));

        let mut settings = StructurePlaceSettings::new();
        settings.add_processor(Arc::new(BlockIgnoreProcessor::air()));
        settings.add_processor(Arc::new(capped));

        let blocks = vec![
            StructureBlockInfo::new(BlockPos::new(0, 0, 0), state_of("minecraft:air"), None),
            StructureBlockInfo::new(
                BlockPos::new(1, 0, 0),
                state_of("minecraft:cobblestone"),
                None,
            ),
            StructureBlockInfo::new(
                BlockPos::new(2, 0, 0),
                state_of("minecraft:cobblestone"),
                None,
            ),
            StructureBlockInfo::new(
                BlockPos::new(3, 0, 0),
                state_of("minecraft:cobblestone"),
                None,
            ),
            StructureBlockInfo::new(
                BlockPos::new(4, 0, 0),
                state_of("minecraft:cobblestone"),
                None,
            ),
        ];

        let run1 = StructureTemplate::process_block_infos(
            &mut level,
            BlockPos::new(10, 64, 10),
            BlockPos::new(10, 64, 10),
            &settings,
            &blocks,
        );
        let run2 = StructureTemplate::process_block_infos(
            &mut level,
            BlockPos::new(10, 64, 10),
            BlockPos::new(10, 64, 10),
            &settings,
            &blocks,
        );

        // Air was dropped by BlockIgnoreProcessor; remaining 4 cobblestones have exactly 2 replaced.
        assert_eq!(run1.len(), 4);
        assert_eq!(run1, run2);
        let blackstone_count = run1
            .iter()
            .filter(|b| b.state == state_of("minecraft:blackstone"))
            .count();
        assert_eq!(blackstone_count, 2);

        // Verify exact index selection matches Paper's LegacyRandomSource positional stream.
        let mut expected_rng = LegacyRandomSource::new(42).fork_positional().at(10, 64, 10);
        let mut expected_indices = [0usize, 1, 2, 3];
        for i in (1..expected_indices.len()).rev() {
            let j = expected_rng.next_int_bound((i + 1) as i32) as usize;
            expected_indices.swap(i, j);
        }
        let mut expected_replaced = [expected_indices[0], expected_indices[1]];
        expected_replaced.sort_unstable();
        let actual_replaced: Vec<usize> = run1
            .iter()
            .enumerate()
            .filter_map(|(i, b)| (b.state == state_of("minecraft:blackstone")).then_some(i))
            .collect();
        assert_eq!(actual_replaced, expected_replaced);
    }

    #[test]
    fn block_age_processor_matches_paper_blocks_tags_and_properties() {
        let mut level = MockLevel::new(0);
        let settings = StructurePlaceSettings::new();

        // Full stone block with mossiness=0.0 produces cracked_stone_bricks or randomized stairs (never slab).
        let dry_age = BlockAgeProcessor::new(0.0);
        let mut saw_cracked = false;
        let mut saw_non_default_stair = false;
        let default_stairs = state_of("minecraft:stone_brick_stairs");
        for x in 0..64 {
            let info = StructureBlockInfo::new(
                BlockPos::new(x, 0, 0),
                state_of("minecraft:stone_bricks"),
                None,
            );
            let out = dry_age
                .process_block(
                    &mut level,
                    BlockPos::ZERO,
                    BlockPos::ZERO,
                    BlockPos::ZERO,
                    info,
                    &settings,
                )
                .unwrap();
            assert_ne!(out.state.block(), block_id("minecraft:stone_brick_slab"));
            if out.state.block() == block_id("minecraft:cracked_stone_bricks") {
                saw_cracked = true;
            }
            if out.state.block() == block_id("minecraft:stone_brick_stairs")
                && out.state != default_stairs
            {
                saw_non_default_stair = true;
            }
        }
        assert!(saw_cracked, "expected cracked_stone_bricks replacement");
        assert!(
            saw_non_default_stair,
            "expected randomized stair facing/half"
        );

        // Tagged slab (e.g. oak_slab) with mossiness=1.0 ages to mossy_stone_brick_slab preserving type=top.
        let mossy_age = BlockAgeProcessor::new(1.0);
        let top_oak_slab = state_of("minecraft:oak_slab")
            .set_value(BlockStateProperties::SLAB_TYPE, PropertyValue::Enum("top"))
            .unwrap();
        let out_slab = mossy_age
            .process_block(
                &mut level,
                BlockPos::ZERO,
                BlockPos::ZERO,
                BlockPos::ZERO,
                StructureBlockInfo::new(BlockPos::new(1, 2, 3), top_oak_slab, None),
                &settings,
            )
            .unwrap();
        assert_eq!(
            out_slab.state.block(),
            block_id("minecraft:mossy_stone_brick_slab")
        );
        assert_eq!(
            out_slab.state.get_value(BlockStateProperties::SLAB_TYPE),
            Some(PropertyValue::Enum("top"))
        );

        // Tagged stairs (e.g. oak_stairs) with mossiness=1.0 preserve facing/half when replaced by mossy stairs.
        let top_south_stairs = state_of("minecraft:oak_stairs")
            .set_value(
                BlockStateProperties::HORIZONTAL_FACING,
                PropertyValue::Enum("south"),
            )
            .unwrap()
            .set_value(BlockStateProperties::HALF, PropertyValue::Enum("top"))
            .unwrap();
        let mut saw_preserved_mossy_stairs = false;
        for x in 0..64 {
            let out_stairs = mossy_age
                .process_block(
                    &mut level,
                    BlockPos::ZERO,
                    BlockPos::ZERO,
                    BlockPos::ZERO,
                    StructureBlockInfo::new(BlockPos::new(x, 10, 0), top_south_stairs, None),
                    &settings,
                )
                .unwrap();
            if out_stairs.state.block() == block_id("minecraft:mossy_stone_brick_stairs") {
                assert_eq!(
                    out_stairs
                        .state
                        .get_value(BlockStateProperties::HORIZONTAL_FACING),
                    Some(PropertyValue::Enum("south"))
                );
                assert_eq!(
                    out_stairs.state.get_value(BlockStateProperties::HALF),
                    Some(PropertyValue::Enum("top"))
                );
                saw_preserved_mossy_stairs = true;
            }
        }
        assert!(
            saw_preserved_mossy_stairs,
            "expected mossy_stone_brick_stairs preserving properties"
        );
    }
}
