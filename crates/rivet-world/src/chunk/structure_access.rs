//! Port of `net.minecraft.world.level.chunk.StructureAccess` (MC 26.2) — the
//! per-chunk structure-starts/references surface.
//!
//! Java: `StructureAccess.java` in `working/Paper`, implemented by `ChunkAccess`
//! (structureStarts + structuresRefences maps with a shared empty
//! `LongOpenHashSet` default). Keys remain caller-supplied structure identities;
//! values own executable native starts or explicitly deferred persisted tags.
//! Reference values use fastutil-compatible long probe tables, preserving
//! constructor capacity, collision history and descending-slot iteration.
//!
//! Java's `getReferencesForStructure` returns the shared immutable
//! `EMPTY_REFERENCE_SET` for an unknown structure; the port yields no
//! elements. The `markUnsaved` side effects of the setters are chunk
//! dirty-tracking performed by the owning `ChunkAccess` wrappers.
//!
//! The outer map retains decoded source-key order. This is not Java's
//! structure-object identity HashMap ordering; multi-structure consumers still
//! need that distinct ordering authority.

use crate::levelgen::structure::{StructureError, StructureStart};
use indexmap::IndexMap;
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::core::ChunkPos;
use rivet_util::long_open_hash_set::LongOpenHashSet;
use std::collections::HashMap;

/// Canonical start ownership. Deferred persisted data is not an executable
/// start and cannot answer validity/bounds queries without real registry and
/// subtype decoding authority.
#[derive(Clone, Debug, PartialEq)]
pub enum StructureStartEntry {
    Native(Box<StructureStart>),
    Deferred {
        tag: Box<Tag>,
        reason: StructureError,
        source_key: Option<String>,
    },
}
impl From<StructureStart> for StructureStartEntry {
    fn from(start: StructureStart) -> Self {
        Self::Native(Box::new(start))
    }
}
impl StructureStartEntry {
    pub fn deferred(tag: Tag) -> Self {
        Self::Deferred {
            tag: Box::new(tag),
            reason: StructureError::RegistryUnavailable,
            source_key: None,
        }
    }
    pub fn native(&self) -> Result<&StructureStart, StructureError> {
        match self {
            Self::Native(start) => Ok(start),
            Self::Deferred { reason, .. } => Err(reason.clone()),
        }
    }
    pub fn native_mut(&mut self) -> Result<&mut StructureStart, StructureError> {
        match self {
            Self::Native(start) => Ok(start),
            Self::Deferred { reason, .. } => Err(reason.clone()),
        }
    }
    pub fn create_tag(&self, chunk: ChunkPos) -> Tag {
        match self {
            Self::Native(start) => Tag::Compound(start.create_tag(chunk)),
            Self::Deferred { tag, .. } => *tag.clone(),
        }
    }
}

/// `net.minecraft.world.level.chunk.StructureAccess`.
#[derive(Clone)]
pub struct StructureAccess<S> {
    /// `structureStarts` — structure identity -> canonical owned start entry.
    structure_starts: HashMap<S, StructureStartEntry>,
    /// Syntactically malformed/aliased source keys cannot be normalized without
    /// dropping previously preserved data. They remain deferred in this same
    /// owner, never overlaid on a native entry with an identical source key.
    unkeyed_starts: CompoundTag,
    /// Source key slots only, never a second payload owner.
    source_start_order: Vec<String>,
    /// `structuresRefences` — structure id -> chunk references. Java's
    /// Values retain the actual fastutil probe-table iteration order. The outer map
    /// is an `IndexMap` — the runtime authority for structure references
    /// (#537), so the decoded `structures.References` source order is carried
    /// and a packet/derivation pass iterates it deterministically.
    structure_references: IndexMap<S, LongOpenHashSet>,
}

impl<S: Eq + std::hash::Hash> StructureAccess<S> {
    /// `ChunkAccess`'s field initializers (`Maps.newHashMap()`); the reference
    /// map is insertion-ordered (#537).
    pub fn new() -> Self {
        StructureAccess {
            structure_starts: HashMap::new(),
            unkeyed_starts: CompoundTag::new(),
            source_start_order: Vec::new(),
            structure_references: IndexMap::new(),
        }
    }

    /// `getStartForStructure(Structure)` — `structureStarts.get(structure)`;
    /// `Option::None` when absent (Java's `null`).
    pub fn get_start_for_structure(&self, structure: &S) -> Option<&StructureStartEntry> {
        self.structure_starts.get(structure)
    }

    pub fn get_start_for_structure_mut(
        &mut self,
        structure: &S,
    ) -> Option<&mut StructureStartEntry> {
        self.structure_starts.get_mut(structure)
    }

    /// `setStartForStructure(Structure, StructureStart)` (the `markUnsaved`
    /// side effect is omitted).
    pub fn set_start_for_structure(&mut self, structure: S, start: impl Into<StructureStartEntry>) {
        self.structure_starts.insert(structure, start.into());
    }

