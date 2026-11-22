//! Port of `net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate`
//! (MC 26.2) core data structures, coordinate transforms, processor pipeline,
//! palette tables, and NBT persistence (`save`/`load`).

use super::structure_place_settings::StructurePlaceSettings;
use super::structure_processor::{
    erased_evaluates_entire_piece_state, erased_finalize_processing, erased_process_block,
};
use crate::level::world_gen_level::WorldGenLevel;
use crate::levelgen::structure::bounding_box::BoundingBox;
use indexmap::IndexMap;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::double_tag::DoubleTag;
use rivet_nbt::int_tag::IntTag;
use rivet_nbt::list_tag::ListTag;
use rivet_nbt::nbt_utils;
use rivet_nbt::tag::Tag;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Rotation};
use rivet_registry::generated::blocks::BlockId;

fn block_id(name: &str) -> BlockId {
    BlockId::from_name(name).unwrap_or_else(|| panic!("unknown block {name}"))
}

fn air_state() -> BlockState {
    BlockState::of(block_id("minecraft:air"))
}

/// `net.minecraft.world.level.block.Mirror` — horizontal axis mirroring used by
/// `StructureTemplate` and `StructurePlaceSettings`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Mirror {
    #[default]
    None,
    LeftRight,
    FrontBack,
}

impl Mirror {
    pub const VALUES: [Mirror; 3] = [Mirror::None, Mirror::LeftRight, Mirror::FrontBack];
}

/// `StructureTemplate.StructureBlockInfo(BlockPos pos, BlockState state, @Nullable CompoundTag nbt)`.
#[derive(Clone, Debug, PartialEq)]
pub struct StructureBlockInfo {
    pub pos: BlockPos,
    pub state: BlockState,
    pub nbt: Option<CompoundTag>,
}

impl StructureBlockInfo {
    pub fn new(pos: BlockPos, state: BlockState, nbt: Option<CompoundTag>) -> Self {
        Self { pos, state, nbt }
    }
}

/// `StructureTemplate.StructureEntityInfo(Vec3 pos, BlockPos blockPos, CompoundTag nbt)`.
#[derive(Clone, Debug, PartialEq)]
pub struct StructureEntityInfo {
    pub pos: [f64; 3],
    pub block_pos: BlockPos,
    pub nbt: CompoundTag,
}

impl StructureEntityInfo {
    pub fn new(pos: [f64; 3], block_pos: BlockPos, nbt: CompoundTag) -> Self {
        Self {
            pos,
            block_pos,
            nbt,
        }
    }
}

/// `StructureTemplate.Palette` — ordered block list with per-block index and
/// jigsaw block list, preserving Paper declaration order.
#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    blocks: Vec<StructureBlockInfo>,
    cache: IndexMap<BlockId, Vec<StructureBlockInfo>>,
    jigsaws: Vec<StructureBlockInfo>,
}

impl Palette {
    pub fn new(blocks: Vec<StructureBlockInfo>) -> Self {
        let mut cache: IndexMap<BlockId, Vec<StructureBlockInfo>> = IndexMap::new();
        let mut jigsaws = Vec::new();
        let jigsaw_id = block_id("minecraft:jigsaw");
        for info in &blocks {
            let id = info.state.block();
            cache.entry(id).or_default().push(info.clone());
            if id == jigsaw_id {
                jigsaws.push(info.clone());
            }
        }
        Self {
            blocks,
            cache,
            jigsaws,
        }
    }

    pub fn blocks(&self) -> &[StructureBlockInfo] {
        &self.blocks
    }

