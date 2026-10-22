//! Canonical structure starts and reference resolution. No generation status,
//! event acceptance, or Java identity-map iteration order is fabricated here.
use super::generated_world::GenerationChunkState;
use rivet_registry::{Identifier, core::ChunkPos};
use rivet_world::chunk::{
    proto_chunk::ProtoChunk, status::ChunkStatus, structure_access::StructureStartEntry,
};
use rivet_world::levelgen::structure::{StructureError, StructureStart};
use std::collections::{HashMap, HashSet};

#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum StructureManagerError {
    #[error("structure chunk {pos:?} is missing (requires {required:?})")]
    MissingChunk {
        pos: ChunkPos,
        required: ChunkStatus,
    },
    #[error("structure chunk {pos:?} is {actual:?}, requires {required:?}")]
    InsufficientStatus {
        pos: ChunkPos,
        actual: ChunkStatus,
        required: ChunkStatus,
    },
    #[error("structure start {key} in {pos:?} is unavailable: {reason}")]
    StartUnavailable {
        pos: ChunkPos,
        key: Identifier,
        reason: StructureError,
    },
    #[error("actual Java {map} iteration order is unavailable for {pos:?}")]
    OrderingUnavailable { pos: ChunkPos, map: &'static str },
    #[error("provided {map} order is not a permutation of canonical keys in {pos:?}")]
    InvalidOrder { pos: ChunkPos, map: &'static str },
    #[error("native structure registry identifier is invalid: {0}")]
    InvalidStructureId(String),
    #[error(
        "structure chunk {pos:?} retains unresolved persisted start keys outside its registry map"
    )]
    UnresolvedUnkeyedStarts { pos: ChunkPos },
}

/// Storage owns the chunks by value. Start references borrow that authority;
/// mutations must use its dirty-tracked chunk methods. Reference values must
/// use the actual reference-set iteration order, not insertion-order guesses.
pub trait StructureChunkStorage {
    fn status(&self, pos: ChunkPos) -> Option<ChunkStatus>;
    fn has_unkeyed_starts(&self, pos: ChunkPos) -> bool;
    fn start_keys(&self, pos: ChunkPos) -> Vec<Identifier>;
    fn start(&self, pos: ChunkPos, key: &Identifier) -> Option<&StructureStartEntry>;
    fn set_start(&mut self, pos: ChunkPos, key: Identifier, start: StructureStartEntry);
    fn reference_keys(&self, pos: ChunkPos) -> Vec<Identifier>;
    fn references(&self, pos: ChunkPos, key: &Identifier) -> Vec<u64>;
    fn add_reference(&mut self, pos: ChunkPos, key: Identifier, reference: u64);
}

macro_rules! storage_impl {
    ($value:ty, $read:expr, $write:expr) => {
        impl StructureChunkStorage for HashMap<ChunkPos, $value> {
            fn has_unkeyed_starts(&self, pos: ChunkPos) -> bool {
                ($read)(&self[&pos]).base().has_unkeyed_starts()
            }
            fn status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
                self.get(&pos)
                    .map(|value| ($read)(value).get_persisted_status())
            }
            fn start_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
                ($read)(&self[&pos])
                    .get_all_starts()
                    .keys()
                    .cloned()
                    .collect()
            }
            fn start(&self, pos: ChunkPos, key: &Identifier) -> Option<&StructureStartEntry> {
                self.get(&pos)
                    .and_then(|value| ($read)(value).get_start_for_structure(key))
            }
            fn set_start(&mut self, pos: ChunkPos, key: Identifier, start: StructureStartEntry) {
                ($write)(self.get_mut(&pos).expect("validated structure chunk"))
                    .set_start_for_structure(key, start);
            }
            fn reference_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
                ($read)(&self[&pos])
                    .get_all_references()
                    .keys()
                    .cloned()
                    .collect()
            }
            fn references(&self, pos: ChunkPos, key: &Identifier) -> Vec<u64> {
                ($read)(&self[&pos])
                    .get_references_for_structure(key)
                    .copied()
                    .collect()
            }
            fn add_reference(&mut self, pos: ChunkPos, key: Identifier, reference: u64) {
                ($write)(self.get_mut(&pos).expect("validated structure chunk"))
                    .add_reference_for_structure(key, reference);
            }
        }
    };
}
type GeneratedProto = ProtoChunk<
    rivet_registry::block_state::BlockState,
    rivet_world::chunk::storage::section_reconstruction::BiomeId,
    super::level_chunk::StructureKey,
>;
fn proto_ref(value: &GeneratedProto) -> &GeneratedProto {
    value
}
fn proto_mut(value: &mut GeneratedProto) -> &mut GeneratedProto {
    value
}
storage_impl!(GeneratedProto, proto_ref, proto_mut);
storage_impl!(
    GenerationChunkState,
    GenerationChunkState::chunk,
    GenerationChunkState::chunk_mut
);

