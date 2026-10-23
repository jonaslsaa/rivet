use super::*;
use crate::server::level::{
    chunk_map::ChunkMap,
    entity_construction::{EntityConstructionRuntime, StructureMinecartFactory},
    entity_id_allocator::{EntityIdAllocator, EntityIdOccupancy},
    entity_lookup::EntityLookupError,
    entity_post_load::DetachedAnimalRemoval,
    entity_region_storage::EntitySaveLimits,
    entity_tracking::{TrackingPacket, TrackingPlayer, TrackingWorldEvent},
    full_chunk_status::{FullChunkStatus, FullChunkStatusBinding},
    generated_chunk_owner::GenerationReadiness,
    generated_world::OverworldGenerator,
    generation_creature_runtime::WorldGenerationCreatureFactory,
    level_block_entity_runtime::{LevelBlockEntityRuntime, NativeTickerKind},
    level_storage_access::LevelStorageAccess,
    server_level::{MissingChunkPolicy, ServerLevelConfig},
    world_runtime_settings::WorldRuntimeSettings,
};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::core::BlockPos;
use rivet_util::random::ThreadUnsafeRandom;
use rivet_world::{
    block::Block, block_entity::NativeBlockEntity, chunk::status::ChunkStatus,
    entity::loaded_entity::NativeLoadedEntity, flag::feature_flags,
};

