//! Port of `net.minecraft.world.level.chunk.LevelChunk` (MC 26.2) — the
//! in-memory chunk data structure, M1 server slice (issue #156).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/chunk/LevelChunk.java`.
//!
//! Owned by the `mc.world.level.chunk.access` manifest unit (#183). This slice
//! is the server-side value wrapper: it owns a generic
//! `rivet_world::chunk::level_chunk::LevelChunk<StateId, BiomeId, StructureKey>`
//! built from the deterministic superflat content (issue #100) and exposes the
//! read spine the M1 join path needs (`pos`/`get_min_y`/`get_height`/the
//! chunk-data accessors). The `ChunkAccess` base surface, the block/fluid
//! mutators, and the block-entity map are provided by the rivet-world value;
//! the remaining deferred items are listed on the rivet-world `LevelChunk`
//! module doc.
//!
//! The content uses the canonical generated block-state `StateId` directly and
//! a thin `BiomeId(pub u16)` wrapper over the generated biome registry ids
//! (plains = 40). This is the same value pair the `rivet-world` golden test
//! drives, so the wire bytes of the M1 spawn chunk byte-compare against the
//! committed #153 capture fixture. Biomes need the wrapper only because the
//! generated table exposes dense name/id maps rather than a newtype.
//!
//! RivetTodo(#184): the send path carries the deterministic superflat light
//! (computed once at construction from `rivet_world::superflat`) instead of
//! the `LevelLightEngine`; the lighting engine unit replaces it when it lands.

use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_protocol::protocol::game::level_chunk_packet_data::LevelChunkPacketData;
use rivet_protocol::protocol::game::light_update_packet_data::LightUpdatePacketData;
use rivet_registry::core::ChunkPos;
use rivet_registry::generated::block_behaviors::{
    BEHAVIOR_FLAG_FLUID_EMPTY, BEHAVIOR_FLAG_RANDOM_TICKING, behavior_of,
};
/// Canonical dense global block-state id from the generated registry.
pub use rivet_registry::generated::block_states::StateId;
use rivet_world::chunk::level_chunk::LevelChunk as WorldLevelChunk;
use rivet_world::chunk::level_chunk_section::LevelChunkSection;
use rivet_world::chunk::paletted_container_factory::PalettedContainerFactory;
use rivet_world::chunk::strategy::Strategy;
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::level::LevelHeightAccessor;
use rivet_world::level::height_accessor::create as create_accessor;
use rivet_world::levelgen::heightmap::Types;
use rivet_world::superflat::{SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y, build_superflat};

/// The chunk's structure-key type. Rivet has no `Structure` type yet, so the
/// chunk is instantiated with the unit key (no structures). RivetTodo(#185):
/// the real `Structure` value type keys the structure maps when the worldgen
/// structure unit lands.
pub type StructureKey = ();

/// A dense biome global id. The `minecraft:worldgen/biome` registry is
/// alphabetically dense (`0..66`; plains = 40) — the generated `biomes.rs`
/// table is the canonical source, but it exposes a name→id map, not a newtype,
/// so the superflat content (a single plains biome, id 40) is built against
/// this thin wrapper. The `mc.world.level.biome.core` unit replaces it with
/// the real `Holder<Biome>` container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BiomeId(pub u16);

impl TryFrom<u16> for BiomeId {
    type Error = u16;

    fn try_from(id: u16) -> Result<Self, Self::Error> {
        (usize::from(id) < rivet_registry::generated::biomes::BIOME_COUNT)
            .then_some(Self(id))
            .ok_or(id)
    }
}

/// `net.minecraft.world.level.chunk.LevelChunk` — the world's loaded chunk
/// content plus its chunk position.
///
/// Wraps the generic rivet-world `LevelChunk<StateId, BiomeId, StructureKey>`
/// value (OWNERSHIP.md — no inheritance; the server chunk *contains* the value
/// chunk). All reads delegate to it.
pub struct LevelChunk {
    /// The generic rivet-world chunk value.
    chunk: WorldLevelChunk<StateId, BiomeId, StructureKey>,
    /// The deterministic superflat light payload, computed once at
    /// construction and cloned per send (issue #184). Java queries the
    /// `LevelLightEngine` per packet; the engine is not ported, so the M1 send
    /// path carries this fixed payload. Building it on every encode would
    /// reallocate the 26 sky/block layer arrays per chunk per player, so the
    /// prebuilt value is reused instead.
    light_data: LightUpdatePacketData,
}