impl StructureChunkStorage for super::chunk_map::ChunkMap {
    fn has_unkeyed_starts(&self, pos: ChunkPos) -> bool {
        self.get_chunk(pos)
            .expect("validated structure chunk")
            .has_unkeyed_starts()
    }
    fn status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
        self.get_chunk(pos)
            .map(super::level_chunk::LevelChunk::get_persisted_status)
    }
    fn start_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
        self.get_chunk(pos)
            .expect("validated structure chunk")
            .get_all_starts()
            .keys()
            .cloned()
            .collect()
    }
    fn start(&self, pos: ChunkPos, key: &Identifier) -> Option<&StructureStartEntry> {
        self.get_chunk(pos)
            .and_then(|chunk| chunk.get_start_for_structure(key))
    }
    fn set_start(&mut self, pos: ChunkPos, key: Identifier, start: StructureStartEntry) {
        self.get_chunk_mut(pos)
            .expect("validated structure chunk")
            .set_start_for_structure(key, start);
    }
    fn reference_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
        self.get_chunk(pos)
            .expect("validated structure chunk")
            .structures_references()
            .into_iter()
            .map(|entry| entry.identifier)
            .collect()
    }
    fn references(&self, pos: ChunkPos, key: &Identifier) -> Vec<u64> {
        self.get_chunk(pos)
            .expect("validated structure chunk")
            .structures_references()
            .into_iter()
            .find(|entry| &entry.identifier == key)
            .map_or_else(Vec::new, |entry| {
                entry
                    .references
                    .into_iter()
                    .map(|value| value as u64)
                    .collect()
            })
    }
    fn add_reference(&mut self, pos: ChunkPos, key: Identifier, reference: u64) {
        self.get_chunk_mut(pos)
            .expect("validated structure chunk")
            .add_reference_for_structure(key, reference);
    }
}

impl StructureChunkStorage for super::generated_chunk_owner::GeneratedChunkOwner {
    fn has_unkeyed_starts(&self, pos: ChunkPos) -> bool {
        self.state(pos)
            .expect("validated canonical chunk")
            .chunk()
            .base()
            .has_unkeyed_starts()
    }
    fn status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
        self.state(pos).map(GenerationChunkState::status)
    }
    fn start_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
        self.state(pos)
            .expect("validated canonical chunk")
            .chunk()
            .get_all_starts()
            .keys()
            .cloned()
            .collect()
    }
    fn start(&self, pos: ChunkPos, key: &Identifier) -> Option<&StructureStartEntry> {
        self.state(pos)
            .and_then(|state| state.chunk().get_start_for_structure(key))
    }
    fn set_start(&mut self, pos: ChunkPos, key: Identifier, start: StructureStartEntry) {
        self.state_mut(pos)
            .expect("validated canonical chunk")
            .chunk_mut()
            .set_start_for_structure(key, start)
    }
    fn reference_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
        self.state(pos)
            .expect("validated canonical chunk")
            .chunk()
            .get_all_references()
            .keys()
            .cloned()
            .collect()
    }
    fn references(&self, pos: ChunkPos, key: &Identifier) -> Vec<u64> {
        self.state(pos)
            .expect("validated canonical chunk")
            .chunk()
            .get_references_for_structure(key)
            .copied()
            .collect()
    }
    fn add_reference(&mut self, pos: ChunkPos, key: Identifier, reference: u64) {
        self.state_mut(pos)
            .expect("validated canonical chunk")
            .chunk_mut()
            .add_reference_for_structure(key, reference)
    }
}

/// HashMap structure keys use Java object identity hashes, which cannot be
/// inferred from Rust map order or registry ID. An authority supplies that
/// outer-map order; the manager validates membership before using it.
pub trait StructureOrderAuthority {
    fn start_keys(
        &self,
        pos: ChunkPos,
        keys: &[Identifier],
    ) -> Result<Vec<Identifier>, StructureManagerError>;
    fn reference_keys(
        &self,
        pos: ChunkPos,
        keys: &[Identifier],
    ) -> Result<Vec<Identifier>, StructureManagerError>;
}

/// Empty and singleton maps have a unique order. Larger maps refuse until an
/// actual ordering authority is installed, rather than selecting a new order.
pub struct UnambiguousStructureOrder;
impl StructureOrderAuthority for UnambiguousStructureOrder {
    fn start_keys(
        &self,
        pos: ChunkPos,
        keys: &[Identifier],
    ) -> Result<Vec<Identifier>, StructureManagerError> {
        unambiguous(pos, "starts", keys)
    }
    fn reference_keys(
        &self,
        pos: ChunkPos,
        keys: &[Identifier],
    ) -> Result<Vec<Identifier>, StructureManagerError> {
        unambiguous(pos, "references", keys)
    }
}
fn unambiguous(
    pos: ChunkPos,
    map: &'static str,
    keys: &[Identifier],
) -> Result<Vec<Identifier>, StructureManagerError> {
    if keys.len() <= 1 {
        Ok(keys.to_vec())
    } else {
        Err(StructureManagerError::OrderingUnavailable { pos, map })
    }
}

