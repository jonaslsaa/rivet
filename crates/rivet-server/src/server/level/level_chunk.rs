//! Port of `net.minecraft.world.level.chunk.LevelChunk` (MC 26.2) — the
//! in-memory chunk data structure, minimal M1 slice (issue #156).
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/level/chunk/LevelChunk.java`.
//!
//! Owned by the `mc.world.level.chunk.access` manifest unit (#183). This slice
//! ports only what issue #100/#156 need: the chunk identity (`ChunkPos`) and
//! the deterministic superflat content — the 24 sections + the three client
//! heightmaps + the full-sky light data produced by
//! `rivet-world::superflat::build_superflat`. The block/fluid mutators,
//! block-entity map, and the full `ChunkAccess` surface are deferred with the
//! owning unit.
//!
//! The content is instantiated with thin local wrappers over the dense
//! block-state / biome global ids — `StateId(pub u16)` (a global block-state
//! id, air = 0, stone = 1) and `BiomeId(pub u16)` (the alphabetically dense
//! biome registry id, plains = 40). This is the same value pair the
//! `rivet-world` golden test drives, so the wire bytes of the M1 spawn chunk
//! byte-compare against the committed #153 capture fixture. The local
//! `StateId` mirrors `rivet-registry::generated::StateId` (which PR #244 made
//! available to `rivet-server` via the `blocks` feature) and `BiomeId` has no
//! generated newtype equivalent — the generated `biomes.rs` exposes only a
//! name→id map — so the pair stays local, exactly as in
//! `rivet-world/tests/superflat_chunk_golden.rs`, until the owning unit replaces
//! them.
//!
//! RivetTodo(#183): the `ChunkAccess` base surface (`getBlockState` at absolute
//! height, section accessors, `setSectionIndex`), the block-entity map, and the
//! mutators are deferred to the owning chunk.access unit, which replaces this
//! slice's content (including the local `StateId` wrapper) with the real
//! generated chunk data.

use rivet_registry::core::ChunkPos;
use rivet_world::chunk::palette::GlobalIdMap;
use rivet_world::chunk::strategy::Strategy;
use rivet_world::superflat::{
    BlockFlags, SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y, SuperflatChunkContent, build_superflat,
};

/// A dense global block-state id (index into the global palette). `rivet-registry`
///'s generated table is the canonical source (`BLOCK_STATE_COUNT = 32366`, air =
/// state 0, stone = state 1 — the default states); the M1 superflat content is
/// built against this thin wrapper, identical in shape to the generated
/// `rivet-registry::generated::StateId` (available to `rivet-server` since
/// #244 enabled `blocks`), so the slice stays coupled to the same value until
/// the owning chunk.access unit replaces it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StateId(pub u16);

/// A dense biome global id. The `minecraft:worldgen/biome` registry is
/// alphabetically dense (`0..66`; plains = 40) — the generated `biomes.rs`
/// table is the canonical source, but it exposes a name→id map, not a newtype,
/// so the superflat content (a single plains biome, id 40) is built against
/// this thin wrapper. The `mc.world.level.biome.core` unit replaces it with
/// the real `Holder<Biome>` container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BiomeId(pub u16);

/// `net.minecraft.world.level.chunk.LevelChunk` — the world's loaded chunk
/// content plus its chunk position.
pub struct LevelChunk {
    pos: ChunkPos,
    content: SuperflatChunkContent<StateId, BiomeId>,
}

impl LevelChunk {
    /// `new LevelChunk(ServerLevel, ChunkPos)` — builds the deterministic
    /// single-stone superflat content for the given chunk.
    pub fn new(pos: ChunkPos) -> Self {
        let content = build_superflat_content();
        LevelChunk { pos, content }
    }

    /// `LevelChunk.getPos()`.
    pub fn pos(&self) -> ChunkPos {
        self.pos
    }

    /// `LevelChunk.getX()`.
    pub fn get_x(&self) -> i32 {
        self.pos.x()
    }

    /// `LevelChunk.getZ()`.
    pub fn get_z(&self) -> i32 {
        self.pos.z()
    }

    /// The superflat content — sections, heightmaps, light — ready for the
    /// #94 `ClientboundLevelChunkWithLightPacket` body.
    pub fn content(&self) -> &SuperflatChunkContent<StateId, BiomeId> {
        &self.content
    }

    /// `LevelChunk.getMinY()` — the overworld superflat min Y (the world's
    /// `LevelHeightAccessor.getMinY()`).
    pub fn get_min_y(&self) -> i32 {
        SUPERFLAT_MIN_Y
    }

    /// `LevelChunk.getHeight()` — the overworld superflat world height.
    pub fn get_height(&self) -> i32 {
        SUPERFLAT_HEIGHT
    }
}

/// Builds the deterministic single-stone superflat content (air = state 0,
/// stone = state 1, plains biome = id 40) with the dense global id maps —
/// byte-identical to the `rivet-world` golden test's `build_superflat` output.
fn build_superflat_content() -> SuperflatChunkContent<StateId, BiomeId> {
    #[derive(Clone, Copy)]
    struct BlockStateGlobalMap;
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
    struct BiomeGlobalMap;
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

    fn block_state_strategy() -> Strategy<StateId> {
        Strategy::create_for_block_states(Box::new(BlockStateGlobalMap))
    }
    fn biome_strategy() -> Strategy<BiomeId> {
        Strategy::create_for_biomes(Box::new(BiomeGlobalMap))
    }

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
    let flags = BlockFlags {
        is_air: &is_air,
        blocks_motion: &blocks_motion,
        has_fluid: &has_fluid,
        is_leaves: &is_leaves,
    };

    build_superflat(
        block_state_strategy(),
        biome_strategy(),
        StateId(0),
        StateId(1),
        BiomeId(40),
        flags,
    )
}
