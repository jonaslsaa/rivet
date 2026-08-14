//! `net.minecraft.world.level.gamerules.GameRuleCategory` — the game-rule
//! category record and its declaration-order registry (`SORT_ORDER`).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! gamerules/GameRuleCategory.java`.

use rivet_registry::Identifier;
use rivet_text::component::{Component, MutableComponent};
use std::sync::{LazyLock, Mutex};

/// `GameRuleCategory.SORT_ORDER` — the private declaration-order list. Java's
/// static `List` (mutated by the public `register`) becomes a shared
/// `Mutex<Vec>`; the built-in categories are registered through it in
/// declaration order.
static SORT_ORDER: LazyLock<Mutex<Vec<GameRuleCategory>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

/// `GameRuleCategory` (record) — the `Identifier id` component, value-equated
/// like the Java record.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GameRuleCategory {
    id: Identifier,
}

impl GameRuleCategory {
    /// `GameRuleCategory.PLAYER` — `register("player")`.
    pub fn player() -> &'static GameRuleCategory {
        &PLAYER
    }

    /// `GameRuleCategory.MOBS` — `register("mobs")`.
    pub fn mobs() -> &'static GameRuleCategory {
        &MOBS
    }

    /// `GameRuleCategory.SPAWNING` — `register("spawning")`.
    pub fn spawning() -> &'static GameRuleCategory {
        &SPAWNING
    }

    /// `GameRuleCategory.DROPS` — `register("drops")`.
    pub fn drops() -> &'static GameRuleCategory {
        &DROPS
    }

    /// `GameRuleCategory.UPDATES` — `register("updates")`.
    pub fn updates() -> &'static GameRuleCategory {
        &UPDATES
    }

    /// `GameRuleCategory.CHAT` — `register("chat")`.
    pub fn chat() -> &'static GameRuleCategory {
        &CHAT
    }

    /// `GameRuleCategory.MISC` — `register("misc")`.
    pub fn misc() -> &'static GameRuleCategory {
        &MISC
    }

    /// The record accessor `id()`.
    pub fn id(&self) -> &Identifier {
        &self.id
    }

    /// `getDescriptionId()` — the `Identifier` id component itself.
    pub fn get_description_id(&self) -> &Identifier {
        &self.id
    }

    /// `GameRuleCategory.register(Identifier)` — appends to `SORT_ORDER`,
    /// throwing `"Category '<id>' is already registered."` on a duplicate
    /// (mirrored as a panic).
    pub fn register(id: Identifier) -> GameRuleCategory {
        let category = GameRuleCategory { id: id.clone() };
        let mut sort_order = SORT_ORDER.lock().unwrap();
        if sort_order.contains(&category) {
            panic!("Category '{}' is already registered.", id);
        }
        sort_order.push(category.clone());
        category
    }

    /// `label()` — `Component.translatable(this.id.toLanguageKey(
    /// "gamerule.category"))`.
    pub fn label(&self) -> MutableComponent {
        Component::translatable(&self.id.to_language_key_with_prefix("gamerule.category"))
    }
}

/// `GameRuleCategory.PLAYER` — `register("player")`.
pub static PLAYER: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("player")));

/// `GameRuleCategory.MOBS` — `register("mobs")`.
pub static MOBS: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("mobs")));

/// `GameRuleCategory.SPAWNING` — `register("spawning")`.
pub static SPAWNING: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("spawning")));

/// `GameRuleCategory.DROPS` — `register("drops")`.
pub static DROPS: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("drops")));

/// `GameRuleCategory.UPDATES` — `register("updates")`.
pub static UPDATES: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("updates")));

/// `GameRuleCategory.CHAT` — `register("chat")`.
pub static CHAT: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("chat")));

/// `GameRuleCategory.MISC` — `register("misc")`.
pub static MISC: LazyLock<GameRuleCategory> =
    LazyLock::new(|| GameRuleCategory::register(Identifier::with_default_namespace("misc")));