pub struct StructureManager<'a, S: StructureChunkStorage> {
    storage: &'a mut S,
    order: &'a dyn StructureOrderAuthority,
}
impl<'a, S: StructureChunkStorage> StructureManager<'a, S> {
    pub fn new(storage: &'a mut S, order: &'a dyn StructureOrderAuthority) -> Self {
        Self { storage, order }
    }
    fn require(&self, pos: ChunkPos, required: ChunkStatus) -> Result<(), StructureManagerError> {
        match self.storage.status(pos) {
            None => Err(StructureManagerError::MissingChunk { pos, required }),
            Some(actual) if actual.is_before(required) => {
                Err(StructureManagerError::InsufficientStatus {
                    pos,
                    actual,
                    required,
                })
            }
            Some(_) => Ok(()),
        }
    }
    fn native<'b>(
        pos: ChunkPos,
        key: &Identifier,
        entry: &'b StructureStartEntry,
    ) -> Result<&'b StructureStart, StructureManagerError> {
        entry
            .native()
            .map_err(|reason| StructureManagerError::StartUnavailable {
                pos,
                key: key.clone(),
                reason,
            })
    }
    pub fn get_start(
        &self,
        pos: ChunkPos,
        key: &Identifier,
    ) -> Result<Option<&StructureStart>, StructureManagerError> {
        self.require(pos, ChunkStatus::StructureStarts)?;
        if self.storage.has_unkeyed_starts(pos) {
            return Err(StructureManagerError::UnresolvedUnkeyedStarts { pos });
        }
        self.storage
            .start(pos, key)
            .map(|entry| Self::native(pos, key, entry))
            .transpose()
    }
    pub fn references_count(
        &self,
        pos: ChunkPos,
        key: &Identifier,
    ) -> Result<i32, StructureManagerError> {
        Ok(self
            .get_start(pos, key)?
            .map_or(0, StructureStart::references))
    }
    pub fn set_start(
        &mut self,
        pos: ChunkPos,
        key: Identifier,
        start: StructureStart,
    ) -> Result<(), StructureManagerError> {
        // The start task writes while the center still has its EMPTY parent.
        self.require(pos, ChunkStatus::Empty)?;
        self.storage.set_start(pos, key, start.into());
        Ok(())
    }
    fn ordered_keys(
        &self,
        pos: ChunkPos,
        references: bool,
    ) -> Result<Vec<Identifier>, StructureManagerError> {
        if !references && self.storage.has_unkeyed_starts(pos) {
            return Err(StructureManagerError::UnresolvedUnkeyedStarts { pos });
        }
        let keys = if references {
            self.storage.reference_keys(pos)
        } else {
            self.storage.start_keys(pos)
        };
        let ordered = if references {
            self.order.reference_keys(pos, &keys)?
        } else {
            self.order.start_keys(pos, &keys)?
        };
        let actual: HashSet<_> = keys.iter().collect();
        let supplied: HashSet<_> = ordered.iter().collect();
        if ordered.len() != keys.len() || actual != supplied {
            return Err(StructureManagerError::InvalidOrder {
                pos,
                map: if references { "references" } else { "starts" },
            });
        }
        Ok(ordered)
    }
    pub fn starts_for_structure(
        &self,
        center: ChunkPos,
        key: &Identifier,
    ) -> Result<Vec<&StructureStart>, StructureManagerError> {
        self.require(center, ChunkStatus::StructureReferences)?;
        let mut starts = Vec::new();
        for packed in self.storage.references(center, key) {
            let pos = ChunkPos::unpack(packed as i64);
            if let Some(start) = self.get_start(pos, key)?
                && start.is_valid()
            {
                starts.push(start);
            }
        }
        Ok(starts)
    }
    pub fn starts_for_chunk(
        &self,
        center: ChunkPos,
        matcher: impl Fn(&Identifier) -> bool,
    ) -> Result<Vec<&StructureStart>, StructureManagerError> {
        self.require(center, ChunkStatus::StructureReferences)?;
        let mut starts = Vec::new();
        for key in self.ordered_keys(center, true)? {
            if matcher(&key) {
                starts.extend(self.starts_for_structure(center, &key)?);
            }
        }
        Ok(starts)
    }
    /// Actual ChunkGenerator.createReferences scan, including the center and
    /// inclusive XZ intersections regardless of Y. Writes occur in source
    /// order; an error leaves preceding dirty-tracked additions intact.
    pub fn create_references(&mut self, center: ChunkPos) -> Result<(), StructureManagerError> {
        self.require(center, ChunkStatus::StructureStarts)?;
        let x = center.get_min_block_x();
        let z = center.get_min_block_z();
        for dx in -8..=8 {
            for dz in -8..=8 {
                let source =
                    ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                self.require(source, ChunkStatus::StructureStarts)?;
                for key in self.ordered_keys(source, false)? {
                    let start = self
                        .get_start(source, &key)?
                        .expect("validated canonical start key");
                    if !start.is_valid() {
                        continue;
                    }
                    let bounds = start.bounding_box().map_err(|reason| {
                        StructureManagerError::StartUnavailable {
                            pos: source,
                            key: key.clone(),
                            reason,
                        }
                    })?;
                    if bounds.max_x >= x
                        && bounds.min_x <= x.wrapping_add(15)
                        && bounds.max_z >= z
                        && bounds.min_z <= z.wrapping_add(15)
                    {
                        let id = start.structure().expect("valid native start").registry_id();
                        let structure = Identifier::try_parse(id).ok_or_else(|| {
                            StructureManagerError::InvalidStructureId(id.to_owned())
                        })?;
                        self.storage
                            .add_reference(center, structure, source.pack() as u64);
                    }
                }
            }
        }
        Ok(())
    }

    /// Successful reference values do not depend on interleaving different
    /// structure keys within one source chunk. Preflight *all* native inputs
    /// before mutation, then retain X-then-Z insertion order for every set.
    /// This intentionally does not certify Java identity-map order, global
    /// add-call traces, or source partial-write/first-error behavior. Call the
    /// ordered method above when those observations are required.
    pub fn create_references_commutative(
        &mut self,
        center: ChunkPos,
    ) -> Result<(), StructureManagerError> {
        self.require(center, ChunkStatus::StructureStarts)?;
        let x = center.get_min_block_x();
        let z = center.get_min_block_z();
        let mut writes = Vec::new();
        for dx in -8..=8 {
            for dz in -8..=8 {
                let source =
                    ChunkPos::new(center.x().wrapping_add(dx), center.z().wrapping_add(dz));
                self.require(source, ChunkStatus::StructureStarts)?;
                if self.storage.has_unkeyed_starts(source) {
                    return Err(StructureManagerError::UnresolvedUnkeyedStarts { pos: source });
                }
                for key in self.storage.start_keys(source) {
                    let start = self.get_start(source, &key)?.expect("canonical start key");
                    if !start.is_valid() {
                        continue;
                    }
                    let bounds = start.bounding_box().map_err(|reason| {
                        StructureManagerError::StartUnavailable {
                            pos: source,
                            key: key.clone(),
                            reason,
                        }
                    })?;
                    if bounds.max_x >= x
                        && bounds.min_x <= x.wrapping_add(15)
                        && bounds.max_z >= z
                        && bounds.min_z <= z.wrapping_add(15)
                    {
                        let id = start.structure().expect("valid native start").registry_id();
                        let key = Identifier::try_parse(id)
                            .ok_or_else(|| StructureManagerError::InvalidStructureId(id.into()))?;
                        writes.push((key, source.pack() as u64));
                    }
                }
            }
        }
        for (key, reference) in writes {
            self.storage.add_reference(center, key, reference);
        }
        Ok(())
    }
}

