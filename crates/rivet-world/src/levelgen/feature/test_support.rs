//! Test double for the selector-feature placement tests (the
//! `mc.world.level.levelgen.feature.selector` unit).
//!
//! The concrete selector features resolve their `Holder<PlacedFeature>`/
//! `Holder<ConfiguredFeatureErased>` through `WorldGenLevel::registry_access`
//! (the `.feature.selector` STUB seam) — the placed-feature lookup for
//! `Holder::value` on the placed holder, and the configured-feature lookup
//! threaded to the resolved `PlacedFeature.place`. This module provides the
//! two-registry `RegistryAccess` those lookups resolve through, a
//! `WorldGenLevel`/`ChunkGenerator` double whose `registry_access` returns it,
//! and a `RecordingRandom` that logs every RNG call so the tests can pin the
//! exact Java draw order.
//!
//! The placed sub-features the tests exercise are inline `Direct` holders
//! wrapping the `minecraft:no_op` configured leaf (id 0): `Direct` resolves
//! without touching the (empty) registries and the `#181` dispatch returns
//! `true`, so the selector's verdict reflects its own RNG walk, not the
//! leaf. A hostile test instead builds a level whose access omits a registry
//! to pin the missing-registry failure, and an `ensure_can_write` gate the
//! tests can trip to pin the return-`false` propagation.

use crate::block::blocks::Blocks;
use crate::chunk::chunk_generator::ChunkGenerator;
use crate::level::WorldGenLevel;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::feature::configurations::{CountConfiguration, NoneFeatureConfiguration};
use crate::levelgen::feature::registry_keys::{CONFIGURED_FEATURE, PLACED_FEATURE};
use crate::levelgen::feature::{ConfiguredFeatureErased, FeatureId};
use crate::levelgen::heightmap::Types;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::Holder;
use rivet_registry::access::RegistryAccess;
use rivet_registry::block_state::BlockState;
use rivet_registry::builder::RegistryBuilder;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::fluid_id::FluidId;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::root::AnyBox;
use rivet_registry::{Identifier, ResourceKey};
use rivet_util::RandomSource;
use rivet_util::random::{LegacyPositionalRandomFactory, LegacyRandomSource};
use std::collections::HashMap;
use std::sync::Arc;

/// The small live block-entity state the feature fixture exposes. Block states
/// and entities are intentionally separate: a chest/spawner state only gains a
/// matching entity when `set_block` materializes it, just as the production
/// world must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestBlockEntity {
    Chest {
        loot: Option<(i64, String)>,
    },
    Spawner {
        next_spawn: Option<String>,
        spawn_potentials: Vec<(String, i32)>,
    },
}

/// The two-registry access the selector features resolve their holders
/// through — frozen empty configured/placed registries (the test configured
/// features are inline `Direct`, so nothing resolves by id).
pub fn access() -> RegistryAccess {
    let configured = RegistryBuilder::new(&*CONFIGURED_FEATURE).freeze();
    let placed = RegistryBuilder::new(&*PLACED_FEATURE).freeze();
    // `from_pairs` stores erased `RegistryKey<()>` keys; `lookup` erases the
    // typed key by re-reading its identifier, so the pair keys must use the
    // same identifier form the registry keys carry.
    RegistryAccess::from_pairs(vec![
        (
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/configured_feature",
            )),
            Box::new(configured) as AnyBox,
        ),
        (
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/placed_feature",
            )),
            Box::new(placed) as AnyBox,
        ),
    ])
}

/// An access over the configured-feature registry only — the hostile fixture
/// for a selector feature whose placed-feature lookup is missing (a
/// `registry_access` that does not carry the placed registry fails explicitly
/// when the feature resolves its holders).
pub fn configured_only_access() -> RegistryAccess {
    let configured = RegistryBuilder::new(&*CONFIGURED_FEATURE).freeze();
    RegistryAccess::from_pairs(vec![(
        ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/configured_feature",
        )),
        Box::new(configured) as AnyBox,
    )])
}