fn source_lifecycle(name: &str) -> serde_json::Value {
    include_str!("../../../../../tools/rivet-oracle/fixtures/full-activation-lifecycle.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|row| row["name"].as_str() == Some(name))
        .unwrap()
}

#[derive(Default)]
struct EmptyObservers;
impl TrackingAuthority for EmptyObservers {
    fn external_uuid_in_use(&self, _: &Uuid) -> bool {
        false
    }
    fn players(
        &mut self,
        _: &NativeLoadedEntity,
    ) -> Result<Vec<TrackingPlayer>, EntityLookupError> {
        Ok(Vec::new())
    }
    fn observer_order(
        &mut self,
        _: &NativeLoadedEntity,
        players: &[i32],
    ) -> Result<Vec<i32>, EntityLookupError> {
        Ok(players.to_vec())
    }
    fn event(
        &mut self,
        _: TrackingWorldEvent,
        _: &NativeLoadedEntity,
        _: Option<i32>,
    ) -> Result<bool, EntityLookupError> {
        panic!("empty observed fixture must not invent tracking events")
    }
    fn send_bundle(&mut self, _: i32, _: &[TrackingPacket]) -> Result<(), EntityLookupError> {
        panic!("no observers")
    }
    fn send(&mut self, _: i32, _: &TrackingPacket) -> Result<(), EntityLookupError> {
        panic!("no observers")
    }
    fn debug_tracking(
        &mut self,
        _: &NativeLoadedEntity,
        _: Option<i32>,
        _: bool,
    ) -> Result<(), EntityLookupError> {
        panic!("no observers")
    }
    fn close_inventory_viewers(&mut self, _: &NativeLoadedEntity) -> Result<(), EntityLookupError> {
        panic!("no removal")
    }
}

#[derive(Clone, Copy, Default)]
enum Failure {
    #[default]
    None,
    Error,
    Panic,
}
#[derive(Default)]
struct Services {
    added: Vec<BlockPos>,
    failure: Failure,
}
impl BlockEntityWorldServices for Services {
    fn on_block_entity_added(&mut self, pos: BlockPos) -> Result<(), BlockEntityTickerError> {
        self.added.push(pos);
        match self.failure {
            Failure::None => Ok(()),
            Failure::Error => Err(BlockEntityTickerError::Body(
                "source onAdded failure".into(),
            )),
            Failure::Panic => panic!("source onAdded panic"),
        }
    }
    fn require_ticker_body(&self, _: NativeTickerKind) -> Result<(), BlockEntityTickerError> {
        panic!("live chest has no ticker; pending hive stays lazy")
    }
    fn tick_body(
        &mut self,
        _: &mut LevelBlockEntityRuntime,
        _: &mut ChunkMap,
        _: &mut ThreadUnsafeRandom,
        _: BlockPos,
        _: NativeTickerKind,
    ) -> Result<(), BlockEntityTickerError> {
        panic!("FULL never runs BE tick bodies")
    }
    fn report_tick_exception(
        &mut self,
        _: BlockPos,
        _: &str,
    ) -> Result<(), BlockEntityTickerError> {
        panic!("FULL does not run tick exception policy")
    }
    fn mid_tick_tasks(&mut self) -> Result<(), BlockEntityTickerError> {
        panic!("FULL does not run tick batches")
    }
}

struct Fixture {
    level: ServerLevel,
    generator: Arc<OverworldGenerator>,
    carts: StructureMinecartFactory,
    animals: WorldGenerationCreatureFactory,
    binding: FullChunkStatusBinding,
    limits: EntitySaveLimits,
    _root: tempfile::TempDir,
}

#[test]
fn holder_border_driver_requires_authority_and_orders_actual_owner_transitions() {
    use crate::server::level::server_level::{
        HolderTransitionAuthority, HolderTransitionEffect as Effect, HolderTransitionError,
    };
    struct Hooks {
        events: Vec<(Effect, FullChunkStatus)>,
        available: bool,
        panic_at: Option<Effect>,
    }
    impl HolderTransitionAuthority for Hooks {
        fn validate(&self, _: &ServerLevel, _: ChunkPos) -> Result<(), HolderTransitionError> {
            if self.available {
                Ok(())
            } else {
                Err(HolderTransitionError::Authority(
                    "fixture listener authority absent".into(),
                ))
            }
        }
        fn apply(
            &mut self,
            world: &mut ServerLevel,
            pos: ChunkPos,
            effect: Effect,
        ) -> Result<(), HolderTransitionError> {
            self.events.push((
                effect,
                world
                    .chunk_map
                    .get_chunk(pos)
                    .unwrap()
                    .get_full_status()
                    .unwrap(),
            ));
            if self.panic_at == Some(effect) {
                std::panic::panic_any(914_i32);
            }
            Ok(())
        }
    }
    let mut fixture = Fixture::new();
    let owner = fixture.level.generated_chunk_owner_mut().unwrap();
    let consumer = owner.new_consumer();
    owner.replace_full_requests(consumer, [(ChunkPos::ZERO, FullChunkStatus::Full)]);
    fixture.binding = owner.holder_status_binding(ChunkPos::ZERO);
    fixture.activate(&mut Services::default()).unwrap();
    assert!(
        matches!(
            fixture.level.prepare_chunk_for_send(ChunkPos::ZERO),
            Ok(crate::server::level::player_chunk_loader::ChunkSendReadiness::Pending)
        ),
        "canonical activation is not real border readiness"
    );
    let mut hooks = Hooks {
        events: Vec::new(),
        available: false,
        panic_at: None,
    };
    assert!(matches!(
        fixture.level.process_generated_holder_status(
            ChunkPos::ZERO,
            &mut hooks,
            &mut EmptyObservers
        ),
        Err(HolderTransitionError::Authority(_))
    ));
    assert_eq!(fixture.binding.get(), FullChunkStatus::Inaccessible);
    hooks.available = true;
    assert!(
        fixture
            .level
            .process_generated_holder_status(ChunkPos::ZERO, &mut hooks, &mut EmptyObservers)
            .unwrap()
    );
    assert_eq!(
        hooks.events,
        vec![
            (Effect::PreBorder, FullChunkStatus::Full),
            (Effect::EnsureAutosave, FullChunkStatus::Full),
            (Effect::Border, FullChunkStatus::Full),
            (
                Effect::CompleteConsumers(FullChunkStatus::Full),
                FullChunkStatus::Full
            ),
        ]
    );
    assert!(matches!(
        fixture.level.prepare_chunk_for_send(ChunkPos::ZERO),
        Ok(crate::server::level::player_chunk_loader::ChunkSendReadiness::Ready)
    ));
    assert!(
        !fixture
            .level
            .generated_chunk_owner()
            .unwrap()
            .is_ticking_ready(ChunkPos::ZERO)
    );
    fixture
        .level
        .generated_chunk_owner_mut()
        .unwrap()
        .release_consumer(consumer);
    hooks.events.clear();
    assert!(
        fixture
            .level
            .process_generated_holder_status(ChunkPos::ZERO, &mut hooks, &mut EmptyObservers)
            .unwrap()
    );
    assert_eq!(
        hooks.events,
        vec![
            (Effect::NotBorder, FullChunkStatus::Full),
            (Effect::PostNotBorder, FullChunkStatus::Full),
            (Effect::CheckUnload, FullChunkStatus::Inaccessible)
        ]
    );
    assert_eq!(fixture.binding.get(), FullChunkStatus::Inaccessible);
    assert!(matches!(
        fixture.level.prepare_chunk_for_send(ChunkPos::ZERO),
        Ok(crate::server::level::player_chunk_loader::ChunkSendReadiness::Pending)
    ));
    assert!(
        fixture.level.block_ticks.has_container(ChunkPos::ZERO),
        "FULL border leave is not chunk eviction"
    );
    let owner = fixture.level.generated_chunk_owner_mut().unwrap();
    let consumer = owner.new_consumer();
    owner.replace_full_requests(consumer, [(ChunkPos::ZERO, FullChunkStatus::Full)]);
    hooks.panic_at = Some(Effect::PreBorder);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = fixture.level.process_generated_holder_status(
            ChunkPos::ZERO,
            &mut hooks,
            &mut EmptyObservers,
        );
    }))
    .unwrap_err();
    assert_eq!(panic.downcast_ref::<i32>(), Some(&914));
    assert_eq!(
        fixture.binding.get(),
        FullChunkStatus::Full,
        "source prehook status publication survives panic"
    );
    hooks.panic_at = None;
    fixture
        .level
        .generated_chunk_owner_mut()
        .unwrap()
        .release_consumer(consumer);
    assert!(
        fixture
            .level
            .process_generated_holder_status(ChunkPos::ZERO, &mut hooks, &mut EmptyObservers)
            .unwrap(),
        "finally must clear processing guard"
    );
    assert_eq!(fixture.binding.get(), FullChunkStatus::Inaccessible);
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let storage = Arc::new(LevelStorageAccess::open(root.path()).unwrap());
        let generator = Arc::new(OverworldGenerator::new(42));
        let config = ServerLevelConfig {
            is_flat: false,
            sea_level: generator.generator().get_sea_level(),
            missing_chunk_policy: MissingChunkPolicy::RequireLoaded,
            ..ServerLevelConfig::default()
        };
        let mut level =
            ServerLevel::new_generated_with_storage(config, Arc::clone(&generator), storage)
                .unwrap();
        let occupancy = EntityIdOccupancy::default();
        level.bind_entity_occupancy(occupancy.clone());
        let settings = WorldRuntimeSettings::from_yaml("", "", "", "fixture").unwrap();
        let limits = EntitySaveLimits::from_entries(
            settings
                .entity_save_limits
                .iter()
                .map(|(key, value)| (key.clone(), *value)),
        )
        .unwrap();
        level
            .install_runtime_settings(generator.registry_access().clone(), settings)
            .unwrap();
        level.open_poi_storage(false).unwrap();
        level.open_entity_storage(false, limits.clone()).unwrap();
        let ids = EntityIdAllocator::new();
        let identity = EntityConstructionRuntime::with_seed(ids.sequence(), 42);
        let uuid = level.world_uuid().unwrap();
        let cart_occupancy = occupancy.clone();
        let carts = StructureMinecartFactory::new(
            identity.clone(),
            uuid,
            feature_flags::vanilla_set(),
            Arc::new(move |id| cart_occupancy.contains(id)),
        );
        let animals = WorldGenerationCreatureFactory::new(
            identity,
            uuid,
            feature_flags::vanilla_set(),
            generator.registry_access(),
            Arc::new(move |id| occupancy.contains(id)),
        );
        let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
        let chest = BlockPos::new(3, 64, 8);
        let chest_state = Block::from_name("minecraft:chest")
            .unwrap()
            .default_block_state();
        state.chunk_mut().set_block_state(3, 64, 8, chest_state);
        state
            .chunk_mut()
            .base_mut()
            .set_block_entity(NativeBlockEntity::fresh(chest, chest_state).unwrap());
        state.chunk_mut().set_block_state(
            4,
            64,
            8,
            Block::from_name("minecraft:bee_nest")
                .unwrap()
                .default_block_state(),
        );
        state
            .chunk_mut()
            .base_mut()
            .set_block_entity_nbt(parse_compound_fully("{id:'DUMMY',x:4,y:64,z:8}").unwrap());
        state.chunk_mut().set_block_state(
            5,
            64,
            8,
            Block::from_name("minecraft:composter")
                .unwrap()
                .default_block_state(),
        );
        // Boundary fixture supplies a real canonical SPAWN parent, not proof
        // of the earlier generation pipeline. All FULL work below is genuine.
        level
            .generated_chunk_owner_mut()
            .unwrap()
            .admit(state)
            .ok()
            .unwrap();
        level
            .generated_chunk_owner_mut()
            .unwrap()
            .state_mut(ChunkPos::ZERO)
            .unwrap()
            .chunk_mut()
            .set_persisted_status(ChunkStatus::Spawn);
        let binding = level
            .generated_chunk_owner_mut()
            .unwrap()
            .holder_status_binding(ChunkPos::ZERO);
        Self {
            level,
            generator,
            carts,
            animals,
            binding,
            limits,
            _root: root,
        }
    }
    fn activate(&mut self, services: &mut Services) -> Result<(), Arc<FullActivationError>> {
        let mut removal = DetachedAnimalRemoval;
        let mut observers = EmptyObservers;
        let mut secure =
            || -> std::io::Result<Uuid> { panic!("empty entity fixture cannot regenerate UUIDs") };
        let mut context = FullActivationContext {
            entities: EntityPostLoadContext {
                cart_factory: &self.carts,
                creature_factory: &self.animals,
                registries: self.generator.registry_access(),
                settings: self.level.runtime_settings().unwrap().duplicate_uuid,
                save_limits: &self.limits,
                removal: &mut removal,
            },
            players: &|_| None,
            secure_uuid: &mut secure,
            tracking: &mut observers,
            block_entities: services,
        };
        self.level
            .activate_generated_chunk(ChunkPos::ZERO, &mut context)
    }
}