    pub fn blocks_of(&self, block: BlockId) -> &[StructureBlockInfo] {
        self.cache.get(&block).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn jigsaws(&self) -> &[StructureBlockInfo] {
        &self.jigsaws
    }
}

/// `StructureTemplate.SimplePalette` — maps `BlockState` <-> sequential `i32` ID
/// for NBT serialization/deserialization. Unknown IDs resolve to `Blocks.AIR`.
#[derive(Clone, Debug, Default)]
pub struct SimplePalette {
    state_to_id: IndexMap<BlockState, i32>,
    id_to_state: IndexMap<i32, BlockState>,
    last_id: i32,
}

impl SimplePalette {
    pub fn new() -> Self {
        Self::default()
    }

    /// `SimplePalette.idFor(BlockState)` — returns existing ID or assigns next sequential ID.
    pub fn id_for(&mut self, state: BlockState) -> i32 {
        if let Some(&id) = self.state_to_id.get(&state) {
            return id;
        }
        let id = self.last_id;
        self.last_id = self.last_id.wrapping_add(1);
        self.state_to_id.insert(state, id);
        self.id_to_state.insert(id, state);
        id
    }

    /// `SimplePalette.stateFor(int)` — returns `Blocks.AIR.defaultBlockState()` for unknown IDs.
    pub fn state_for(&self, id: i32) -> BlockState {
        self.id_to_state.get(&id).copied().unwrap_or_else(air_state)
    }

    /// `SimplePalette.addMapping(BlockState, int)`.
    pub fn add_mapping(&mut self, state: BlockState, id: i32) {
        self.state_to_id.insert(state, id);
        self.id_to_state.insert(id, state);
        if id >= self.last_id {
            self.last_id = id.wrapping_add(1);
        }
    }

    /// Iterate states in ID order (`0..last_id` for densely populated palettes).
    pub fn iter_states(&self) -> impl Iterator<Item = BlockState> + '_ {
        self.id_to_state.values().copied()
    }

    pub fn len(&self) -> usize {
        self.id_to_state.len()
    }

    pub fn is_empty(&self) -> bool {
        self.id_to_state.is_empty()
    }
}

/// `net.minecraft.world.level.levelgen.structure.templatesystem.StructureTemplate`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StructureTemplate {
    palettes: Vec<Palette>,
    entity_info_list: Vec<StructureEntityInfo>,
    size: [i32; 3],
    author: String,
}

impl StructureTemplate {
    pub const PALETTE_TAG: &'static str = "palette";
    pub const PALETTE_LIST_TAG: &'static str = "palettes";
    pub const ENTITIES_TAG: &'static str = "entities";
    pub const ENTITY_TAG_POS: &'static str = "pos";
    pub const ENTITY_TAG_BLOCKPOS: &'static str = "blockPos";
    pub const ENTITY_TAG_NBT: &'static str = "nbt";
    pub const BLOCKS_TAG: &'static str = "blocks";
    pub const BLOCK_TAG_POS: &'static str = "pos";
    pub const BLOCK_TAG_STATE: &'static str = "state";
    pub const BLOCK_TAG_NBT: &'static str = "nbt";
    pub const SIZE_TAG: &'static str = "size";

    pub fn new() -> Self {
        Self {
            palettes: Vec::new(),
            entity_info_list: Vec::new(),
            size: [0, 0, 0],
            author: "?".to_string(),
        }
    }

    pub fn from_parts(
        size: [i32; 3],
        palettes: Vec<Palette>,
        entity_info_list: Vec<StructureEntityInfo>,
    ) -> Self {
        Self {
            palettes,
            entity_info_list,
            size,
            author: "?".to_string(),
        }
    }

    pub fn get_size(&self) -> [i32; 3] {
        self.size
    }

    pub fn get_size_rotated(&self, rotation: Rotation) -> [i32; 3] {
        match rotation {
            Rotation::Counterclockwise90 | Rotation::Clockwise90 => {
                [self.size[2], self.size[1], self.size[0]]
            }
            Rotation::None | Rotation::Clockwise180 => self.size,
        }
    }

    pub fn set_author(&mut self, author: impl Into<String>) {
        self.author = author.into();
    }

