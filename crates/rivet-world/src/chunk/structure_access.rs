//! Port of `net.minecraft.world.level.chunk.StructureAccess` (MC 26.2) — the
//! per-chunk structure-starts/references surface.
//!
//! Java: `StructureAccess.java` in `working/Paper`, implemented by `ChunkAccess`
//! (structureStarts + structuresRefences maps with a shared empty
//! `LongOpenHashSet` default). `Structure`/`StructureStart`/`LongSet` are not
//! ported, so the port keys the two maps by a caller-supplied `S` structure id
//! (the value type of the maps) and models the reference set as an
//! `IndexSet<u64>` — the Rust stand-in for `LongOpenHashSet` (O(1) insert,
//! dedup, first-insertion order).
//!
//! Java's `getReferencesForStructure` returns the shared immutable
//! `EMPTY_REFERENCE_SET` for an unknown structure; the port yields no
//! elements. The `markUnsaved` side effects of the setters are chunk
//! dirty-tracking, omitted with the owning access unit. `getAllStarts`/
//! `setAllStarts` live on `ChunkAccess`, not this interface, so they are not
//! part of the port.
//!
//! RivetTodo(#185): `LongOpenHashSet` iterates in its hash-probe slot order,
//! which is deterministic per instance but *not* insertion order, and
//! `SerializableChunkData` writes `getAllReferences().toLongArray()` into the
//! NBT `References` tag in that slot order. The port's `IndexSet` yields
//! first-insertion order, so a byte-for-byte parity check on serialized
//! references diverges once the owning unit serializes them; #185 must model
//! fastutil's probe order there.
//!
//! RivetTodo(#185): the `Structure`/`StructureStart`/`LongSet` types and the
//! `ChunkAccess` implementation live with the structure and access units;
//! this module ports the interface shape keyed by the caller's structure id.

use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;

/// `net.minecraft.world.level.chunk.StructureAccess`.
pub struct StructureAccess<S> {
    /// `structureStarts` — structure id -> start. `StructureStart` is absent,
    /// so the start value is modeled as an `i64`.
    structure_starts: HashMap<S, i64>,
    /// `structuresRefences` — structure id -> chunk references. Java's
    /// `LongOpenHashSet` is not ported; modeled as an `IndexSet<u64>` that
    /// keeps the set's semantics — amortized O(1) insert/contains, dedup on
    /// insert — but with first-insertion order where Java uses fastutil's
    /// deterministic hash-probe slot order (see the module `RivetTodo(#185)`
    /// note on the parity divergence once references serialize). The outer map
    /// is an `IndexMap` — the runtime authority for structure references
    /// (#537), so the decoded `structures.References` source order is carried
    /// and a packet/derivation pass iterates it deterministically.
    structure_references: IndexMap<S, IndexSet<u64>>,
}

impl<S: Eq + std::hash::Hash> StructureAccess<S> {
    /// `ChunkAccess`'s field initializers (`Maps.newHashMap()`); the reference
    /// map is insertion-ordered (#537).
    pub fn new() -> Self {
        StructureAccess {
            structure_starts: HashMap::new(),
            structure_references: IndexMap::new(),
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

    /// `getReferencesForStructure(Structure)` — iterate the reference set for a
    /// structure, or yield nothing when absent (Java's `EMPTY_REFERENCE_SET`).
    /// `IndexSet` exposes its entries as a contiguous `Slice` facade
    /// (`as_slice()`) but not as a literal `&[u64]`, so the port returns an
    /// iterator, matching Java's `LongSet` callers which only iterate it.
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
    /// a no-op; the `IndexSet` deduplicates on insert to preserve that.
    pub fn add_reference_for_structure(&mut self, structure: S, reference: u64) {
        self.structure_references
            .entry(structure)
            .or_default()
            .insert(reference);
    }

    /// `ChunkAccess.getAllStarts()` — `Collections.unmodifiableMap(this.structureStarts)`.
    /// The `markUnsaved` side effect of `setAllStarts` lives on `ChunkAccess`.
    pub fn get_all_starts(&self) -> &HashMap<S, i64> {
        &self.structure_starts
    }

    /// `ChunkAccess.setAllStarts(Map)` — `structureStarts.clear()` + `putAll`
    /// (the `markUnsaved` side effect lives on `ChunkAccess`).
    pub fn set_all_starts(&mut self, starts: HashMap<S, i64>) {
        self.structure_starts.clear();
        self.structure_starts.extend(starts);
    }

    /// `getAllReferences()` — `Collections.unmodifiableMap(...)` (read-only
    /// view; the port returns a reference). The map is insertion-ordered
    /// (#537): the decoded `structures.References` source order is carried, so
    /// a derivation pass iterates it deterministically.
    pub fn get_all_references(&self) -> &IndexMap<S, IndexSet<u64>> {
        &self.structure_references
    }

    /// `setAllReferences(Map)` — clear + putAll (the `markUnsaved` side
    /// effect is omitted). Java's values are already `LongOpenHashSet`s, so a
    /// caller handing over raw `Vec`s here must get the same set semantics:
    /// duplicates dedupe on insert (first-insertion order preserved). The
    /// caller's iteration order is preserved by the insertion-ordered outer
    /// map (#537).
    ///
    /// Unlike Java's `Map` input (unique keys), this takes an `IntoIterator`,
    /// so a key repeated across iterator items is not a replace: its
    /// references merge into the same set, keeping the first-insertion key slot.
    pub fn set_all_references<I: IntoIterator<Item = (S, Vec<u64>)>>(&mut self, data: I) {
        self.structure_references.clear();
        for (structure, references) in data {
            let set = self.structure_references.entry(structure).or_default();
            set.extend(references);
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
        access.set_start_for_structure("monument", 7);
        access.add_reference_for_structure("monument", 0x1234);
        access.add_reference_for_structure("monument", 0xABCD);
        access.add_reference_for_structure("village", 0x1);
        assert_eq!(access.get_start_for_structure(&"monument"), Some(7));
        assert_eq!(
            access
                .get_references_for_structure(&"monument")
                .copied()
                .collect::<Vec<_>>(),
            vec![0x1234, 0xABCD]
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
            vec![0x1234, 0xABCD]
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
            vec![9, 8]
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
            vec![1, 2, 3]
        );
    }
}
