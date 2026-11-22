//! Port of `net.minecraft.util.context.{ContextKey, ContextKeySet, ContextMap}`
//! and `net.minecraft.world.level.storage.loot.parameters.{LootContextParams, LootContextParamSets}`.
//!
//! Java source (Paper 26.2):
//! - `net/minecraft/util/context/ContextKey.java`
//! - `net/minecraft/util/context/ContextKeySet.java`
//! - `net/minecraft/util/context/ContextMap.java`
//! - `net/minecraft/world/level/storage/loot/parameters/LootContextParams.java`
//! - `net/minecraft/world/level/storage/loot/parameters/LootContextParamSets.java`

use std::fmt;
use std::sync::LazyLock;

use indexmap::{IndexMap, IndexSet};
use rivet_registry::Identifier;

use super::item_stack::LootItemStack;

/// `net.minecraft.util.context.ContextKey<T>` — a named loot context parameter key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContextKey {
    name: Identifier,
}

impl ContextKey {
    /// Creates a `ContextKey` with the given identifier.
    #[must_use]
    pub fn new(name: Identifier) -> Self {
        Self { name }
    }

    /// `ContextKey.vanilla(String)` — creates a `minecraft:<name>` key.
    #[must_use]
    pub fn vanilla(name: &str) -> Self {
        Self {
            name: Identifier::with_default_namespace(name),
        }
    }

    /// `ContextKey.name()`.
    #[must_use]
    pub fn name(&self) -> &Identifier {
        &self.name
    }
}

impl fmt::Display for ContextKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<parameter {}>", self.name)
    }
}

/// `net.minecraft.util.context.ContextKeySet` — declares required and allowed
/// [`ContextKey`]s for a loot table or validation pass.
///
/// Uses [`IndexSet`] so iteration order is deterministic and matches insertion order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextKeySet {
    required: IndexSet<ContextKey>,
    allowed: IndexSet<ContextKey>,
}

impl ContextKeySet {
    /// Starts a new [`ContextKeySetBuilder`].
    #[must_use]
    pub fn builder() -> ContextKeySetBuilder {
        ContextKeySetBuilder::default()
    }

    /// `ContextKeySet.required()`.
    #[must_use]
    pub fn required(&self) -> &IndexSet<ContextKey> {
        &self.required
    }

    /// `ContextKeySet.allowed()` (union of required and optional keys).
    #[must_use]
    pub fn allowed(&self) -> &IndexSet<ContextKey> {
        &self.allowed
    }

    /// Returns `true` if `key` is allowed by this parameter set.
    #[must_use]
    pub fn is_allowed(&self, key: &ContextKey) -> bool {
        self.allowed.contains(key)
    }
}

/// Builder for [`ContextKeySet`], mirroring `ContextKeySet.Builder`.
#[derive(Debug, Default)]
pub struct ContextKeySetBuilder {
    required: IndexSet<ContextKey>,
    optional: IndexSet<ContextKey>,
}

impl ContextKeySetBuilder {
    /// Marks `param` as required. Panics if already marked optional (matching Java).
    #[must_use]
    pub fn required(mut self, param: ContextKey) -> Self {
        assert!(
            !self.optional.contains(&param),
            "Parameter {param} is already optional"
        );
        self.required.insert(param);
        self
    }

    /// Marks `param` as optional. Panics if already marked required (matching Java).
    #[must_use]
    pub fn optional(mut self, param: ContextKey) -> Self {
        assert!(
            !self.required.contains(&param),
            "Parameter {param} is already required"
        );
        self.optional.insert(param);
        self
    }

    /// Builds the [`ContextKeySet`].
    #[must_use]
    pub fn build(self) -> ContextKeySet {
        let mut allowed = self.required.clone();
        for key in self.optional {
            allowed.insert(key);
        }
        ContextKeySet {
            required: self.required,
            allowed,
        }
    }
}

