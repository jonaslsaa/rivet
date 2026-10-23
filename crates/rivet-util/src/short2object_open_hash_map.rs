//! The fastutil 8.5.18 Short2ObjectOpenHashMap surface used by PoiSection.
//! Iterators visit zero-key first, then descending table slots; sequential
//! streams use the spliterator's zero-first ASCENDING table traversal.
//! Copy-construction
//! sizes from the source size and reinserts in source iterator order; it is
//! deliberately distinct from cloning the probe table.
#[derive(Debug, Clone)]
pub struct Short2ObjectOpenHashMap<V> {
    keys: Vec<i16>,
    values: Vec<Option<V>>,
    size: usize,
    min_n: usize,
}
fn mix(key: i16) -> usize {
    let hash = (key as i32 as u32).wrapping_mul(0x9e37_79b9);
    (hash ^ (hash >> 16)) as usize
}
fn array_size(size: usize) -> usize {
    let n = ((size as f64 / 0.75).ceil() as usize)
        .max(2)
        .checked_next_power_of_two()
        .expect("fastutil short map capacity overflow");
    assert!(n <= 1 << 30, "fastutil short map capacity exceeds 2^30");
    n
}
impl<V> Default for Short2ObjectOpenHashMap<V> {
    fn default() -> Self {
        Self::with_expected(16)
    }
}
impl<V> Short2ObjectOpenHashMap<V> {
    fn with_expected(expected: usize) -> Self {
        let n = array_size(expected);
        Self {
            keys: vec![0; n + 1],
            values: std::iter::repeat_with(|| None).take(n + 1).collect(),
            size: 0,
            min_n: n,
        }
    }
    fn n(&self) -> usize {
        self.keys.len() - 1
    }
    pub fn len(&self) -> usize {
        self.size
    }
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }
    fn find(&self, key: i16) -> Option<usize> {
        if key == 0 {
            return self.values[self.n()].as_ref().map(|_| self.n());
        }
        let mut slot = mix(key) & (self.n() - 1);
        while self.keys[slot] != 0 {
            if self.keys[slot] == key {
                return Some(slot);
            }
            slot = (slot + 1) & (self.n() - 1);
        }
        None
    }
    pub fn get(&self, key: i16) -> Option<&V> {
        self.find(key).and_then(|slot| self.values[slot].as_ref())
    }
    pub fn put(&mut self, key: i16, value: V) -> Option<V> {
        if let Some(slot) = self.find(key) {
            return self.values[slot].replace(value);
        }
        let n = self.n();
        let slot = if key == 0 {
            n
        } else {
            let mut slot = mix(key) & (n - 1);
            while self.keys[slot] != 0 {
                slot = (slot + 1) & (n - 1);
            }
            self.keys[slot] = key;
            slot
        };
        self.values[slot] = Some(value);
        let previous = self.size;
        self.size += 1;
        if previous >= (n * 3).div_ceil(4).min(n - 1) {
            self.rehash(array_size(self.size + 1));
        }
        None
    }
    pub fn remove(&mut self, key: i16) -> Option<V> {
        let slot = self.find(key)?;
        let value = self.values[slot].take();
        if key != 0 {
            self.shift_keys(slot);
        }
        self.size -= 1;
        let n = self.n();
        if n > self.min_n && self.size < (n * 3).div_ceil(4).min(n - 1) / 4 && n > 16 {
            self.rehash(n / 2);
        }
        value
    }
    fn shift_keys(&mut self, mut gap: usize) {
        let mask = self.n() - 1;
        loop {
            let mut pos = (gap + 1) & mask;
            loop {
                let key = self.keys[pos];
                if key == 0 {
                    self.keys[gap] = 0;
                    self.values[gap] = None;
                    return;
                }
                let slot = mix(key) & mask;
                if if gap <= pos {
                    gap >= slot || slot > pos
                } else {
                    gap >= slot && slot > pos
                } {
                    break;
                }
                pos = (pos + 1) & mask;
            }
            self.keys[gap] = self.keys[pos];
            self.values[gap] = self.values[pos].take();
            gap = pos;
        }
    }
    fn rehash(&mut self, n: usize) {
        let old_n = self.n();
        let mut keys = vec![0; n + 1];
        let mut values: Vec<_> = std::iter::repeat_with(|| None).take(n + 1).collect();
        for old in (0..old_n).rev() {
            let key = self.keys[old];
            if key == 0 {
                continue;
            }
            let mut slot = mix(key) & (n - 1);
            while keys[slot] != 0 {
                slot = (slot + 1) & (n - 1);
            }
            keys[slot] = key;
            values[slot] = self.values[old].take();
        }
        values[n] = self.values[old_n].take();
        self.keys = keys;
        self.values = values;
    }
    pub fn clear(&mut self) {
        self.size = 0;
        self.keys.fill(0);
        for value in &mut self.values {
            *value = None;
        }
    }
    pub fn entries(&self) -> impl Iterator<Item = (i16, &V)> {
        self.values[self.n()]
            .as_ref()
            .map(|value| (0, value))
            .into_iter()
            .chain(
                (0..self.n())
                    .rev()
                    .filter_map(|i| self.values[i].as_ref().map(|v| (self.keys[i], v))),
            )
    }
    /// ObjectCollection.stream uses MapSpliterator, not MapIterator. In
    /// fastutil these deliberately scan the backing table in opposite orders.
    pub fn values_stream(&self) -> impl Iterator<Item = &V> {
        self.values[self.n()]
            .as_ref()
            .into_iter()
            .chain(self.values[..self.n()].iter().filter_map(Option::as_ref))
    }
    pub fn copy_constructed(&self) -> Self
    where
        V: Clone,
    {
        let mut copy = Self::with_expected(self.size);
        for (key, value) in self.entries() {
            copy.put(key, value.clone());
        }
        copy
    }
}