/// An inline placed feature wrapping the `minecraft:no_op` configured leaf —
/// the sub-feature the selector placement tests place (id 0 dispatches to
/// `NoOpFeature`, returning `true`).
pub fn no_op_placed() -> Holder<PlacedFeature> {
    Holder::direct(PlacedFeature::new(
        Holder::direct(ConfiguredFeatureErased {
            feature: FeatureId::new(0),
            config: Arc::new(NoneFeatureConfiguration),
        }),
        Vec::new(),
    ))
}

/// An inline placed feature wrapping the `minecraft:sea_pickle` configured
/// leaf (id 38, a `CountConfiguration`) — the sub-feature that *fails*
/// placement on an empty level (the sea-pickle gate needs a water cell plus
/// survival, neither present on a default `TestLevel`, so it writes nothing
/// and returns `false`). The selector placement tests use it as the
/// always-false counterpart to the always-true `no_op_placed` leaf, so the
/// short-circuit/`false`-verdict routing of a selector feature is pinned
/// (id 38 is `Feature.SEA_PICKLE`, the feature registry's insertion index).
pub fn failing_placed() -> Holder<PlacedFeature> {
    Holder::direct(PlacedFeature::new(
        Holder::direct(ConfiguredFeatureErased {
            feature: FeatureId::new(38),
            config: Arc::new(CountConfiguration::new_with_value(1)),
        }),
        Vec::new(),
    ))
}

/// The `WorldGenLevel` double: `registry_access` returns the given access, and
/// `ensure_can_write`/`set_block` are inspectable so the tests can trip the
/// write gate and observe writes.
///
/// The vegetation/aquatic features read the world through
/// `get_block_state`/`get_height_at`/`get_sea_level`/`is_empty_block`/
/// `can_survive`/`is_face_sturdy`, so the double carries a mutable block-state
/// map (`set_block` records into it and the reads answer from it, air for
/// unset positions) plus fixed overrides for the other reads. `get_height_at`
/// returns a single column height for every `(x, z)`; tests that need a
/// specific surface set [`TestLevel::height`] and populate the map at those
/// positions.
pub struct TestLevel {
    /// The access `registry_access()` returns.
    pub access: RegistryAccess,
    /// `ensure_can_write` — `true` (Java's default) unless a test tripped it.
    pub can_write: bool,
    /// The `set_block` calls in order (the `Block.UPDATE_*` flag value).
    pub writes: Vec<(BlockPos, BlockState)>,
    /// The `Block.UPDATE_*` flag passed to each `set_block` call, in the same
    /// order as [`writes`](Self::writes).
    pub writes_flags: Vec<u32>,
    /// The `destroy_block` calls in order — the drop-and-set features
    /// (`EndPodiumFeature` active, `EndPlatformFeature` dropResources) destroy
    /// before overwriting, and the double records each destroyed position.
    pub destroyed: Vec<BlockPos>,
    /// The mutable block-state map: `get_block_state` reads it (air for an
    /// unset position), `set_block` writes it.
    pub states: HashMap<BlockPos, BlockState>,
    /// Live block entities materialized by block writes. This is separate from
    /// [`states`](Self::states) so tests cannot treat a block-state id as an
    /// entity lookup result.
    pub block_entities: HashMap<BlockPos, TestBlockEntity>,
    /// `get_height_at` — the column height returned for every `(x, z)`.
    pub height: i32,
    /// `get_sea_level` — the fixed sea level.
    pub sea_level: i32,
    /// `can_survive` — the fixed survival verdict (Java's block
    /// `canSurvive`, which the vegetation features gate their writes on).
    pub survive: bool,
    /// `is_face_sturdy` — the fixed face-sturdiness verdict
    /// (`BlockPileFeature.mayPlaceOn`).
    pub face_sturdy: bool,
    /// `is_face_sturdy` per-position overrides — a `(pos, face)` entry, when
    /// present, answers that exact query instead of the global `face_sturdy`
    /// (`VinesFeature` walks six faces and needs a per-neighbour verdict).
    pub face_sturdy_at: HashMap<(BlockPos, Direction), bool>,
    /// `should_freeze` — the fixed `Biome.shouldFreeze` verdict
    /// (`SnowAndFreezeFeature`).
    pub freeze: bool,
    /// `should_snow` — the fixed `Biome.shouldSnow` verdict
    /// (`SnowAndFreezeFeature`).
    pub snow: bool,
    /// `mark_pos_for_post_processing` — the positions the geology/cave leaves
    /// mark for post-processing (in call order).
    pub post_processing: Vec<BlockPos>,
    /// `schedule_tick` — the `(pos, fluid, delay)` tick requests the
    /// geology/cave leaves schedule (in call order).
    pub ticks: Vec<(BlockPos, FluidId, i32)>,
    /// `schedule_block_tick` — the `(pos, block, delay)` block tick requests
    /// `SimpleBlockFeature` (with `config.scheduleTick()`) and `LakeFeature.place`
    /// (the placed cave-air cells) schedule (in call order).
    pub block_ticks: Vec<(BlockPos, crate::block::Block, i32)>,
    /// `set_block_entity_loot_table` — the `(pos, seed, loot_table)` chest-loot
    /// attachments (`MonsterRoomFeature` wall-pass chest) in call order.
    pub chest_loot: Vec<(BlockPos, i64, String)>,
    /// `set_spawner_entity` — the `(pos, entity_id)` spawner entity writes
    /// (`MonsterRoomFeature` final spawner write) in call order.
    pub spawner_entities: Vec<(BlockPos, String)>,
}

