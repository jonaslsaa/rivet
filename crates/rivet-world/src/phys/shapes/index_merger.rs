//! `net.minecraft.world.phys.shapes.IndexMerger` — merges two sorted
//! coordinate lists and walks the merged index triples.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/IndexMerger.java`.

use std::sync::Arc;

use rivet_util::double_list::DoubleList;

/// `IndexMerger.IndexConsumer.merge(int firstIndex, int secondIndex, int
/// resultIndex)` — returns whether to continue.
pub type IndexConsumer<'a> = dyn FnMut(i32, i32, i32) -> bool + 'a;

/// `IndexMerger`.
pub trait IndexMerger {
    /// `IndexMerger.getList()`.
    fn get_list(&self) -> Arc<dyn DoubleList>;

    /// `IndexMerger.forMergedIndexes(IndexConsumer)` — `false` when the
    /// consumer stopped the walk.
    fn for_merged_indexes(&self, consumer: &mut IndexConsumer<'_>) -> bool;

    /// `IndexMerger.size()`.
    fn size(&self) -> usize;
}
