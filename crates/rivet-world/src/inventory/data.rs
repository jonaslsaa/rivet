//! Paper `net.minecraft.world.inventory.ContainerData`,
//! `SimpleContainerData`, and `DataSlot`.

/// Paper `net.minecraft.world.inventory.ContainerData`.
pub trait ContainerData {
    /// `ContainerData.get(int)`.
    fn get(&self, index: usize) -> i32;

    /// `ContainerData.set(int, int)`.
    fn set(&mut self, index: usize, value: i32);

    /// `ContainerData.getCount()`.
    fn get_count(&self) -> usize;
}

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

/// Backing storage for a [`DataSlot`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DataSlotBacking {
    /// `DataSlot.standalone()` — owns its `i32` value.
    Standalone(i32),
    /// `DataSlot.forContainer(ContainerData, int)` — indexes a menu-local
    /// `ContainerData` table by `(data_id, index)`.
    ContainerData { data_id: usize, index: usize },
}

/// Paper `net.minecraft.world.inventory.DataSlot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataSlot {
    backing: DataSlotBacking,
    prev_value: i32,
}

impl DataSlot {
    /// `DataSlot.standalone()`.
    pub const fn standalone() -> Self {
        Self {
            backing: DataSlotBacking::Standalone(0),
            prev_value: 0,
        }
    }

    /// `DataSlot.forContainer(ContainerData, int)`.
    pub const fn for_container(data_id: usize, index: usize) -> Self {
        Self {
            backing: DataSlotBacking::ContainerData { data_id, index },
            prev_value: 0,
        }
    }

    /// `DataSlot.get()`.
    pub fn get(&self, data_sources: &[&dyn ContainerData]) -> i32 {
        match self.backing {
            DataSlotBacking::Standalone(value) => value,
            DataSlotBacking::ContainerData { data_id, index } => data_sources
                .get(data_id)
                .map_or(0, |source| source.get(index)),
        }
    }

    /// `DataSlot.set(int)`.
    pub fn set(&mut self, data_sources: &mut [&mut dyn ContainerData], value: i32) {
        match &mut self.backing {
            DataSlotBacking::Standalone(slot) => *slot = value,
            DataSlotBacking::ContainerData { data_id, index } => {
                if let Some(source) = data_sources.get_mut(*data_id) {
                    source.set(*index, value);
                }
            }
        }
    }

    /// `DataSlot.checkAndClearUpdateFlag()` — compares the current value with
    /// `prev_value`, updates `prev_value`, and returns whether it changed.
    pub fn check_and_clear_update_flag(&mut self, current_value: i32) -> bool {
        let changed = current_value != self.prev_value;
        self.prev_value = current_value;
        changed
    }
}