impl LevelChunk {
    /// `new LevelChunk(ServerLevel, ChunkPos)` — builds the deterministic
    /// single-stone superflat content for the given chunk.
    pub fn new(pos: ChunkPos) -> Self {
        let content = superflat_content();
        let light_data = content.light_data;
        let height_accessor = create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT);
        let mut chunk = WorldLevelChunk::new(
            pos,
            UpgradeData::empty(height_accessor.get_sections_count() as usize),
            height_accessor,
            &container_factory(),
            0,
            Some(content.sections),
            StateId(0),
            // The same predicates as the superflat build above: the server's
            // block state is the local `StateId` newtype, air is 0, everything
            // else blocks motion, and nothing here has a fluid or is leaves.
            &|s: &StateId| rivet_world::levelgen::heightmap::StateFlags {
                is_air: s.0 == 0,
                blocks_motion: s.0 != 0,
                has_fluid: false,
                is_leaves: false,
            },
        );
        // The constructor primes `FINAL_HEIGHTMAPS` (which includes the three
        // client types) as unprimed (all-zero) entries; `set_heightmap` fills
        // the client types with the deterministic superflat data the golden
        // fixture pins.
        for (ty, raw) in content.heightmaps {
            chunk.set_heightmap(Types::from_protocol(ty), &raw);
        }
        LevelChunk { chunk, light_data }
    }

    /// The prebuilt superflat light payload — a clone of the value computed
    /// once at construction (the packet body takes it by value).
    pub fn light_data(&self) -> LightUpdatePacketData {
        self.light_data.clone()
    }

    /// `LevelChunk.getPos()`.
    pub fn pos(&self) -> ChunkPos {
        self.chunk.get_pos()
    }

    /// `LevelChunk.getX()`.
    pub fn get_x(&self) -> i32 {
        self.chunk.get_x()
    }

    /// `LevelChunk.getZ()`.
    pub fn get_z(&self) -> i32 {
        self.chunk.get_z()
    }

    /// `LevelChunk.getMinY()` — the overworld superflat min Y (the world's
    /// `LevelHeightAccessor.getMinY()`).
    pub fn get_min_y(&self) -> i32 {
        self.chunk.get_min_y()
    }

    /// `LevelChunk.getHeight()` — the overworld superflat world height.
    pub fn get_height(&self) -> i32 {
        self.chunk.get_height()
    }

    /// The three `Usage.CLIENT` heightmaps as the `LevelChunkPacketData`
    /// `(HeightmapType, long[])` pairs, in the client `EnumMap` order — the
    /// `#94 ClientboundLevelChunkWithLightPacket` heightmap payload.
    pub fn client_heightmaps(&self) -> Vec<(HeightmapType, Vec<i64>)> {
        self.chunk.client_heightmaps()
    }

    /// `LevelChunk.getSections()` — the deterministic superflat content's 24
    /// sections (384 / 16, minY -64), section 0 (Y=-4) holding the stone layer.
    pub fn get_sections(&self) -> &[LevelChunkSection<StateId, BiomeId>] {
        self.chunk.get_sections()
    }

    /// The opaque sections buffer — the `[bits][palette][raw]` wire bytes of
    /// every section concatenated (Java `calculateChunkSize` +
    /// `extractChunkData`).
    pub fn sections_buffer(&self) -> Vec<u8> {
        self.chunk.sections_buffer()
    }

    /// `new ClientboundLevelChunkPacketData(levelChunk, null)` — the send
    /// payload (no block entities).
    pub fn chunk_packet_data(&self) -> LevelChunkPacketData {
        LevelChunkPacketData::new(self.client_heightmaps(), self.sections_buffer(), vec![])
    }

    /// `ChunkAccess.getBlockState(int, int, int)` — the Paper `getBlockStateFinal`
    /// read through the base.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> StateId {
        self.chunk.get_block_state(x, y, z)
    }
}

/// The dense global-id maps the superflat content is built against: air =
/// state 0, stone = state 1, plains biome = id 40 — the exact pair the
/// `rivet-world` golden test drives, so the wire bytes byte-compare.
mod maps {
    use super::{BiomeId, StateId};
    use rivet_world::chunk::palette::GlobalIdMap;

    #[derive(Clone, Copy)]
    pub struct BlockStateGlobalMap;
    impl GlobalIdMap<StateId> for BlockStateGlobalMap {
        fn get_id(&self, value: &StateId) -> i32 {
            value.0 as i32
        }
        fn by_id_or_throw(&self, id: i32) -> StateId {
            assert!((0..32366).contains(&id), "No value with id {id}");
            StateId(id as u16)
        }
        fn size(&self) -> i32 {
            32366 // `BLOCK_STATE_COUNT`.
        }
        fn by_id(&self, id: i32) -> Option<StateId> {
            (0..32366).contains(&id).then_some(StateId(id as u16))
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<StateId>> {
            Box::new(*self)
        }
    }