#[test]
fn full_executes_canonical_owners_in_order_without_block_tick_activation() {
    let source = source_lifecycle("normal");
    let mut fixture = Fixture::new();
    let mut services = Services::default();
    fixture.activate(&mut services).unwrap();
    let level = &mut fixture.level;
    let pos = ChunkPos::ZERO;
    assert!(matches!(
        level
            .generated_chunk_owner()
            .unwrap()
            .readiness(pos, ChunkStatus::Full),
        GenerationReadiness::Ready
    ));
    let runtime = level.chunk_map.get_chunk(pos).unwrap();
    assert_eq!(
        runtime.is_loaded(),
        source["loaded_after"].as_bool().unwrap()
    );
    assert!(runtime.has_unsaved_listener());
    assert_eq!(services.added, vec![BlockPos::new(3, 64, 8)]);
    assert_eq!(runtime.pending_block_entities().len(), 1);
    assert!(
        runtime
            .block_entities()
            .get(&BlockPos::new(4, 64, 8))
            .is_none()
    );
    assert_eq!(
        format!("{:?}", runtime.get_full_status().unwrap()).to_ascii_uppercase(),
        source["status_before"].as_str().unwrap()
    );
    fixture.binding.set(FullChunkStatus::BlockTicking);
    assert_eq!(
        runtime.get_full_status(),
        Some(FullChunkStatus::BlockTicking)
    );
    assert!(level.block_ticks.has_container(pos));
    assert!(level.fluid_ticks.has_container(pos));
    assert!(level.poi_storage.as_mut().unwrap().has_chunk(pos));
    // Completion is idempotent without replaying onAdded or disk admission.
    fixture.activate(&mut services).unwrap();
    assert_eq!(services.added.len(), 1);
}

