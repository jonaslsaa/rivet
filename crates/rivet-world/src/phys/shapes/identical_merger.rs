//! `net.minecraft.world.phys.shapes.IdenticalMerger`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/IdenticalMerger.java`.

use std::sync::Arc;

use rivet_util::double_list::DoubleList;

use super::index_merger::{IndexConsumer, IndexMerger};

/// `IdenticalMerger implements IndexMerger`.
#[derive(Clone, Debug)]
pub struct IdenticalMerger {
    coords: Arc<dyn DoubleList>,
}

impl IdenticalMerger {
    /// `IdenticalMerger(DoubleList)`.
    pub fn new(coords: Arc<dyn DoubleList>) -> IdenticalMerger {
        IdenticalMerger { coords }
    }
}

impl IndexMerger for IdenticalMerger {
    fn for_merged_indexes(&self, consumer: &mut IndexConsumer<'_>) -> bool {
        let size = self.coords.size() as i32 - 1;
        for i in 0..size {
            if !consumer(i, i, i) {
                return false;
            }
        }

        true
    }

    fn size(&self) -> usize {
        self.coords.size()
    }

    fn get_list(&self) -> Arc<dyn DoubleList> {
        Arc::clone(&self.coords)
    }
}
