//! fastutil 8.5.18 Long2LongOpenHashMap's default-construction, put, remove,
//! lookup and fast-entry iterator update/removal surface. The probe table is
//! observable through LevelTicks' equal-priority container drain order.

#[derive(Clone, Debug)]
pub struct Long2LongOpenHashMap {
    keys: Vec<i64>,
    values: Vec<i64>,
    contains_zero: bool,
    size: usize,
    min_n: usize,
}

fn mix(key: i64) -> usize {
    let hash = (key as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let hash = hash ^ (hash >> 32);
    (hash ^ (hash >> 16)) as usize
}
fn array_size(size: usize) -> usize {
    let n = ((size as f64 / 0.75).ceil() as usize)
        .max(2)
        .checked_next_power_of_two()
        .expect("fastutil map capacity overflow");
    assert!(n <= 1 << 30, "fastutil map capacity exceeds 2^30");
    n
}
impl Default for Long2LongOpenHashMap {
    fn default() -> Self {
        let n = array_size(16);
        Self {
            keys: vec![0; n + 1],
            values: vec![0; n + 1],
            contains_zero: false,
            size: 0,
            min_n: n,
        }
    }
}
impl Long2LongOpenHashMap {
    fn n(&self) -> usize {
        self.keys.len() - 1
    }
    fn find(&self, key: i64) -> Option<usize> {
        if key == 0 {
            return self.contains_zero.then_some(self.n());
        }
        let mask = self.n() - 1;
        let mut slot = mix(key) & mask;
        while self.keys[slot] != 0 {
            if self.keys[slot] == key {
                return Some(slot);
            }
            slot = (slot + 1) & mask;
        }
        None
    }
    pub fn get(&self, key: i64) -> Option<i64> {
        self.find(key).map(|slot| self.values[slot])
    }
    pub fn put(&mut self, key: i64, value: i64) -> Option<i64> {
        if let Some(slot) = self.find(key) {
            return Some(std::mem::replace(&mut self.values[slot], value));
        }
        let n = self.n();
        let slot = if key == 0 {
            self.contains_zero = true;
            n
        } else {
            let mut slot = mix(key) & (n - 1);
            while self.keys[slot] != 0 {
                slot = (slot + 1) & (n - 1);
            }
            self.keys[slot] = key;
            slot
        };
        self.values[slot] = value;
        let previous = self.size;
        self.size += 1;
        if previous >= (n * 3).div_ceil(4).min(n - 1) {
            self.rehash(array_size(self.size + 1));
        }
        None
    }
    pub fn remove(&mut self, key: i64) -> Option<i64> {
        let slot = self.find(key)?;
        let value = self.values[slot];
        if key == 0 {
            self.contains_zero = false;
        } else {
            self.shift_keys(slot, None);
        }
        self.size -= 1;
        let n = self.n();
        if n > self.min_n && self.size < (n * 3).div_ceil(4).min(n - 1) / 4 && n > 16 {
            self.rehash(n / 2);
        }
        Some(value)
    }
    fn shift_keys(&mut self, mut gap: usize, mut wrapped: Option<&mut Vec<i64>>) {
        let mask = self.n() - 1;
        loop {
            let mut pos = (gap + 1) & mask;
            loop {
                let key = self.keys[pos];
                if key == 0 {
                    self.keys[gap] = 0;
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
            if pos < gap
                && let Some(wrapped) = &mut wrapped
            {
                wrapped.push(self.keys[pos]);
            }
            self.keys[gap] = self.keys[pos];
            self.values[gap] = self.values[pos];
            gap = pos;
        }
    }
    fn rehash(&mut self, n: usize) {
        let mut keys = vec![0; n + 1];
        let mut values = vec![0; n + 1];
        for old in (0..self.n()).rev() {
            let key = self.keys[old];
            if key == 0 {
                continue;
            }
            let mut slot = mix(key) & (n - 1);
            while keys[slot] != 0 {
                slot = (slot + 1) & (n - 1);
            }
            keys[slot] = key;
            values[slot] = self.values[old];
        }
        values[n] = self.values[self.n()];
        self.keys = keys;
        self.values = values;
    }
    /// Long2LongMaps.fastIterator: zero first, then descending table slots.
    /// Return None to invoke iterator.remove (not ordinary map.remove), or
    /// Some(value) to invoke the live entry's setValue. Wrapped clusters and
    /// iterator removal's deliberately different shrinking rules are retained.
    pub fn edit_entries(&mut self, mut edit: impl FnMut(i64, i64) -> Option<i64>) {
        let mut remaining = self.size;
        let mut pos = self.n() as isize;
        let mut zero = self.contains_zero;
        let mut wrapped = Vec::new();
        while remaining != 0 {
            remaining -= 1;
            let slot = if zero {
                zero = false;
                self.n()
            } else {
                loop {
                    pos -= 1;
                    if pos < 0 {
                        break self.find(wrapped[(-pos - 1) as usize]).unwrap();
                    }
                    if self.keys[pos as usize] != 0 {
                        break pos as usize;
                    }
                }
            };
            let key = self.keys[slot];
            if let Some(value) = edit(key, self.values[slot]) {
                self.values[slot] = value;
            } else if slot == self.n() {
                self.contains_zero = false;
                self.size -= 1;
            } else if pos >= 0 {
                self.shift_keys(slot, Some(&mut wrapped));
                self.size -= 1;
            } else {
                self.remove(key);
            }
        }
    }
    pub fn entries(&self) -> impl Iterator<Item = (i64, i64)> + '_ {
        self.contains_zero
            .then_some((0, self.values[self.n()]))
            .into_iter()
            .chain(
                (0..self.n())
                    .rev()
                    .filter_map(|i| (self.keys[i] != 0).then_some((self.keys[i], self.values[i]))),
            )
    }
}
