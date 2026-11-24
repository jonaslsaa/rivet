//! Mirrors `net.minecraft.world.entity.ai.goal.UseItemGoal`.

use super::context::{
    EquipmentSlot, GoalContext, GoalRandom, InteractionHand, ItemStackRef, SoundId,
};
use super::goal::{Goal, GoalFlags};

/// Equips an item in the main hand, starts using it while `is_using_item()` stays true,
/// and clears the slot (playing an optional finish sound) on stop.
///
/// Mirrors Paper 26.2: `UseItemGoal` does not set explicit flags, so its flag set
/// defaults to [`GoalFlags::UNKNOWN_BEHAVIOR`].
pub struct UseItemGoal<C> {
    flags: GoalFlags,
    item: ItemStackRef,
    finish_using_sound: Option<SoundId>,
    can_use_selector: Box<dyn FnMut(&mut C) -> bool>,
}

impl<C> UseItemGoal<C> {
    pub fn new(
        item: ItemStackRef,
        finish_using_sound: Option<SoundId>,
        can_use_selector: impl FnMut(&mut C) -> bool + 'static,
    ) -> Self {
        Self {
            flags: GoalFlags::UNKNOWN_BEHAVIOR,
            item,
            finish_using_sound,
            can_use_selector: Box::new(can_use_selector),
        }
    }
}

impl<C: GoalContext> Goal<C> for UseItemGoal<C> {
    fn can_use(&mut self, ctx: &mut C) -> bool {
        (self.can_use_selector)(ctx)
    }

    fn can_continue_to_use(&mut self, ctx: &mut C) -> bool {
        ctx.is_using_item()
    }

    fn start(&mut self, ctx: &mut C) {
        ctx.set_item_slot(EquipmentSlot::MainHand, self.item.clone());
        ctx.start_using_item(InteractionHand::MainHand);
    }

    fn stop(&mut self, ctx: &mut C) {
        ctx.set_item_slot(EquipmentSlot::MainHand, ItemStackRef::EMPTY);
        if let Some(sound) = &self.finish_using_sound {
            let pitch = ctx.random().next_float() * 0.2 + 0.9;
            ctx.play_sound(sound, 1.0, pitch);
        }
    }

    fn flags(&self) -> GoalFlags {
        self.flags
    }

    fn set_flags(&mut self, required_control_flags: GoalFlags) {
        self.flags.set_for_goal(required_control_flags);
    }
}