    pub fn get_author(&self) -> &str {
        &self.author
    }

    pub fn palettes(&self) -> &[Palette] {
        &self.palettes
    }

    pub fn entity_info_list(&self) -> &[StructureEntityInfo] {
        &self.entity_info_list
    }

    /// `StructureTemplate.transform(BlockPos, Mirror, Rotation, BlockPos)` —
    /// exact Java wrapping `i32` arithmetic.
    pub fn transform_pos(
        pos: BlockPos,
        mirror: Mirror,
        rotation: Rotation,
        pivot: BlockPos,
    ) -> BlockPos {
        let mut x = pos.get_x();
        let y = pos.get_y();
        let mut z = pos.get_z();
        let mut was_mirrored = true;
        match mirror {
            Mirror::LeftRight => z = z.wrapping_neg(),
            Mirror::FrontBack => x = x.wrapping_neg(),
            Mirror::None => was_mirrored = false,
        }

        let px = pivot.get_x();
        let pz = pivot.get_z();
        match rotation {
            Rotation::Counterclockwise90 => BlockPos::new(
                px.wrapping_sub(pz).wrapping_add(z),
                y,
                px.wrapping_add(pz).wrapping_sub(x),
            ),
            Rotation::Clockwise90 => BlockPos::new(
                px.wrapping_add(pz).wrapping_sub(z),
                y,
                pz.wrapping_sub(px).wrapping_add(x),
            ),
            Rotation::Clockwise180 => BlockPos::new(
                px.wrapping_add(px).wrapping_sub(x),
                y,
                pz.wrapping_add(pz).wrapping_sub(z),
            ),
            Rotation::None => {
                if was_mirrored {
                    BlockPos::new(x, y, z)
                } else {
                    pos
                }
            }
        }
    }

    /// `StructureTemplate.transform(Vec3, Mirror, Rotation, BlockPos)`.
    pub fn transform_vec3(
        pos: [f64; 3],
        mirror: Mirror,
        rotation: Rotation,
        pivot: BlockPos,
    ) -> [f64; 3] {
        let mut x = pos[0];
        let y = pos[1];
        let mut z = pos[2];
        let mut was_mirrored = true;
        match mirror {
            Mirror::LeftRight => z = 1.0 - z,
            Mirror::FrontBack => x = 1.0 - x,
            Mirror::None => was_mirrored = false,
        }

        let px = pivot.get_x() as f64;
        let pz = pivot.get_z() as f64;
        match rotation {
            Rotation::Counterclockwise90 => [px - pz + z, y, px + pz + 1.0 - x],
            Rotation::Clockwise90 => [px + pz + 1.0 - z, y, pz - px + x],
            Rotation::Clockwise180 => [px + px + 1.0 - x, y, pz + pz + 1.0 - z],
            Rotation::None => {
                if was_mirrored {
                    [x, y, z]
                } else {
                    pos
                }
            }
        }
    }

    /// `StructureTemplate.calculateRelativePosition(StructurePlaceSettings, BlockPos)`.
    pub fn calculate_relative_position(
        settings: &StructurePlaceSettings,
        pos: BlockPos,
    ) -> BlockPos {
        Self::transform_pos(
            pos,
            settings.get_mirror(),
            settings.get_rotation(),
            settings.get_rotation_pivot(),
        )
    }

