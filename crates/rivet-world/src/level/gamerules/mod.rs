//! `net.minecraft.world.level.gamerules` — the game-rules value unit.
//!
//! Java sources:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/` — `GameRule.java`, `GameRuleCategory.java`, `GameRuleMap.java`,
//! `GameRules.java`, `GameRuleType.java`, `GameRuleTypeVisitor.java`,
//! `package-info.java` (`@NullMarked` only, no port surface).
//!
//! ## The erased wildcard
//!
//! Java `GameRule<T>` is generic over the value type (`Boolean` or `Integer`),
//! and the registry (`BuiltInRegistries.GAME_RULE`) and the `GameRuleMap` store
//! the erased wildcard `GameRule<?>`. The Rust port mirrors that with
//! [`game_rule::GameRuleErased`] — the same erased-wildcard pattern as
//! `levelgen::feature::ConfiguredFeatureErased` — carrying the value-typed
//! seams as the [`game_rule::GameRuleValue`] union (`Bool`/`Int`), the erased
//! [`game_rule::ArgumentErased`] and [`game_rule::GameRuleValueCodec`], and
//! erased closures for the visitor caller and command-result function.
//!
//! ## Ownership and deferrals
//!
//! - `GameRuleMap extends SavedData` — the SavedData `dirty` flag is embedded
//!   directly; the `SavedDataType<TYPE>` registration (`RivetTodo(#388)`)
//!   defers with the saveddata unit.
//! - `GameRules` (the aggregate: the `rules` field, `get`/`set`/`copy`/
//!   `setAll`/`setFromOther` with the `@Nullable ServerLevel`
//!   `onGameRuleChanged` callback seam, `availableRules`, `getAsString`,
//!   `visitGameRuleTypes`, the enabled-feature-reconciling constructor) defers
//!   with `ServerLevel`/`MinecraftServer` (`rivet-server`); the 59 built-in
//!   rules, the GAME_RULE registry and the `GameRuleMap` CODEC are ported here
//!   (see [`game_rules`]).
//! - `FeatureElement.isEnabled` is inlined on the rule (`requiredFeatures().
//!   isSubsetOf(enabledFeatures)`) — the `FeatureElement` trait defers with
//!   `RivetTodo(#387)`.

pub mod game_rule;
pub mod game_rule_category;
pub mod game_rule_map;
pub mod game_rule_type;
pub mod game_rule_type_visitor;
pub mod game_rules;

pub use game_rule::{
    ArgumentErased, GameRuleErased, GameRuleValue, GameRuleValueCodec, VisitorCaller,
    last_game_rule_index,
};
pub use game_rule_category::{
    CHAT, DROPS, GameRuleCategory, MISC, MOBS, PLAYER, SPAWNING, UPDATES,
};
pub use game_rule_map::{Builder, DispatchedMapCodec, GameRuleMap};
pub use game_rule_type::GameRuleType;
pub use game_rule_type_visitor::GameRuleTypeVisitor;
pub use game_rules::{GAME_RULE, bootstrap, built_in_registry};