/// Typed runtime value stored in a [`ContextMap`].
#[derive(Debug, Clone, PartialEq)]
pub enum ContextValue {
    /// Entity identity / scoreboard holder name (for `THIS_ENTITY`, `ATTACKING_ENTITY`, etc.).
    Entity(String),
    /// Block state identifier (`BLOCK_STATE`).
    BlockState(Identifier),
    /// Block entity identifier (`BLOCK_ENTITY`).
    BlockEntity(Identifier),
    /// Damage source identifier (`DAMAGE_SOURCE`).
    DamageSource(Identifier),
    /// World position (`ORIGIN`).
    Origin([f64; 3]),
    /// Tool item stack (`TOOL`).
    Tool(LootItemStack),
    /// Explosion radius (`EXPLOSION_RADIUS`).
    Float(f32),
    /// Enchantment level (`ENCHANTMENT_LEVEL`).
    Int(i32),
    /// Boolean flag (`ENCHANTMENT_ACTIVE`, `ADDITIONAL_COST_COMPONENT_ALLOWED`).
    Bool(bool),
}

/// Errors produced when validating a [`ContextMap`] against a [`ContextKeySet`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContextMapError {
    #[error("Parameters {missing:?} are not provided in this context")]
    MissingRequired { missing: Vec<ContextKey> },
    #[error("Parameters {extra:?} are not allowed in this context")]
    DisallowedParams { extra: Vec<ContextKey> },
}

/// `net.minecraft.util.context.ContextMap` — immutable parameter map bound to a
/// [`super::LootParams`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextMap {
    params: IndexMap<ContextKey, ContextValue>,
}

impl ContextMap {
    /// Starts a [`ContextMapBuilder`].
    #[must_use]
    pub fn builder() -> ContextMapBuilder {
        ContextMapBuilder::default()
    }

    /// `ContextMap.has(ContextKey)`.
    #[must_use]
    pub fn has(&self, key: &ContextKey) -> bool {
        self.params.contains_key(key)
    }

    /// `ContextMap.getOptional(ContextKey)`.
    #[must_use]
    pub fn get_optional(&self, key: &ContextKey) -> Option<&ContextValue> {
        self.params.get(key)
    }

    /// `ContextMap.getOrThrow(ContextKey)`.
    ///
    /// # Panics
    /// Panics with Java's `NoSuchElementException`-equivalent message if absent.
    #[must_use]
    pub fn get_or_throw(&self, key: &ContextKey) -> &ContextValue {
        self.params
            .get(key)
            .unwrap_or_else(|| panic!("No value for parameter {key}"))
    }

    /// Convenience accessor for a `Float` parameter (e.g. `EXPLOSION_RADIUS`).
    #[must_use]
    pub fn get_float(&self, key: &ContextKey) -> Option<f32> {
        match self.params.get(key) {
            Some(ContextValue::Float(v)) => Some(*v),
            _ => None,
        }
    }

