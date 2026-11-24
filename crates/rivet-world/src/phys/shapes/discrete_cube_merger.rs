//! `net.minecraft.world.phys.shapes.DiscreteCubeMerger`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/DiscreteCubeMerger.java`.

use std::sync::Arc;

use rivet_util::double_list::DoubleList;

use super::cube_point_range::CubePointRange;
use super::index_merger::{IndexConsumer, IndexMerger};
use super::shapes::{gcd, lcm};

/// `DiscreteCubeMerger implements IndexMerger`.
#[derive(Clone, Debug)]
pub struct DiscreteCubeMerger {
    result: Arc<CubePointRange>,
    first_div: i32,
    second_div: i32,
}

impl DiscreteCubeMerger {
    /// `DiscreteCubeMerger(int firstSize, int secondSize)`.
    pub fn new(first_size: i32, second_size: i32) -> DiscreteCubeMerger {
        let result = Arc::new(CubePointRange::new(lcm(first_size, second_size) as i32));
        let gcd = gcd(first_size, second_size);
        DiscreteCubeMerger {
            result,
            first_div: first_size / gcd,
            second_div: second_size / gcd,
        }
    }
}

impl IndexMerger for DiscreteCubeMerger {
    fn for_merged_indexes(&self, consumer: &mut IndexConsumer<'_>) -> bool {
        let size = self.result.size() as i32 - 1;
        for i in 0..size {
            if !consumer(i / self.second_div, i / self.first_div, i) {
                return false;
            }
        }

        true
    }

    fn size(&self) -> usize {
        self.result.size()
    }

    fn get_list(&self) -> Arc<dyn DoubleList> {
        self.result.clone()
    }
}
