//! Port of `ca.spottedleaf.dataconverter.util.Long2IntArraySortedMap`.

/// Sorted array-backed `long -> int` map with `getFloor` lookup.
#[derive(Debug, Clone)]
pub struct Long2IntArraySortedMap {
    key: Vec<i64>,
    val: Vec<i32>,
    size: usize,
}

impl Default for Long2IntArraySortedMap {
    fn default() -> Self {
        Self::new()
    }
}

impl Long2IntArraySortedMap {
    /// `new Long2IntArraySortedMap()`.
    pub fn new() -> Self {
        Self {
            key: vec![0; 8],
            val: vec![0; 8],
            size: 0,
        }
    }

    /// `put(long, int)` — returns the previous value, or `0` if newly inserted.
    pub fn put(&mut self, key: i64, value: i32) -> i32 {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => {
                let current = self.val[index];
                self.val[index] = value;
                current
            }
            Err(insert) => {
                if self.size >= self.val.len() {
                    let new_len = self.key.len() * 2;
                    self.key.resize(new_len, 0);
                    self.val.resize(new_len, 0);
                }
                self.key.copy_within(insert..self.size, insert + 1);
                self.val.copy_within(insert..self.size, insert + 1);
                self.size += 1;

                self.key[insert] = key;
                self.val[insert] = value;
                0
            }
        }
    }

    /// `computeIfAbsent(long, Long2IntFunction)`.
    pub fn compute_if_absent<F: FnOnce(i64) -> i32>(&mut self, key: i64, producer: F) -> i32 {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index],
            Err(insert) => {
                if self.size >= self.val.len() {
                    let new_len = self.key.len() * 2;
                    self.key.resize(new_len, 0);
                    self.val.resize(new_len, 0);
                }
                self.key.copy_within(insert..self.size, insert + 1);
                self.val.copy_within(insert..self.size, insert + 1);
                self.size += 1;

                self.key[insert] = key;
                let value = producer(key);
                self.val[insert] = value;
                value
            }
        }
    }

    /// `get(long)` — returns `0` when absent.
    pub fn get(&self, key: i64) -> i32 {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index],
            Err(_) => 0,
        }
    }

    /// `getFloor(long)` — returns the value for the greatest key `<= key`, or `0` if none.
    pub fn get_floor(&self, key: i64) -> i32 {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index],
            Err(insert) => {
                if insert == 0 {
                    0
                } else {
                    self.val[insert - 1]
                }
            }
        }
    }
}
