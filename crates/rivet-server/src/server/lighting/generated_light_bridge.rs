//! The typed worldgen-to-runtime boundary for generated LIGHT.
//!
//! `GenerationChunkHolder` owns a worldgen `ProtoChunk`, while Starlight owns
//! runtime `StateId`/server-biome values. This module keeps that boundary
//! explicit: the caller supplies a provider whose take/put callback belongs to
//! the tick-thread runtime storage, and this value seam maps the center chunk
//! across the boundary, lights it, then maps it back. It does not install a
//! chunk, attach a live `ChunkMap`, or promote a status past `LIGHT`.

use rivet_registry::block_state::BlockState;
use rivet_registry::generated::blocks::BlockId;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::ChunkStatus;
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId as WorldgenBiomeId, current_version_container_factory,
};
use rivet_world::level::height_accessor::SimpleLevelHeightAccessor;
use rivet_world::levelgen::heightmap::StateFlags;
use rivet_world::lighting::star_light_engine::get_empty_sections_for_chunk;
use rivet_world::lighting::star_light_provider::{LightProviderError, StarLightProvider};

use super::star_light_provider_impl::SkyLightProvider;
use crate::server::level::level_chunk::{
    BiomeId as ServerBiomeId, StateId, StructureKey, state_flags, strategies,
};

/// The generated holder's worldgen value pair.
pub type GeneratedProto = ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>;

/// The runtime value pair used by [`SkyLightProvider`].
pub type RuntimeProto = ProtoChunk<StateId, ServerBiomeId, StructureKey>;

/// Why the generated LIGHT bridge refused to run.
#[derive(Debug, thiserror::Error)]
pub enum GeneratedLightBridgeError {
    /// The caller attempted to light before the `INITIALIZE_LIGHT` rung.
    #[error("generated chunk at {0:?} is not ready for LIGHT")]
    NotReady(ChunkStatus),
    /// A generated chunk must not be changed by this pre-FULL seam after the
    /// LIGHT rung. SPAWN/FULL retain their own downstream owners.
    #[error("generated chunk at {0:?} is outside the pre-FULL LIGHT bridge")]
    UnsupportedStatus(ChunkStatus),
    /// The dense palette conversion rejected hostile source data. The proto
    /// remains borrowed by the bridge and is therefore available for retry.
    #[error("generated LIGHT value bridge failed: {0}")]
    ValueMap(String),
    /// The provider's load/edge seam is not complete for persisted light.
    #[error("persisted generated LIGHT at {status:?} cannot be reconciled by the provider")]
    PersistedLightLoadUnsupported { status: ChunkStatus },
    /// A provider callback panicked. The generated proto is untouched and can
    /// be retried after the caller repairs the provider/storage condition.
    #[error("generated LIGHT provider panicked: {0}")]
    ProviderPanic(String),
    /// A non-provider conversion panic was contained before publication.
    #[error("generated LIGHT conversion panicked: {0}")]
    ConversionPanic(String),
}

/// A synchronous value bridge that owns one Starlight provider on the tick
/// thread. Runtime neighbour ownership remains with the provider's callback;
/// the generated center is borrowed only while its mapped runtime value is lit.
pub struct GeneratedLightBridge {
    provider: SkyLightProvider,
}

fn runtime_state_flags(state: &StateId) -> StateFlags {
    state_flags(*state)
}

impl GeneratedLightBridge {
    /// Attach the provider supplied by the caller-owned runtime storage path.
    pub fn new(provider: SkyLightProvider) -> Self {
        Self { provider }
    }

    /// Mutably expose the provider for the owning tick-thread integration.
    pub fn provider_mut(&mut self) -> &mut SkyLightProvider {
        &mut self.provider
    }

