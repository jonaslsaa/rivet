//! Key-table metadata for ChunkAccess.blockEntities' fastutil8.5.18
//! Object2ObjectOpenHashMap<BlockPos,...>. No payloads are copied here. Its
//! table/resize/shift history is necessary because registration consumes world
//! RNG in descending-table iteration order. BlockPos keys cannot be null.
use rivet_registry::core::BlockPos;

#[derive(Debug, Clone)]
pub(super) struct BlockEntityOrder {
    slots: Vec<Option<BlockPos>>,
    size: usize,
}

fn mix(pos: BlockPos) -> usize {
    let hash = (pos.hash_code() as u32).wrapping_mul(0x9e37_79b9);
    (hash ^ (hash >> 16)) as usize
}
fn max_fill(n: usize) -> usize {
    (n * 3 / 4).min(n - 1)
}
fn array_size(expected: usize) -> usize {
    ((expected as f64 / 0.75).ceil() as usize)
        .max(2)
        .next_power_of_two()
}
impl Default for BlockEntityOrder {
    fn default() -> Self {
        Self {
            slots: vec![None; 32],
            size: 0,
        }
    }
}
impl BlockEntityOrder {
    pub(super) fn iter(&self) -> impl Iterator<Item = BlockPos> + '_ {
        self.slots.iter().rev().filter_map(|slot| *slot)
    }
    pub(super) fn insert(&mut self, pos: BlockPos) {
        let mask = self.slots.len() - 1;
        let mut index = mix(pos) & mask;
        while let Some(existing) = self.slots[index] {
            if existing == pos {
                return;
            }
            index = (index + 1) & mask;
        }
        self.slots[index] = Some(pos);
        let old_size = self.size;
        self.size += 1;
        if old_size >= max_fill(self.slots.len()) {
            self.rehash(array_size(self.size + 1));
        }
    }
    pub(super) fn remove(&mut self, pos: BlockPos) {
        let mask = self.slots.len() - 1;
        let mut index = mix(pos) & mask;
        loop {
            match self.slots[index] {
                None => return,
                Some(key) if key == pos => break,
                Some(_) => index = (index + 1) & mask,
            }
        }
        self.size -= 1;
        loop {
            let last = index;
            index = (index + 1) & mask;
            loop {
                let Some(key) = self.slots[index] else {
                    self.slots[last] = None;
                    if self.slots.len() > 32 && self.size < max_fill(self.slots.len()) / 4 {
                        self.rehash(self.slots.len() / 2);
                    }
                    return;
                };
                let slot = mix(key) & mask;
                let must_move = if last <= index {
                    last >= slot || slot > index
                } else {
                    last >= slot && slot > index
                };
                if must_move {
                    break;
                }
                index = (index + 1) & mask;
            }
            self.slots[last] = self.slots[index];
        }
    }
    pub(super) fn clear(&mut self) {
        self.slots.fill(None);
        self.size = 0;
    }
    fn rehash(&mut self, n: usize) {
        let mut next = vec![None; n];
        for key in self.iter() {
            let mut index = mix(key) & (n - 1);
            while next[index].is_some() {
                index = (index + 1) & (n - 1);
            }
            next[index] = Some(key);
        }
        self.slots = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_fastutil_live_map_resize_shift_shrink_clear_and_promotion_order() {
        let mut order = BlockEntityOrder::default();
        let mut rows = 0;
        for line in
            include_str!("../../../../tools/rivet-oracle/fixtures/block-entity-map-order.jsonl")
                .lines()
        {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            for operation in row["operations"].as_array().unwrap() {
                let operation = operation.as_array().unwrap();
                let pos = || {
                    BlockPos::new(
                        operation[1].as_i64().unwrap() as i32,
                        operation[2].as_i64().unwrap() as i32,
                        operation[3].as_i64().unwrap() as i32,
                    )
                };
                match operation[0].as_str().unwrap() {
                    "put" => order.insert(pos()),
                    "remove" => order.remove(pos()),
                    "clear" => order.clear(),
                    "clone" => order = order.clone(),
                    "promote" => {
                        let mut fresh = BlockEntityOrder::default();
                        for pos in order.iter() {
                            fresh.insert(pos);
                        }
                        order = fresh;
                    }
                    op => panic!("unknown oracle operation {op}"),
                }
            }
            let actual: Vec<_> = order
                .iter()
                .map(|pos| [pos.get_x(), pos.get_y(), pos.get_z()])
                .collect();
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                row["order"],
                "{}",
                row["name"]
            );
            assert_eq!(
                order.slots.len() as u64,
                row["capacity"].as_u64().unwrap(),
                "{}",
                row["name"]
            );
            rows += 1;
        }
        assert_eq!(rows, 9);
    }
}
