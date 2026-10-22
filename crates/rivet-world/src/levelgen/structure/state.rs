//! ChunkGeneratorStructureState: registry-backed filtering, keyed Paper salts,
//! lazy real-biome ring caches, and ORIGINAL-registry exclusion edges.
use super::{
    placement::{
        ConcentricRingsPlacement, PaperStructureSeeds, PlacementAuthority, PlacementError,
        StructurePlacement, generate_ring_positions_with_biome_source,
    },
    registry_keys::{STRUCTURE, STRUCTURE_SET},
    structure_registry::StructureSelectionEntry,
};
use crate::biome::{biome_source::BiomeSource, climate::Sampler};
use rivet_registry::{Identifier, access::RegistryAccess, core::ChunkPos};
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
#[path = "state_test.rs"]
mod tests;

#[derive(Clone, Debug)]
pub struct PossibleStructureSet {
    pub key: Identifier,
    pub structures: Vec<StructureSelectionEntry>,
    pub placement: Arc<StructurePlacement>,
    /// Only injected vanilla random spreads retain Paper's keyed subclass.
    pub keyed_random_spread: bool,
}

pub struct ChunkGeneratorStructureState {
    access: RegistryAccess,
    source: Arc<dyn BiomeSource>,
    sampler: Sampler,
    level_seed: i64,
    concentric_seed: i64,
    seeds: PaperStructureSeeds,
    possible_sets: Vec<PossibleStructureSet>,
    original_placements: HashMap<Identifier, Arc<StructurePlacement>>,
    ring_positions: HashMap<Identifier, Vec<ChunkPos>>,
    placements_for_structure: HashMap<Identifier, Vec<Arc<StructurePlacement>>>,
    positions_generated: bool,
    exclusion_stack: Vec<Identifier>,
}
impl ChunkGeneratorStructureState {
    pub fn create_for_normal(
        access: RegistryAccess,
        level_seed: i64,
        source: Arc<dyn BiomeSource>,
        sampler: Sampler,
        seeds: PaperStructureSeeds,
    ) -> Result<Self, PlacementError> {
        Self::create(access, level_seed, level_seed, source, sampler, seeds, None)
    }
    /// The override list is the real flat generator's supplied structure sets;
    /// None means normal registry enumeration, never an invented empty list.
    pub fn create_for_flat(
        access: RegistryAccess,
        level_seed: i64,
        source: Arc<dyn BiomeSource>,
        sampler: Sampler,
        seeds: PaperStructureSeeds,
        overrides: &[Identifier],
    ) -> Result<Self, PlacementError> {
        Self::create(
            access,
            level_seed,
            0,
            source,
            sampler,
            seeds,
            Some(overrides),
        )
    }
    fn create(
        access: RegistryAccess,
        level_seed: i64,
        concentric_seed: i64,
        source: Arc<dyn BiomeSource>,
        sampler: Sampler,
        seeds: PaperStructureSeeds,
        overrides: Option<&[Identifier]>,
    ) -> Result<Self, PlacementError> {
        let sets = access.lookup(&STRUCTURE_SET).ok_or_else(|| {
            PlacementError::AuthorityUnavailable("STRUCTURE_SET registry missing".into())
        })?;
        let structures = access.lookup(&STRUCTURE).ok_or_else(|| {
            PlacementError::AuthorityUnavailable("STRUCTURE registry missing".into())
        })?;
        let possible_biomes = source.possible_biomes();
        let originals: HashMap<_, _> = sets
            .entry_set_ref()
            .into_iter()
            .map(|(key, set)| (key.identifier().clone(), Arc::new(set.placement.clone())))
            .collect();
        let keys = overrides.map_or_else(|| sets.key_set(), |keys| keys.to_vec());
        let mut possible_sets = Vec::new();
        for key in keys {
            let set = sets.get_optional(&key).ok_or_else(|| {
                PlacementError::AuthorityUnavailable(format!("unknown structure set {key}"))
            })?;
            if !set.structures.iter().any(|entry| {
                entry
                    .structure
                    .value(structures)
                    .settings
                    .biomes
                    .iter()
                    .any(|biome| possible_biomes.contains(biome))
            }) {
                continue;
            }
            let original = originals[&key].clone();
            let keyed_random_spread = key.namespace() == "minecraft"
                && matches!(&*original, StructurePlacement::RandomSpread(_));
            let placement = if keyed_random_spread {
                let mut injected = (*original).clone();
                seeds.inject_random_spread(&key, &mut injected);
                Arc::new(injected)
            } else {
                original
            };
            possible_sets.push(PossibleStructureSet {
                key,
                structures: set.structures.clone(),
                placement,
                keyed_random_spread,
            });
        }
        Ok(Self {
            access,
            source,
            sampler,
            level_seed,
            concentric_seed,
            seeds,
            possible_sets,
            original_placements: originals,
            ring_positions: HashMap::new(),
            placements_for_structure: HashMap::new(),
            positions_generated: false,
            exclusion_stack: Vec::new(),
        })
    }
    pub fn level_seed(&self) -> i64 {
        self.level_seed
    }
    pub fn possible_structure_sets(&self) -> &[PossibleStructureSet] {
        &self.possible_sets
    }