    /// Convenience accessor for an `Int` parameter (e.g. `ENCHANTMENT_LEVEL`).
    #[must_use]
    pub fn get_int(&self, key: &ContextKey) -> Option<i32> {
        match self.params.get(key) {
            Some(ContextValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    /// Convenience accessor for a `Tool` parameter (`TOOL`).
    #[must_use]
    pub fn get_tool(&self, key: &ContextKey) -> Option<&LootItemStack> {
        match self.params.get(key) {
            Some(ContextValue::Tool(stack)) => Some(stack),
            _ => None,
        }
    }

    /// Convenience accessor for an `Entity` holder name parameter.
    #[must_use]
    pub fn get_entity(&self, key: &ContextKey) -> Option<&str> {
        match self.params.get(key) {
            Some(ContextValue::Entity(name)) => Some(name.as_str()),
            _ => None,
        }
    }
}

/// Builder for [`ContextMap`], mirroring `ContextMap.Builder`.
#[derive(Debug, Default, Clone)]
pub struct ContextMapBuilder {
    params: IndexMap<ContextKey, ContextValue>,
}

impl ContextMapBuilder {
    /// `Builder.withParameter(ContextKey, T)`.
    pub fn with_parameter(&mut self, key: ContextKey, value: ContextValue) -> &mut Self {
        self.params.insert(key, value);
        self
    }

    /// `Builder.withOptionalParameter(ContextKey, @Nullable T)`.
    pub fn with_optional_parameter(
        &mut self,
        key: ContextKey,
        value: Option<ContextValue>,
    ) -> &mut Self {
        match value {
            Some(v) => {
                self.params.insert(key, v);
            }
            None => {
                self.params.shift_remove(&key);
            }
        }
        self
    }

    /// `Builder.create(ContextKeySet)` — validates that all required keys are
    /// present and no disallowed keys were supplied.
    pub fn create(self, key_set: &ContextKeySet) -> Result<ContextMap, ContextMapError> {
        let extra: Vec<ContextKey> = self
            .params
            .keys()
            .filter(|k| !key_set.allowed().contains(*k))
            .cloned()
            .collect();
        if !extra.is_empty() {
            return Err(ContextMapError::DisallowedParams { extra });
        }

        let missing: Vec<ContextKey> = key_set
            .required()
            .iter()
            .filter(|k| !self.params.contains_key(*k))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(ContextMapError::MissingRequired { missing });
        }

        Ok(ContextMap {
            params: self.params,
        })
    }
}

/// `net.minecraft.world.level.storage.loot.parameters.LootContextParams` — all
/// 15 vanilla/Paper 26.2 context parameter keys.
pub struct LootContextParams;

impl LootContextParams {
    #[must_use]
    pub fn this_entity() -> ContextKey {
        ContextKey::vanilla("this_entity")
    }
    #[must_use]
    pub fn interacting_entity() -> ContextKey {
        ContextKey::vanilla("interacting_entity")
    }
    #[must_use]
    pub fn target_entity() -> ContextKey {
        ContextKey::vanilla("target_entity")
    }
    #[must_use]
    pub fn last_damage_player() -> ContextKey {
        ContextKey::vanilla("last_damage_player")
    }
    #[must_use]
    pub fn damage_source() -> ContextKey {
        ContextKey::vanilla("damage_source")
    }
    #[must_use]
    pub fn attacking_entity() -> ContextKey {
        ContextKey::vanilla("attacking_entity")
    }
    #[must_use]
    pub fn direct_attacking_entity() -> ContextKey {
        ContextKey::vanilla("direct_attacking_entity")
    }
    #[must_use]
    pub fn origin() -> ContextKey {
        ContextKey::vanilla("origin")
    }
    #[must_use]
    pub fn block_state() -> ContextKey {
        ContextKey::vanilla("block_state")
    }
    #[must_use]
    pub fn block_entity() -> ContextKey {
        ContextKey::vanilla("block_entity")
    }
    #[must_use]
    pub fn tool() -> ContextKey {
        ContextKey::vanilla("tool")
    }
    #[must_use]
    pub fn explosion_radius() -> ContextKey {
        ContextKey::vanilla("explosion_radius")
    }
    #[must_use]
    pub fn enchantment_level() -> ContextKey {
        ContextKey::vanilla("enchantment_level")
    }
    #[must_use]
    pub fn enchantment_active() -> ContextKey {
        ContextKey::vanilla("enchantment_active")
    }
    #[must_use]
    pub fn additional_cost_component_allowed() -> ContextKey {
        ContextKey::vanilla("additional_cost_component_allowed")
    }
}

/// `net.minecraft.world.level.storage.loot.parameters.LootContextParamSets` —
/// the registered parameter sets from Paper 26.2.
pub struct LootContextParamSets;

static REGISTRY: LazyLock<IndexMap<Identifier, ContextKeySet>> = LazyLock::new(|| {
    let mut map = IndexMap::new();
    let mut reg = |name: &str, set: ContextKeySet| {
        map.insert(Identifier::with_default_namespace(name), set);
    };

    reg("empty", ContextKeySet::builder().build());
    reg(
        "chest",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .optional(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "command",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .optional(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "selector",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "villager_trade",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::this_entity())
            .required(LootContextParams::additional_cost_component_allowed())
            .build(),
    );
    reg(
        "fishing",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::tool())
            .optional(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "entity",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::origin())
            .required(LootContextParams::damage_source())
            .optional(LootContextParams::attacking_entity())
            .optional(LootContextParams::direct_attacking_entity())
            .optional(LootContextParams::last_damage_player())
            .build(),
    );
    reg(
        "equipment",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "archaeology",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::this_entity())
            .required(LootContextParams::tool())
            .build(),
    );
    reg(
        "gift",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "barter",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .build(),
    );
    reg(
        "vault",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .optional(LootContextParams::this_entity())
            .optional(LootContextParams::tool())
            .build(),
    );
    reg(
        "advancement_reward",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::origin())
            .build(),
    );
    reg(
        "advancement_entity",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::origin())
            .build(),
    );
    reg(
        "advancement_location",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::origin())
            .required(LootContextParams::tool())
            .required(LootContextParams::block_state())
            .build(),
    );
    reg(
        "block_use",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::origin())
            .required(LootContextParams::block_state())
            .build(),
    );
    reg(
        "generic",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::last_damage_player())
            .required(LootContextParams::damage_source())
            .required(LootContextParams::attacking_entity())
            .required(LootContextParams::direct_attacking_entity())
            .required(LootContextParams::origin())
            .required(LootContextParams::block_state())
            .required(LootContextParams::block_entity())
            .required(LootContextParams::tool())
            .required(LootContextParams::explosion_radius())
            .required(LootContextParams::additional_cost_component_allowed())
            .build(),
    );
    reg(
        "block",
        ContextKeySet::builder()
            .required(LootContextParams::block_state())
            .required(LootContextParams::origin())
            .required(LootContextParams::tool())
            .optional(LootContextParams::this_entity())
            .optional(LootContextParams::block_entity())
            .optional(LootContextParams::explosion_radius())
            .build(),
    );
    reg(
        "shearing",
        ContextKeySet::builder()
            .required(LootContextParams::origin())
            .required(LootContextParams::this_entity())
            .required(LootContextParams::tool())
            .build(),
    );
    reg(
        "entity_interact",
        ContextKeySet::builder()
            .required(LootContextParams::target_entity())
            .optional(LootContextParams::interacting_entity())
            .required(LootContextParams::tool())
            .build(),
    );
    reg(
        "block_interact",
        ContextKeySet::builder()
            .required(LootContextParams::block_state())
            .optional(LootContextParams::block_entity())
            .optional(LootContextParams::interacting_entity())
            .optional(LootContextParams::tool())
            .build(),
    );
    reg(
        "enchanted_damage",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::enchantment_level())
            .required(LootContextParams::origin())
            .required(LootContextParams::damage_source())
            .optional(LootContextParams::direct_attacking_entity())
            .optional(LootContextParams::attacking_entity())
            .build(),
    );
    reg(
        "enchanted_item",
        ContextKeySet::builder()
            .required(LootContextParams::tool())
            .required(LootContextParams::enchantment_level())
            .build(),
    );
    reg(
        "enchanted_location",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::enchantment_level())
            .required(LootContextParams::origin())
            .required(LootContextParams::enchantment_active())
            .build(),
    );
    reg(
        "enchanted_entity",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::enchantment_level())
            .required(LootContextParams::origin())
            .build(),
    );
    reg(
        "hit_block",
        ContextKeySet::builder()
            .required(LootContextParams::this_entity())
            .required(LootContextParams::enchantment_level())
            .required(LootContextParams::origin())
            .required(LootContextParams::block_state())
            .build(),
    );

    map
});