impl TestLevel {
    /// A level over the two-registry access, writable, no writes yet, air
    /// everywhere at column height 0, sea level 63, everything survives and is
    /// face-sturdy.
    pub fn over(access: RegistryAccess) -> TestLevel {
        TestLevel {
            access,
            can_write: true,
            writes: Vec::new(),
            writes_flags: Vec::new(),
            destroyed: Vec::new(),
            states: HashMap::new(),
            block_entities: HashMap::new(),
            height: 0,
            sea_level: 63,
            survive: true,
            face_sturdy: true,
            face_sturdy_at: HashMap::new(),
            freeze: false,
            snow: false,
            post_processing: Vec::new(),
            ticks: Vec::new(),
            block_ticks: Vec::new(),
            chest_loot: Vec::new(),
            spawner_entities: Vec::new(),
        }
    }

    fn materialize_block_entity(&mut self, pos: &BlockPos, state: BlockState) {
        if state.block() == Blocks::CHEST.id() {
            if !matches!(
                self.block_entities.get(pos),
                Some(TestBlockEntity::Chest { .. })
            ) {
                self.block_entities
                    .insert(*pos, TestBlockEntity::Chest { loot: None });
            }
        } else if state.block() == Blocks::SPAWNER.id() {
            if !matches!(
                self.block_entities.get(pos),
                Some(TestBlockEntity::Spawner { .. })
            ) {
                self.block_entities.insert(
                    *pos,
                    TestBlockEntity::Spawner {
                        next_spawn: None,
                        spawn_potentials: Vec::new(),
                    },
                );
            }
        } else {
            self.block_entities.remove(pos);
        }
    }

    /// Seed a pre-existing spawner entity for a state-transition/RNG test.
    pub fn set_spawner_state(
        &mut self,
        pos: BlockPos,
        next_spawn: Option<String>,
        spawn_potentials: Vec<(String, i32)>,
    ) {
        self.states
            .insert(pos, Blocks::SPAWNER.default_block_state());
        self.block_entities.insert(
            pos,
            TestBlockEntity::Spawner {
                next_spawn,
                spawn_potentials,
            },
        );
    }
}