    pub fn ensure_structures_generated(&mut self) -> Result<(), PlacementError> {
        if self.positions_generated {
            return Ok(());
        }
        let structures = self.access.lookup(&STRUCTURE).ok_or_else(|| {
            PlacementError::AuthorityUnavailable("STRUCTURE registry missing".into())
        })?;
        let possible_biomes = self.source.possible_biomes();
        let mut placements: HashMap<Identifier, Vec<Arc<StructurePlacement>>> = HashMap::new();
        let mut rings = HashMap::new();
        for set in &self.possible_sets {
            let mut has_any = false;
            for entry in &set.structures {
                let structure = entry.structure.value(structures);
                if structure
                    .settings
                    .biomes
                    .iter()
                    .any(|biome| possible_biomes.contains(biome))
                {
                    let key = entry.structure.unwrap_key(structures).ok_or_else(|| {
                        PlacementError::AuthorityUnavailable(
                            "direct structure has no registry identity".into(),
                        )
                    })?;
                    placements
                        .entry(key.identifier().clone())
                        .or_default()
                        .push(set.placement.clone());
                    has_any = true;
                }
            }
            if has_any && let StructurePlacement::ConcentricRings(placement) = &*set.placement {
                rings.insert(
                    set.key.clone(),
                    generate_ring_positions_with_biome_source(
                        placement,
                        self.concentric_seed,
                        self.seeds.ring_override(&set.key),
                        &*self.source,
                        &self.sampler,
                    )?,
                );
            }
        }
        self.placements_for_structure = placements;
        self.ring_positions = rings;
        self.positions_generated = true;
        Ok(())
    }
    pub fn placements_for_structure(
        &mut self,
        key: &Identifier,
    ) -> Result<&[Arc<StructurePlacement>], PlacementError> {
        self.ensure_structures_generated()?;
        Ok(self
            .placements_for_structure
            .get(key)
            .map_or(&[], Vec::as_slice))
    }
    pub fn ring_positions_for_set(
        &mut self,
        key: &Identifier,
    ) -> Result<Option<&[ChunkPos]>, PlacementError> {
        self.ensure_structures_generated()?;
        Ok(self.ring_positions.get(key).map(Vec::as_slice))
    }
    /// Main generation queries the injected keyed placement, not the original
    /// registry object. Filtering only affects this possible-set entry point.
    pub fn is_structure_chunk(
        &mut self,
        key: &Identifier,
        x: i32,
        z: i32,
    ) -> Result<bool, PlacementError> {
        let Some(set) = self.possible_sets.iter().find(|set| &set.key == key) else {
            return Ok(false);
        };
        let placement = set.placement.clone();
        let override_seed = self
            .seeds
            .frequency_override(set.keyed_random_spread.then_some(&set.key));
        let seed = self.level_seed;
        placement.is_structure_chunk(self, seed, x, z, override_seed)
    }
}
impl PlacementAuthority for ChunkGeneratorStructureState {
    fn is_structure_chunk(
        &mut self,
        key: &Identifier,
        x: i32,
        z: i32,
    ) -> Result<bool, PlacementError> {
        let placement = self.original_placements.get(key).cloned().ok_or_else(|| {
            PlacementError::AuthorityUnavailable(format!("exclusion structure set {key} missing"))
        })?;
        if self.exclusion_stack.contains(key) {
            return Err(PlacementError::AuthorityUnavailable(format!(
                "cyclic exclusion structure set {key}"
            )));
        }
        self.exclusion_stack.push(key.clone());
        // ExclusionZone.otherSet still holds the ORIGINAL registry object in
        // Paper, not injectSpigot's keyed copy. No Paper override key applies.
        let seed = self.level_seed;
        let result = placement.is_structure_chunk(self, seed, x, z, None);
        self.exclusion_stack.pop();
        result
    }
    fn ring_positions(
        &mut self,
        placement: &ConcentricRingsPlacement,
    ) -> Result<Option<&[ChunkPos]>, PlacementError> {
        self.ensure_structures_generated()?;
        let key = self
            .original_placements
            .iter()
            .find_map(|(key, candidate)| match &**candidate {
                StructurePlacement::ConcentricRings(candidate)
                    if std::ptr::eq(candidate, placement) =>
                {
                    Some(key)
                }
                _ => None,
            });
        Ok(key
            .and_then(|key| self.ring_positions.get(key))
            .map(Vec::as_slice))
    }
}
