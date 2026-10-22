//! Native structure pieces. Buried treasure supports placement; mineshafts
//! support graph generation/persistence and refuse terrain-writing placement.
//! Other registered subtypes refuse decoding instead of silently disappearing.

use super::{BoundingBox, StructureDefinition, StructureError};
use crate::{block::blocks::Blocks, level::WorldGenLevel, levelgen::heightmap::Types};
use rivet_nbt::{compound_tag::CompoundTag, nbt_ops::NbtOps};
use rivet_registry::{
    block_state::BlockState,
    block_state_properties::BlockStateProperties,
    core::{BlockPos, ChunkPos, Direction},
};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_util::RandomSource;

/// The actual Paper transformer/CraftChest boundary. No blanket world adapter:
/// an authority must implement entity/event-aware placement before starts run.
pub trait StructurePlacementAuthority {
    fn level(&mut self) -> &mut dyn WorldGenLevel;
    fn begin_structure(
        &mut self,
        structure: &StructureDefinition,
        bounds: BoundingBox,
        chunk: ChunkPos,
    ) -> Result<(), StructureError>;
    fn finish_structure(&mut self) -> Result<(), StructureError>;
    /// Abandon a partially begun/failed scope WITHOUT saving staged entities.
    /// Must be non-panicking, idempotent, safe after partial begin/finish, and release
    /// transformer/region links. Keep already-applied terrain/piece/RNG effects
    /// and any entities already committed before a finish failure. This is
    /// resource cleanup, not rollback and never the successful commit path.
    fn abort_structure(&mut self);
    fn place_loot_chest(
        &mut self,
        pos: BlockPos,
        state: BlockState,
        loot_table: &str,
        seed: i64,
    ) -> Result<(), StructureError>;

    /// Attest the complete block, Craft spawner and entity-placement authority
    /// before an intersecting mineshaft start performs ANY mutation.
    fn require_mineshaft_placement(&self) -> Result<(), StructureError> {
        Err(StructureError::PlacementUnavailable(
            "mineshaft block/spawner/minecart placement authority is unavailable".into(),
        ))
    }

    /// True only while a genuine TransformerLevelAccessor owns fluid and shape
    /// bookkeeping. Ordinary direct WorldGenRegion writes use the default.
    fn structure_updates_handled(&self) -> bool {
        false
    }

    /// Paper's placeCraftSpawner snapshot/placed-block-entity bridge. Its RNG
    /// belongs to the placed world, never the feature random passed to pieces.
    fn place_cave_spider_spawner(
        &mut self,
        _pos: BlockPos,
        _flags: u32,
    ) -> Result<(), StructureError> {
        Err(StructureError::PlacementUnavailable(
            "Craft cave-spider spawner placement is unavailable".into(),
        ))
    }

