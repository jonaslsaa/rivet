//! `net.minecraft.world.InteractionResult` — the sealed result of a block or
//! item interaction (#448), including Paper's `PaperSuccessContext`.
//!
//! Java source: `working/Paper/paper-server/src/minecraft/java/net/minecraft/
//! world/InteractionResult.java`. The sealed interface's four records become
//! enum variants; `Success`'s `ItemContext` and `PaperSuccessContext` records
//! are ported as their own structs. `heldItemTransformedTo` holds an owned
//! [`ItemStack`] (Java holds a reference to the stack an item use produced).

use rivet_registry::core::BlockPos;

use crate::item::item_stack::ItemStack;

/// `InteractionResult.SwingSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwingSource {
    /// `NONE`.
    None,
    /// `CLIENT`.
    Client,
    /// `SERVER`.
    Server,
}

/// `InteractionResult.ItemContext(boolean wasItemInteraction, @Nullable ItemStack heldItemTransformedTo)`.
#[derive(Debug, Clone)]
pub struct ItemContext {
    pub was_item_interaction: bool,
    pub held_item_transformed_to: Option<ItemStack>,
}

impl ItemContext {
    /// `ItemContext.NONE`.
    pub const NONE: ItemContext = ItemContext {
        was_item_interaction: false,
        held_item_transformed_to: None,
    };

    /// `ItemContext.DEFAULT`.
    pub const DEFAULT: ItemContext = ItemContext {
        was_item_interaction: true,
        held_item_transformed_to: None,
    };
}

/// Paper `InteractionResult.PaperSuccessContext(@Nullable BlockPos placedPos, boolean updatedBlockEntity)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaperSuccessContext {
    pub placed_pos: Option<BlockPos>,
    pub updated_block_entity: bool,
}

impl PaperSuccessContext {
    /// `PaperSuccessContext.DEFAULT`.
    pub const DEFAULT: PaperSuccessContext = PaperSuccessContext {
        placed_pos: None,
        updated_block_entity: false,
    };

    /// `placedBlockAt(BlockPos)`.
    pub fn placed_block_at(self, pos: BlockPos) -> Self {
        PaperSuccessContext {
            placed_pos: Some(pos),
            updated_block_entity: self.updated_block_entity,
        }
    }

    /// `updatedBlockEntity(boolean)`.
    pub fn updated_block_entity(self, updated_block_entity: bool) -> Self {
        PaperSuccessContext {
            placed_pos: self.placed_pos,
            updated_block_entity,
        }
    }
}

/// `InteractionResult.Success(SwingSource, ItemContext, PaperSuccessContext)`.
#[derive(Debug, Clone)]
pub struct Success {
    pub swing_source: SwingSource,
    pub item_context: ItemContext,
    pub paper_success_context: PaperSuccessContext,
}

impl Success {
    /// `new Success(SwingSource, ItemContext)` — Paper's two-argument
    /// constructor defaulting the Paper context.
    pub const fn new(swing_source: SwingSource, item_context: ItemContext) -> Self {
        Success {
            swing_source,
            item_context,
            paper_success_context: PaperSuccessContext::DEFAULT,
        }
    }

    /// `configurePaper(UnaryOperator<PaperSuccessContext>)`.
    pub fn configure_paper(
        self,
        context: impl FnOnce(PaperSuccessContext) -> PaperSuccessContext,
    ) -> Self {
        Success {
            swing_source: self.swing_source,
            item_context: self.item_context,
            paper_success_context: context(self.paper_success_context),
        }
    }

    /// `heldItemTransformedTo(ItemStack)`.
    pub fn with_held_item_transformed_to(self, item_stack: ItemStack) -> Self {
        Success {
            swing_source: self.swing_source,
            item_context: ItemContext {
                was_item_interaction: true,
                held_item_transformed_to: Some(item_stack),
            },
            paper_success_context: self.paper_success_context,
        }
    }

    /// `withoutItem()`.
    pub fn without_item(self) -> Self {
        Success {
            swing_source: self.swing_source,
            item_context: ItemContext::NONE,
            paper_success_context: self.paper_success_context,
        }
    }

    /// `swingSource()`.
    pub fn swing_source(&self) -> SwingSource {
        self.swing_source
    }

    /// `wasItemInteraction()`.
    pub fn was_item_interaction(&self) -> bool {
        self.item_context.was_item_interaction
    }

    /// `heldItemTransformedTo()`.
    pub fn held_item_transformed_to(&self) -> Option<&ItemStack> {
        self.item_context.held_item_transformed_to.as_ref()
    }
}

/// `InteractionResult` — `permits Success, Fail, Pass, TryEmptyHandInteraction`.
#[derive(Debug, Clone)]
pub enum InteractionResult {
    Success(Success),
    Fail,
    Pass,
    TryEmptyHandInteraction,
}

impl InteractionResult {
    /// `InteractionResult.SUCCESS`.
    pub const SUCCESS: InteractionResult =
        InteractionResult::Success(Success::new(SwingSource::Client, ItemContext::DEFAULT));
    /// `InteractionResult.SUCCESS_SERVER`.
    pub const SUCCESS_SERVER: InteractionResult =
        InteractionResult::Success(Success::new(SwingSource::Server, ItemContext::DEFAULT));
    /// `InteractionResult.CONSUME`.
    pub const CONSUME: InteractionResult =
        InteractionResult::Success(Success::new(SwingSource::None, ItemContext::DEFAULT));
    /// `InteractionResult.FAIL`.
    pub const FAIL: InteractionResult = InteractionResult::Fail;
    /// `InteractionResult.PASS`.
    pub const PASS: InteractionResult = InteractionResult::Pass;
    /// `InteractionResult.TRY_WITH_EMPTY_HAND`.
    pub const TRY_WITH_EMPTY_HAND: InteractionResult = InteractionResult::TryEmptyHandInteraction;

    /// `consumesAction()` — `true` only for `Success`.
    pub fn consumes_action(&self) -> bool {
        matches!(self, InteractionResult::Success(_))
    }

    /// `this instanceof Success success && success.swingSource() == source`.
    pub fn is_success_with_swing_source(&self, source: SwingSource) -> bool {
        matches!(self, InteractionResult::Success(success) if success.swing_source == source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_java_records() {
        for (result, source) in [
            (InteractionResult::SUCCESS, SwingSource::Client),
            (InteractionResult::SUCCESS_SERVER, SwingSource::Server),
            (InteractionResult::CONSUME, SwingSource::None),
        ] {
            assert!(result.consumes_action());
            assert!(result.is_success_with_swing_source(source));
            let InteractionResult::Success(success) = result else {
                unreachable!()
            };
            assert!(success.was_item_interaction());
            assert!(success.held_item_transformed_to().is_none());
            assert_eq!(success.paper_success_context, PaperSuccessContext::DEFAULT);
        }
        assert!(!InteractionResult::FAIL.consumes_action());
        assert!(!InteractionResult::PASS.consumes_action());
        assert!(!InteractionResult::TRY_WITH_EMPTY_HAND.consumes_action());
    }

    #[test]
    fn success_builders_keep_other_components() {
        let pos = BlockPos::new(1, 2, 3);
        let success = Success::new(SwingSource::Server, ItemContext::DEFAULT)
            .configure_paper(|context| context.placed_block_at(pos).updated_block_entity(true))
            .without_item();
        assert_eq!(success.swing_source(), SwingSource::Server);
        assert!(!success.was_item_interaction());
        assert_eq!(success.paper_success_context.placed_pos, Some(pos));
        assert!(success.paper_success_context.updated_block_entity);
    }
}
