//! Port of `net.minecraft.world.level.chunk.DataLayer` (MC 26.2) — a 16×16×16
//! light layer.
//!
//! Java: `DataLayer.java` in `working/Paper`. One 2048-byte byte array of
//! 4-bit nibbles: entry `getIndex(x, y, z) = y << 8 | z << 4 | x` lives at byte
//! `index >> 1`, low nibble for even x, high nibble for odd x. `data == null`
//! means the layer is uniform at `defaultValue` (allocation deferred until
//! `getData`); `isEmpty()` is `data == null && defaultValue == 0`.

use std::fmt;

/// `DataLayer.LAYER_COUNT` — the per-axis entry count.
pub const LAYER_COUNT: i32 = 16;
/// `DataLayer.LAYER_SIZE` — bytes per 16×16 horizontal slice (256 entries).
pub const LAYER_SIZE: i32 = 128;
/// `DataLayer.SIZE` — the fixed byte size of one layer.
pub const SIZE: i32 = 2048;

/// `net.minecraft.world.level.chunk.DataLayer`.
pub struct DataLayer {
    data: Option<Box<[u8]>>,
    default_value: i32,
}

impl DataLayer {
    /// `DataLayer()` / `DataLayer(int defaultValue)` — a uniform layer
    /// (`defaultValue` 0 unless `fill`).
    pub fn new(default_value: i32) -> Self {
        DataLayer {
            data: None,
            default_value,
        }
    }

    /// `DataLayer(byte[] data)` — an explicit 2048-byte layer; panics like
    /// Java's `IllegalArgumentException` when the length differs.
    pub fn with_data(data: Vec<u8>) -> Self {
        assert_eq!(
            data.len(),
            SIZE as usize,
            "DataLayer should be 2048 bytes not: {}",
            data.len()
        );
        DataLayer {
            data: Some(data.into_boxed_slice()),
            default_value: 0,
        }
    }

    /// `get(x, y, z)` — `get(y << 8 | z << 4 | x)`.
    pub fn get(&self, x: i32, y: i32, z: i32) -> i32 {
        self.get_index(get_index(x, y, z))
    }

    fn get_index(&self, index: i32) -> i32 {
        match &self.data {
            None => self.default_value,
            Some(data) => {
                let position = index >> 1;
                let nibble = index & 1;
                (data[position as usize] >> (4 * nibble) & 15) as i32
            }
        }
    }

    /// `set(x, y, z, val)`.
    pub fn set(&mut self, x: i32, y: i32, z: i32, val: i32) {
        let index = get_index(x, y, z);
        let data = self.get_data_mut();
        let position = index >> 1;
        let nibble = index & 1;
        let mask = !(15 << (4 * nibble));
        let value_to_set = (val & 15) << (4 * nibble);
        data[position as usize] &= mask as u8;
        data[position as usize] |= value_to_set as u8;
    }

    /// `fill(int value)` — sets the uniform default (drops any explicit data).
    pub fn fill(&mut self, value: i32) {
        self.default_value = value;
        self.data = None;
    }

    /// `getData()` — materializes the 2048 bytes.
    pub fn get_data(&self) -> Vec<u8> {
        match &self.data {
            Some(data) => data.to_vec(),
            None => {
                let mut out = vec![0u8; SIZE as usize];
                if self.default_value != 0 {
                    let packed = pack_filled(self.default_value);
                    out.fill(packed);
                }
                out
            }
        }
    }

    fn get_data_mut(&mut self) -> &mut [u8] {
        if self.data.is_none() {
            let mut out = vec![0u8; SIZE as usize];
            if self.default_value != 0 {
                let packed = pack_filled(self.default_value);
                out.fill(packed);
            }
            self.data = Some(out.into_boxed_slice());
        }
        self.data.as_mut().unwrap()
    }

    /// `copy()` — a fresh independent layer with identical contents.
    pub fn copy(&self) -> DataLayer {
        match &self.data {
            None => DataLayer::new(self.default_value),
            Some(data) => DataLayer::with_data(data.to_vec()),
        }
    }

    /// `isDefinitelyHomogenous()`.
    pub fn is_definitely_homogenous(&self) -> bool {
        self.data.is_none()
    }

    /// `isDefinitelyFilledWith(int value)`.
    pub fn is_definitely_filled_with(&self, value: i32) -> bool {
        self.data.is_none() && self.default_value == value
    }

    /// `isEmpty()` — the uniform-zero layer.
    pub fn is_empty(&self) -> bool {
        self.data.is_none() && self.default_value == 0
    }
}

/// `DataLayer.getIndex(x, y, z)` — `y << 8 | z << 4 | x`.
fn get_index(x: i32, y: i32, z: i32) -> i32 {
    y << 8 | z << 4 | x
}

/// `DataLayer.packFilled(int value)` — `(byte)(value | value << 4)`, repeating
/// the 4-bit value into both nibble slots of a byte.
fn pack_filled(value: i32) -> u8 {
    let mut packed = value as u8;
    for i in (4..8).step_by(4) {
        packed |= (value << i) as u8;
    }
    packed
}

impl Clone for DataLayer {
    /// `copy()` — a fresh independent layer with identical contents.
    fn clone(&self) -> Self {
        self.copy()
    }
}

impl fmt::Debug for DataLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DataLayer")
            .field("default_value", &self.default_value)
            .field("has_data", &self.data.is_some())
            .finish()
    }
}

impl PartialEq for DataLayer {
    fn eq(&self, other: &Self) -> bool {
        match (&self.data, &other.data) {
            (None, None) => self.default_value == other.default_value,
            (None, Some(_)) => self.get_data() == other.get_data(),
            (Some(_), None) => self.get_data() == other.get_data(),
            (Some(a), Some(b)) => a == b,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_layout_is_y_then_z_then_x() {
        // getIndex(x, y, z) = y << 8 | z << 4 | x; byte = index >> 1; even x
        // in the low nibble, odd x in the high nibble.
        let mut layer = DataLayer::new(0);
        layer.set(0, 0, 0, 5); // index 0 -> byte 0 low nibble
        layer.set(1, 0, 0, 10); // index 1 -> byte 0 high nibble
        assert_eq!(layer.get(0, 0, 0), 5);
        assert_eq!(layer.get(1, 0, 0), 10);
        assert_eq!(layer.get_data()[0], 0xA5);
        // y=1 -> byte offset 128.
        layer.set(0, 1, 0, 7);
        assert_eq!(layer.get_data()[128], 0x07);
    }

    #[test]
    fn uniform_layers_materialize_and_compare() {
        let empty = DataLayer::new(0);
        assert!(empty.is_empty());
        assert!(empty.is_definitely_homogenous());
        let filled = DataLayer::new(15);
        assert!(!filled.is_empty());
        assert_eq!(filled.get_data(), vec![0xFF; SIZE as usize]);
        // packFilled keeps the value in both nibbles: 0x33 for default 3.
        assert_eq!(DataLayer::new(3).get_data(), vec![0x33; SIZE as usize]);
        assert_eq!(empty.copy().get_data(), vec![0x00; SIZE as usize]);
        assert_eq!(empty, DataLayer::new(0));
        assert_ne!(empty, filled);
    }

    #[test]
    fn with_data_requires_exact_length() {
        assert!(std::panic::catch_unwind(|| DataLayer::with_data(vec![0; 100])).is_err());
        let layer = DataLayer::with_data(vec![0x11; SIZE as usize]);
        assert!(!layer.is_empty());
        assert_eq!(layer.get_data(), vec![0x11; SIZE as usize]);
    }
}
