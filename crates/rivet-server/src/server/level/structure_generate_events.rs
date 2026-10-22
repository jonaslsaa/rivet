//! The distinct AsyncStructureGenerateEvent channel. Native handlers share the
//! spawn registry's real plugin lifecycle, priorities, diagnostics and nag state.
//! Transformers are native typed callbacks, not a fabricated Bukkit/JVM host.
use super::structure_spawn_events::{
    NativeListenerError, NativeListenerId, NativePluginMetadata, NativeStructureEventRegistry,
    StructureSpawnPayload,
};
use super::structure_transformer::{BlockTransformer, EntityTransformer};
use rivet_registry::Identifier;
use rivet_world::{
    entity::chest_minecart::NativeChestMinecart, levelgen::structure::StructureError,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StructureGenerateCause {
    Command,
    WorldGeneration,
    Custom,
}
#[derive(Clone, Debug)]
pub struct StructureGeneratePayload {
    pub structure: StructureSpawnPayload,
    pub cause: StructureGenerateCause,
    asynchronous: bool,
}
impl StructureGeneratePayload {
    pub fn new(structure: StructureSpawnPayload, cause: StructureGenerateCause) -> Self {
        Self {
            structure,
            cause,
            asynchronous: false,
        }
    }
    pub fn is_asynchronous(&self) -> bool {
        self.asynchronous
    }
    pub(super) fn set_asynchronous(&mut self, value: bool) {
        self.asynchronous = value;
    }
}
pub type NativeBlockTransformer = BlockTransformer<NativeChestMinecart>;
pub type NativeEntityTransformer = EntityTransformer<NativeChestMinecart>;

/// LinkedHashMap semantics: replacement keeps its slot; remove/reinsert moves
/// it to the end. The event is not cancellable.
#[derive(Default)]
pub struct StructureTransformers {
    blocks: Vec<(Identifier, NativeBlockTransformer)>,
    entities: Vec<(Identifier, NativeEntityTransformer)>,
}
impl StructureTransformers {
    pub fn block(&self, key: &Identifier) -> Option<&NativeBlockTransformer> {
        self.blocks.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn entity(&self, key: &Identifier) -> Option<&NativeEntityTransformer> {
        self.entities.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn set_block(&mut self, key: Identifier, callback: NativeBlockTransformer) {
        if let Some((_, value)) = self.blocks.iter_mut().find(|(k, _)| k == &key) {
            *value = callback;
        } else {
            self.blocks.push((key, callback));
        }
    }
    pub fn set_entity(&mut self, key: Identifier, callback: NativeEntityTransformer) {
        if let Some((_, value)) = self.entities.iter_mut().find(|(k, _)| k == &key) {
            *value = callback;
        } else {
            self.entities.push((key, callback));
        }
    }
    pub fn remove_block(&mut self, key: &Identifier) {
        self.blocks.retain(|(k, _)| k != key);
    }
    pub fn remove_entity(&mut self, key: &Identifier) {
        self.entities.retain(|(k, _)| k != key);
    }
    pub fn clear_blocks(&mut self) {
        self.blocks.clear();
    }
    pub fn clear_entities(&mut self) {
        self.entities.clear();
    }
    pub fn into_lists(self) -> (Vec<NativeBlockTransformer>, Vec<NativeEntityTransformer>) {
        (
            self.blocks.into_iter().map(|(_, v)| v).collect(),
            self.entities.into_iter().map(|(_, v)| v).collect(),
        )
    }
}
pub type GenerateListener = dyn Fn(&StructureGeneratePayload, &mut StructureTransformers) -> Result<(), NativeListenerError>
    + Send
    + Sync;
pub type GenerateExceptionListener = dyn Fn(&StructureGenerateException, &mut StructureTransformers) -> Result<(), NativeListenerError>
    + Send
    + Sync;
pub struct StructureGenerateException {
    pub plugin: NativePluginMetadata,
    pub listener: NativeListenerId,
    pub event: Arc<StructureGeneratePayload>,
    pub message: String,
}
#[derive(Clone)]
pub enum StructureGenerateAuthority {
    Unavailable(String),
    RegisteredNative(Arc<NativeStructureEventRegistry>),
}
impl StructureGenerateAuthority {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self::Unavailable(reason.into())
    }
    pub fn registered_native(registry: Arc<NativeStructureEventRegistry>) -> Self {
        Self::RegisteredNative(registry)
    }
    /// Dispatch on the real caller thread. Paper derives this event's async
    /// flag from !isPrimaryThread; unlike AsyncStructureSpawnEvent, /place is
    /// legitimately synchronous. No tick-owned world is moved to a fake thread.
    pub fn dispatch(
        &self,
        payload: StructureGeneratePayload,
    ) -> Result<StructureTransformers, StructureError> {
        match self {
            Self::Unavailable(reason) => Err(StructureError::PlacementUnavailable(format!(
                "AsyncStructureGenerateEvent authority unavailable: {reason}"
            ))),
            Self::RegisteredNative(registry) => registry
                .dispatch_generate_on_current_thread(payload)
                .map_err(|error| StructureError::PlacementUnavailable(error.to_string())),
        }
    }
}
