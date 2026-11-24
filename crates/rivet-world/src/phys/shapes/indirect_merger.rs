//! `net.minecraft.world.phys.shapes.IndirectMerger`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/IndirectMerger.java`
//! (Paper: fast path for the infinite-head/tail first list).

use std::sync::{Arc, LazyLock};

use rivet_util::double_list::{DoubleArrayList, DoubleList};

use super::index_merger::{IndexConsumer, IndexMerger};

/// `IndirectMerger.EMPTY` — `DoubleLists.unmodifiable(wrap({0.0}))`.
static EMPTY: LazyLock<Arc<dyn DoubleList>> =
    LazyLock::new(|| Arc::new(DoubleArrayList::from_slice(&[0.0])));

// Paper
const INFINITE_B_1: [i32; 2] = [1, 1];
const INFINITE_B_0: [i32; 2] = [0, 0];
const INFINITE_C: [i32; 2] = [0, 1];

/// `IndirectMerger implements IndexMerger`.
#[derive(Clone, Debug)]
pub struct IndirectMerger {
    result: Vec<f64>,
    first_indices: Vec<i32>,
    second_indices: Vec<i32>,
    result_length: usize,
}

impl IndirectMerger {
    /// `IndirectMerger(DoubleList, DoubleList, boolean firstOnlyMatters,
    /// boolean secondOnlyMatters)`.
    pub fn new(
        first: &dyn DoubleList,
        second: &dyn DoubleList,
        first_only_matters: bool,
        second_only_matters: bool,
    ) -> IndirectMerger {
        let mut last_value = f64::NAN;
        let first_size = first.size();
        let second_size = second.size();
        let capacity = first_size + second_size;
        // Paper start - optimize common path of infinity doublelist
        let tail = first.get_double(first_size - 1);
        let head = first.get_double(0);
        if head == f64::NEG_INFINITY
            && tail == f64::INFINITY
            && !first_only_matters
            && !second_only_matters
            && (first_size == 2 || first_size == 4)
        {
            let first_indices = if first_size == 2 {
                INFINITE_B_0.to_vec()
            } else {
                INFINITE_B_1.to_vec()
            };
            return IndirectMerger {
                result: second.to_double_array(),
                result_length: second.size(),
                first_indices,
                second_indices: INFINITE_C.to_vec(),
            };
        }
        // Paper end
        let mut result = vec![0.0; capacity];
        let mut first_indices = vec![0; capacity];
        let mut second_indices = vec![0; capacity];
        let can_skip_first = !first_only_matters;
        let can_skip_second = !second_only_matters;
        let mut result_index: usize = 0;
        let mut first_index: usize = 0;
        let mut second_index: usize = 0;

        loop {
            let ran_out_of_first = first_index >= first_size;
            let ran_out_of_second = second_index >= second_size;
            if ran_out_of_first && ran_out_of_second {
                return IndirectMerger {
                    result,
                    first_indices,
                    second_indices,
                    result_length: 1.max(result_index),
                };
            }

            let chose_first = !ran_out_of_first
                && (ran_out_of_second
                    || first.get_double(first_index) < second.get_double(second_index) + 1.0E-7);
            if chose_first {
                first_index += 1;
                if can_skip_first && (second_index == 0 || ran_out_of_second) {
                    continue;
                }
            } else {
                second_index += 1;
                if can_skip_second && (first_index == 0 || ran_out_of_first) {
                    continue;
                }
            }

            let current_first_index = first_index as i32 - 1;
            let current_second_index = second_index as i32 - 1;
            let next_value = if chose_first {
                first.get_double(current_first_index as usize)
            } else {
                second.get_double(current_second_index as usize)
            };
            if !(last_value >= next_value - 1.0E-7) {
                first_indices[result_index] = current_first_index;
                second_indices[result_index] = current_second_index;
                result[result_index] = next_value;
                result_index += 1;
                last_value = next_value;
            } else {
                first_indices[result_index - 1] = current_first_index;
                second_indices[result_index - 1] = current_second_index;
            }
        }
    }
}

impl IndexMerger for IndirectMerger {
    fn for_merged_indexes(&self, consumer: &mut IndexConsumer<'_>) -> bool {
        let length = self.result_length - 1;
        for i in 0..length {
            if !consumer(self.first_indices[i], self.second_indices[i], i as i32) {
                return false;
            }
        }

        true
    }

    fn size(&self) -> usize {
        self.result_length
    }

    fn get_list(&self) -> Arc<dyn DoubleList> {
        if self.result_length <= 1 {
            Arc::clone(&EMPTY)
        } else {
            Arc::new(DoubleArrayList::wrap_with_length(
                Arc::from(self.result.as_slice()),
                self.result_length,
            ))
        }
    }
}
