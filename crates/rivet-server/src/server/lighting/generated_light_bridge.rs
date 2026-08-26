//! The typed worldgen-to-runtime boundary for generated LIGHT.
//!
//! `GenerationChunkHolder` owns a worldgen `ProtoChunk`, while Starlight owns
//! runtime `StateId`/server-biome values. This module keeps that boundary
//! explicit: the caller supplies either a narrow take/put callback or one
//! finite, tick-thread-owned workspace; this value seam maps the center chunk
//! across the boundary, lights it, then maps it back. It does not install a
//! chunk, attach a live `ChunkMap`, or promote a status past `LIGHT`.

use std::collections::{HashMap, HashSet};

use rivet_registry::block_state::BlockState;
use rivet_registry::core::ChunkPos;
use rivet_registry::generated::blocks::BlockId;
use rivet_world::block::blocks::Blocks;
use rivet_world::chunk::proto_chunk::ProtoChunk;
use rivet_world::chunk::status::{ChunkStatus, GenError, GeneratedLightTask};
use rivet_world::chunk::storage::chunk_reconstruction::resolve_state_flags;
use rivet_world::chunk::storage::section_reconstruction::{
    BiomeId as WorldgenBiomeId, current_version_container_factory,
};
use rivet_world::level::height_accessor::SimpleLevelHeightAccessor;
use rivet_world::levelgen::heightmap::StateFlags;
use rivet_world::lighting::star_light_engine::get_empty_sections_for_chunk;
use rivet_world::lighting::star_light_provider::{LightProviderError, StarLightProvider};

use super::star_light_provider_impl::{LightChunk, SkyLightProvider};
use crate::server::level::level_chunk::{
    BiomeId as ServerBiomeId, StateId, StructureKey, state_flags, strategies,
};

/// The generated holder's worldgen value pair.
pub type GeneratedProto = ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>;

/// The runtime value pair used by [`SkyLightProvider`].
pub type RuntimeProto = ProtoChunk<StateId, ServerBiomeId, StructureKey>;

/// The finite runtime storage returned to the G4 owner when a generated-light
/// task detaches or is torn down.
pub type GeneratedLightStorage = HashMap<(i32, i32), LightChunk>;

/// The exact finite runtime coverage required by one generated LIGHT task.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GeneratedLightWorkspaceError {
    /// The requested channel combination is not implemented by this sky-only
    /// generated LIGHT slice. The check happens before taking ownership of the
    /// caller's map.
    #[error(
        "generated LIGHT requires sky-only channels (has_sky_light={has_sky_light}, has_block_light={has_block_light})"
    )]
    UnsupportedLightChannels {
        has_sky_light: bool,
        has_block_light: bool,
    },
    /// No runtime neighbour storage was supplied.
    #[error("generated LIGHT workspace at {center:?} has no runtime neighbours")]
    EmptyStorage { center: ChunkPos },
    /// A required radius-two neighbour is absent.
    #[error("generated LIGHT workspace at {center:?} is missing neighbour {pos:?}")]
    MissingNeighbour { center: ChunkPos, pos: ChunkPos },
    /// A required neighbour is present but cannot be used by Starlight because
    /// it is not already light-correct.
    #[error("generated LIGHT workspace neighbour {pos:?} at {center:?} is not light-correct")]
    NeighbourNotLightCorrect { center: ChunkPos, pos: ChunkPos },
    /// Storage contains a chunk outside the exact radius-two neighbourhood.
    #[error("generated LIGHT workspace at {center:?} contains unexpected chunk {pos:?}")]
    UnexpectedChunk { center: ChunkPos, pos: ChunkPos },
    /// A storage key does not agree with the runtime chunk's own position.
    #[error(
        "generated LIGHT workspace storage key ({key_x}, {key_z}) contains runtime chunk at {actual:?}"
    )]
    ChunkPositionMismatch {
        key_x: i32,
        key_z: i32,
        actual: ChunkPos,
    },
}

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
    /// The provider's block-light channel is enabled but not yet computed by
    /// this sky-only generated LIGHT bridge.
    #[error("generated LIGHT requires unsupported block-light completion at {0:?}")]
    UnsupportedLightChannel(ChunkStatus),
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

    /// Convert a borrowed generated proto into the runtime value pair used by
    /// the owned provider. This narrow crate-local seam lets the G4 owner build
    /// one workspace without duplicating the value mapping.
    pub(crate) fn runtime_from_generated(
        chunk: &GeneratedProto,
    ) -> Result<RuntimeProto, GeneratedLightBridgeError> {
        to_runtime(chunk)
    }

    /// Convert a runtime proto back to the generated value pair used by the
    /// holder. This is the inverse of [`Self::runtime_from_generated`].
    fn generated_from_runtime(
        chunk: RuntimeProto,
    ) -> Result<GeneratedProto, GeneratedLightBridgeError> {
        to_generated(chunk)
    }

    /// Whether this bridge owns a non-empty finite runtime workspace. Callback
    /// bridges deliberately report false: their storage belongs to the caller
    /// and cannot be proven usable without taking chunks.
    pub fn has_owned_runtime_storage(&self) -> bool {
        self.provider.has_owned_runtime_storage()
    }

    /// Whether this bridge has the sky-light channel used by generated LIGHT.
    pub fn supports_generated_light(&self) -> bool {
        self.provider.supports_generated_light()
    }

    /// Whether the owned workspace contains the required runtime chunk.
    pub fn has_owned_runtime_chunk(&self, pos: rivet_registry::core::ChunkPos) -> bool {
        self.provider.has_owned_runtime_chunk(pos)
    }

    /// Whether the owned workspace contains a usable, light-correct runtime
    /// neighbour.
    pub fn has_owned_usable_runtime_chunk(&self, pos: rivet_registry::core::ChunkPos) -> bool {
        self.provider.has_owned_usable_runtime_chunk(pos)
    }

    /// Consume the bridge's finite provider storage for transactional
    /// generated-workspace write-back. Callback-backed bridges return `None`
    /// when clean; any chunks stranded by a callback panic are returned in an
    /// owned map so consuming extraction cannot discard them.
    pub fn into_owned_runtime_storage(self) -> Option<HashMap<(i32, i32), LightChunk>> {
        self.provider.into_owned_storage()
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
        if !self.supports_generated_light() {
            return Err(GeneratedLightBridgeError::UnsupportedLightChannel(status));
        }

        let mut runtime = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::runtime_from_generated(chunk)
        })) {
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
                self.provider
                    .try_force_load_in_chunk_with(pos, &empty_sections)
                    .map_err(|error| match error {
                        LightProviderError::MissingChunk(_) => {
                            GeneratedLightBridgeError::PersistedLightLoadUnsupported { status }
                        }
                        LightProviderError::CallbackPanicked => {
                            GeneratedLightBridgeError::ProviderPanic(
                                "light-provider storage callback panicked".to_string(),
                            )
                        }
                    })?;
                self.provider.check_chunk_edges(pos);
                if self.provider.supports_persisted_light_load() {
                    Ok(())
                } else {
                    Err(GeneratedLightBridgeError::PersistedLightLoadUnsupported { status })
                }
            }));
            match provider_result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(error),
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

        let generated = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Self::generated_from_runtime(runtime)
        }))
        .map_err(|payload| GeneratedLightBridgeError::ConversionPanic(panic_message(payload)))??;
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