    /// `StructureTemplate.getZeroPositionWithTransform(BlockPos, Mirror, Rotation, int, int)`.
    pub fn get_zero_position_with_transform(
        zero_pos: BlockPos,
        mirror: Mirror,
        rotation: Rotation,
        size_x: i32,
        size_z: i32,
    ) -> BlockPos {
        let sx = size_x.wrapping_sub(1);
        let sz = size_z.wrapping_sub(1);
        let mirror_delta_x = if mirror == Mirror::FrontBack { sx } else { 0 };
        let mirror_delta_z = if mirror == Mirror::LeftRight { sz } else { 0 };

        let (dx, dz) = match rotation {
            Rotation::Counterclockwise90 => (mirror_delta_z, sx.wrapping_sub(mirror_delta_x)),
            Rotation::Clockwise90 => (sz.wrapping_sub(mirror_delta_z), mirror_delta_x),
            Rotation::Clockwise180 => (
                sx.wrapping_sub(mirror_delta_x),
                sz.wrapping_sub(mirror_delta_z),
            ),
            Rotation::None => (mirror_delta_x, mirror_delta_z),
        };
        zero_pos.offset(dx, 0, dz)
    }

    /// `StructureTemplate.getBoundingBox(StructurePlaceSettings, BlockPos)`.
    pub fn get_bounding_box(
        &self,
        settings: &StructurePlaceSettings,
        position: BlockPos,
    ) -> BoundingBox {
        self.get_bounding_box_with_pivot(
            position,
            settings.get_rotation(),
            settings.get_rotation_pivot(),
            settings.get_mirror(),
        )
    }

    /// `StructureTemplate.getBoundingBox(BlockPos, Rotation, BlockPos, Mirror)`.
    pub fn get_bounding_box_with_pivot(
        &self,
        position: BlockPos,
        rotation: Rotation,
        pivot: BlockPos,
        mirror: Mirror,
    ) -> BoundingBox {
        Self::compute_bounding_box(position, rotation, pivot, mirror, self.size)
    }

    /// `StructureTemplate.getBoundingBox(BlockPos, Rotation, BlockPos, Mirror, Vec3i)`.
    pub fn compute_bounding_box(
        position: BlockPos,
        rotation: Rotation,
        pivot: BlockPos,
        mirror: Mirror,
        size: [i32; 3],
    ) -> BoundingBox {
        let delta = BlockPos::new(
            size[0].wrapping_sub(1),
            size[1].wrapping_sub(1),
            size[2].wrapping_sub(1),
        );
        let corner1 = Self::transform_pos(BlockPos::ZERO, mirror, rotation, pivot);
        let corner2 = Self::transform_pos(delta, mirror, rotation, pivot);
        let bb = BoundingBox::new([
            corner1.get_x(),
            corner1.get_y(),
            corner1.get_z(),
            corner2.get_x(),
            corner2.get_y(),
            corner2.get_z(),
        ]);
        BoundingBox::new([
            bb.min_x.wrapping_add(position.get_x()),
            bb.min_y.wrapping_add(position.get_y()),
            bb.min_z.wrapping_add(position.get_z()),
            bb.max_x.wrapping_add(position.get_x()),
            bb.max_y.wrapping_add(position.get_y()),
            bb.max_z.wrapping_add(position.get_z()),
        ])
    }

