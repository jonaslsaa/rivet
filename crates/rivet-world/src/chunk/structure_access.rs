//! Port of `net.minecraft.world.level.chunk.StructureAccess` (MC 26.2) — the
//! per-chunk structure-starts/references surface.
//!
//! Java: `StructureAccess.java` in `working/Paper`, implemented by `ChunkAccess`
//! (structureStarts + structuresRefences maps with a shared empty
//! `LongOpenHashSet` default). `Structure`/`StructureStart`/`LongSet` are not
//! ported, so the port keys the two maps by a caller-supplied `S` structure id
//! (the value type of the maps) and models the reference set as a `Vec<u64>`.
//!
//! Java's `getReferencesForStructure` returns the shared immutable
//! `EMPTY_REFERENCE_SET` for an unknown structure; the port returns an empty
//! slice (`&[]`). The `markUnsaved` side effects of the setters are chunk
//! dirty-tracking, omitted with the owning access unit. `getAllStarts`/
//! `setAllStarts` live on `ChunkAccess`, not this interface, so they are not
//! part of the port.
//!
//! RivetTodo(#185): the `Structure`/`StructureStart`/`LongSet` types and the
//! `ChunkAccess` implementation live with the structure and access units;
//! this module ports the interface shape keyed by the caller's structure id.

use std::collections::HashMap;

/// `net.minecraft.world.level.chunk.StructureAccess`.
pub struct StructureAccess<S> {
    /// `structureStarts` — structure id -> start. `StructureStart` is absent,
    /// so the start value is the caller's `V`.
    structure_starts: HashMap<S, i64>,
    /// `structuresRefences` — structure id -> chunk references. Java's
    /// `LongOpenHashSet` is not ported; modeled as a `Vec<u64>` that keeps the
    /// set's semantics by deduplicating on insert (first-insertion order; Java
    /// defines no iteration order for its hash set).
    structure_references: HashMap<S, Vec<u64>>,
}

impl<S: Eq + std::hash::Hash> StructureAccess<S> {
    /// `ChunkAccess`'s field initializers (`Maps.newHashMap()`).
    pub fn new() -> Self {
        StructureAccess {
            structure_starts: HashMap::new(),
            structure_references: HashMap::new(),
        }
    }

    /// `getStartForStructure(Structure)` — `structureStarts.get(structure)`;
    /// `Option::None` when absent (Java's `null`).
    pub fn get_start_for_structure(&self, structure: &S) -> Option<i64> {
        self.structure_starts.get(structure).copied()
    }

    /// `setStartForStructure(Structure, StructureStart)` (the `markUnsaved`
    /// side effect is omitted).
    pub fn set_start_for_structure(&mut self, structure: S, start: i64) {
        self.structure_starts.insert(structure, start);
    }

    /// `getReferencesForStructure(Structure)` — the reference set for a
    /// structure, or the shared empty set when absent (Java's
    /// `EMPTY_REFERENCE_SET`; the port returns `&[]`).
    pub fn get_references_for_structure(&self, structure: &S) -> &[u64] {
        self.structure_references
            .get(structure)
            .map_or(&[], |set| set.as_slice())
    }

    /// `addReferenceForStructure(Structure, long)` — `computeIfAbsent(...,
    /// LongOpenHashSet).add(reference)` (the `markUnsaved` side effect is
    /// omitted). The set add is idempotent, so an already-present reference is
    /// a no-op; the `Vec` deduplicates on insert to preserve that.
    pub fn add_reference_for_structure(&mut self, structure: S, reference: u64) {
        let set = self.structure_references.entry(structure).or_default();
        if !set.contains(&reference) {
            set.push(reference);
        }
    }

    /// `getAllReferences()` — `Collections.unmodifiableMap(...)` (read-only
    /// view; the port returns a reference).
    pub fn get_all_references(&self) -> &HashMap<S, Vec<u64>> {
        &self.structure_references
    }

    /// `setAllReferences(Map)` — clear + putAll (the `markUnsaved` side
    /// effect is omitted). Java's values are already `LongOpenHashSet`s, so a
    /// caller handing over raw `Vec`s here must get the same set semantics:
    /// duplicates dedupe on insert (first-insertion order preserved).
    pub fn set_all_references(&mut self, data: HashMap<S, Vec<u64>>) {
        self.structure_references.clear();
        for (structure, references) in data {
            let set = self.structure_references.entry(structure).or_default();
            for reference in references {
                if !set.contains(&reference) {
                    set.push(reference);
                }
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
    fn starts_default_to_none_and_references_to_empty() {
        let access = StructureAccess::<&str>::new();
        assert_eq!(access.get_start_for_structure(&"monument"), None);
        assert!(access.get_references_for_structure(&"monument").is_empty());
    }

    #[test]
    fn starts_and_references_round_trip() {
        let mut access = StructureAccess::<&str>::new();
        access.set_start_for_structure("monument", 7);
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0xABCD);
        access.add_reference_for_structure("village", 0x1);
        assert_eq!(access.get_start_for_structure(&"monument"), Some(7));
        assert_eq!(
            access.get_references_for_structure(&"monument"),
            &[0x1234, 0xABCD]
        );
        assert_eq!(access.get_references_for_structure(&"village"), &[0x1]);
        assert_eq!(access.get_all_references().len(), 2);
    }

    #[test]
    fn add_reference_is_idempotent_like_the_java_set() {
        let mut access = StructureAccess::<&str>::new();
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0xABCD);
        assert_eq!(
            access.get_references_for_structure(&"monument"),
            &[0x1234, 0xABCD]
        );
    }

    #[test]
    fn set_all_references_replaces_previous() {
        let mut access = StructureAccess::<&str>::new();
        access.add_reference_for_structure("old", 1);
        let data = std::collections::HashMap::from([("new", vec![9u64, 8])]);
        access.set_all_references(data);
        assert!(access.get_references_for_structure(&"old").is_empty());
        assert_eq!(access.get_references_for_structure(&"new"), &[9, 8]);
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
        assert_eq!(access.get_references_for_structure(&"monument"), &[1, 2, 3]);
    }
}
