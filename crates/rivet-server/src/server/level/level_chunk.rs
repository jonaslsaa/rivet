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
//! The block-entity and structure-reference data are not duplicated here
//! (#537): the rivet-world base's `pendingBlockEntities` insertion-ordered map
//! and its `StructureAccess` reference map are the single runtime authority.
//! The packet path materializes the current authority through the merged #520
//! pure materializer (see [`LevelChunk::chunk_packet_data`]).
//!
//! The content uses the canonical generated block-state `StateId` directly and
//! a thin `BiomeId` wrapper over the generated biome registry ids
//! (plains = 40). This is the same value pair the `rivet-world` golden test
//! drives, so the wire bytes of the M1 spawn chunk byte-compare against the
//! committed #153 capture fixture. Biomes need the wrapper only because the
//! generated table exposes dense name/id maps rather than a newtype.
//!
//! RivetTodo(#184): the send path carries the deterministic superflat light
//! (computed once at construction from `rivet_world::superflat`) instead of
//! the `LevelLightEngine`; the lighting engine unit replaces it when it lands.

use rivet_nbt::compound_tag::CompoundTag;
use rivet_protocol::protocol::game::heightmap_types::HeightmapType;
use rivet_protocol::protocol::game::level_chunk_packet_data::{
    BlockEntityInfo, LevelChunkPacketData,
};
use rivet_protocol::protocol::game::light_update_packet_data::LightUpdatePacketData;
use rivet_registry::Identifier;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::block_behaviors::{
    BEHAVIOR_FLAG_FLUID_EMPTY, BEHAVIOR_FLAG_RANDOM_TICKING, behavior_of,
};
/// Canonical dense global block-state id from the generated registry.
pub use rivet_registry::generated::block_states::StateId;
use rivet_world::block::Block;
use rivet_world::chunk::data_layer::DataLayer;
use rivet_world::chunk::level_chunk::LevelChunk as WorldLevelChunk;
use rivet_world::chunk::level_chunk_section::LevelChunkSection;
use rivet_world::chunk::paletted_container_factory::PalettedContainerFactory;
use rivet_world::chunk::storage::ChunkReconstruction;
use rivet_world::chunk::storage::block_entity_materialization::{
    BlockEntityMaterialization, materialize_block_entities,
};
use rivet_world::chunk::storage::serializable_chunk_data::{
    BlockEntityChunkKind, SerializedBlockEntityOutcome, StructureReference,
    reconstruct_block_entities,
};
use rivet_world::chunk::strategy::Strategy;
use rivet_world::chunk::upgrade_data::UpgradeData;
use rivet_world::level::LevelHeightAccessor;
use rivet_world::level::height_accessor::create as create_accessor;
use rivet_world::levelgen::heightmap::{StateFlags, Types};
use rivet_world::lighting::light_update_data::build_light_update_data;
use rivet_world::lighting::swmr_nibble_array::SwmrNibbleArray;
use rivet_world::superflat::{SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y, build_superflat};
use rivet_world::ticks::SavedTick;

/// The chunk's structure-key type — the structure `Identifier` the
/// `structures.References` map is keyed by, matching the #519
/// `ReconstructedLevelChunk` `S` parameter. Rivet has no `Structure` value
/// type yet (#369), so the chunk holds the reference map keyed by identifier
/// and `starts` remain an `UnsupportedStructures` boundary.
pub type StructureKey = Identifier;

/// A dense biome global id. The `minecraft:worldgen/biome` registry is
/// alphabetically dense (`0..66`; plains = 40) — the generated `biomes.rs`
/// table is the canonical source, but it exposes a name→id map, not a newtype,
/// so the superflat content (a single plains biome, id 40) is built against
/// this thin wrapper. The `mc.world.level.biome.core` unit replaces it with
/// the real `Holder<Biome>` container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BiomeId(pub(crate) u16);

