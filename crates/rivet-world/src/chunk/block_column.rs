//! Port of `net.minecraft.world.level.chunk.BlockColumn` (MC 26.2).
//!
//! Java: `BlockColumn.java` in `working/Paper`. A two-method read/write seam
//! over a block-state column (`getBlock(int blockY)` / `setBlock(int,
//! BlockState)`). The port is a trait over the caller's block-state type; the
//! owning `mc.world.level.chunk.access` unit implements it on its chunk types.

/// `net.minecraft.world.level.chunk.BlockColumn`.
pub trait BlockColumn<T> {
    /// `getBlock(int)`.
    fn get_block(&self, block_y: i32) -> T;
    /// `setBlock(int, BlockState)`.
    fn set_block(&mut self, block_y: i32, state: T);
}

#[cfg(test)]
mod tests {
    use super::BlockColumn;

    /// A tiny column for the seam contract test.
    struct Column {
        states: Vec<u32>,
    }

    impl BlockColumn<u32> for Column {
        fn get_block(&self, block_y: i32) -> u32 {
            self.states[block_y as usize]
        }

        fn set_block(&mut self, block_y: i32, state: u32) {
            self.states[block_y as usize] = state;
        }
    }

    #[test]
    fn get_then_set_round_trips_at_absolute_y() {
        let mut column = Column {
            states: vec![0; 64],
        };
        assert_eq!(column.get_block(0), 0);
        column.set_block(63, 7);
        assert_eq!(column.get_block(63), 7);
        assert_eq!(column.get_block(0), 0);
    }
}