/// Build a generated-light bridge over finite runtime chunks owned by the
/// caller's tick-thread workspace.
pub fn provider_for_owned_storage(
    height_accessor: SimpleLevelHeightAccessor,
    has_sky_light: bool,
    has_block_light: bool,
    chunks: HashMap<(i32, i32), LightChunk>,
) -> GeneratedLightBridge {
    GeneratedLightBridge::new(SkyLightProvider::with_owned_storage(
        height_accessor,
        has_sky_light,
        has_block_light,
        chunks,
    ))
}

/// The radius of the finite runtime window required by one Starlight LIGHT
/// task. Paper's Moonrise LIGHT step has a write radius of two; the owned
/// workspace therefore requires the complete 5x5 window around its center.
pub const GENERATED_LIGHT_NEIGHBOR_RADIUS: i32 = 2;

fn required_light_neighbors(center: ChunkPos) -> Vec<ChunkPos> {
    let mut neighbors = Vec::with_capacity(24);
    for dz in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
        for dx in -GENERATED_LIGHT_NEIGHBOR_RADIUS..=GENERATED_LIGHT_NEIGHBOR_RADIUS {
            if dx != 0 || dz != 0 {
                neighbors.push(ChunkPos::new(
                    center.x().wrapping_add(dx),
                    center.z().wrapping_add(dz),
                ));
            }
        }
    }
    neighbors
}

/// A finite, tick-thread-owned generated-light workspace.
///
/// The center remains the caller's borrowed worldgen `ProtoChunk`; this value
/// owns exactly the 24 runtime neighbours in Paper's radius-two LIGHT window.
/// G4 converts its real generated support chunks into [`LightChunk`] values and
/// supplies them here. No chunk is installed into `ChunkMap`, and a partial,
/// empty, extra, or mis-keyed window is rejected at construction.
pub struct GeneratedLightWorkspace {
    /// `None` after the owner has detached the finite runtime workspace. A
    /// detached task remains safely droppable but cannot be run again.
    bridge: Option<GeneratedLightBridge>,
    center: ChunkPos,
    required_neighbors: Vec<ChunkPos>,
}