impl BiomeId {
    pub const fn raw(self) -> u16 {
        self.0
    }
}

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
    /// The typed stored block ticks carried off the #519 reconstruction
    /// (`ChunkAccess.PackedTicks.blocks()`), owned here as tick-thread state.
    /// Nothing schedules, spawns, installs, or writes them (#370 defers the
    /// `LevelChunkTicks`/`ProtoChunkTicks` execution containers).
    stored_block_ticks: Vec<SavedTick<Block>>,
    /// The typed stored fluid ticks — same carry semantics as
    /// [`Self::stored_block_ticks`].
    stored_fluid_ticks: Vec<SavedTick<FluidId>>,
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
        LevelChunk {
            chunk,
            light_data,
            stored_block_ticks: Vec::new(),
            stored_fluid_ticks: Vec::new(),
        }
    }

    /// Reconstructed chunk → server `LevelChunk` — the #516 boot bridge.
    ///
    /// `reconstruct_runtime_chunk` (#383) produces a generic
    /// `LevelChunk<BlockState, BiomeId, Identifier>` whose sections carry the
    /// generated `BlockState`/`section_reconstruction::BiomeId` values; the
    /// server chunk stores the same dense global `StateId` (air = 0, stone = 1,
    /// ... — `BlockState::id()` IS the
    /// `rivet_registry::generated::block_states::StateId`) and a `u16`-backed
    /// `BiomeId`, so each section's containers are re-encoded against the
    /// server strategies with `map_values` — the byte-identical-on-wire
    /// conversion the packet path needs. The stored heightmaps/light nibbles/
    /// pending block entities are preserved by the value transform; the packet
    /// light payload is derived once through `to_vanilla_nibble` +
    /// `build_light_update_data` (the #184 send seam).
    ///
    /// The #519 auxiliary payloads are carried onto the server chunk as owned
    /// tick-thread state — [`ChunkReconstruction::stored_block_ticks`] /
    /// [`ChunkReconstruction::stored_fluid_ticks`] — without scheduling,
    /// spawning, materializing, or writing anything. Block entities and
    /// structure references are NOT duplicated here: the reconstruction installs
    /// them into the chunk's pending-map / `StructureAccess` runtime authority
    /// (#537), so the server chunk reads them straight off the base.
    ///
    /// The `ChunkReconstruction` diagnostics are consumed by the caller before
    /// this bridge: the boot rejects a non-empty set rather than silently
    /// installing a chunk whose content differs from what was stored.
    ///
    /// The conversion is fallible with a typed [`LevelChunkBridgeError`]: the
    /// #184 send seam panics on an unsupported persisted Starlight state, so
    /// that mismatch is rejected here first, and the `map_values` re-encode
    /// surfaces its error instead of the `.expect` that used to abort the
    /// process (defense-in-depth: the reconstructed and server strategies share
    /// the same dense global-id ladder, so a failure is hostile input).
    pub fn from_bridge(reconstruction: ChunkReconstruction) -> Result<Self, LevelChunkBridgeError> {
        let ChunkReconstruction {
            chunk: world_chunk,
            stored_block_ticks,
            stored_fluid_ticks,
            ..
        } = reconstruction;
        // Reject an unsupported persisted Starlight state before the #184 send
        // seam converts it: `to_vanilla_nibble` panics on `Other` (the packet
        // seam has no typed error surface), which would abort the process
        // instead of failing the boot with a `RegionBackedBootError`.
        if world_chunk
            .block_nibbles()
            .iter()
            .chain(world_chunk.sky_nibbles())
            .any(|nibble| nibble.has_unknown_state_visible())
        {
            return Err(LevelChunkBridgeError::UnsupportedLightState(
                UnsupportedLightState,
            ));
        }
        let (block_strategy, biome_strategy) = strategies();
        let world_chunk = world_chunk
            .map_values(
                block_strategy,
                biome_strategy,
                StateId(0),
                BiomeId(40),
                &|state: &BlockState| state.id(),
                &|biome: &rivet_world::chunk::storage::section_reconstruction::BiomeId| {
                    BiomeId(biome.0)
                },
                &|state: &StateId| state_flags(*state),
            )
            .map_err(LevelChunkBridgeError::PaletteMap)?;
        let light_data = light_data_from_nibbles(
            world_chunk.block_nibbles(),
            world_chunk.sky_nibbles(),
            world_chunk.get_height(),
        );
        Ok(LevelChunk {
            chunk: world_chunk,
            light_data,
            stored_block_ticks,
            stored_fluid_ticks,
        })
    }

    /// The prebuilt light payload — a clone of the value computed once at
    /// construction (the packet body takes it by value).
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
    /// payload. The block-entity list is materialized from the current pending
    /// authority (#537) through the merged #520 pure materializer: each tag in
    /// insertion order is resolved to its registry-grounded outcome, and the
    /// unpacked entries are turned into wire `BlockEntityInfo` values in
    /// authority order.
    ///
    /// The wire list order is deliberately the authority's insertion order:
    /// Paper iterates its live `blockEntities` map, whose fastutil probe order
    /// is not insertion order. RivetTodo(#537): model Paper's probe order in
    /// the send path once the live block-entity map unit lands.
    ///
    /// Refused entries are surfaced loudly, never silently dropped or
    /// fabricated: Paper does not send `keepPacked`/pending or invalid-type
    /// entries in the chunk packet (they never join the live block-entity map),
    /// and a resolved type whose update tag the port cannot reproduce is a
    /// hard boundary — so each refusal is printed here and the entry is skipped
    /// while the send path continues. The full ordered refusal stream is also
    /// available through [`Self::materialize_block_entities`] for callers that
    /// need to act on it.
    pub fn chunk_packet_data(&self) -> LevelChunkPacketData {
        let materialization = self.materialize_block_entities();
        let infos: Vec<BlockEntityInfo> = materialization
            .infos
            .into_iter()
            .filter_map(|result| match result {
                Ok(info) => Some(info),
                Err(error) => {
                    eprintln!("refusing to send block entity: {error}");
                    None
                }
            })
            .collect();
        // The entry-level refusals above are printed per entry; the recoverable
        // field-level drops the materializer surfaced must not be silent either
        // (Paper logs the field decode problem and continues).
        for diagnostic in &materialization.diagnostics {
            eprintln!("dropping field while materializing block entity: {diagnostic}");
        }
        LevelChunkPacketData::new(self.client_heightmaps(), self.sections_buffer(), infos)
    }

    /// `ChunkAccess.getBlockState(int, int, int)` — the Paper `getBlockStateFinal`
    /// read through the base.
    pub fn get_block_state(&self, x: i32, y: i32, z: i32) -> StateId {
        self.chunk.get_block_state(x, y, z)
    }

    /// The typed stored block ticks carried off the #519 reconstruction
    /// (`ChunkAccess.PackedTicks.blocks()`). Owned tick-thread state — never
    /// scheduled or executed (#370).
    pub fn stored_block_ticks(&self) -> &[SavedTick<Block>] {
        &self.stored_block_ticks
    }

    /// The typed stored fluid ticks (`ChunkAccess.PackedTicks.fluids()`). Same
    /// carry semantics as [`Self::stored_block_ticks`].
    pub fn stored_fluid_ticks(&self) -> &[SavedTick<FluidId>] {
        &self.stored_fluid_ticks
    }

    /// The pending block entities — the runtime authority (insertion-ordered,
    /// position-keyed; source order for the surviving positions, #537). The
    /// server chunk owns no duplicate snapshot: reads and packet materialization
    /// come from this map.
    pub fn pending_block_entities(&self) -> &indexmap::IndexMap<BlockPos, CompoundTag> {
        self.chunk.pending_block_entities()
    }

    /// `ChunkAccess.setBlockEntityNbt(CompoundTag)` — the runtime set/update
    /// mutator: installs or updates the entry at its corrected position
    /// (duplicate positions collapse last-wins in place, #537). A later
    /// `chunk_packet_data` reflects this mutation.
    pub fn set_block_entity_nbt(&mut self, entity_tag: CompoundTag) {
        self.chunk.set_block_entity_nbt(entity_tag);
    }

    /// `ProtoChunk.removeBlockEntity(BlockPos)`'s pending half — removes the
    /// position from the runtime authority, so a later `chunk_packet_data` no
    /// longer emits it (#537).
    pub fn remove_block_entity_nbt(&mut self, pos: &BlockPos) -> Option<CompoundTag> {
        self.chunk.remove_block_entity_nbt(pos)
    }

    /// The registry-grounded block-entity outcomes derived from the current
    /// authority (#537/#520): each tag in insertion order is resolved to its
    /// `SerializedBlockEntityOutcome` — unpacked entries resolve their
    /// `BlockEntityType`, `keepPacked`/proto entries stay pending, invalid ids
    /// surface as entry-local failures. Each outcome's `source_index` is its
    /// index in the surviving authority iteration — the `.values()` position
    /// after duplicate corrected positions collapsed last-wins in place — not
    /// an index into the original decoded NBT list.
    pub fn block_entity_outcomes(&self) -> Vec<SerializedBlockEntityOutcome> {
        let pos = self.pos();
        reconstruct_block_entities(
            &pos,
            self.chunk
                .pending_block_entities()
                .values()
                .cloned()
                .collect::<Vec<_>>()
                .as_slice(),
            BlockEntityChunkKind::Level,
        )
    }

    /// The merged #520 pure materializer over the current authority: resolve
    /// every pending tag to its outcome, then materialize each outcome into a
    /// wire `BlockEntityInfo` (or a typed refusal). Refusals are preserved, in
    /// authority order, so the caller decides how to surface them — the packet
    /// path refuses loudly rather than fabricating or dropping.
    pub fn materialize_block_entities(&self) -> BlockEntityMaterialization {
        let outcomes = self.block_entity_outcomes();
        materialize_block_entities(&outcomes)
    }

    /// The decoded `structures.References` after the >8-chunk distance filter
    /// (#369) — derived from the chunk's `StructureAccess` authority (#537), in
    /// deterministic key-insertion order.
    pub fn structures_references(&self) -> Vec<StructureReference> {
        self.chunk
            .get_all_references()
            .iter()
            .map(|(identifier, set)| StructureReference {
                identifier: identifier.clone(),
                references: set.iter().map(|r| *r as i64).collect(),
            })
            .collect()
    }

    /// `ChunkAccess.setAllReferences` — the structure-reference runtime mutator
    /// (#537): installs the decoded reference map into the chunk's
    /// `StructureAccess` authority, so a later [`Self::structures_references`]
    /// derivation reflects it. The caller's key order is preserved; duplicate
    /// references within a key dedupe like Java's `LongOpenHashSet`.
    pub fn set_all_references<I: IntoIterator<Item = (StructureKey, Vec<u64>)>>(
        &mut self,
        data: I,
    ) {
        self.chunk.set_all_references(data);
    }
}