    /// `getReferencesForStructure(Structure)` — iterate the reference set for a
    /// structure, or yield nothing when absent (Java's `EMPTY_REFERENCE_SET`).
    /// The iterator follows fastutil zero-first, descending-slot order.
    pub fn get_references_for_structure<'a>(
        &'a self,
        structure: &S,
    ) -> impl Iterator<Item = &'a u64> + 'a {
        self.structure_references
            .get(structure)
            .into_iter()
            .flat_map(|set| set.iter())
    }

    /// `addReferenceForStructure(Structure, long)` — `computeIfAbsent(...,
    /// LongOpenHashSet).add(reference)` (the `markUnsaved` side effect is
    /// omitted). The set add is idempotent, so an already-present reference is
    /// a no-op, including no capacity change.
    pub fn add_reference_for_structure(&mut self, structure: S, reference: u64) {
        self.structure_references
            .entry(structure)
            .or_default()
            .insert(reference);
    }

    /// `ChunkAccess.getAllStarts()` — `Collections.unmodifiableMap(this.structureStarts)`.
    /// The `markUnsaved` side effect of `setAllStarts` lives on `ChunkAccess`.
    pub fn get_all_starts(&self) -> &HashMap<S, StructureStartEntry> {
        &self.structure_starts
    }

    /// Persisted aliases/malformed keys remain owned but unresolved outside
    /// the keyed map. Executable traversals must not mistake them for absence.
    pub fn has_unkeyed_starts(&self) -> bool {
        !self.unkeyed_starts.is_empty()
    }

    /// `ChunkAccess.setAllStarts(Map)` — `structureStarts.clear()` + `putAll`
    /// (the `markUnsaved` side effect lives on `ChunkAccess`).
    pub fn set_all_starts(&mut self, starts: HashMap<S, StructureStartEntry>) {
        self.structure_starts.clear();
        self.structure_starts.extend(starts);
        self.unkeyed_starts = CompoundTag::new();
        self.source_start_order.clear();
    }

    /// Carry persisted entries without claiming registry resolution. The key
    /// callback is syntax-only; executable installation is a separate boundary.
    pub fn defer_serialized_starts(&mut self, raw: &CompoundTag, key: impl Fn(&str) -> Option<S>) {
        self.structure_starts.clear();
        self.unkeyed_starts = CompoundTag::new();
        self.source_start_order = raw.key_set().cloned().collect();
        for (name, tag) in raw.entry_set() {
            if let Some(id) = key(name) {
                let entry = self.structure_starts.entry(id);
                match entry {
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(StructureStartEntry::Deferred {
                            tag: Box::new(tag.clone()),
                            reason: StructureError::RegistryUnavailable,
                            source_key: Some(name.clone()),
                        });
                    }
                    std::collections::hash_map::Entry::Occupied(_) => {
                        self.unkeyed_starts.put(name.clone(), tag.clone());
                    }
                }
            } else {
                self.unkeyed_starts.put(name.clone(), tag.clone());
            }
        }
    }

    pub fn serialized_starts(&self, chunk: ChunkPos, name: impl Fn(&S) -> String) -> CompoundTag {
        let mut values: HashMap<String, Tag> = self
            .unkeyed_starts
            .entry_set()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        for (key, entry) in &self.structure_starts {
            let key = match entry {
                StructureStartEntry::Deferred {
                    source_key: Some(source),
                    ..
                } => source.clone(),
                _ => name(key),
            };
            values.insert(key, entry.create_tag(chunk));
        }
        let mut tag = CompoundTag::new();
        for key in &self.source_start_order {
            if let Some(value) = values.remove(key) {
                tag.put(key.clone(), value);
            }
        }
        // New native entries have no persisted source slot. Stable key order
        // is an explicit save policy, not a claim of Java registry/HashMap order.
        let mut appended: Vec<_> = values.into_iter().collect();
        appended.sort_by(|(left, _), (right, _)| left.cmp(right));
        for (key, value) in appended {
            tag.put(key, value);
        }
        tag
    }

    /// `getAllReferences()` — `Collections.unmodifiableMap(...)` (read-only
    /// view; the port returns a reference). The map is insertion-ordered
    /// (#537): the decoded `structures.References` source order is carried, so
    /// a derivation pass iterates it deterministically.
    pub fn get_all_references(&self) -> &IndexMap<S, LongOpenHashSet> {
        &self.structure_references
    }

    /// `setAllReferences(Map)` — clear + putAll (the `markUnsaved` side
    /// effect is omitted). Java's values are already `LongOpenHashSet`s, so a
    /// caller handing over raw `Vec`s here must get the same set semantics:
    /// array-constructor capacity is preserved before duplicate removal. The
    /// caller's iteration order is preserved by the insertion-ordered outer
    /// map (#537).
    ///
    /// Unlike Java's `Map` input (unique keys), this takes an `IntoIterator`,
    /// so a key repeated across iterator items is not a replace: its
    /// references merge into the same set, keeping the first-insertion key slot.
    pub fn set_all_references<I: IntoIterator<Item = (S, Vec<u64>)>>(&mut self, data: I) {
        self.structure_references.clear();
        for (structure, references) in data {
            match self.structure_references.entry(structure) {
                indexmap::map::Entry::Vacant(entry) => {
                    entry.insert(LongOpenHashSet::from_slice(&references));
                }
                indexmap::map::Entry::Occupied(mut entry) => entry.get_mut().extend(references),
            }
        }
    }
}