#[test]
fn postload_refusal_keeps_source_carrier_and_stops_before_loaded_or_registration() {
    let source = source_lifecycle("callback_throw");
    let mut fixture = Fixture::new();
    let pos = ChunkPos::ZERO;
    fixture
        .level
        .generated_chunk_owner_mut()
        .unwrap()
        .state_mut(pos)
        .unwrap()
        .chunk_mut()
        .add_entity(
            parse_compound_fully(
                "{id:'minecraft:chest_minecart',Pos:[1d,64d,1d],Passengers:[{id:'minecraft:pig'}]}",
            )
            .unwrap(),
        );
    let mut services = Services::default();
    let error = fixture.activate(&mut services).unwrap_err();
    assert_eq!(error.stage, FullActivationStage::PostLoad);
    let runtime = fixture.level.chunk_map.get_chunk(pos).unwrap();
    assert_eq!(
        !runtime.post_load_entities().is_empty(),
        source["retained"].as_bool().unwrap()
    );
    assert_eq!(
        runtime.is_loaded(),
        source["loaded_before"].as_bool().unwrap()
    );
    assert!(services.added.is_empty());
    assert!(!fixture.level.block_ticks.has_container(pos));
}

#[test]
fn late_error_and_panic_keep_promotion_loaded_flag_and_real_prior_effects() {
    for failure in [Failure::Error, Failure::Panic] {
        let mut fixture = Fixture::new();
        let mut services = Services {
            failure,
            ..Services::default()
        };
        let error = fixture.activate(&mut services).unwrap_err();
        assert_eq!(error.stage, FullActivationStage::BlockEntities);
        let level = &mut fixture.level;
        let pos = ChunkPos::ZERO;
        let runtime = level.chunk_map.get_chunk(pos).unwrap();
        assert!(runtime.is_loaded());
        assert!(!runtime.has_unsaved_listener());
        assert!(runtime.post_load_entities().is_empty());
        assert!(!level.block_ticks.has_container(pos));
        assert!(level.generated_chunk_owner().unwrap().is_promoted(pos));
        assert!(matches!(
            level
                .generated_chunk_owner()
                .unwrap()
                .readiness(pos, ChunkStatus::Full),
            GenerationReadiness::Failed { .. }
        ));
        assert!(Arc::ptr_eq(
            &error,
            &fixture.activate(&mut services).unwrap_err()
        ));
        assert_eq!(
            services.added.len(),
            1,
            "cached failure must not hot-replay partial effects"
        );
        fixture
            .level
            .generated_chunk_owner_mut()
            .unwrap()
            .retry(pos);
        services.failure = Failure::None;
        fixture.activate(&mut services).unwrap();
        assert_eq!(services.added.len(), 2);
    }
}