/// The dense global-id maps the superflat content is built against: air =
/// state 0, stone = state 1, plains biome = id 40 — the exact pair the
/// `rivet-world` golden test drives, so the wire bytes byte-compare.
mod maps {
    use super::{BiomeId, StateId};
    use rivet_registry::generated::biomes::BIOME_COUNT;
    use rivet_registry::generated::block_states::BLOCK_STATE_COUNT;
    use rivet_world::chunk::palette::GlobalIdMap;

    #[derive(Clone, Copy)]
    pub struct BlockStateGlobalMap;
    impl GlobalIdMap<StateId> for BlockStateGlobalMap {
        fn get_id(&self, value: &StateId) -> i32 {
            value.0 as i32
        }
        fn by_id_or_throw(&self, id: i32) -> StateId {
            assert!(
                (0..i32::from(BLOCK_STATE_COUNT)).contains(&id),
                "No value with id {id}"
            );
            StateId(id as u16)
        }
        fn size(&self) -> i32 {
            i32::from(BLOCK_STATE_COUNT)
        }
        fn by_id(&self, id: i32) -> Option<StateId> {
            (0..i32::from(BLOCK_STATE_COUNT))
                .contains(&id)
                .then_some(StateId(id as u16))
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
            assert!(
                (0..BIOME_COUNT as i32).contains(&id),
                "No value with id {id}"
            );
            BiomeId(id as u16)
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

/// The `StateFlags` resolver for the server's `StateId` — the same behavior-table
/// bit-tests the world reconstruction uses (`BlockState::is_air`/
/// `blocks_motion`/`fluid_empty` and the `minecraft:leaves` tag), applied to the
/// dense `StateId` via `BlockState::new`. This is the `resolve` closure stored
/// on a chunk rebuilt by `from_reconstructed`, so on-demand heightmap primes
/// classify real reconstructed states (not the all-air/all-motion superflat
/// predicates).
fn state_flags(state: StateId) -> StateFlags {
    let s = BlockState::new(state);
    StateFlags {
        is_air: s.is_air(),
        blocks_motion: s.blocks_motion(),
        has_fluid: !s.fluid_empty(),
        is_leaves: s.is_in_tag("minecraft:leaves"),
    }
}

/// The `26 block_nibbles`/`sky_nibbles` Starlight arrays → the packet light
/// payload, once per chunk (the #184 send seam). Each array is converted with
/// `to_vanilla_nibble` (`Null`/`Hidden` → `None`, `Uninitialised` → an empty
/// layer, `Initialised` → the bytes), then `build_light_update_data` folds them
/// into the four masks + layer lists.
fn light_data_from_nibbles(
    block_nibbles: &[SwmrNibbleArray],
    sky_nibbles: &[SwmrNibbleArray],
    height: i32,
) -> LightUpdatePacketData {
    let light_section_count = (height / 16) as usize + 2;
    let block_layers: Vec<Option<DataLayer>> = block_nibbles
        .iter()
        .take(light_section_count)
        .map(|nibble| nibble.to_vanilla_nibble())
        .collect();
    let sky_layers: Vec<Option<DataLayer>> = sky_nibbles
        .iter()
        .take(light_section_count)
        .map(|nibble| nibble.to_vanilla_nibble())
        .collect();
    build_light_update_data(&sky_layers, &block_layers)
}

/// A reconstructed chunk carries a persisted Starlight initialisation state
/// this port does not understand (`InitState::Other`). Paper keeps the raw int
/// through `toVanillaNibble` and re-emits it on save; the port's packet seam
/// (`to_vanilla_nibble`) has no representation for it and panics, so the #516
/// boot surfaces the mismatch as a typed error instead of aborting the process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnsupportedLightState;

impl std::fmt::Display for UnsupportedLightState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "UNVERIFIED chunk carries an unsupported persisted Starlight state"
        )
    }
}