/// Connect resolved native starts to the actual terrain collector. Registry
/// settings provide the source matcher before reference chunks are traversed.
pub struct StructureBeardifierSource<'a, 'storage, S: StructureChunkStorage> {
    pub manager: &'a StructureManager<'storage, S>,
    pub access: &'a rivet_registry::RegistryAccess,
}
impl<S: StructureChunkStorage> rivet_world::levelgen::noise::beardifier::BeardifierSource
    for StructureBeardifierSource<'_, '_, S>
{
    fn starts_for_chunk(
        &self,
        center: ChunkPos,
    ) -> Result<
        Vec<rivet_world::levelgen::noise::beardifier::BeardifierStart>,
        rivet_world::levelgen::noise::beardifier::BeardifierError,
    > {
        use rivet_world::levelgen::noise::beardifier::{
            BeardifierError, BeardifierPiece, BeardifierStart,
        };
        use rivet_world::levelgen::structure::{
            StructurePiece, TerrainAdjustment, registry_keys::STRUCTURE,
        };
        let map_error = |error: StructureManagerError| match error {
            StructureManagerError::StartUnavailable { .. }
            | StructureManagerError::UnresolvedUnkeyedStarts { .. } => {
                BeardifierError::DeferredInput(error.to_string())
            }
            StructureManagerError::OrderingUnavailable { .. }
            | StructureManagerError::InvalidOrder { .. } => {
                BeardifierError::OrderingUnavailable(error.to_string())
            }
            _ => BeardifierError::AuthorityUnavailable(error.to_string()),
        };
        self.manager
            .require(center, ChunkStatus::StructureReferences)
            .map_err(map_error)?;
        let registry = self.access.lookup(&STRUCTURE).ok_or_else(|| {
            BeardifierError::AuthorityUnavailable("STRUCTURE registry missing".into())
        })?;
        let mut result = Vec::new();
        // Registry terrain settings are pure. Discarding known NONE entries
        // makes outer order unobservable when at most one contributor remains.
        let mut contributing = Vec::new();
        for key in self.manager.storage.reference_keys(center) {
            let structure = registry.get_optional(&key).ok_or_else(|| {
                BeardifierError::AuthorityUnavailable(format!("unknown referenced structure {key}"))
            })?;
            if structure.settings.terrain_adaptation == TerrainAdjustment::None {
                continue;
            }
            contributing.push(key);
        }
        let keys = if contributing.len() <= 1 {
            contributing
        } else {
            let contributing: HashSet<_> = contributing.into_iter().collect();
            self.manager
                .ordered_keys(center, true)
                .map_err(map_error)?
                .into_iter()
                .filter(|key| contributing.contains(key))
                .collect()
        };
        for key in keys {
            for start in self
                .manager
                .starts_for_structure(center, &key)
                .map_err(map_error)?
            {
                let terrain_adjustment = start
                    .structure()
                    .expect("resolved valid start")
                    .terrain_adjustment();
                let pieces = start
                    .pieces()
                    .pieces()
                    .iter()
                    .map(|piece| match piece {
                        StructurePiece::BuriedTreasure(data) => BeardifierPiece::Ordinary {
                            bounds: data.bounding_box,
                        },
                        StructurePiece::Mineshaft(_) => BeardifierPiece::Ordinary {
                            bounds: piece.data().bounding_box,
                        },
                    })
                    .collect();
                result.push(BeardifierStart {
                    terrain_adjustment,
                    pieces,
                });
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_world::levelgen::structure::{
        BoundingBox, PiecesContainer, StructureDefinition, StructurePiece, StructurePieceData,
        TerrainAdjustment,
    };
    fn key() -> Identifier {
        Identifier::parse("minecraft:buried_treasure")
    }
    fn chunks() -> HashMap<ChunkPos, GenerationChunkState> {
        chunks_at(ChunkPos::ZERO)
    }
    fn chunks_at(center: ChunkPos) -> HashMap<ChunkPos, GenerationChunkState> {
        let generator = std::sync::LazyLock::force(&GENERATOR);
        let mut result = HashMap::new();
        for x in -8..=8 {
            for z in -8..=8 {
                let pos = ChunkPos::new(center.x().wrapping_add(x), center.z().wrapping_add(z));
                let mut state = generator.create_holder(pos).into_state();
                // Standalone manager fixtures supply a completed starts view;
                // this does not admit these labels to the live generation owner.
                state
                    .chunk_mut()
                    .set_persisted_status(ChunkStatus::StructureStarts);
                state.chunk_mut().base_mut().try_mark_saved();
                result.insert(pos, state);
            }
        }
        result
    }
    static GENERATOR: std::sync::LazyLock<
        std::sync::Arc<super::super::generated_world::OverworldGenerator>,
    > = std::sync::LazyLock::new(|| {
        std::sync::Arc::new(super::super::generated_world::OverworldGenerator::new(42))
    });
    fn start(pos: ChunkPos, bounds: [i32; 6], references: i32) -> StructureStart {
        StructureStart::new(
            StructureDefinition::BuriedTreasure {
                registry_id: key().to_string(),
                terrain_adjustment: TerrainAdjustment::None,
            },
            pos,
            references,
            PiecesContainer::new(vec![StructurePiece::BuriedTreasure(StructurePieceData {
                bounding_box: BoundingBox::new(bounds),
                gen_depth: 0,
                orientation: None,
            })]),
        )
    }

    #[test]
    fn references_use_inclusive_horizontal_bounds_and_preserve_start_reference_counts() {
        let mut chunks = chunks();
        let touching = ChunkPos::new(-8, -8);
        let high = ChunkPos::new(-1, 1);
        let outside = ChunkPos::new(8, 8);
        chunks
            .get_mut(&touching)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(key(), start(touching, [-4, -900, -4, 0, -899, 0], 7));
        chunks
            .get_mut(&high)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(key(), start(high, [15, 900, 15, 18, 910, 18], 2));
        chunks
            .get_mut(&outside)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(key(), start(outside, [16, 0, 16, 20, 8, 20], 1));
        let mut manager = StructureManager::new(&mut chunks, &UnambiguousStructureOrder);
        manager.create_references(ChunkPos::ZERO).unwrap();
        assert_eq!(manager.references_count(touching, &key()).unwrap(), 7);
        assert_eq!(manager.references_count(high, &key()).unwrap(), 2);
        let refs: HashSet<_> = manager
            .storage
            .references(ChunkPos::ZERO, &key())
            .into_iter()
            .collect();
        assert_eq!(
            refs,
            HashSet::from([touching.pack() as u64, high.pack() as u64])
        );
        assert!(manager.storage[&ChunkPos::ZERO].chunk().base().is_unsaved());
        assert_eq!(
            manager.storage[&ChunkPos::ZERO].status(),
            ChunkStatus::StructureStarts,
            "manager never stamps completion"
        );
        manager
            .storage
            .get_mut(&ChunkPos::ZERO)
            .unwrap()
            .chunk_mut()
            .set_persisted_status(ChunkStatus::StructureReferences);
        assert_eq!(
            manager
                .starts_for_structure(ChunkPos::ZERO, &key())
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn commutative_references_keep_each_fastutil_set_identical_across_start_map_orders() {
        use rivet_world::levelgen::structure::{
            mineshaft::{MineshaftPiece, MineshaftPieceKind},
            structure_body::MineshaftKind,
        };
        let mine_key = Identifier::parse("minecraft:mineshaft");
        let mut snapshots = Vec::new();
        for reverse in [false, true] {
            let mut storage = chunks();
            for x in -8..=8 {
                let source = ChunkPos::new(x, 0);
                let mine = StructureStart::new(
                    StructureDefinition::Mineshaft {
                        registry_id: mine_key.to_string(),
                        terrain_adjustment: TerrainAdjustment::None,
                        kind: MineshaftKind::Normal,
                    },
                    source,
                    4,
                    PiecesContainer::new(vec![StructurePiece::Mineshaft(MineshaftPiece {
                        data: StructurePieceData {
                            bounding_box: BoundingBox::new([0, 40, 0, 15, 50, 15]),
                            gen_depth: 0,
                            orientation: None,
                        },
                        mine_type: MineshaftKind::Normal,
                        kind: MineshaftPieceKind::Room {
                            entrances: Vec::new(),
                        },
                    })]),
                );
                let treasure = start(source, [0, -100, 0, 0, -99, 0], 7);
                let entries = if reverse {
                    vec![(mine_key.clone(), mine), (key(), treasure)]
                } else {
                    vec![(key(), treasure), (mine_key.clone(), mine)]
                };
                for (key, start) in entries {
                    storage
                        .get_mut(&source)
                        .unwrap()
                        .chunk_mut()
                        .set_start_for_structure(key, start);
                }
            }
            let mut manager = StructureManager::new(&mut storage, &UnambiguousStructureOrder);
            assert!(matches!(
                manager.create_references(ChunkPos::ZERO),
                Err(StructureManagerError::OrderingUnavailable { .. })
            ));
            manager
                .create_references_commutative(ChunkPos::ZERO)
                .unwrap();
            snapshots.push((
                manager.storage.references(ChunkPos::ZERO, &key()),
                manager.storage.references(ChunkPos::ZERO, &mine_key),
            ));
            assert_eq!(
                manager.references_count(ChunkPos::ZERO, &mine_key).unwrap(),
                4
            );
            assert_eq!(manager.references_count(ChunkPos::ZERO, &key()).unwrap(), 7);
        }
        assert_eq!(snapshots[0], snapshots[1]);
        assert_eq!(snapshots[0].0.len(), 17);
        assert_eq!(snapshots[0].1.len(), 17);
    }

    #[test]
    fn commutative_reference_preflight_refuses_deferred_inputs_before_any_write() {
        let mut storage = chunks();
        let first = ChunkPos::new(-8, -8);
        storage
            .get_mut(&first)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(key(), start(first, [0, 0, 0, 0, 0, 0], 0));
        storage
            .get_mut(&ChunkPos::ZERO)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(
                key(),
                StructureStartEntry::deferred(rivet_nbt::tag::Tag::Int(
                    rivet_nbt::int_tag::IntTag::value_of(9),
                )),
            );
        let mut manager = StructureManager::new(&mut storage, &UnambiguousStructureOrder);
        assert!(matches!(
            manager.create_references_commutative(ChunkPos::ZERO),
            Err(StructureManagerError::StartUnavailable { .. })
        ));
        assert!(manager.storage.reference_keys(ChunkPos::ZERO).is_empty());
        assert_eq!(manager.references_count(first, &key()).unwrap(), 0);
    }

    #[test]
    fn deferred_and_missing_sources_refuse_without_erasing_prior_reference_writes() {
        let mut chunks = chunks();
        let first = ChunkPos::new(-8, -8);
        let next = ChunkPos::new(-8, -7);
        chunks
            .get_mut(&first)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(key(), start(first, [0, 0, 0, 0, 0, 0], 0));
        chunks
            .get_mut(&next)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(
                key(),
                StructureStartEntry::deferred(rivet_nbt::tag::Tag::Int(
                    rivet_nbt::int_tag::IntTag::value_of(9),
                )),
            );
        let mut manager = StructureManager::new(&mut chunks, &UnambiguousStructureOrder);
        assert!(
            matches!(manager.create_references(ChunkPos::ZERO),Err(StructureManagerError::StartUnavailable{pos,..}) if pos==next)
        );
        assert_eq!(
            manager.storage.references(ChunkPos::ZERO, &key()),
            [first.pack() as u64]
        );
        manager.storage.remove(&next);
        assert!(
            matches!(manager.create_references(ChunkPos::ZERO),Err(StructureManagerError::MissingChunk{pos,..}) if pos==next)
        );
    }

    #[test]
    fn unknown_multi_structure_identity_order_is_not_rust_hash_order() {
        let mut chunks = chunks();
        let source = ChunkPos::new(-8, -8);
        chunks
            .get_mut(&source)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(key(), StructureStart::invalid());
        chunks
            .get_mut(&source)
            .unwrap()
            .chunk_mut()
            .set_start_for_structure(
                Identifier::parse("minecraft:other"),
                StructureStart::invalid(),
            );
        let mut manager = StructureManager::new(&mut chunks, &UnambiguousStructureOrder);
        assert!(
            matches!(manager.create_references(ChunkPos::ZERO),Err(StructureManagerError::OrderingUnavailable{pos,map:"starts"}) if pos==source)
        );
        assert!(
            manager
                .storage
                .references(ChunkPos::ZERO, &key())
                .is_empty()
        );
        assert!(manager.get_start(source, &key()).unwrap().is_some());
    }

    #[test]
    fn unresolved_source_alias_is_preserved_and_cannot_be_treated_as_no_start() {
        let mut chunks = chunks();
        let source = ChunkPos::new(-8, -8);
        let mut payload = rivet_nbt::compound_tag::CompoundTag::new();
        payload.put_string("id", "minecraft:buried_treasure");
        payload.put_string("deferred_payload", "must remain owned");
        let mut raw = rivet_nbt::compound_tag::CompoundTag::new();
        raw.put(
            "buried_treasure".into(),
            rivet_nbt::tag::Tag::Compound(payload),
        );
        chunks
            .get_mut(&source)
            .unwrap()
            .chunk_mut()
            .base_mut()
            .defer_serialized_starts(&raw, |key| {
                Identifier::try_parse(key).filter(|id| id.to_string() == key)
            });
        assert!(
            chunks[&source].chunk().get_all_starts().is_empty(),
            "alias has no resolved registry-key entry yet"
        );
        let mut manager = StructureManager::new(&mut chunks, &UnambiguousStructureOrder);
        assert!(
            matches!(manager.get_start(source,&key()),Err(StructureManagerError::UnresolvedUnkeyedStarts{pos}) if pos==source)
        );
        assert!(
            matches!(manager.create_references(ChunkPos::ZERO),Err(StructureManagerError::UnresolvedUnkeyedStarts{pos}) if pos==source)
        );
        assert!(
            manager
                .storage
                .references(ChunkPos::ZERO, &key())
                .is_empty()
        );
        assert_eq!(
            manager.storage[&source]
                .chunk()
                .base()
                .serialized_starts(ToString::to_string),
            raw,
            "refusal cannot discard deferred alias payload"
        );
    }

    #[test]
    fn beardifier_filters_known_none_before_order_but_preserves_relevant_errors() {
        use rivet_world::levelgen::noise::beardifier::{BeardifierError, BeardifierSource};
        let generator = std::sync::LazyLock::force(&GENERATOR);
        let mut center = generator.create_holder(ChunkPos::ZERO).into_state();
        center
            .chunk_mut()
            .set_persisted_status(ChunkStatus::StructureReferences);
        let source = ChunkPos::new(1, 0);
        for key in [key(), Identifier::parse("minecraft:mineshaft")] {
            center
                .chunk_mut()
                .add_reference_for_structure(key, source.pack() as u64);
        }
        let mut chunks = HashMap::from([(ChunkPos::ZERO, center)]);
        let manager = StructureManager::new(&mut chunks, &UnambiguousStructureOrder);
        let view = StructureBeardifierSource {
            manager: &manager,
            access: generator.registry_access(),
        };
        assert!(
            view.starts_for_chunk(ChunkPos::ZERO).unwrap().is_empty(),
            "actual NONE settings do not need source chunks or outer order"
        );
        manager
            .storage
            .get_mut(&ChunkPos::ZERO)
            .unwrap()
            .chunk_mut()
            .add_reference_for_structure(
                Identifier::parse("minecraft:stronghold"),
                source.pack() as u64,
            );
        let view = StructureBeardifierSource {
            manager: &manager,
            access: generator.registry_access(),
        };
        assert!(
            matches!(
                view.starts_for_chunk(ChunkPos::ZERO),
                Err(BeardifierError::AuthorityUnavailable(_))
            ),
            "one relevant key bypasses irrelevant ordering but not missing chunks"
        );
        let mut pending = generator.create_holder(source).into_state();
        pending
            .chunk_mut()
            .set_persisted_status(ChunkStatus::StructureStarts);
        pending.chunk_mut().set_start_for_structure(
            Identifier::parse("minecraft:stronghold"),
            StructureStartEntry::deferred(rivet_nbt::tag::Tag::Int(
                rivet_nbt::int_tag::IntTag::value_of(9),
            )),
        );
        manager.storage.insert(source, pending);
        let view = StructureBeardifierSource {
            manager: &manager,
            access: generator.registry_access(),
        };
        assert!(matches!(
            view.starts_for_chunk(ChunkPos::ZERO),
            Err(BeardifierError::DeferredInput(_))
        ));
        manager
            .storage
            .get_mut(&ChunkPos::ZERO)
            .unwrap()
            .chunk_mut()
            .add_reference_for_structure(
                Identifier::parse("minecraft:village_plains"),
                source.pack() as u64,
            );
        let view = StructureBeardifierSource {
            manager: &manager,
            access: generator.registry_access(),
        };
        assert!(
            matches!(
                view.starts_for_chunk(ChunkPos::ZERO),
                Err(BeardifierError::OrderingUnavailable(_))
            ),
            "two relevant keys still require actual relative order"
        );
    }

    struct TraceStorage {
        chunks: HashMap<ChunkPos, GenerationChunkState>,
        reads: std::cell::RefCell<Vec<[i32; 2]>>,
        writes: Vec<[i64; 1]>,
    }
    impl StructureChunkStorage for TraceStorage {
        fn has_unkeyed_starts(&self, pos: ChunkPos) -> bool {
            self.chunks.has_unkeyed_starts(pos)
        }
        fn status(&self, pos: ChunkPos) -> Option<ChunkStatus> {
            self.chunks.status(pos)
        }
        fn start_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
            self.reads.borrow_mut().push([pos.x(), pos.z()]);
            self.chunks.start_keys(pos)
        }
        fn start(&self, pos: ChunkPos, key: &Identifier) -> Option<&StructureStartEntry> {
            self.chunks.start(pos, key)
        }
        fn set_start(&mut self, pos: ChunkPos, key: Identifier, start: StructureStartEntry) {
            self.chunks.set_start(pos, key, start)
        }
        fn reference_keys(&self, pos: ChunkPos) -> Vec<Identifier> {
            self.chunks.reference_keys(pos)
        }
        fn references(&self, pos: ChunkPos, key: &Identifier) -> Vec<u64> {
            self.chunks.references(pos, key)
        }
        fn add_reference(&mut self, pos: ChunkPos, key: Identifier, reference: u64) {
            self.writes.push([reference as i64]);
            self.chunks.add_reference(pos, key, reference)
        }
    }
    #[test]
    fn reference_scan_reads_writes_fastutil_order_and_dirty_state_match_live_paper() {
        for line in
            include_str!("../../../../../tools/rivet-oracle/fixtures/structure-references.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let pos = |value: &serde_json::Value| {
                ChunkPos::new(
                    value[0].as_i64().unwrap() as i32,
                    value[1].as_i64().unwrap() as i32,
                )
            };
            let center = pos(&row["center"]);
            let mut storage = TraceStorage {
                chunks: chunks_at(center),
                reads: std::cell::RefCell::new(Vec::new()),
                writes: Vec::new(),
            };
            for input in row["inputs"].as_array().unwrap() {
                let source = pos(&input["pos"]);
                let start = if input["invalid"].as_bool().unwrap() {
                    StructureStart::invalid()
                } else {
                    start(
                        source,
                        std::array::from_fn(|index| {
                            input["bounds"][index].as_i64().unwrap() as i32
                        }),
                        input["references"].as_i64().unwrap() as i32,
                    )
                };
                storage
                    .chunks
                    .get_mut(&source)
                    .unwrap()
                    .chunk_mut()
                    .set_start_for_structure(key(), start);
            }
            if row["preexisting"].as_bool().unwrap() {
                storage.chunks.add_reference(
                    center,
                    key(),
                    ChunkPos::new(center.x() - 8, center.z() - 8).pack() as u64,
                );
            }
            storage
                .chunks
                .get_mut(&center)
                .unwrap()
                .chunk_mut()
                .base_mut()
                .try_mark_saved();
            let mut manager = StructureManager::new(&mut storage, &UnambiguousStructureOrder);
            manager.create_references(center).unwrap();
            assert_eq!(
                serde_json::json!(*manager.storage.reads.borrow()),
                row["reads"]
            );
            assert_eq!(serde_json::json!(manager.storage.writes), row["writes"]);
            assert_eq!(
                serde_json::json!(
                    manager
                        .storage
                        .references(center, &key())
                        .into_iter()
                        .map(|value| value as i64)
                        .collect::<Vec<_>>()
                ),
                row["references"]
            );
            assert_eq!(
                manager.storage.chunks[&center].chunk().base().is_unsaved(),
                row["dirty"].as_bool().unwrap()
            );
            assert_eq!(
                manager.storage.chunks[&center].status(),
                ChunkStatus::from_identifier(row["status"].as_str().unwrap()).unwrap()
            );
            for input in row["inputs"].as_array().unwrap() {
                assert_eq!(
                    manager
                        .references_count(pos(&input["pos"]), &key())
                        .unwrap(),
                    input["references"].as_i64().unwrap() as i32
                );
            }
        }
    }
}
