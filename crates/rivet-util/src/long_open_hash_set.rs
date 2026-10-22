//! The append-only surface of fastutil 8.5.18 `LongOpenHashSet` used by chunk
//! structure references: default/array construction, add, membership and
//! iterator order. Values are Java long bit patterns, not unsigned arithmetic
//! coordinates. Removal/iterator mutation and non-default load factors are not
//! exposed. Clone preserves the actual probe table, including its capacity.

#[derive(Clone, Debug)]
pub struct LongOpenHashSet {
    slots: Vec<u64>,
    contains_zero: bool,
    size: usize,
}

impl Default for LongOpenHashSet {
    fn default() -> Self {
        Self::with_expected(16)
    }
}

fn array_size(expected: usize) -> usize {
    let needed = ((expected as f64 / 0.75).ceil() as usize).max(2);
    let size = needed
        .checked_next_power_of_two()
        .expect("reference set capacity overflow");
    assert!(
        size <= 1 << 30,
        "fastutil reference set capacity exceeds 2^30"
    );
    size
}

fn mix(value: u64) -> usize {
    let hash = value.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let hash = hash ^ (hash >> 32);
    (hash ^ (hash >> 16)) as usize
}

impl LongOpenHashSet {
    fn with_expected(expected: usize) -> Self {
        Self {
            slots: vec![0; array_size(expected)],
            contains_zero: false,
            size: 0,
        }
    }

    /// Java's long[] constructor sizes from the input length BEFORE deduplication.
    pub fn from_slice(values: &[u64]) -> Self {
        let mut set = Self::with_expected(values.len());
        for value in values {
            set.insert(*value);
        }
        set
    }

    pub fn len(&self) -> usize {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    pub fn contains(&self, value: &u64) -> bool {
        if *value == 0 {
            return self.contains_zero;
        }
        let mask = self.slots.len() - 1;
        let mut slot = mix(*value) & mask;
        while self.slots[slot] != 0 {
            if self.slots[slot] == *value {
                return true;
            }
            slot = (slot + 1) & mask;
        }
        false
    }

    pub fn insert(&mut self, value: u64) -> bool {
        if value == 0 {
            if self.contains_zero {
                return false;
            }
            self.contains_zero = true;
        } else {
            let mask = self.slots.len() - 1;
            let mut slot = mix(value) & mask;
            while self.slots[slot] != 0 {
                if self.slots[slot] == value {
                    return false;
                }
                slot = (slot + 1) & mask;
            }
            self.slots[slot] = value;
        }
        // fastutil compares the pre-increment size against maxFill.
        let old_size = self.size;
        self.size += 1;
        let max_fill = ((self.slots.len() * 3).div_ceil(4)).min(self.slots.len() - 1);
        if old_size >= max_fill {
            self.rehash(array_size(self.size + 1));
        }
        true
    }

    fn rehash(&mut self, capacity: usize) {
        let mut slots = vec![0; capacity];
        // Rehash scans the old table DOWNWARD, which matters for collisions.
        for &value in self.slots.iter().rev().filter(|&&value| value != 0) {
            let mut slot = mix(value) & (capacity - 1);
            while slots[slot] != 0 {
                slot = (slot + 1) & (capacity - 1);
            }
            slots[slot] = value;
        }
        self.slots = slots;
    }

    /// Java's iterator visits zero first, then occupied slots in descending order.
    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.contains_zero
            .then_some(&0)
            .into_iter()
            .chain(self.slots.iter().rev().filter(|&&value| value != 0))
    }
}

impl Extend<u64> for LongOpenHashSet {
    fn extend<T: IntoIterator<Item = u64>>(&mut self, values: T) {
        for value in values {
            self.insert(value);
        }
    }
}

impl PartialEq for LongOpenHashSet {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|value| other.contains(value))
    }
}
impl Eq for LongOpenHashSet {}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn values(text: &str) -> Vec<u64> {
        if text == "-" {
            return Vec::new();
        }
        text.split(',')
            .map(|value| u64::from_str_radix(value, 16).unwrap())
            .collect()
    }
    fn digest(set: &LongOpenHashSet) -> String {
        let mut hash = Sha256::new();
        for value in set.iter() {
            hash.update(value.to_be_bytes());
        }
        format!("{:x}", hash.finalize())
    }

    #[test]
    fn constructors_growth_duplicates_and_cloned_tables_match_bundled_fastutil() {
        for line in
            include_str!("../../../tools/rivet-oracle/fixtures/long-reference-set.tsv").lines()
        {
            let fields: Vec<_> = line.split('\t').collect();
            let input = values(fields[2]);
            let mut set = if fields[1] == "array" {
                LongOpenHashSet::from_slice(&input)
            } else {
                let mut set = LongOpenHashSet::default();
                set.extend(input.iter().copied());
                set
            };
            assert_eq!(digest(&set), fields[3], "{} {}", fields[0], fields[1]);
            for value in input {
                assert!(set.contains(&value));
                assert!(!set.insert(value));
            }
            let mut cloned = set.clone();
            cloned.extend(values(fields[4]));
            assert_eq!(
                digest(&cloned),
                fields[5],
                "cloned {} {}",
                fields[0],
                fields[1]
            );
            assert_eq!(digest(&set), fields[3], "clone must not mutate original");
            assert_eq!(set.len(), set.iter().count());
        }
    }
}