impl GeneratedLightWorkspace {
    /// Build a workspace over the exact radius-two runtime neighbour window
    /// already owned by the current tick thread. Validation borrows `chunks`
    /// first; ownership transfers only after every check succeeds, so every
    /// refusal leaves the caller's authoritative map intact for recovery.
    pub fn new(
        height_accessor: SimpleLevelHeightAccessor,
        has_sky_light: bool,
        has_block_light: bool,
        center: ChunkPos,
        chunks: &mut HashMap<(i32, i32), LightChunk>,
    ) -> Result<Self, GeneratedLightWorkspaceError> {
        // This slice computes sky light only. Reject unsupported channel
        // combinations before touching the caller's map; in particular, the
        // later `mem::take` must never strand block-light workspaces on error.
        if !has_sky_light || has_block_light {
            return Err(GeneratedLightWorkspaceError::UnsupportedLightChannels {
                has_sky_light,
                has_block_light,
            });
        }
        let required_neighbors = required_light_neighbors(center);
        let expected = required_neighbors.iter().copied().collect::<HashSet<_>>();
        if chunks.is_empty() {
            return Err(GeneratedLightWorkspaceError::EmptyStorage { center });
        }
        for (&(key_x, key_z), chunk) in chunks.iter() {
            let actual = chunk.get_pos();
            if actual != ChunkPos::new(key_x, key_z) {
                return Err(GeneratedLightWorkspaceError::ChunkPositionMismatch {
                    key_x,
                    key_z,
                    actual,
                });
            }
            if !expected.contains(&actual) {
                return Err(GeneratedLightWorkspaceError::UnexpectedChunk {
                    center,
                    pos: actual,
                });
            }
            if !chunk.is_light_correct() {
                return Err(GeneratedLightWorkspaceError::NeighbourNotLightCorrect {
                    center,
                    pos: actual,
                });
            }
        }
        if let Some(pos) = required_neighbors
            .iter()
            .copied()
            .find(|pos| !chunks.contains_key(&(pos.x(), pos.z())))
        {
            return Err(GeneratedLightWorkspaceError::MissingNeighbour { center, pos });
        }

        let chunks = std::mem::take(chunks);
        Ok(Self {
            bridge: Some(provider_for_owned_storage(
                height_accessor,
                has_sky_light,
                has_block_light,
                chunks,
            )),
            center,
            required_neighbors,
        })
    }

    /// The holder center this workspace is bound to.
    pub fn center(&self) -> ChunkPos {
        self.center
    }

    /// Whether the workspace has a real provider and complete owned runtime
    /// coverage. Construction rejects incomplete coverage; this remains a
    /// runtime capability check for the executor's preflight contract.
    pub fn has_usable_engine(&self) -> bool {
        self.bridge.as_ref().is_some_and(|bridge| {
            bridge.has_owned_runtime_storage() && bridge.supports_generated_light()
        })
    }

    /// The exact radius-two neighbour positions required by this workspace.
    pub fn required_neighbors(&self) -> &[ChunkPos] {
        &self.required_neighbors
    }

    /// Consume the finite runtime storage when the owner is rotating or
    /// handing the workspace to another tick-thread-owned integration.
    pub fn into_owned_runtime_storage(self) -> Option<GeneratedLightStorage> {
        self.bridge
            .and_then(GeneratedLightBridge::into_owned_runtime_storage)
    }

    /// Detach the provider storage while leaving the task wrapper in place for
    /// the typed `GeneratedLightTask` handoff. The task is unusable after this
    /// call, which prevents a detached provider from being run twice.
    pub fn take_owned_runtime_storage(&mut self) -> Option<GeneratedLightStorage> {
        self.bridge
            .take()
            .and_then(GeneratedLightBridge::into_owned_runtime_storage)
    }

    fn missing_neighbor(&self) -> Option<ChunkPos> {
        self.required_neighbors.iter().copied().find(|pos| {
            self.bridge
                .as_ref()
                .is_none_or(|bridge| !bridge.has_owned_usable_runtime_chunk(*pos))
        })
    }

    fn map_error(status: ChunkStatus, error: GeneratedLightBridgeError) -> GenError {
        match error {
            GeneratedLightBridgeError::NotReady(status)
            | GeneratedLightBridgeError::UnsupportedStatus(status) => {
                GenError::UnsupportedStatus(status)
            }
            GeneratedLightBridgeError::UnsupportedLightChannel(status) => {
                GenError::LightEngineMissing { status }
            }
            GeneratedLightBridgeError::PersistedLightLoadUnsupported { status } => {
                GenError::PersistedLightLoadUnsupported { status }
            }
            GeneratedLightBridgeError::ProviderPanic(_) => {
                GenError::LightProviderPanicked { status }
            }
            GeneratedLightBridgeError::ValueMap(_)
            | GeneratedLightBridgeError::ConversionPanic(_) => GenError::LightTaskFailed { status },
        }
    }
}