    /// `StructureTemplate.processBlockInfos(...)` — exact Paper 26.2 pipeline:
    /// 1. Check whether any processor has `evaluates_entire_piece_state() == true`.
    /// 2. For each block in `block_info_list`:
    ///    - Transform position relative to `position`.
    ///    - Skip if `process_only_in_current_chunk` and `chunk_bb` excludes `block_pos`.
    ///    - Run every processor's `process_block` in declaration order; short-circuit on `None`.
    ///    - Append surviving `(original, processed)` entries in lockstep.
    /// 3. Run every processor's `finalize_processing` in declaration order.
    pub fn process_block_infos(
        level: &mut dyn WorldGenLevel,
        position: BlockPos,
        reference_pos: BlockPos,
        settings: &StructurePlaceSettings,
        block_info_list: &[StructureBlockInfo],
    ) -> Vec<StructureBlockInfo> {
        let mut original_list: Vec<StructureBlockInfo> = Vec::new();
        let mut processed_list: Vec<StructureBlockInfo> = Vec::new();
        let process_only_in_current_chunk = !settings
            .get_processors()
            .iter()
            .any(erased_evaluates_entire_piece_state);
        let chunk_bb = settings.get_bounding_box();

        for block_info in block_info_list {
            let block_pos = Self::calculate_relative_position(settings, block_info.pos).offset(
                position.get_x(),
                position.get_y(),
                position.get_z(),
            );
            let inside = match chunk_bb {
                Some(bb) if process_only_in_current_chunk => bb.is_inside(&block_pos),
                _ => true,
            };
            if !inside {
                continue;
            }

            let mut current = Some(StructureBlockInfo::new(
                block_pos,
                block_info.state,
                block_info.nbt.clone(),
            ));
            for processor in settings.get_processors() {
                let Some(info) = current else {
                    break;
                };
                current = erased_process_block(
                    processor,
                    level,
                    position,
                    reference_pos,
                    block_info.pos,
                    info,
                    settings,
                );
            }
            if let Some(final_info) = current {
                processed_list.push(final_info);
                original_list.push(block_info.clone());
            }
        }

        for processor in settings.get_processors() {
            processed_list = erased_finalize_processing(
                processor,
                level,
                position,
                reference_pos,
                &original_list,
                processed_list,
                settings,
            );
        }

        processed_list
    }

    /// `StructureTemplate.save(CompoundTag)` — exact Paper NBT schema.
    pub fn save(&self, tag: &mut CompoundTag) {
        if self.palettes.is_empty() {
            tag.put(
                Self::BLOCKS_TAG.to_string(),
                Tag::List(ListTag { list: Vec::new() }),
            );
            tag.put(
                Self::PALETTE_TAG.to_string(),
                Tag::List(ListTag { list: Vec::new() }),
            );
        } else {
            let mut simple_palettes: Vec<SimplePalette> = (0..self.palettes.len())
                .map(|_| SimplePalette::new())
                .collect();
            let mut block_list: Vec<Tag> = Vec::new();
            let main_blocks = self.palettes[0].blocks();

            for (i, block_info) in main_blocks.iter().enumerate() {
                let mut block_tag = CompoundTag::new();
                block_tag.put(
                    Self::BLOCK_TAG_POS.to_string(),
                    int_list_tag(&[
                        block_info.pos.get_x(),
                        block_info.pos.get_y(),
                        block_info.pos.get_z(),
                    ]),
                );
                let id = simple_palettes[0].id_for(block_info.state);
                block_tag.put_int(Self::BLOCK_TAG_STATE, id);
                if let Some(nbt) = &block_info.nbt {
                    block_tag.put(Self::BLOCK_TAG_NBT.to_string(), Tag::Compound(nbt.clone()));
                }
                block_list.push(Tag::Compound(block_tag));

                for (palette, simple_palette) in
                    self.palettes.iter().zip(simple_palettes.iter_mut()).skip(1)
                {
                    if let Some(alt_info) = palette.blocks().get(i) {
                        simple_palette.add_mapping(alt_info.state, id);
                    }
                }
            }

            tag.put(
                Self::BLOCKS_TAG.to_string(),
                Tag::List(ListTag { list: block_list }),
            );
            if simple_palettes.len() == 1 {
                let palette_list: Vec<Tag> = simple_palettes[0]
                    .iter_states()
                    .map(|s| Tag::Compound(nbt_utils::write_block_state(s)))
                    .collect();
                tag.put(
                    Self::PALETTE_TAG.to_string(),
                    Tag::List(ListTag { list: palette_list }),
                );
            } else {
                let palette_list_list: Vec<Tag> = simple_palettes
                    .iter()
                    .map(|palette| {
                        let items: Vec<Tag> = palette
                            .iter_states()
                            .map(|s| Tag::Compound(nbt_utils::write_block_state(s)))
                            .collect();
                        Tag::List(ListTag { list: items })
                    })
                    .collect();
                tag.put(
                    Self::PALETTE_LIST_TAG.to_string(),
                    Tag::List(ListTag {
                        list: palette_list_list,
                    }),
                );
            }
        }

        let mut entity_list: Vec<Tag> = Vec::new();
        for entity_info in &self.entity_info_list {
            let mut entity_tag = CompoundTag::new();
            entity_tag.put(
                Self::ENTITY_TAG_POS.to_string(),
                double_list_tag(&entity_info.pos),
            );
            entity_tag.put(
                Self::ENTITY_TAG_BLOCKPOS.to_string(),
                int_list_tag(&[
                    entity_info.block_pos.get_x(),
                    entity_info.block_pos.get_y(),
                    entity_info.block_pos.get_z(),
                ]),
            );
            entity_tag.put(
                Self::ENTITY_TAG_NBT.to_string(),
                Tag::Compound(entity_info.nbt.clone()),
            );
            entity_list.push(Tag::Compound(entity_tag));
        }
        tag.put(
            Self::ENTITIES_TAG.to_string(),
            Tag::List(ListTag { list: entity_list }),
        );
        tag.put(Self::SIZE_TAG.to_string(), int_list_tag(&self.size));
        nbt_utils::add_current_data_version(tag);
    }