impl LevelHeightAccessor for TestLevel {
    fn get_height(&self) -> i32 {
        384
    }

    fn get_min_y(&self) -> i32 {
        -64
    }
}

impl WorldGenLevel for TestLevel {
    fn get_seed(&self) -> i64 {
        0
    }

    fn get_block_state(&self, pos: &BlockPos) -> BlockState {
        self.states
            .get(pos)
            .copied()
            .unwrap_or_else(|| BlockState::of(BlockId(0)))
    }

    fn ensure_can_write(&self, _pos: &BlockPos) -> bool {
        self.can_write
    }

    fn set_block(&mut self, pos: &BlockPos, state: BlockState, flags: u32) -> bool {
        self.writes.push((*pos, state));
        self.writes_flags.push(flags);
        self.states.insert(*pos, state);
        self.materialize_block_entity(pos, state);
        true
    }

    fn destroy_block(&mut self, pos: &BlockPos, _drop: bool) -> bool {
        self.destroyed.push(*pos);
        let air = BlockState::of(BlockId(0));
        self.states.insert(*pos, air);
        self.materialize_block_entity(pos, air);
        true
    }

    fn registry_access(&self) -> RegistryAccess {
        self.access.clone()
    }

    fn get_height_at(&mut self, _ty: Types, _x: i32, _z: i32) -> i32 {
        self.height
    }

    fn get_sea_level(&self) -> i32 {
        self.sea_level
    }

    fn is_empty_block(&self, pos: &BlockPos) -> bool {
        self.get_block_state(pos).is_air()
    }

    fn can_survive(&self, _state: &BlockState, _pos: &BlockPos) -> bool {
        self.survive
    }

    fn is_face_sturdy(&self, pos: &BlockPos, _state: &BlockState, direction: &Direction) -> bool {
        self.face_sturdy_at
            .get(&(*pos, *direction))
            .copied()
            .unwrap_or(self.face_sturdy)
    }

    fn should_freeze(&self, _pos: &BlockPos, _check_neighbors: bool) -> bool {
        self.freeze
    }

    fn should_snow(&self, _pos: &BlockPos) -> bool {
        self.snow
    }

    fn schedule_tick(&mut self, pos: &BlockPos, fluid: FluidId, delay: i32) {
        self.ticks.push((*pos, fluid, delay));
    }

    fn schedule_block_tick(&mut self, pos: &BlockPos, block: crate::block::Block, delay: i32) {
        self.block_ticks.push((*pos, block, delay));
    }

    fn mark_pos_for_post_processing(&mut self, pos: &BlockPos) {
        self.post_processing.push(*pos);
    }

    fn is_randomizable_container(&self, pos: &BlockPos) -> bool {
        matches!(
            self.block_entities.get(pos),
            Some(TestBlockEntity::Chest { .. })
        )
    }

    fn is_spawner_block_entity(&self, pos: &BlockPos) -> bool {
        matches!(
            self.block_entities.get(pos),
            Some(TestBlockEntity::Spawner { .. })
        )
    }

    fn set_block_entity_loot_table(&mut self, pos: &BlockPos, seed: i64, loot_table: &str) {
        if let Some(TestBlockEntity::Chest { loot }) = self.block_entities.get_mut(pos) {
            *loot = Some((seed, loot_table.to_string()));
            self.chest_loot.push((*pos, seed, loot_table.to_string()));
        }
    }

    fn spawner_potential_weight(&self, pos: &BlockPos) -> Option<i32> {
        match self.block_entities.get(pos) {
            Some(TestBlockEntity::Spawner {
                next_spawn: None,
                spawn_potentials,
            }) if !spawn_potentials.is_empty() => {
                Some(spawn_potentials.iter().map(|(_, weight)| *weight).sum())
            }
            _ => None,
        }
    }

