//! Paper `net.minecraft.world.inventory.ContainerData`.

/// Paper `net.minecraft.world.inventory.ContainerData`.
pub trait ContainerData {
    /// `ContainerData.get(int)`.
    fn get(&self, index: usize) -> i32;

    /// `ContainerData.set(int, int)`.
    fn set(&mut self, index: usize, value: i32);

    /// `ContainerData.getCount()`.
    fn get_count(&self) -> usize;
}