    /// Run the Paper LIGHT task over a generated center value.
    ///
    /// `INITIALIZE_LIGHT` computes nothing. At `LIGHT`, an already-lighted
    /// chunk takes the load/edge-check branch; every other accepted state
    /// clears `light_correct`, computes through Starlight, restores the flag,
    /// and advances only `INITIALIZE_LIGHT` to `LIGHT`. The proto is borrowed
    /// until the complete runtime round trip succeeds, so every refusal,
    /// conversion error, provider panic, and capability refusal leaves the
    /// caller's value untouched for inspection or retry.
    pub fn light(&mut self, chunk: &mut GeneratedProto) -> Result<(), GeneratedLightBridgeError> {
        let status = chunk.get_persisted_status();
        if status.is_before(ChunkStatus::InitializeLight) {
            return Err(GeneratedLightBridgeError::NotReady(status));
        }
        if status.is_after(ChunkStatus::Light) {
            return Err(GeneratedLightBridgeError::UnsupportedStatus(status));
        }

        let mut runtime =
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| to_runtime(chunk))) {
                Ok(Ok(runtime)) => runtime,
                Ok(Err(error)) => return Err(error),
                Err(payload) => {
                    return Err(GeneratedLightBridgeError::ConversionPanic(panic_message(
                        payload,
                    )));
                }
            };
        let already_lighted = status == ChunkStatus::Light && runtime.is_light_correct();
        if already_lighted {
            let pos = runtime.get_pos();
            let provider_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let empty_sections = get_empty_sections_for_chunk(&runtime);
                self.provider.force_load_in_chunk(pos, &empty_sections);
                self.provider.check_chunk_edges(pos);
                self.provider.supports_persisted_light_load()
            }));
            match provider_result {
                Ok(true) => {}
                Ok(false) => {
                    return Err(GeneratedLightBridgeError::PersistedLightLoadUnsupported {
                        status,
                    });
                }
                Err(payload) => {
                    return Err(GeneratedLightBridgeError::ProviderPanic(panic_message(
                        payload,
                    )));
                }
            }
        } else {
            runtime.set_light_correct(false);
            let provider_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || -> Result<(), GeneratedLightBridgeError> {
                    let empty_sections = get_empty_sections_for_chunk(&runtime);
                    self.provider
                        .light_chunk_with(runtime.base_mut(), &empty_sections)
                        .map_err(|error| match error {
                            LightProviderError::MissingChunk(pos) => {
                                GeneratedLightBridgeError::ProviderPanic(format!(
                                    "center chunk {pos} disappeared from runtime storage"
                                ))
                            }
                            LightProviderError::CallbackPanicked => {
                                GeneratedLightBridgeError::ProviderPanic(
                                    "light-provider storage callback panicked".to_string(),
                                )
                            }
                        })?;
                    Ok(())
                },
            ));
            match provider_result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
                Err(payload) => {
                    return Err(GeneratedLightBridgeError::ProviderPanic(panic_message(
                        payload,
                    )));
                }
            }
            runtime.set_light_correct(true);
            if status == ChunkStatus::InitializeLight {
                runtime.set_persisted_status(ChunkStatus::Light);
            }
        }

        let generated =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| to_generated(runtime)))
                .map_err(|payload| {
                    GeneratedLightBridgeError::ConversionPanic(panic_message(payload))
                })??;
        *chunk = generated;
        Ok(())
    }
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

fn to_runtime(chunk: &GeneratedProto) -> Result<RuntimeProto, GeneratedLightBridgeError> {
    let (block_strategy, biome_strategy) = strategies();
    chunk
        .map_values_ref(
            block_strategy,
            biome_strategy,
            StateId(0),
            StateId(15292),
            ServerBiomeId(40),
            &|state: &BlockState| state.id(),
            &|biome: &WorldgenBiomeId| ServerBiomeId(biome.0),
            &runtime_state_flags,
        )
        .map_err(GeneratedLightBridgeError::ValueMap)
}

fn to_generated(chunk: RuntimeProto) -> Result<GeneratedProto, GeneratedLightBridgeError> {
    let factory = current_version_container_factory();
    chunk
        .map_values(
            factory.block_states_strategy().clone(),
            factory.biome_strategy().clone(),
            Blocks::AIR.default_block_state(),
            BlockState::of(BlockId(794)),
            WorldgenBiomeId(40),
            &|state: &StateId| BlockState::new(*state),
            &|biome: &ServerBiomeId| WorldgenBiomeId(biome.raw()),
            &resolve_state_flags,
        )
        .map_err(GeneratedLightBridgeError::ValueMap)
}

