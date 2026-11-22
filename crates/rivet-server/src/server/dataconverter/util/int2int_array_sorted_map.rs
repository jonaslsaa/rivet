//! Port of `ca.spottedleaf.dataconverter.util.Int2IntArraySortedMap`.

/// Sorted array-backed `int -> int` map with `getFloor` lookup.
#[derive(Debug, Clone)]
pub struct Int2IntArraySortedMap {
    key: Vec<i32>,
    val: Vec<i32>,
    size: usize,
}

impl Default for Int2IntArraySortedMap {
    fn default() -> Self {
        Self::new()
    }
}

impl Int2IntArraySortedMap {
    /// `new Int2IntArraySortedMap()`.
    pub fn new() -> Self {
        Self {
            key: vec![0; 8],
            val: vec![0; 8],
            size: 0,
        }
    }

    /// `put(int, int)` — returns the previous value, or `0` if newly inserted.
    pub fn put(&mut self, key: i32, value: i32) -> i32 {
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

    /// `computeIfAbsent(int, Int2IntFunction)`.
    pub fn compute_if_absent<F: FnOnce(i32) -> i32>(&mut self, key: i32, producer: F) -> i32 {
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

    /// `get(int)` — returns `0` when absent.
    pub fn get(&self, key: i32) -> i32 {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index],
            Err(_) => 0,
        }
    }

    /// `getFloor(int)` — returns the value for the greatest key `<= key`, or `0` if none.
    pub fn get_floor(&self, key: i32) -> i32 {
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
