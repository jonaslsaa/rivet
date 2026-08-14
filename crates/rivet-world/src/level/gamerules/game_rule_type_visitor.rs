//! `net.minecraft.world.level.gamerules.GameRuleTypeVisitor` — the visitor a
//! game-rule key dispatches itself to (by value type).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/GameRuleTypeVisitor.java`. Every method has an empty default body;
//! a visitor overrides the typed methods it cares about.

use crate::level::gamerules::game_rule::GameRuleErased;

/// `GameRuleTypeVisitor` — the type-erased `GameRule<?>` wildcard replaces the
/// Java generic `<T>` on `visit`; the erased rule is passed by reference (the
/// registry-held `Arc` identity is recoverable by the visitor through the
/// GAME_RULE registry, exactly as Java's `GameRule` object identity).
pub trait GameRuleTypeVisitor {
    /// `visit(GameRule<T>)` — the generic entry, empty by default.
    fn visit(&mut self, _game_rule: &GameRuleErased) {}

    /// `visitBoolean(GameRule<Boolean>)`.
    fn visit_boolean(&mut self, _game_rule: &GameRuleErased) {}

    /// `visitInteger(GameRule<Integer>)`.
    fn visit_integer(&mut self, _game_rule: &GameRuleErased) {}
}