    /// `StructureTemplate.load(HolderGetter<Block>, CompoundTag)` — exact Paper NBT schema.
    pub fn load(&mut self, tag: &CompoundTag) {
        self.palettes.clear();
        self.entity_info_list.clear();

        let size_list = get_list_slice(tag, Self::SIZE_TAG);
        self.size = [
            list_int_or(size_list, 0, 0),
            list_int_or(size_list, 1, 0),
            list_int_or(size_list, 2, 0),
        ];

        let block_list = get_list_slice(tag, Self::BLOCKS_TAG);
        if let Some(Tag::List(palette_lists)) = tag.get(Self::PALETTE_LIST_TAG) {
            for entry in &palette_lists.list {
                if let Tag::List(palette_entries) = entry {
                    self.load_palette(&palette_entries.list, block_list);
                } else {
                    self.load_palette(&[], block_list);
                }
            }
        } else {
            let single_palette = get_list_slice(tag, Self::PALETTE_TAG);
            self.load_palette(single_palette, block_list);
        }

        for entry in get_list_slice(tag, Self::ENTITIES_TAG) {
            if let Tag::Compound(entity_tag) = entry {
                let pos_list = get_list_slice(entity_tag, Self::ENTITY_TAG_POS);
                let pos = [
                    list_double_or(pos_list, 0, 0.0),
                    list_double_or(pos_list, 1, 0.0),
                    list_double_or(pos_list, 2, 0.0),
                ];
                let bp_list = get_list_slice(entity_tag, Self::ENTITY_TAG_BLOCKPOS);
                let block_pos = BlockPos::new(
                    list_int_or(bp_list, 0, 0),
                    list_int_or(bp_list, 1, 0),
                    list_int_or(bp_list, 2, 0),
                );
                if let Some(Tag::Compound(nbt)) = entity_tag.get(Self::ENTITY_TAG_NBT) {
                    self.entity_info_list.push(StructureEntityInfo::new(
                        pos,
                        block_pos,
                        nbt.clone(),
                    ));
                }
            }
        }
    }