    /// Genuine CHEST_MINECART.create(CHUNK_GENERATION), then initial position
    /// (.5 offsets), conditional nextLong loot seed, and addFreshEntity. A null
    /// construction is valid only when actual entity feature checks reject it;
    /// an unimplemented entity runtime must return an error, not pretend null.
    fn place_loot_minecart<R: RandomSource>(
        &mut self,
        _pos: BlockPos,
        _loot_table: &str,
        _random: &mut R,
    ) -> Result<(), StructureError> {
        Err(StructureError::PlacementUnavailable(
            "chest-minecart construction/loot/admission is unavailable".into(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructurePieceData {
    pub bounding_box: BoundingBox,
    pub gen_depth: i32,
    pub orientation: Option<Direction>,
}

impl StructurePieceData {
    pub fn load(tag: &CompoundTag) -> Result<Self, StructureError> {
        let bounds: [i32; 6] = tag
            .get("BB")
            .and_then(|value| NbtOps::instance().get_int_stream(value).result().cloned())
            .and_then(|v| v.as_slice().try_into().ok())
            .ok_or(StructureError::MalformedBoundingBox)?;
        let orientation = tag.get_int_or("O", 0);
        Ok(Self {
            bounding_box: BoundingBox::new(bounds),
            gen_depth: tag.get_int_or("GD", 0),
            orientation: (orientation != -1).then(|| Direction::from_2d_data_value(orientation)),
        })
    }

    pub fn create_tag(&self, piece_id: &str) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_string("id", piece_id);
        tag.put_int_array("BB", self.bounding_box.to_array().to_vec());
        tag.put_int("O", self.orientation.map_or(-1, |d| d.get_2d_data_value()));
        tag.put_int("GD", self.gen_depth);
        tag
    }

    pub fn world_pos(&self, x: i32, y: i32, z: i32) -> BlockPos {
        let bb = self.bounding_box;
        let wx = match self.orientation {
            Some(Direction::North | Direction::South) => bb.min_x.wrapping_add(x),
            Some(Direction::West) => bb.max_x.wrapping_sub(z),
            Some(Direction::East) => bb.min_x.wrapping_add(z),
            _ => x,
        };
        let wy = if self.orientation.is_none() {
            y
        } else {
            y.wrapping_add(bb.min_y)
        };
        let wz = match self.orientation {
            Some(Direction::North) => bb.max_z.wrapping_sub(z),
            Some(Direction::South) => bb.min_z.wrapping_add(z),
            Some(Direction::West | Direction::East) => bb.min_z.wrapping_add(x),
            _ => z,
        };
        BlockPos::new(wx, wy, wz)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StructurePiece {
    BuriedTreasure(StructurePieceData),
    Mineshaft(super::mineshaft::MineshaftPiece),
}

impl StructurePiece {
    pub fn buried_treasure(pos: BlockPos) -> Self {
        Self::BuriedTreasure(StructurePieceData {
            bounding_box: BoundingBox::at(pos),
            gen_depth: 0,
            orientation: None,
        })
    }

    pub fn load(tag: &CompoundTag) -> Result<Self, StructureError> {
        let id = tag.get_string_or("id", "").to_lowercase();
        match id.strip_prefix("minecraft:").unwrap_or(&id) {
            "btp" => Ok(Self::BuriedTreasure(StructurePieceData::load(tag)?)),
            "msroom" | "mscorridor" | "mscrossing" | "msstairs" => {
                Ok(Self::Mineshaft(super::mineshaft::MineshaftPiece::load(
                    tag,
                    id.strip_prefix("minecraft:").unwrap_or(&id),
                )?))
            }
            _ => Err(StructureError::UnsupportedPiece(id)),
        }
    }

    pub fn data(&self) -> &StructurePieceData {
        match self {
            Self::BuriedTreasure(data) => data,
            Self::Mineshaft(piece) => &piece.data,
        }
    }
    pub fn data_mut(&mut self) -> &mut StructurePieceData {
        match self {
            Self::BuriedTreasure(data) => data,
            Self::Mineshaft(piece) => &mut piece.data,
        }
    }
    pub fn create_tag(&self) -> CompoundTag {
        match self {
            Self::BuriedTreasure(data) => data.create_tag("minecraft:btp"),
            Self::Mineshaft(piece) => piece.create_tag(),
        }
    }

    pub(super) fn validate_placement(
        &self,
        authority: &impl StructurePlacementAuthority,
    ) -> Result<(), StructureError> {
        match self {
            Self::BuriedTreasure(_) => Ok(()),
            Self::Mineshaft(_) => authority.require_mineshaft_placement(),
        }
    }

    /// Pinned BuriedTreasurePiece.postProcess. Fills are intentionally NOT
    /// clipped by chunkBB; only createChest checks that box in Paper.
    pub fn post_process<R: RandomSource>(
        &mut self,
        authority: &mut impl StructurePlacementAuthority,
        random: &mut R,
        chunk_bb: BoundingBox,
    ) -> Result<(), StructureError> {
        match self {
            Self::Mineshaft(piece) => {
                super::mineshaft_placement::post_process(piece, authority, random, chunk_bb)
            }
            Self::BuriedTreasure(data) => {
                let mut pos = BlockPos::new(
                    data.bounding_box.min_x,
                    authority.level().get_height_at(
                        Types::OceanFloorWg,
                        data.bounding_box.min_x,
                        data.bounding_box.min_z,
                    ),
                    data.bounding_box.min_z,
                );
                while pos.get_y() > authority.level().get_min_y() {
                    let current = authority.level().get_block_state(&pos);
                    let below = authority.level().get_block_state(&pos.below());
                    if matches!(
                        below.block().name(),
                        "minecraft:sandstone"
                            | "minecraft:stone"
                            | "minecraft:andesite"
                            | "minecraft:granite"
                            | "minecraft:diorite"
                    ) {
                        let soft = if !current.is_air() && !is_liquid(current) {
                            current
                        } else {
                            Blocks::SAND.default_block_state()
                        };
                        for direction in Direction::stream() {
                            let relative = pos.relative(&direction);
                            let state = authority.level().get_block_state(&relative);
                            if state.is_air() || is_liquid(state) {
                                let lower = authority.level().get_block_state(&relative.below());
                                let fill = if (lower.is_air() || is_liquid(lower))
                                    && direction != Direction::Up
                                {
                                    below
                                } else {
                                    soft
                                };
                                authority.level().set_block(&relative, fill, 3);
                            }
                        }
                        data.bounding_box = BoundingBox::at(pos);
                        if chunk_bb.is_inside(&pos)
                            && authority.level().get_block_state(&pos).block() != Blocks::CHEST.id()
                        {
                            let state = reorient_chest(authority.level(), &pos);
                            let seed = random.next_long();
                            authority.place_loot_chest(
                                pos,
                                state,
                                "minecraft:chests/buried_treasure",
                                seed,
                            )?;
                        }
                        return Ok(());
                    }
                    pos = pos.below();
                }
                Ok(())
            }
        }
    }
}

fn is_liquid(state: BlockState) -> bool {
    state.block() == Blocks::WATER.id() || state.block() == Blocks::LAVA.id()
}

pub(super) fn reorient_chest(level: &dyn WorldGenLevel, pos: &BlockPos) -> BlockState {
    let state = Blocks::CHEST.default_block_state();
    let mut solid = None;
    for direction in [
        Direction::North,
        Direction::East,
        Direction::South,
        Direction::West,
    ] {
        let neighbor = level.get_block_state(&pos.relative(&direction));
        if neighbor.block() == Blocks::CHEST.id() {
            return state;
        }
        if neighbor.solid_render() {
            if solid.is_some() {
                solid = None;
                break;
            }
            solid = Some(direction);
        }
    }
    let facing = if let Some(direction) = solid {
        direction.get_opposite()
    } else {
        let mut lock = Direction::North;
        if level.get_block_state(&pos.relative(&lock)).solid_render() {
            lock = lock.get_opposite();
        }
        if level.get_block_state(&pos.relative(&lock)).solid_render() {
            lock = lock.get_clock_wise();
        }
        if level.get_block_state(&pos.relative(&lock)).solid_render() {
            lock = lock.get_opposite();
        }
        lock
    };
    state
        .set_value(BlockStateProperties::HORIZONTAL_FACING, facing)
        .expect("chest facing")
}
