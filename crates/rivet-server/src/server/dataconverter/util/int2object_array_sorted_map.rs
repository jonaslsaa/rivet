//! Port of `ca.spottedleaf.dataconverter.util.Int2ObjectArraySortedMap`.
//!
//! Fidelity note: in pinned Paper 26.2 (`0a993450f129c4942c2a9ed45ba047412b4667cf`),
//! `Int2ObjectArraySortedMap.put` and `computeIfAbsent` omit `++this.size;`, and
//! `getFloor` indexes `this.val[insert]` directly (without subtracting 1 or bounds
//! checking). This port preserves that exact Paper 26.2 behavior verbatim.

/// Sorted array-backed `int -> V` map matching Paper 26.2 `Int2ObjectArraySortedMap`.
#[derive(Debug, Clone)]
pub struct Int2ObjectArraySortedMap<V> {
    key: Vec<i32>,
    val: Vec<Option<V>>,
    size: usize,
}

impl<V> Default for Int2ObjectArraySortedMap<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> Int2ObjectArraySortedMap<V> {
    /// `new Int2ObjectArraySortedMap()`.
    pub fn new() -> Self {
        let mut val = Vec::with_capacity(8);
        val.resize_with(8, || None);
        Self {
            key: vec![0; 8],
            val,
            size: 0,
        }
    }

    /// `put(int, V)`.
    pub fn put(&mut self, key: i32, value: V) -> Option<V> {
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
                // Paper 26.2 omits `++this.size;` here.

                self.key[insert] = key;
                self.val[insert] = Some(value);
                None
            }
        }
    }

    /// `computeIfAbsent(int, IntFunction<V>)`.
    pub fn compute_if_absent<F: FnOnce(i32) -> V>(&mut self, key: i32, producer: F) -> Option<&V> {
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
                // Paper 26.2 omits `++this.size;` here.

                self.key[insert] = key;
                self.val[insert] = Some(producer(key));
                self.val[insert].as_ref()
            }
        }
    }

    /// `get(int)`.
    pub fn get(&self, key: i32) -> Option<&V> {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index].as_ref(),
            Err(_) => None,
        }
    }

    /// `getFloor(int)`.
    pub fn get_floor(&self, key: i32) -> Option<&V> {
        match self.key[..self.size].binary_search(&key) {
            Ok(index) => self.val[index].as_ref(),
            Err(insert) => self.val[insert].as_ref(),
        }
    }
}