impl GeneratedLightTask<BlockState, WorldgenBiomeId, StructureKey, GeneratedLightStorage>
    for GeneratedLightWorkspace
{
    fn has_usable_engine(&self) -> bool {
        Self::has_usable_engine(self)
    }

    fn validate_light(&self, chunk: &GeneratedProto) -> Result<(), GenError> {
        if chunk.get_pos() != self.center {
            return Err(GenError::LightChunkMissing {
                status: ChunkStatus::Light,
                pos: self.center,
            });
        }
        if !self.has_usable_engine() {
            return Err(GenError::LightEngineMissing {
                status: ChunkStatus::Light,
            });
        }
        if let Some(pos) = self.missing_neighbor() {
            return Err(GenError::LightChunkMissing {
                status: ChunkStatus::Light,
                pos,
            });
        }
        Ok(())
    }

    fn initialize_light(&mut self, chunk: &mut GeneratedProto) -> Result<(), GenError> {
        if chunk.get_pos() != self.center {
            return Err(GenError::LightChunkMissing {
                status: ChunkStatus::InitializeLight,
                pos: self.center,
            });
        }
        if !self.has_usable_engine() {
            return Err(GenError::LightEngineMissing {
                status: ChunkStatus::InitializeLight,
            });
        }
        // Paper associates the engine before its initializeLight future is
        // returned. The runtime provider remains owned by this workspace.
        chunk.set_light_engine();
        Ok(())
    }

    fn light(&mut self, chunk: &mut GeneratedProto) -> Result<(), GenError> {
        let status = chunk.get_persisted_status();
        if chunk.get_pos() != self.center {
            return Err(GenError::LightChunkMissing {
                status: ChunkStatus::Light,
                pos: self.center,
            });
        }
        if status.is_before(ChunkStatus::InitializeLight) || status.is_after(ChunkStatus::Light) {
            return Err(GenError::UnsupportedStatus(status));
        }
        if !self.has_usable_engine() {
            return Err(GenError::LightEngineMissing {
                status: ChunkStatus::Light,
            });
        }
        if let Some(pos) = self.missing_neighbor() {
            return Err(GenError::LightChunkMissing {
                status: ChunkStatus::Light,
                pos,
            });
        }
        let bridge = self.bridge.as_mut().ok_or(GenError::LightEngineMissing {
            status: ChunkStatus::Light,
        })?;
        bridge
            .light(chunk)
            .map_err(|error| Self::map_error(ChunkStatus::Light, error))
    }

    fn take_owned_runtime_storage(&mut self) -> Option<GeneratedLightStorage> {
        Self::take_owned_runtime_storage(self)
    }
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
    use rivet_world::chunk::status::WorldGenContext;
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

    fn light_correct_runtime_air(pos: ChunkPos) -> LightChunk {
        let mut chunk = runtime_air(pos);
        chunk.set_light_correct(true);
        chunk.set_sky_emptiness_map(Some(vec![true; 24]));
        chunk
    }

    fn generated_chunk() -> GeneratedProto {
        generated_chunk_at(ChunkPos::ZERO)
    }

    fn generated_chunk_at(pos: ChunkPos) -> GeneratedProto {
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
            pos,
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
        let provider = provider_for_storage(overworld(), true, false, chunks);
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
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);
        // INITIALIZE_LIGHT itself is the executor's record-only rung. This
        // bridge is the subsequent LIGHT task and may compute from that status.
        bridge
            .light(&mut chunk)
            .expect("LIGHT from INITIALIZE_LIGHT");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
    }

    #[test]
    fn generated_light_capability_requires_supported_channels() {
        let (sky_only_chunks, _) = storage_closure(HashMap::new());
        assert!(
            provider_for_storage(overworld(), true, false, sky_only_chunks)
                .supports_generated_light()
        );

        let (overworld_chunks, _) = storage_closure(HashMap::new());
        let mut overworld_bridge = provider_for_storage(overworld(), true, true, overworld_chunks);
        assert!(!overworld_bridge.supports_generated_light());
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        assert!(matches!(
            overworld_bridge.light(&mut chunk),
            Err(GeneratedLightBridgeError::UnsupportedLightChannel(
                ChunkStatus::InitializeLight
            ))
        ));
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::InitializeLight);

        let (block_only_chunks, _) = storage_closure(HashMap::new());
        assert!(
            !provider_for_storage(overworld(), false, true, block_only_chunks)
                .supports_generated_light()
        );
    }

    #[test]
    fn pre_light_and_post_light_statuses_are_typed_refusals() {
        let (chunks, _shared) = storage_closure(HashMap::new());
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);
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
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);
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
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);

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
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);
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

    #[test]
    fn consuming_callback_bridge_returns_stranded_chunk_ownership() {
        let panic_once = Arc::new(AtomicBool::new(true));
        let closure_panic = Arc::clone(&panic_once);
        let shared = Arc::new(Mutex::new(HashMap::from([(
            (0, 0),
            runtime_air(ChunkPos::ZERO),
        )])));
        let closure_shared = Arc::clone(&shared);
        let chunks: super::super::star_light_provider_impl::ChunkAccessFn =
            Box::new(move |x, z, put| {
                if put.is_some() && closure_panic.swap(false, Ordering::SeqCst) {
                    panic!("hostile put callback");
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
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);

        assert_eq!(
            bridge
                .provider_mut()
                .try_light_chunk(ChunkPos::ZERO, &[Some(true); 24]),
            Err(LightProviderError::CallbackPanicked)
        );
        assert!(!shared.lock().unwrap().contains_key(&(0, 0)));

        let recovered = bridge
            .into_owned_runtime_storage()
            .expect("consuming extraction must return stranded callback storage");
        assert!(recovered.contains_key(&(0, 0)));
    }

    /// Drive a generated centre through the LIGHT status rung end to end: a
    /// `WorldGenContext` carries the real `GeneratedLightBridge` through
    /// [`WorldGenContext::with_light`], `generate_through(Light)` dispatches the
    /// Light step to the seam, and the bridge lights the generated proto,
    /// persisting the computed sky nibbles/emptiness and the LIGHT status back
    /// into it. This pins the reviewer-flagged integration: the write-back
    /// bridge is reachable from the LIGHT task, not only from its own unit
    /// tests.
    #[test]
    fn light_status_task_runs_the_bridge_and_persists_write_back() {
        use rivet_world::chunk::status::{GENERATION_PYRAMID, GenError, WorldGenContext};

        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        assert!(!chunk.is_light_correct());

        let (chunks, shared) =
            storage_closure(HashMap::from([((1, 0), runtime_air(ChunkPos::new(1, 0)))]));
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);

        let mut ctx: WorldGenContext<BlockState, WorldgenBiomeId, StructureKey> =
            WorldGenContext::new(
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| Ok(()),
            )
            .with_light(move |c: &mut GeneratedProto| {
                bridge.light(c).map_err(|error| match error {
                    GeneratedLightBridgeError::NotReady(status)
                    | GeneratedLightBridgeError::UnsupportedStatus(status) => {
                        GenError::UnsupportedStatus(status)
                    }
                    GeneratedLightBridgeError::PersistedLightLoadUnsupported { status } => {
                        GenError::PersistedLightLoadUnsupported { status }
                    }
                    _ => GenError::LightChunkMissing {
                        status: ChunkStatus::Light,
                        pos: c.get_pos(),
                    },
                })
            });

        ctx.generate_through(&GENERATION_PYRAMID, &mut chunk, ChunkStatus::Light)
            .expect("through LIGHT with the real bridge");

        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        // The bridge's write-back persisted real sky light: the floor light
        // section is no longer null (the engine computed it), a sky emptiness
        // map is present, and the stone-bearing section is non-empty.
        assert!(
            chunk.sky_nibbles()[1].to_vanilla_nibble().is_some(),
            "the engine computed the floor light section"
        );
        let emptiness = chunk.sky_emptiness_map().expect("emptiness map persisted");
        assert!(
            !emptiness[0],
            "the stone section is non-empty in the persisted map"
        );
        // The neighbour was returned to its original slot.
        assert!(shared.lock().unwrap().contains_key(&(1, 0)));
    }

    /// A light seam that fails to reconcile a persisted light load is a typed
    /// refusal that leaves the generated proto untouched — the same
    /// `PersistedLightLoadUnsupported` the engine-position path reports.
    #[test]
    fn light_status_task_persisted_load_refusal_is_atomic() {
        use rivet_world::chunk::status::{GENERATION_PYRAMID, GenError, WorldGenContext};

        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_light_correct(true);
        chunk.set_sky_emptiness_map(Some(vec![false; 24]));
        let before = chunk.sky_nibbles()[0].to_vanilla_nibble();

        let (chunks, _shared) = storage_closure(HashMap::new());
        let mut bridge = provider_for_storage(overworld(), true, false, chunks);
        let mut ctx: WorldGenContext<BlockState, WorldgenBiomeId, StructureKey> =
            WorldGenContext::new(
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| {},
                |_c: &mut GeneratedProto| Ok(()),
            )
            .with_light(move |c: &mut GeneratedProto| {
                bridge.light(c).map_err(|error| match error {
                    GeneratedLightBridgeError::NotReady(status)
                    | GeneratedLightBridgeError::UnsupportedStatus(status) => {
                        GenError::UnsupportedStatus(status)
                    }
                    GeneratedLightBridgeError::PersistedLightLoadUnsupported { status } => {
                        GenError::PersistedLightLoadUnsupported { status }
                    }
                    _ => GenError::LightChunkMissing {
                        status: ChunkStatus::Light,
                        pos: c.get_pos(),
                    },
                })
            });
        // The LIGHT step on an already-lighted (Light, light-correct) chunk is
        // the load branch — dispatch it directly (generate_through short-circuits
        // its idempotent target == current case before reaching the step).
        let step = GENERATION_PYRAMID.get_step_to(ChunkStatus::Light);
        let err = ctx
            .run_step(step, &mut chunk)
            .expect_err("persisted load must refuse without edge reconciliation");
        assert!(
            matches!(
                err,
                GenError::PersistedLightLoadUnsupported {
                    status: ChunkStatus::Light
                }
            ),
            "unexpected error: {err:?}"
        );
        // The generated proto is untouched: status, light-correct, nibbles.
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert_eq!(chunk.sky_nibbles()[0].to_vanilla_nibble(), before);
    }

    fn workspace_context(
        workspace: GeneratedLightWorkspace,
    ) -> WorldGenContext<BlockState, WorldgenBiomeId, StructureKey, GeneratedLightStorage> {
        let (context, detached) = WorldGenContext::new(
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| Ok(()),
        )
        .with_generated_light_task(workspace);
        assert!(
            detached.is_none(),
            "a fresh context cannot detach an existing generated-light workspace"
        );
        context
    }

    fn all_neighbor_storage(center: ChunkPos) -> GeneratedLightStorage {
        required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect()
    }

    fn workspace_with_all_neighbors(center: ChunkPos) -> GeneratedLightWorkspace {
        let mut chunks = all_neighbor_storage(center);
        GeneratedLightWorkspace::new(overworld(), true, false, center, &mut chunks)
            .expect("complete radius-two light window")
    }

    struct PanickingGeneratedLightTask {
        storage: Option<GeneratedLightStorage>,
    }

    impl GeneratedLightTask<BlockState, WorldgenBiomeId, StructureKey, GeneratedLightStorage>
        for PanickingGeneratedLightTask
    {
        fn has_usable_engine(&self) -> bool {
            true
        }

        fn validate_light(&self, _chunk: &GeneratedProto) -> Result<(), GenError> {
            Ok(())
        }

        fn initialize_light(&mut self, _chunk: &mut GeneratedProto) -> Result<(), GenError> {
            Ok(())
        }

        fn light(&mut self, _chunk: &mut GeneratedProto) -> Result<(), GenError> {
            panic!("test generated LIGHT task panic");
        }

        fn take_owned_runtime_storage(&mut self) -> Option<GeneratedLightStorage> {
            self.storage.take()
        }
    }

    #[test]
    fn required_light_neighbors_wrap_chunk_coordinates_like_java_ints() {
        let center = ChunkPos::new(i32::MAX, i32::MIN);
        let neighbors = required_light_neighbors(center);

        assert_eq!(neighbors.len(), 24);
        assert!(neighbors.contains(&ChunkPos::new(i32::MAX.wrapping_add(2), i32::MIN)));
        assert!(neighbors.contains(&ChunkPos::new(i32::MAX, i32::MIN.wrapping_sub(2))));
    }

    #[test]
    fn generated_workspace_associates_at_initialize_then_computes_at_light() {
        use rivet_world::chunk::status::GENERATION_PYRAMID;

        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::Carvers);
        let mut ctx = workspace_context(workspace_with_all_neighbors(ChunkPos::ZERO));

        ctx.run_step(
            GENERATION_PYRAMID.get_step_to(ChunkStatus::InitializeLight),
            &mut chunk,
        )
        .expect("INITIALIZE_LIGHT association");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::InitializeLight);
        assert!(chunk.has_light_engine());
        assert!(!chunk.is_light_correct());

        ctx.run_step(
            GENERATION_PYRAMID.get_step_to(ChunkStatus::Light),
            &mut chunk,
        )
        .expect("LIGHT compute");
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert!(chunk.sky_emptiness_map().is_some());
        assert!(
            chunk
                .sky_nibbles()
                .iter()
                .any(|nibble| nibble.to_vanilla_nibble().is_some()),
            "LIGHT must persist at least one computed sky nibble"
        );
        let storage = ctx
            .take_generated_light_storage()
            .expect("successful LIGHT must return the complete owned workspace");
        assert_eq!(storage.len(), 24);
        assert!(storage.contains_key(&(2, 2)));
        assert!(storage.contains_key(&(-2, -2)));
    }

    #[test]
    fn generated_workspace_extreme_chunk_coordinates_use_wrapping_arithmetic() {
        use rivet_world::chunk::status::GENERATION_PYRAMID;

        let center = ChunkPos::new(i32::MAX, i32::MIN);
        let mut ctx = workspace_context(workspace_with_all_neighbors(center));
        let mut chunk = generated_chunk_at(center);
        chunk.set_persisted_status(ChunkStatus::Carvers);
        ctx.run_step(
            GENERATION_PYRAMID.get_step_to(ChunkStatus::InitializeLight),
            &mut chunk,
        )
        .expect("INITIALIZE_LIGHT at extreme coordinates");
        ctx.run_step(
            GENERATION_PYRAMID.get_step_to(ChunkStatus::Light),
            &mut chunk,
        )
        .expect("LIGHT at extreme coordinates");
        let storage = ctx
            .take_generated_light_storage()
            .expect("extreme-coordinate LIGHT keeps ownership recoverable");
        assert_eq!(storage.len(), 24);
    }

    #[test]
    fn generated_workspace_typed_error_still_returns_owned_storage() {
        use rivet_world::chunk::status::{GENERATION_PYRAMID, GenError};

        let mut ctx = workspace_context(workspace_with_all_neighbors(ChunkPos::ZERO));
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::Carvers);
        let err = ctx
            .run_step(
                GENERATION_PYRAMID.get_step_to(ChunkStatus::Light),
                &mut chunk,
            )
            .expect_err("LIGHT before INITIALIZE_LIGHT must refuse");
        assert_eq!(err, GenError::UnsupportedStatus(ChunkStatus::Carvers));
        let storage = ctx
            .take_generated_light_storage()
            .expect("typed LIGHT refusal must return the complete owned workspace");
        assert_eq!(storage.len(), 24);
    }

    #[test]
    fn generated_workspace_replacement_returns_previous_owned_storage() {
        let mut ctx = workspace_context(workspace_with_all_neighbors(ChunkPos::ZERO));
        let detached = ctx
            .attach_generated_light_task(workspace_with_all_neighbors(ChunkPos::ZERO))
            .expect("replacing a workspace must return the previous storage");
        assert_eq!(detached.len(), 24);
        let current = ctx
            .take_generated_light_storage()
            .expect("replacement workspace remains recoverable");
        assert_eq!(current.len(), 24);
    }

    #[test]
    fn builder_workspace_replacement_returns_previous_owned_storage() {
        let (mut ctx, detached) = WorldGenContext::new(
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| Ok(()),
        )
        .with_generated_light_task(workspace_with_all_neighbors(ChunkPos::ZERO));
        assert!(detached.is_none());

        let (ctx_after_replacement, detached) =
            ctx.with_generated_light_task(workspace_with_all_neighbors(ChunkPos::ZERO));
        ctx = ctx_after_replacement;
        assert_eq!(
            detached.expect("builder replacement returns storage").len(),
            24
        );
        assert_eq!(
            ctx.take_generated_light_storage()
                .expect("replacement task remains owned")
                .len(),
            24
        );
    }

    #[test]
    fn generated_workspace_teardown_returns_owned_storage() {
        let ctx = workspace_context(workspace_with_all_neighbors(ChunkPos::ZERO));
        let storage = ctx
            .into_generated_light_storage()
            .expect("teardown must return the complete owned workspace");
        assert_eq!(storage.len(), 24);
    }

    #[test]
    fn generated_light_task_panic_still_returns_owned_storage() {
        use rivet_world::chunk::status::GENERATION_PYRAMID;

        let (mut ctx, detached): (
            WorldGenContext<BlockState, WorldgenBiomeId, StructureKey, GeneratedLightStorage>,
            Option<GeneratedLightStorage>,
        ) = WorldGenContext::new(
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| {},
            |_c: &mut GeneratedProto| Ok(()),
        )
        .with_generated_light_task(PanickingGeneratedLightTask {
            storage: Some(all_neighbor_storage(ChunkPos::ZERO)),
        });
        assert!(detached.is_none());
        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::InitializeLight);
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ctx.run_step(
                GENERATION_PYRAMID.get_step_to(ChunkStatus::Light),
                &mut chunk,
            )
            .expect("the test task panics before returning");
        }));
        assert!(panic.is_err());
        let storage = ctx
            .take_generated_light_storage()
            .expect("task panic must return the complete owned workspace");
        assert_eq!(storage.len(), 24);
    }

    #[test]
    fn generated_workspace_load_branch_refuses_without_edge_reconciliation() {
        use rivet_world::chunk::status::{GENERATION_PYRAMID, GenError};

        let mut chunk = generated_chunk();
        chunk.set_persisted_status(ChunkStatus::Light);
        chunk.set_light_correct(true);
        chunk.set_sky_emptiness_map(Some(vec![false; 24]));
        let before = chunk.sky_nibbles()[0].to_vanilla_nibble();
        let mut ctx = workspace_context(workspace_with_all_neighbors(ChunkPos::ZERO));

        let err = ctx
            .run_step(
                GENERATION_PYRAMID.get_step_to(ChunkStatus::Light),
                &mut chunk,
            )
            .expect_err("edge reconciliation is not implemented");
        assert_eq!(
            err,
            GenError::PersistedLightLoadUnsupported {
                status: ChunkStatus::Light
            }
        );
        assert_eq!(chunk.get_persisted_status(), ChunkStatus::Light);
        assert!(chunk.is_light_correct());
        assert_eq!(chunk.sky_nibbles()[0].to_vanilla_nibble(), before);
    }

    #[test]
    fn generated_workspace_rejects_partial_coverage_before_attachment() {
        let center = ChunkPos::ZERO;
        let missing = ChunkPos::new(2, 0);
        let mut chunks: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .filter(|pos| *pos != missing)
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        let before = chunks.len();

        let err = match GeneratedLightWorkspace::new(overworld(), true, false, center, &mut chunks)
        {
            Ok(_) => panic!("partial radius-two window must be rejected"),
            Err(err) => err,
        };
        assert_eq!(chunks.len(), before);
        assert_eq!(
            err,
            GeneratedLightWorkspaceError::MissingNeighbour {
                center,
                pos: missing,
            }
        );
    }

    #[test]
    fn generated_workspace_rejects_unsupported_light_channel_before_taking_storage() {
        let center = ChunkPos::ZERO;
        let mut chunks: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        let before_len = chunks.len();
        let before_keys: HashSet<_> = chunks.keys().copied().collect();

        let err = match GeneratedLightWorkspace::new(overworld(), false, true, center, &mut chunks)
        {
            Ok(_) => panic!("block-only generated lighting is unsupported"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            GeneratedLightWorkspaceError::UnsupportedLightChannels {
                has_sky_light: false,
                has_block_light: true,
            }
        );
        assert_eq!(chunks.len(), before_len);
        assert_eq!(chunks.keys().copied().collect::<HashSet<_>>(), before_keys);
    }

    #[test]
    fn generated_workspace_rejects_block_light_before_taking_storage() {
        let center = ChunkPos::ZERO;
        let mut chunks: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        let before_len = chunks.len();
        let before_keys: HashSet<_> = chunks.keys().copied().collect();

        let err = match GeneratedLightWorkspace::new(overworld(), true, true, center, &mut chunks) {
            Ok(_) => panic!("sky plus block lighting is unsupported"),
            Err(err) => err,
        };
        assert_eq!(
            err,
            GeneratedLightWorkspaceError::UnsupportedLightChannels {
                has_sky_light: true,
                has_block_light: true,
            }
        );
        assert_eq!(chunks.len(), before_len);
        assert_eq!(chunks.keys().copied().collect::<HashSet<_>>(), before_keys);
    }

    #[test]
    fn generated_workspace_rejects_unusable_light_neighbor() {
        let center = ChunkPos::ZERO;
        let unusable = ChunkPos::new(2, 0);
        let mut chunks: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        chunks.insert((unusable.x(), unusable.z()), runtime_air(unusable));

        let before = chunks.len();
        let err = match GeneratedLightWorkspace::new(overworld(), true, false, center, &mut chunks)
        {
            Ok(_) => panic!("an unlit neighbour must be rejected"),
            Err(err) => err,
        };
        assert_eq!(chunks.len(), before);
        assert_eq!(
            err,
            GeneratedLightWorkspaceError::NeighbourNotLightCorrect {
                center,
                pos: unusable,
            }
        );
    }

    #[test]
    fn generated_workspace_preserves_storage_on_miskey_and_extra_refusals() {
        let center = ChunkPos::ZERO;
        let mut miskeyed: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        let moved = miskeyed
            .remove(&(2, 0))
            .expect("radius-two neighbour exists");
        miskeyed.insert((99, 99), moved);
        let before = miskeyed.len();
        let error =
            match GeneratedLightWorkspace::new(overworld(), true, false, center, &mut miskeyed) {
                Ok(_) => panic!("mis-keyed runtime storage must be rejected"),
                Err(error) => error,
            };
        assert!(matches!(
            error,
            GeneratedLightWorkspaceError::ChunkPositionMismatch { .. }
        ));
        assert_eq!(miskeyed.len(), before);

        let mut extra: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        extra.insert((99, 99), light_correct_runtime_air(ChunkPos::new(99, 99)));
        let before = extra.len();
        let error = match GeneratedLightWorkspace::new(overworld(), true, false, center, &mut extra)
        {
            Ok(_) => panic!("extra runtime storage must be rejected"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            GeneratedLightWorkspaceError::UnexpectedChunk { .. }
        ));
        assert_eq!(extra.len(), before);
    }

    #[test]
    fn generated_workspace_rejects_empty_and_miscentered_coverage() {
        let center = ChunkPos::ZERO;
        let mut empty_chunks = HashMap::new();
        let empty_error =
            match GeneratedLightWorkspace::new(overworld(), true, false, center, &mut empty_chunks)
            {
                Ok(_) => panic!("empty runtime window must be rejected"),
                Err(err) => err,
            };
        assert!(empty_chunks.is_empty());
        assert_eq!(
            empty_error,
            GeneratedLightWorkspaceError::EmptyStorage { center }
        );

        let mut chunks: HashMap<_, _> = required_light_neighbors(center)
            .into_iter()
            .map(|pos| ((pos.x(), pos.z()), light_correct_runtime_air(pos)))
            .collect();
        chunks.insert((center.x(), center.z()), light_correct_runtime_air(center));
        let before = chunks.len();
        let center_error =
            match GeneratedLightWorkspace::new(overworld(), true, false, center, &mut chunks) {
                Ok(_) => panic!("center must remain borrowed, not stored"),
                Err(err) => err,
            };
        assert_eq!(chunks.len(), before);
        assert_eq!(
            center_error,
            GeneratedLightWorkspaceError::UnexpectedChunk {
                center,
                pos: center
            }
        );
    }
}
