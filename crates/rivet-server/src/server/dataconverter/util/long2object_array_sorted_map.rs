//! Port of `ca.spottedleaf.dataconverter.util.Long2ObjectArraySortedMap`.

/// Sorted array-backed `long -> V` map with `getFloor` lookup.
#[derive(Debug, Clone)]
pub struct Long2ObjectArraySortedMap<V> {
    key: Vec<i64>,
    val: Vec<Option<V>>,
    size: usize,
}

impl<V> Default for Long2ObjectArraySortedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Long2ObjectArraySortedMap<V> {
    /// `new Long2ObjectArraySortedMap()`.
    pub fn new() -> Self {
        let mut val = Vec::with_capacity(8);
        val.resize_with(8, || None);
        Self {
            key: vec![0; 8],
            val,
            size: 0,
        }
    }

    /// `put(long, V)` — returns the previous value, or `None` if newly inserted.
    pub fn put(&mut self, key: i64, value: V) -> Option<V> {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index].replace(value),
            Err(insert) => {
                if self.size >= self.val.len() {
                    let new_len = self.key.len() * 2;
                    self.key.resize(new_len, 0);
                    self.val.resize_with(new_len, || None);
                }
                self.key.copy_within(insert..self.size, insert + 1);
                for i in (insert..self.size).rev() {
                    self.val[i + 1] = self.val[i].take();
                }
                self.size += 1;

                self.key[insert] = key;
                self.val[insert] = Some(value);
                None
            }
        }
    }

    /// `computeIfAbsent(long, LongFunction<V>)`.
    pub fn compute_if_absent<F: FnOnce(i64) -> V>(&mut self, key: i64, producer: F) -> Option<&V> {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index].as_ref(),
            Err(insert) => {
                if self.size >= self.val.len() {
                    let new_len = self.key.len() * 2;
                    self.key.resize(new_len, 0);
                    self.val.resize_with(new_len, || None);
                }
                self.key.copy_within(insert..self.size, insert + 1);
                for i in (insert..self.size).rev() {
                    self.val[i + 1] = self.val[i].take();
                }
                self.size += 1;

                self.key[insert] = key;
                self.val[insert] = Some(producer(key));
                self.val[insert].as_ref()
            }
        }
    }

    /// `get(long)` — returns `None` when absent.
    pub fn get(&self, key: i64) -> Option<&V> {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index].as_ref(),
            Err(_) => None,
        }
    }

    /// `getFloor(long)` — returns the value for the greatest key `<= key`, or `None` if none.
    pub fn get_floor(&self, key: i64) -> Option<&V> {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index].as_ref(),
            Err(insert) => {
                if insert == 0 {
                    None
                } else {
                    self.val[insert - 1].as_ref()
                }
            }
        }
    }
}