impl std::error::Error for UnsupportedLightState {}

/// Why a reconstructed chunk cannot be bridged into the server value pair.
#[derive(Debug, thiserror::Error)]
pub enum LevelChunkBridgeError {
    /// The chunk carries a persisted Starlight state the #184 send seam cannot
    /// represent.
    #[error(transparent)]
    UnsupportedLightState(#[from] UnsupportedLightState),
    /// A section's paletted containers failed to re-encode into the server
    /// `StateId`/`BiomeId` value pair (`map_values`). The reconstructed and
    /// server strategies share the same dense global-id ladder, so this is
    /// hostile-input defense: the `.expect` that used to abort the process is
    /// now a typed error.
    #[error("UNVERIFIED reconstructed chunk failed to re-encode into the server value pair: {0}")]
    PaletteMap(String),
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

#[cfg(test)]
mod tests {
    use super::LevelChunk;
    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_protocol::protocol::game::level_chunk_packet_data::BlockEntityInfo;
    use rivet_registry::Identifier;
    use rivet_registry::core::{BlockPos, ChunkPos};
    use rivet_world::chunk::palette::GlobalIdMap;
    use rivet_world::chunk::paletted_container::PalettedContainer;
    use rivet_world::chunk::storage::block_entity_materialization::BlockEntityMaterializeError;
    use rivet_world::chunk::storage::serializable_chunk_data::PendingBlockEntityReason;
    use rivet_world::chunk::strategy::Strategy;

    /// A serialized block-entity tag carrying its position and `id`.
    fn block_entity(id: &str, x: i32, y: i32, z: i32) -> CompoundTag {
        let mut tag = CompoundTag::new();
        tag.put_int("x", x);
        tag.put_int("y", y);
        tag.put_int("z", z);
        tag.put_string("id", id);
        tag
    }

    /// `setBlockEntityNbt` mutations flow to a later `chunk_packet_data`: the
    /// packet is materialized from the current pending authority (#537), so an
    /// install, an in-place update, and a removal each change what the next
    /// packet emits — there is no stale snapshot to desync.
    #[test]
    fn block_entity_mutations_flow_to_the_chunk_packet() {
        let mut chunk = LevelChunk::new(ChunkPos::ZERO);
        assert!(chunk.chunk_packet_data().block_entities().is_empty());

        chunk.set_block_entity_nbt(block_entity("minecraft:chest", 1, 65, 1));
        let packet = chunk.chunk_packet_data();
        assert_eq!(packet.block_entities().len(), 1);
        assert_eq!(packet.block_entities()[0].packed_xz(), 0x11);
        assert_eq!(packet.block_entities()[0].y(), 65);
        assert_eq!(
            packet.block_entities()[0].entity_type().name(),
            "minecraft:chest"
        );

        // Updating the same position with a different type is reflected by the
        // next packet (the authority slot is overwritten in place).
        chunk.set_block_entity_nbt(block_entity("minecraft:furnace", 1, 65, 1));
        let packet = chunk.chunk_packet_data();
        assert_eq!(packet.block_entities().len(), 1);
        assert_eq!(
            packet.block_entities()[0].entity_type().name(),
            "minecraft:furnace"
        );

        // Removal drops the position from the authority, so the packet no
        // longer emits it.
        assert!(
            chunk
                .remove_block_entity_nbt(&BlockPos::new(1, 65, 1))
                .is_some()
        );
        assert!(chunk.chunk_packet_data().block_entities().is_empty());
    }

    /// `removeBlockEntity`'s pending half drops the position from the runtime
    /// authority, keeping the survivors' order, so a later packet omits only
    /// that entry.
    #[test]
    fn removing_a_block_entity_drops_it_from_the_authority_and_packet() {
        let mut chunk = LevelChunk::new(ChunkPos::ZERO);
        chunk.set_block_entity_nbt(block_entity("minecraft:chest", 1, 65, 1));
        chunk.set_block_entity_nbt(block_entity("minecraft:furnace", 2, 65, 2));
        assert_eq!(chunk.pending_block_entities().len(), 2);

        let removed = chunk.remove_block_entity_nbt(&BlockPos::new(1, 65, 1));
        assert_eq!(
            removed
                .as_ref()
                .and_then(|tag| tag.get_string("id"))
                .map(|s| s.as_str()),
            Some("minecraft:chest")
        );
        assert_eq!(chunk.pending_block_entities().len(), 1);
        assert_eq!(
            chunk
                .pending_block_entities()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![BlockPos::new(2, 65, 2)]
        );

        let packet = chunk.chunk_packet_data();
        assert_eq!(packet.block_entities().len(), 1);
        assert_eq!(
            packet.block_entities()[0].entity_type().name(),
            "minecraft:furnace"
        );

        assert!(
            chunk
                .remove_block_entity_nbt(&BlockPos::new(2, 65, 2))
                .is_some()
        );
        assert!(chunk.pending_block_entities().is_empty());
        assert!(chunk.chunk_packet_data().block_entities().is_empty());
    }

    /// A tag whose position corrects to an already-present position (a raw
    /// `x` in a different section re-anchored by `getPosFromTag`) collapses
    /// last-wins IN PLACE — one entry, the later tag's type — never a second
    /// map entry or a duplicated packet entry.
    #[test]
    fn duplicate_corrected_positions_collapse_last_wins_in_place() {
        let mut chunk = LevelChunk::new(ChunkPos::ZERO);
        // x=1 corrects to (1,65,1); x=17 lives in section 1 (17 & 15 = 1), so
        // `getPosFromTag` re-anchors it to the same corrected (1,65,1).
        chunk.set_block_entity_nbt(block_entity("minecraft:chest", 1, 65, 1));
        chunk.set_block_entity_nbt(block_entity("minecraft:furnace", 17, 65, 1));
        assert_eq!(
            chunk.pending_block_entities().len(),
            1,
            "a duplicate corrected position must not create a second entry"
        );
        assert_eq!(
            chunk
                .pending_block_entities()
                .get(&BlockPos::new(1, 65, 1))
                .and_then(|tag| tag.get_string("id"))
                .map(|s| s.as_str()),
            Some("minecraft:furnace"),
            "the later tag wins the corrected position"
        );

        let packet = chunk.chunk_packet_data();
        assert_eq!(packet.block_entities().len(), 1);
        assert_eq!(
            packet.block_entities()[0].entity_type().name(),
            "minecraft:furnace"
        );
    }

    /// The materializer over the current authority preserves the exact
    /// insertion order of typed refusals (unsupported update tag, pending,
    /// invalid type) among resolvable entries, and the packet path refuses
    /// loudly — skipping only the refused entries while keeping the
    /// resolvable ones in authority order.
    #[test]
    fn materialization_preserves_typed_refusals_in_authority_order() {
        let mut chunk = LevelChunk::new(ChunkPos::ZERO);
        chunk.set_block_entity_nbt(block_entity("minecraft:chest", 1, 65, 1));
        chunk.set_block_entity_nbt(block_entity("minecraft:banner", 2, 65, 2));
        let mut pending = block_entity("minecraft:chest", 3, 65, 3);
        pending.put_byte("keepPacked", 1);
        chunk.set_block_entity_nbt(pending);
        chunk.set_block_entity_nbt(block_entity("not valid", 4, 65, 4));

        let materialization = chunk.materialize_block_entities();
        assert!(materialization.diagnostics.is_empty());
        let infos = materialization.infos;
        assert_eq!(infos.len(), 4);
        assert!(infos[0].is_ok());
        assert_eq!(
            infos[1].as_ref().unwrap_err(),
            &BlockEntityMaterializeError::UnsupportedUpdateTag {
                position: BlockPos::new(2, 65, 2),
                entity_type: "minecraft:banner".to_string(),
            }
        );
        assert_eq!(
            infos[2].as_ref().unwrap_err(),
            &BlockEntityMaterializeError::Pending {
                position: BlockPos::new(3, 65, 3),
                reason: PendingBlockEntityReason::KeepPacked,
            }
        );
        assert!(matches!(
            infos[3].as_ref().unwrap_err(),
            BlockEntityMaterializeError::InvalidType { position, .. }
                if *position == BlockPos::new(4, 65, 4)
        ));

        // The packet keeps only the resolvable entries, in authority order.
        let packet = chunk.chunk_packet_data();
        let packet_infos: Vec<&BlockEntityInfo> = packet.block_entities().iter().collect();
        assert_eq!(packet_infos.len(), 1);
        assert_eq!(packet_infos[0].entity_type().name(), "minecraft:chest");
    }

    /// `structures_references` is derived from the chunk's `StructureAccess`
    /// authority (#537) in deterministic key-insertion order, and duplicate
    /// references within a key dedupe like Java's `LongOpenHashSet`.
    #[test]
    fn structure_references_are_derived_from_the_authority_in_order() {
        let mut chunk = LevelChunk::new(ChunkPos::ZERO);
        assert!(chunk.structures_references().is_empty());

        chunk.set_all_references(vec![
            (Identifier::parse("minecraft:mineshaft"), vec![5u64, 9]),
            (Identifier::parse("minecraft:village"), vec![1u64]),
        ]);
        let references = chunk.structures_references();
        assert_eq!(references.len(), 2);
        assert_eq!(references[0].identifier.to_string(), "minecraft:mineshaft");
        assert_eq!(references[0].references, vec![5, 9]);
        assert_eq!(references[1].identifier.to_string(), "minecraft:village");
        assert_eq!(references[1].references, vec![1]);

        // Duplicate references within a key collapse like the Java set.
        chunk.set_all_references(vec![(
            Identifier::parse("minecraft:mineshaft"),
            vec![5u64, 5, 9],
        )]);
        let references = chunk.structures_references();
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].references, vec![5, 9]);
    }

