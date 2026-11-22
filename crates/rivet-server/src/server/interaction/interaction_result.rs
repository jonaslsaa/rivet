//! Paper 26.2 `net.minecraft.world.InteractionResult` port.

use rivet_registry::core::BlockPos;

/// `InteractionResult.SwingSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwingSource {
    None,
    Client,
    Server,
}

/// `InteractionResult.PaperSuccessContext`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaperSuccessContext {
    pub placed_pos: Option<BlockPos>,
    pub updated_block_entity: bool,
}

impl PaperSuccessContext {
    pub fn placed_block_at(self, pos: BlockPos) -> Self {
        Self {
            placed_pos: Some(pos),
            updated_block_entity: self.updated_block_entity,
        }
    }

    pub fn updated_block_entity(self, updated_block_entity: bool) -> Self {
        Self {
            placed_pos: self.placed_pos,
            updated_block_entity,
        }
    }
}

/// `net.minecraft.world.InteractionResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionResult {
    Success {
        swing_source: SwingSource,
        was_item_interaction: bool,
        paper_context: PaperSuccessContext,
    },
    Fail,
    Pass,
    TryEmptyHandInteraction,
}

impl InteractionResult {
    pub const SUCCESS: Self = Self::Success {
        swing_source: SwingSource::Client,
        was_item_interaction: true,
        paper_context: PaperSuccessContext {
            placed_pos: None,
            updated_block_entity: false,
        },
    };

    pub const SUCCESS_SERVER: Self = Self::Success {
        swing_source: SwingSource::Server,
        was_item_interaction: true,
        paper_context: PaperSuccessContext {
            placed_pos: None,
            updated_block_entity: false,
        },
    };

    pub const CONSUME: Self = Self::Success {
        swing_source: SwingSource::None,
        was_item_interaction: true,
        paper_context: PaperSuccessContext {
            placed_pos: None,
            updated_block_entity: false,
        },
    };

    /// `InteractionResult.consumesAction()`.
    pub fn consumes_action(self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// `Success.swingSource()` when `self` is `Success`.
    pub fn swing_source(self) -> Option<SwingSource> {
        match self {
            Self::Success { swing_source, .. } => Some(swing_source),
            _ => None,
        }
    }
}