impl LootContextParamSets {
    /// Looks up a registered parameter set by identifier.
    #[must_use]
    pub fn get(id: &Identifier) -> Option<&'static ContextKeySet> {
        REGISTRY.get(id)
    }

    /// Returns all registered parameter sets in deterministic registration order.
    #[must_use]
    pub fn all() -> &'static IndexMap<Identifier, ContextKeySet> {
        &REGISTRY
    }

    #[must_use]
    pub fn empty() -> &'static ContextKeySet {
        &REGISTRY[&Identifier::with_default_namespace("empty")]
    }

    #[must_use]
    pub fn chest() -> &'static ContextKeySet {
        &REGISTRY[&Identifier::with_default_namespace("chest")]
    }

    #[must_use]
    pub fn entity() -> &'static ContextKeySet {
        &REGISTRY[&Identifier::with_default_namespace("entity")]
    }

    #[must_use]
    pub fn block() -> &'static ContextKeySet {
        &REGISTRY[&Identifier::with_default_namespace("block")]
    }

    #[must_use]
    pub fn all_params() -> &'static ContextKeySet {
        &REGISTRY[&Identifier::with_default_namespace("generic")]
    }

    #[must_use]
    pub fn fishing() -> &'static ContextKeySet {
        &REGISTRY[&Identifier::with_default_namespace("fishing")]
    }
}