    fn set_spawner_entity(&mut self, pos: &BlockPos, entity_id: &str, potential_roll: Option<i32>) {
        if let Some(TestBlockEntity::Spawner {
            next_spawn,
            spawn_potentials,
        }) = self.block_entities.get_mut(pos)
        {
            if let Some(mut roll) = potential_roll {
                for (_, weight) in spawn_potentials.iter() {
                    if roll < *weight {
                        break;
                    }
                    roll -= *weight;
                }
            }
            *next_spawn = Some(entity_id.to_string());
            spawn_potentials.clear();
            self.spawner_entities.push((*pos, entity_id.to_string()));
        }
    }
}

/// The `ChunkGenerator` double over the overworld window.
pub struct TestGenerator;

impl ChunkGenerator for TestGenerator {
    fn get_min_y(&self) -> i32 {
        -64
    }

    fn get_gen_depth(&self) -> i32 {
        384
    }
}

/// A `ChunkGenerator` double that answers a fixed sea level
/// (`BasaltColumnsFeature` reads `context.chunkGenerator().getSeaLevel()`, the
/// `ChunkGenerator::get_sea_level` seam that the plain [`TestGenerator`] does
/// not override).
pub struct SeaLevelGenerator {
    /// `get_sea_level` — the fixed value returned for every call.
    pub sea_level: i32,
}

impl ChunkGenerator for SeaLevelGenerator {
    fn get_min_y(&self) -> i32 {
        -64
    }

    fn get_gen_depth(&self) -> i32 {
        384
    }

    fn get_sea_level(&self) -> i32 {
        self.sea_level
    }
}

/// The recorded RNG call kinds the placement tests assert on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RngCall {
    /// `nextInt()` — unbounded.
    Int,
    /// `nextInt(bound)` — the bound argument.
    IntBound(i32),
    /// `nextLong()` — the chest-loot seed draw (`MonsterRoomFeature` tail).
    Long,
    /// `nextBoolean()`.
    Boolean,
    /// `nextFloat()`.
    Float,
    /// `nextDouble()`.
    Double,
}

/// A `RandomSource` wrapper that records every draw, so placement tests can
/// pin the exact Java draw order and arguments (feature selectors' booleans,
/// weighted-entry rolls and indices, plus feature-specific long/int draws).
pub struct RecordingRandom {
    /// The wrapped legacy source (deterministic per seed).
    pub inner: LegacyRandomSource,
    /// The draws so far, in order.
    pub calls: Vec<RngCall>,
}

impl RecordingRandom {
    pub fn new(seed: i64) -> RecordingRandom {
        RecordingRandom {
            inner: LegacyRandomSource::new(seed),
            calls: Vec::new(),
        }
    }
}

impl RandomSource for RecordingRandom {
    type Positional = LegacyPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        RecordingRandom {
            inner: self.inner.fork(),
            calls: self.calls.clone(),
        }
    }

    fn fork_positional(&mut self) -> Self::Positional {
        self.inner.fork_positional()
    }

    fn set_seed(&mut self, seed: i64) {
        self.inner.set_seed(seed);
    }

    fn next_int(&mut self) -> i32 {
        self.calls.push(RngCall::Int);
        self.inner.next_int()
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        self.calls.push(RngCall::IntBound(bound));
        self.inner.next_int_bound(bound)
    }

    fn next_long(&mut self) -> i64 {
        self.calls.push(RngCall::Long);
        self.inner.next_long()
    }

    fn next_boolean(&mut self) -> bool {
        self.calls.push(RngCall::Boolean);
        self.inner.next_boolean()
    }

    fn next_float(&mut self) -> f32 {
        self.calls.push(RngCall::Float);
        self.inner.next_float()
    }

    fn next_double(&mut self) -> f64 {
        self.calls.push(RngCall::Double);
        self.inner.next_double()
    }

    fn next_gaussian(&mut self) -> f64 {
        self.inner.next_gaussian()
    }
}
