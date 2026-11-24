//! `net.minecraft.world.phys.shapes.NonOverlappingMerger` — both a
//! `DoubleList` (the concatenated lists) and an `IndexMerger`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/NonOverlappingMerger.java`.
//!
//! Java's `getList()` returns `this`; the fields are shared `Arc`s, so the
//! returned list is an equal copy of the merger (list identity is never
//! observed).

use std::any::Any;
use std::sync::Arc;

use rivet_util::double_list::DoubleList;

use super::index_merger::{IndexConsumer, IndexMerger};

/// `NonOverlappingMerger extends AbstractDoubleList implements IndexMerger`.
#[derive(Clone, Debug)]
pub struct NonOverlappingMerger {
    lower: Arc<dyn DoubleList>,
    upper: Arc<dyn DoubleList>,
    swap: bool,
}

impl NonOverlappingMerger {
    /// `NonOverlappingMerger(DoubleList lower, DoubleList upper, boolean
    /// swap)`.
    pub fn new(
        lower: Arc<dyn DoubleList>,
        upper: Arc<dyn DoubleList>,
        swap: bool,
    ) -> NonOverlappingMerger {
        NonOverlappingMerger { lower, upper, swap }
    }

    fn for_non_swapped_indexes(&self, consumer: &mut IndexConsumer<'_>) -> bool {
        let lower_size = self.lower.size() as i32;

        for i in 0..lower_size {
            if !consumer(i, -1, i) {
                return false;
            }
        }

        let upper_size = self.upper.size() as i32 - 1;

        for i in 0..upper_size {
            if !consumer(lower_size - 1, i, lower_size + i) {
                return false;
            }
        }

        true
    }
}

impl DoubleList for NonOverlappingMerger {
    /// `NonOverlappingMerger.getDouble(int)`.
    fn get_double(&self, index: usize) -> f64 {
        if index < self.lower.size() {
            self.lower.get_double(index)
        } else {
            self.upper.get_double(index - self.lower.size())
        }
    }

    /// `NonOverlappingMerger.size()`.
    fn size(&self) -> usize {
        self.lower.size() + self.upper.size()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IndexMerger for NonOverlappingMerger {
    fn for_merged_indexes(&self, consumer: &mut IndexConsumer<'_>) -> bool {
        if self.swap {
            self.for_non_swapped_indexes(&mut |first_index, second_index, result_index| {
                consumer(second_index, first_index, result_index)
            })
        } else {
            self.for_non_swapped_indexes(consumer)
        }
    }

    fn size(&self) -> usize {
        DoubleList::size(self)
    }

    fn get_list(&self) -> Arc<dyn DoubleList> {
        Arc::new(self.clone())
    }
}
