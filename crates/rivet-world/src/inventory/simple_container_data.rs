//! Paper `net.minecraft.world.inventory.SimpleContainerData`.

use super::container_data::ContainerData;

/// Paper `net.minecraft.world.inventory.SimpleContainerData`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleContainerData {
    ints: Vec<i32>,
}

impl SimpleContainerData {
    /// `new SimpleContainerData(int)`.
    pub fn new(count: usize) -> Self {
        Self {
            ints: vec![0; count],
        }
    }

    /// Read all underlying data values.
    pub fn values(&self) -> &[i32] {
        &self.ints
    }
}

impl ContainerData for SimpleContainerData {
    fn get(&self, index: usize) -> i32 {
        self.ints.get(index).copied().unwrap_or(0)
    }

    fn set(&mut self, index: usize, value: i32) {
        if let Some(slot) = self.ints.get_mut(index) {
            *slot = value;
        }
    }

    fn get_count(&self) -> usize {
        self.ints.len()
    }
}