    fn load_palette(&mut self, palette_entries: &[Tag], block_list: &[Tag]) {
        let mut simple = SimplePalette::new();
        for (i, entry) in palette_entries.iter().enumerate() {
            let state = match entry {
                Tag::Compound(c) => nbt_utils::read_block_state(c),
                _ => air_state(),
            };
            simple.add_mapping(state, i as i32);
        }

        let mut full_blocks: Vec<StructureBlockInfo> = Vec::new();
        let mut block_entities: Vec<StructureBlockInfo> = Vec::new();
        let mut other_blocks: Vec<StructureBlockInfo> = Vec::new();

        for entry in block_list {
            let Tag::Compound(block_tag) = entry else {
                continue;
            };
            let pos_list = get_list_slice(block_tag, Self::BLOCK_TAG_POS);
            let pos = BlockPos::new(
                list_int_or(pos_list, 0, 0),
                list_int_or(pos_list, 1, 0),
                list_int_or(pos_list, 2, 0),
            );
            let state_id = block_tag.get_int(Self::BLOCK_TAG_STATE).unwrap_or(0);
            let state = simple.state_for(state_id);
            let nbt = match block_tag.get(Self::BLOCK_TAG_NBT) {
                Some(Tag::Compound(c)) => Some(c.clone()),
                _ => None,
            };
            let info = StructureBlockInfo::new(pos, state, nbt);
            add_to_lists(
                info,
                &mut full_blocks,
                &mut block_entities,
                &mut other_blocks,
            );
        }

        let sorted = build_info_list(full_blocks, block_entities, other_blocks);
        self.palettes.push(Palette::new(sorted));
    }
}

/// `StructureTemplate.addToLists` — buckets blocks into full-cube blocks,
/// block-entity blocks, and other non-full blocks (matches Paper's ordering
/// when saving/loading templates).
fn add_to_lists(
    info: StructureBlockInfo,
    full_blocks: &mut Vec<StructureBlockInfo>,
    block_entities: &mut Vec<StructureBlockInfo>,
    other_blocks: &mut Vec<StructureBlockInfo>,
) {
    if info.nbt.is_some() {
        block_entities.push(info);
    } else if info.state.is_solid() {
        full_blocks.push(info);
    } else {
        other_blocks.push(info);
    }
}

/// `StructureTemplate.buildInfoList` — sorts each bucket by `(y, x, z)` and
/// concatenates `full_blocks`, `other_blocks`, `block_entities` in Paper order.
fn build_info_list(
    mut full_blocks: Vec<StructureBlockInfo>,
    mut block_entities: Vec<StructureBlockInfo>,
    mut other_blocks: Vec<StructureBlockInfo>,
) -> Vec<StructureBlockInfo> {
    let cmp = |a: &StructureBlockInfo, b: &StructureBlockInfo| {
        a.pos
            .get_y()
            .cmp(&b.pos.get_y())
            .then_with(|| a.pos.get_x().cmp(&b.pos.get_x()))
            .then_with(|| a.pos.get_z().cmp(&b.pos.get_z()))
    };
    full_blocks.sort_by(cmp);
    other_blocks.sort_by(cmp);
    block_entities.sort_by(cmp);

    let mut result =
        Vec::with_capacity(full_blocks.len() + other_blocks.len() + block_entities.len());
    result.extend(full_blocks);
    result.extend(other_blocks);
    result.extend(block_entities);
    result
}

fn int_list_tag(values: &[i32]) -> Tag {
    Tag::List(ListTag {
        list: values
            .iter()
            .map(|&v| Tag::Int(IntTag::value_of(v)))
            .collect(),
    })
}

fn double_list_tag(values: &[f64]) -> Tag {
    Tag::List(ListTag {
        list: values
            .iter()
            .map(|&v| Tag::Double(DoubleTag::value_of(v)))
            .collect(),
    })
}

fn get_list_slice<'a>(tag: &'a CompoundTag, key: &str) -> &'a [Tag] {
    match tag.get(key) {
        Some(Tag::List(list)) => list.list.as_slice(),
        _ => &[],
    }
}

fn list_int_or(list: &[Tag], index: usize, default: i32) -> i32 {
    match list.get(index) {
        Some(Tag::Int(v)) => v.value,
        _ => default,
    }
}