    #[derive(Clone, Copy)]
    pub struct BiomeGlobalMap;
    impl GlobalIdMap<BiomeId> for BiomeGlobalMap {
        fn get_id(&self, value: &BiomeId) -> i32 {
            value.0 as i32
        }
        fn by_id_or_throw(&self, id: i32) -> BiomeId {
            assert!((0..66).contains(&id), "No value with id {id}");
            BiomeId(id as u16)
        }
        fn size(&self) -> i32 {
            66 // the 26.2 biome registry (plains = 40, alphabetical).
        }
        fn by_id(&self, id: i32) -> Option<BiomeId> {
            (0..66).contains(&id).then_some(BiomeId(id as u16))
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<BiomeId>> {
            Box::new(*self)
        }
    }
}

/// The `PalettedContainerFactory` the `LevelChunk` constructor uses for any
/// default (all-air) sections.
fn container_factory() -> PalettedContainerFactory<StateId, BiomeId> {
    let (block_strategy, biome_strategy) = strategies();
    PalettedContainerFactory::new(block_strategy, StateId(0), biome_strategy, BiomeId(40))
}

fn strategies() -> (Strategy<StateId>, Strategy<BiomeId>) {
    (
        Strategy::create_for_block_states(Box::new(maps::BlockStateGlobalMap)),
        Strategy::create_for_biomes(Box::new(maps::BiomeGlobalMap)),
    )
}

/// Builds the deterministic single-stone superflat chunk content (air = state
/// 0, stone = state 1, plains biome = id 40) — byte-identical to the
/// `rivet-world` golden test's `build_superflat` output. The light payload is
/// retained by `LevelChunk::new` (prebuilt once, cloned per send).
fn superflat_content() -> rivet_world::superflat::SuperflatChunkContent<StateId, BiomeId> {
    let (block_strategy, biome_strategy) = strategies();

    // The superflat air + stone content: air (state 0) is air, stone (state 1)
    // blocks motion, neither has a fluid nor is leaves — the exact predicates
    // the `rivet-world` golden test drives.
    fn is_air(s: &StateId) -> bool {
        s.0 == 0
    }
    fn blocks_motion(s: &StateId) -> bool {
        s.0 != 0
    }
    fn has_fluid(_s: &StateId) -> bool {
        false
    }
    fn is_leaves(_s: &StateId) -> bool {
        false
    }
    // `state.isRandomlyTicking()` — the generated behavior-table flag (air +
    // stone are both non-randomly-ticking, matching the table).
    fn is_randomly_ticking(s: &StateId) -> bool {
        behavior_of(*s) & BEHAVIOR_FLAG_RANDOM_TICKING != 0
    }
    // `state.getFluidState().isEmpty()` — the generated behavior-table flag
    // (air + stone both carry no fluid, matching the table).
    fn fluid_is_empty(s: &StateId) -> bool {
        behavior_of(*s) & BEHAVIOR_FLAG_FLUID_EMPTY != 0
    }
    // `state.getFluidState().isRandomlyTicking()` — exact for air + stone (no
    // fluid to tick).
    //
    // The generated behavior table has no fluid-random-tick flag; this
    // predicate is exact for the superflat content (air + stone have no fluid).
    fn fluid_is_randomly_ticking(_s: &StateId) -> bool {
        false
    }
    // `CollisionUtil.isSpecialCollidingBlock(state)` — exact for air + stone
    // (neither has a large collision shape nor is `MOVING_PISTON`).
    //
    // The generated behavior table has no special-colliding flag; this
    // predicate is exact for the superflat content (air + stone never match).
    fn is_special_colliding(_s: &StateId) -> bool {
        false
    }
    let flags = rivet_world::superflat::BlockFlags {
        is_air: &is_air,
        blocks_motion: &blocks_motion,
        has_fluid: &has_fluid,
        is_leaves: &is_leaves,
        is_randomly_ticking: &is_randomly_ticking,
        fluid_is_empty: &fluid_is_empty,
        fluid_is_randomly_ticking: &fluid_is_randomly_ticking,
        is_special_colliding: &is_special_colliding,
    };

    build_superflat(
        block_strategy,
        biome_strategy,
        StateId(0),
        StateId(1),
        BiomeId(40),
        flags,
    )
}