impl<S: Eq + std::hash::Hash> Default for StructureAccess<S> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::StructureAccess;

    #[test]
    fn deferred_source_key_order_survives_native_replacement_without_shadow_payload() {
        use crate::levelgen::structure::{StructureDefinition, StructureStart, TerrainAdjustment};
        use rivet_nbt::compound_tag::CompoundTag;
        use rivet_registry::{Identifier, core::ChunkPos};
        let mut raw = CompoundTag::new();
        for key in [
            "minecraft:village",
            "INVALID KEY!",
            "minecraft:buried_treasure",
            "buried_treasure",
            "minecraft:monument",
        ] {
            raw.put_string(key, key);
        }
        let mut access = StructureAccess::<Identifier>::new();
        access.defer_serialized_starts(&raw, |key| {
            Identifier::try_parse(key).filter(|id| id.to_string() == key)
        });
        let pos = ChunkPos::new(2, -1);
        let native = StructureStart::new(
            StructureDefinition::BuriedTreasure {
                registry_id: "minecraft:buried_treasure".into(),
                terrain_adjustment: TerrainAdjustment::None,
            },
            pos,
            0,
            StructureDefinition::buried_treasure_pieces(pos),
        );
        access.set_start_for_structure(Identifier::parse("minecraft:buried_treasure"), native);
        access
            .get_start_for_structure_mut(&Identifier::parse("minecraft:buried_treasure"))
            .unwrap()
            .native_mut()
            .unwrap()
            .add_reference();
        let saved = access.serialized_starts(pos, ToString::to_string);
        assert_eq!(
            saved.key_set().collect::<Vec<_>>(),
            raw.key_set().collect::<Vec<_>>(),
            "source slots include canonical, malformed and noncanonical entries"
        );
        assert_eq!(
            saved
                .get_compound("minecraft:buried_treasure")
                .unwrap()
                .get_int_or("references", -1),
            1
        );
        assert_eq!(saved.get("INVALID KEY!"), raw.get("INVALID KEY!"));
        assert_eq!(saved.get("buried_treasure"), raw.get("buried_treasure"));
        let second = access.serialized_starts(pos, ToString::to_string);
        assert_eq!(
            saved.entry_set().collect::<Vec<_>>(),
            second.entry_set().collect::<Vec<_>>()
        );
    }

    #[test]
    fn starts_default_to_none_and_references_to_empty() {
        let access = StructureAccess::<&str>::new();
        assert_eq!(access.get_start_for_structure(&"monument"), None);
        assert!(
            access
                .get_references_for_structure(&"monument")
                .next()
                .is_none()
        );
    }

    #[test]
    fn starts_and_references_round_trip() {
        let mut access = StructureAccess::<&str>::new();
        access.set_start_for_structure(
            "monument",
            crate::levelgen::structure::StructureStart::invalid(),
        );
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0xABCD);
        access.add_reference_for_structure("village", 0x1);
        assert!(
            !access
                .get_start_for_structure(&"monument")
                .unwrap()
                .native()
                .unwrap()
                .is_valid()
        );
        assert_eq!(
            access
                .get_references_for_structure(&"monument")
                .copied()
                .collect::<Vec<_>>(),
            vec![0xABCD, 0x1234]
        );
        assert_eq!(
            access
                .get_references_for_structure(&"village")
                .copied()
                .collect::<Vec<_>>(),
            vec![0x1]
        );
        assert_eq!(access.get_all_references().len(), 2);
    }

    #[test]
    fn add_reference_is_idempotent_like_the_java_set() {
        let mut access = StructureAccess::<&str>::new();
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0xABCD);
        assert_eq!(
            access
                .get_references_for_structure(&"monument")
                .copied()
                .collect::<Vec<_>>(),
            vec![0xABCD, 0x1234]
        );
    }

    #[test]
    fn set_all_references_replaces_previous() {
        let mut access = StructureAccess::<&str>::new();
        access.add_reference_for_structure("old", 1);
        let data = std::collections::HashMap::from([("new", vec![9u64, 8])]);
        access.set_all_references(data);
        assert!(access.get_references_for_structure(&"old").next().is_none());
        assert_eq!(
            access
                .get_references_for_structure(&"new")
                .copied()
                .collect::<Vec<_>>(),
            vec![8, 9]
        );
    }

    #[test]
    fn set_all_references_deduplicates_like_the_java_set() {
        // Java's `setAllReferences` values are `LongOpenHashSet`s, so duplicate
        // references cannot survive a `putAll`. The `Vec` model must dedupe on
        // insert the same way (counterfactual: a raw `extend` would keep the
        // duplicate `1`, `2`, `3`).
        let mut access = StructureAccess::<&str>::new();
        let data = std::collections::HashMap::from([("monument", vec![1u64, 2, 1, 3, 2, 3])]);
        access.set_all_references(data);
        assert_eq!(
            access
                .get_references_for_structure(&"monument")
                .copied()
                .collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
    }
}
