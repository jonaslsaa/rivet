//! Paper `net.minecraft.world.inventory.ContainerLevelAccess`.

use rivet_registry::core::BlockPos;

/// Paper `net.minecraft.world.inventory.ContainerLevelAccess` seam for
/// block-entity / world-bound menus without coupling to `ServerLevel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerLevelAccess {
    /// `ContainerLevelAccess.NULL`.
    Null,
    /// `ContainerLevelAccess.create(Level, BlockPos)` identity seam.
    At { dimension_id: String, pos: BlockPos },
}

impl ContainerLevelAccess {
    /// `ContainerLevelAccess.create(...)`.
    pub fn create(dimension_id: impl Into<String>, pos: BlockPos) -> Self {
        Self::At {
            dimension_id: dimension_id.into(),
            pos,
        }
    }

    /// `ContainerLevelAccess.isBlock()`.
    pub const fn is_block(&self) -> bool {
        matches!(self, Self::At { .. })
    }

    /// `ContainerLevelAccess.evaluate(BiFunction)`.
    pub fn evaluate<T, F: FnOnce(&str, BlockPos) -> T>(&self, f: F) -> Option<T> {
        match self {
            Self::Null => None,
            Self::At { dimension_id, pos } => Some(f(dimension_id.as_str(), *pos)),
        }
    }

    /// `ContainerLevelAccess.evaluate(BiFunction, T)`.
    pub fn evaluate_or<T, F: FnOnce(&str, BlockPos) -> T>(&self, default: T, f: F) -> T {
        self.evaluate(f).unwrap_or(default)
    }

    /// `ContainerLevelAccess.execute(BiConsumer)`.
    pub fn execute<F: FnOnce(&str, BlockPos)>(&self, f: F) {
        if let Self::At { dimension_id, pos } = self {
            f(dimension_id.as_str(), *pos);
        }
    }
}