    /// A dense id map (global id = value), matching the server `StateId`/biome
    /// maps' dense `0..size` shape.
    #[derive(Clone, Copy)]
    struct DenseMap;
    impl GlobalIdMap<u8> for DenseMap {
        fn get_id(&self, value: &u8) -> i32 {
            *value as i32
        }
        fn by_id_or_throw(&self, id: i32) -> u8 {
            id as u8
        }
        fn size(&self) -> i32 {
            256
        }
        fn by_id(&self, id: i32) -> Option<u8> {
            Some(id as u8)
        }
        fn clone_box(&self) -> Box<dyn GlobalIdMap<u8>> {
            Box::new(*self)
        }
    }

    /// A hostile palette re-encode is a typed error, not the `.expect` panic
    /// `from_reconstructed` used to abort on. A block-states-kind source
    /// container with four distinct values packs at 4 bits (the block-states
    /// ladder's `four_bits_linear`); mapping it into a biomes-kind target
    /// strategy resolves the same four palette entries to a 2-bit biomes
    /// config — the `PackedData::with_bits` bit-count mismatch the
    /// [`super::from_bridge`] `map_values` surfaces as
    /// [`LevelChunkBridgeError::PaletteMap`] instead of panicking.
    #[test]
    fn hostile_palette_mapping_is_a_typed_error_not_a_panic() {
        let source = Strategy::create_for_block_states(Box::new(DenseMap));
        let mut container = PalettedContainer::new(0u8, source);
        container.set(1, 0, 0, 1);
        container.set(2, 0, 0, 2);
        container.set(3, 0, 0, 3);
        // Four distinct values → palette size 4 → the block-states 4-bit config.
        assert_eq!(container.pack().bits_per_entry, 4);

        let target = Strategy::create_for_biomes(Box::new(DenseMap));
        let error = container
            .map_values(&target, &|value| *value)
            .err()
            .expect("the hostile re-encode must fail");
        assert!(
            error.contains("Invalid bit count"),
            "expected the bit-count mismatch, got {error}"
        );
    }
}