fn list_double_or(list: &[Tag], index: usize, default: f64) -> f64 {
    match list.get(index) {
        Some(Tag::Double(v)) => v.value,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_pos_matches_paper_rotations_and_mirrors() {
        let p = BlockPos::new(2, 5, 3);
        let pivot = BlockPos::new(1, 0, 1);

        assert_eq!(
            StructureTemplate::transform_pos(p, Mirror::None, Rotation::None, pivot),
            BlockPos::new(2, 5, 3)
        );
        assert_eq!(
            StructureTemplate::transform_pos(p, Mirror::None, Rotation::Clockwise90, pivot),
            BlockPos::new(-1, 5, 2)
        );
        assert_eq!(
            StructureTemplate::transform_pos(p, Mirror::None, Rotation::Clockwise180, pivot),
            BlockPos::new(0, 5, -1)
        );
        assert_eq!(
            StructureTemplate::transform_pos(p, Mirror::None, Rotation::Counterclockwise90, pivot),
            BlockPos::new(3, 5, 0)
        );
        assert_eq!(
            StructureTemplate::transform_pos(p, Mirror::LeftRight, Rotation::None, pivot),
            BlockPos::new(2, 5, -3)
        );
        assert_eq!(
            StructureTemplate::transform_pos(p, Mirror::FrontBack, Rotation::None, pivot),
            BlockPos::new(-2, 5, 3)
        );
    }

    #[test]
    fn get_zero_position_with_transform_matches_paper() {
        let origin = BlockPos::new(10, 64, 20);
        assert_eq!(
            StructureTemplate::get_zero_position_with_transform(
                origin,
                Mirror::None,
                Rotation::Clockwise90,
                4,
                6
            ),
            BlockPos::new(15, 64, 20)
        );
        assert_eq!(
            StructureTemplate::get_zero_position_with_transform(
                origin,
                Mirror::FrontBack,
                Rotation::Clockwise180,
                4,
                6
            ),
            BlockPos::new(10, 64, 25)
        );
    }

    #[test]
    fn save_and_load_round_trip_single_and_multi_palette() {
        let mut be_nbt = CompoundTag::new();
        be_nbt.put_string("id", "minecraft:chest");

        let p0 = Palette::new(vec![
            StructureBlockInfo::new(
                BlockPos::new(0, 0, 0),
                BlockState::of(block_id("minecraft:stone")),
                None,
            ),
            StructureBlockInfo::new(
                BlockPos::new(1, 0, 0),
                BlockState::of(block_id("minecraft:chest")),
                Some(be_nbt.clone()),
            ),
        ]);
        let p1 = Palette::new(vec![
            StructureBlockInfo::new(
                BlockPos::new(0, 0, 0),
                BlockState::of(block_id("minecraft:cobblestone")),
                None,
            ),
            StructureBlockInfo::new(
                BlockPos::new(1, 0, 0),
                BlockState::of(block_id("minecraft:chest")),
                Some(be_nbt.clone()),
            ),
        ]);
        let entity =
            StructureEntityInfo::new([0.5, 1.0, 0.5], BlockPos::new(0, 1, 0), be_nbt.clone());
        let original = StructureTemplate::from_parts([2, 2, 1], vec![p0, p1], vec![entity]);

        let mut tag = CompoundTag::new();
        original.save(&mut tag);
        assert!(tag.contains(StructureTemplate::PALETTE_LIST_TAG));
        assert!(!tag.contains(StructureTemplate::PALETTE_TAG));

        let mut loaded = StructureTemplate::new();
        loaded.load(&tag);
        assert_eq!(loaded.get_size(), [2, 2, 1]);
        assert_eq!(loaded.palettes().len(), 2);
        assert_eq!(loaded.palettes()[0].blocks().len(), 2);
        assert_eq!(
            loaded.palettes()[1].blocks()[0].state.block(),
            block_id("minecraft:cobblestone")
        );
        assert_eq!(loaded.entity_info_list().len(), 1);
    }

    #[test]
    fn simple_palette_unknown_id_returns_air() {
        let palette = SimplePalette::new();
        assert_eq!(palette.state_for(999), air_state());
    }
}