/// Build the provider callback type without exposing the server's concrete
/// chunk-map implementation. The callback is the sole runtime ownership seam.
pub fn provider_for_storage(
    height_accessor: SimpleLevelHeightAccessor,
    has_sky_light: bool,
    has_block_light: bool,
    chunks: super::star_light_provider_impl::ChunkAccessFn,
) -> GeneratedLightBridge {
    GeneratedLightBridge::new(SkyLightProvider::new(
        height_accessor,
        has_sky_light,
        has_block_light,
        chunks,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    use rivet_nbt::compound_tag::CompoundTag;
    use rivet_registry::Identifier;
    use rivet_registry::core::ChunkPos;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_world::chunk::chunk_access::ChunkAccess;
    use rivet_world::chunk::level_chunk_section::LevelChunkSection;
    use rivet_world::chunk::paletted_container::PalettedContainer;
    use rivet_world::chunk::storage::chunk_reconstruction::block_state_predicates;
    use rivet_world::chunk::upgrade_data::UpgradeData;
    use rivet_world::level::LevelHeightAccessor;
    use rivet_world::level::height_accessor::create as create_accessor;
    use rivet_world::levelgen::heightmap::Types;
    use rivet_world::lighting::swmr_nibble_array::{ARRAY_SIZE, SwmrNibbleArray};
    use rivet_world::superflat::{SUPERFLAT_HEIGHT, SUPERFLAT_MIN_Y};

    use super::super::star_light_provider_impl::LightChunk;

    type Storage = Arc<Mutex<HashMap<(i32, i32), LightChunk>>>;

    fn runtime_state_flags(state: &StateId) -> rivet_world::levelgen::heightmap::StateFlags {
        state_flags(*state)
    }

    fn overworld() -> SimpleLevelHeightAccessor {
        create_accessor(SUPERFLAT_MIN_Y, SUPERFLAT_HEIGHT)
    }

    fn storage_closure(
        storage: HashMap<(i32, i32), LightChunk>,
    ) -> (
        super::super::star_light_provider_impl::ChunkAccessFn,
        Storage,
    ) {
        let shared = Arc::new(Mutex::new(storage));
        let closure_shared = Arc::clone(&shared);
        let closure = Box::new(
            move |x: i32, z: i32, put: Option<&mut Option<LightChunk>>| {
                let mut storage = closure_shared.lock().unwrap();
                match put {
                    Some(slot) => {
                        if let Some(chunk) = slot.take() {
                            storage.insert((x, z), chunk);
                        }
                        None
                    }
                    None => storage.remove(&(x, z)),
                }
            },
        );
        (closure, shared)
    }

    fn runtime_air(pos: ChunkPos) -> LightChunk {
        let accessor = overworld();
        ChunkAccess::new(
            pos,
            UpgradeData::empty(accessor.get_sections_count() as usize),
            accessor,
            &crate::server::level::level_chunk::container_factory(),
            0,
            None,
            &runtime_state_flags,
        )
    }

    fn generated_chunk() -> GeneratedProto {
        let accessor = overworld();
        let factory = current_version_container_factory();
        let predicates = block_state_predicates();
        let count = accessor.get_sections_count() as usize;
        let mut sections = Vec::with_capacity(count);
        for index in 0..count {
            let mut section = LevelChunkSection::new(
                PalettedContainer::new(
                    Blocks::AIR.default_block_state(),
                    factory.block_states_strategy().clone(),
                ),
                PalettedContainer::new(WorldgenBiomeId(40), factory.biome_strategy().clone()),
                predicates.is_air,
                predicates.is_randomly_ticking,
                predicates.fluid_is_empty,
                predicates.fluid_is_randomly_ticking,
                predicates.is_special_colliding,
            );
            if index == 0 {
                section.set_block_state(
                    0,
                    0,
                    0,
                    BlockState::of(BlockId(1)),
                    &predicates.is_air,
                    &predicates.is_randomly_ticking,
                    &predicates.fluid_is_empty,
                    &predicates.fluid_is_randomly_ticking,
                    &predicates.is_special_colliding,
                );
                section.set_noise_biome(0, 0, 0, WorldgenBiomeId(1));
            }
            sections.push(section);
        }
        ProtoChunk::new(
            ChunkPos::ZERO,
            UpgradeData::empty(count),
            accessor,
            &factory,
            Some(sections),
            Blocks::AIR.default_block_state(),
            BlockState::of(BlockId(794)),
            &resolve_state_flags,
        )
    }

    fn initialised_nibbles(byte: u8) -> Vec<SwmrNibbleArray> {
        (0..26)
            .map(|_| SwmrNibbleArray::new_with_bytes(vec![byte; ARRAY_SIZE]))
            .collect()
    }

    #[test]
    fn generated_light_round_trip_preserves_values_and_publishes_sky() {
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        chunk.set_heightmap(Types::WorldSurfaceWg, &vec![7; 37]);
        chunk.set_block_nibbles(initialised_nibbles(0xAB));
        chunk.set_sky_nibbles(initialised_nibbles(0xCD));
        chunk.add_entity({
            let mut tag = CompoundTag::new();
            tag.put_string("id", "minecraft:pig");
            tag
        });
        chunk.mark_pos_for_post_processing(&rivet_registry::core::BlockPos::new(1, -64, 2));
        chunk.get_or_create_carving_mask().set(1, -64, 2);
        let village = Identifier::parse("minecraft:village");
        chunk.set_start_for_structure(village.clone(), 9);
        chunk.add_reference_for_structure(village.clone(), 4);

        let (chunks, shared) =
            storage_closure(HashMap::from([((1, 0), runtime_air(ChunkPos::new(1, 0)))]));
        let provider = provider_for_storage(overworld(), true, true, chunks);
        let mut bridge = provider;
        bridge.light(&mut chunk).expect("generated LIGHT succeeds");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert_eq!(
            chunk.get_sections()[0].get_noise_biome(0, 0, 0),
            WorldgenBiomeId(1)
        );
        assert_eq!(
            chunk.get_block_state(0, SUPERFLAT_MIN_Y, 0),
            BlockState::of(BlockId(1))
        );
        assert_eq!(
            chunk.block_nibbles()[0]
                .to_vanilla_nibble()
                .unwrap()
                .get_data(),
            vec![0xAB; ARRAY_SIZE]
        );
        assert!(chunk.sky_nibbles()[1].to_vanilla_nibble().is_some());
        assert!(!chunk.sky_emptiness_map().unwrap()[0]);
        assert_eq!(
            chunk.heightmaps()[Types::WorldSurfaceWg as usize]
                .as_ref()
                .unwrap()
                .get_raw_data(),
            &vec![7; 37]
        );
        assert_eq!(chunk.get_entities().len(), 1);
        assert!(
            chunk
                .get_carving_mask()
                .is_some_and(|mask| mask.get(1, -64, 2))
        );
        assert_eq!(chunk.get_post_processing()[0].len(), 1);
        assert_eq!(chunk.get_start_for_structure(&village), Some(9));
        assert_eq!(
            chunk
                .get_references_for_structure(&village)
                .copied()
                .collect::<Vec<_>>(),
            vec![4]
        );
        assert!(
            shared.lock().unwrap().contains_key(&(1, 0)),
            "neighbour must be restored"
        );
    }

    #[test]
    fn light_from_initialize_light_computes_and_advances() {
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        let (chunks, _shared) = storage_closure(HashMap::new());
        let mut bridge = provider_for_storage(overworld(), true, true, chunks);
        // INITIALIZE_LIGHT itself is the executor's record-only rung. This
        // bridge is the subsequent LIGHT task and may compute from that status.
        bridge
            .light(&mut chunk)
            .expect("LIGHT from INITIALIZE_LIGHT");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
    }

    #[test]
    fn pre_light_and_post_light_statuses_are_typed_refusals() {
        let (chunks, _shared) = storage_closure(HashMap::new());
        let mut bridge = provider_for_storage(overworld(), true, true, chunks);
        let mut empty = generated_chunk();
        assert!(matches!(
            bridge.light(&mut empty),
            Err(GeneratedLightBridgeError::NotReady(ChunkStatus::Empty))
        ));
        assert_eq!(empty.get_persisted_status(), ChunkStatus::Empty);

        empty = generated_chunk();
        empty.set_persisted_status(ChunkStatus::Spawn);
        assert!(matches!(
            bridge.light(&mut empty),
            Err(GeneratedLightBridgeError::UnsupportedStatus(
                ChunkStatus::Spawn
            ))
        ));
        assert_eq!(empty.get_persisted_status(), ChunkStatus::Spawn);
    }

    #[test]
    fn already_lighted_chunk_refuses_without_complete_load_reconciliation() {
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_light_correct(true);
        chunk.set_sky_emptiness_map(Some(vec![false; 24]));
        let before = chunk.sky_nibbles()[0].to_vanilla_nibble();
        let (chunks, _shared) = storage_closure(HashMap::new());
        let mut bridge = provider_for_storage(overworld(), true, true, chunks);
        assert!(matches!(
            bridge.light(&mut chunk),
            Err(GeneratedLightBridgeError::PersistedLightLoadUnsupported {
                status: ChunkStatus::Light
            })
        ));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert_eq!(chunk.sky_nibbles()[0].to_vanilla_nibble(), before);
    }

    #[test]
    fn value_map_refusal_preserves_the_proto_and_retry_succeeds() {
        struct UncloneableMask;
        impl rivet_world::chunk::carving_mask::Mask for UncloneableMask {
            fn test(&self, _x: i32, _y: i32, _z: i32) -> bool {
                false
            }
        }

        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        chunk
            .get_or_create_carving_mask()
            .set_additional_mask(Box::new(UncloneableMask));
        let (chunks, _shared) = storage_closure(HashMap::new());
        let mut bridge = provider_for_storage(overworld(), true, true, chunks);

        assert!(matches!(
            bridge.light(&mut chunk),
            Err(GeneratedLightBridgeError::ValueMap(message))
                if message.contains("carving-mask")
        ));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::InitializeLight);
        assert!(chunk.get_carving_mask().is_some());

        chunk.take_carving_mask();
        bridge
            .light(&mut chunk)
            .expect("retry after value-map refusal succeeds");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
    }

    #[test]
    fn provider_panic_preserves_the_proto_and_retry_succeeds() {
        let panic_once = Arc::new(AtomicBool::new(true));
        let closure_panic = Arc::clone(&panic_once);
        let shared = Arc::new(Mutex::new(HashMap::from([(
            (1, 0),
            runtime_air(ChunkPos::new(1, 0)),
        )])));
        let closure_shared = Arc::clone(&shared);
        let chunks: super::super::star_light_provider_impl::ChunkAccessFn =
            Box::new(move |x, z, put| {
                if closure_panic.swap(false, Ordering::SeqCst) {
                    panic!("hostile provider callback");
                }
                let mut storage = closure_shared.lock().unwrap();
                match put {
                    Some(slot) => {
                        if let Some(chunk) = slot.take() {
                            storage.insert((x, z), chunk);
                        }
                        None
                    }
                    None => storage.remove(&(x, z)),
                }
            });
        let mut bridge = provider_for_storage(overworld(), true, true, chunks);
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);

        assert!(matches!(
            bridge.light(&mut chunk),
            Err(GeneratedLightBridgeError::ProviderPanic(message))
                if message.contains("hostile provider callback")
        ));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::InitializeLight);
        assert!(!chunk.is_light_correct());

        bridge
            .light(&mut chunk)
            .expect("retry after provider panic succeeds");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert!(shared.lock().unwrap().contains_key(&(1, 0)));
    }
}
