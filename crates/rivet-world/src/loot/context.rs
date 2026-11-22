//! Port of `net.minecraft.world.level.storage.loot.{LootParams, LootContext}`
//! and the loot registry/resolver surface.
//!
//! Java source (Paper 26.2):
//! - `net/minecraft/world/level/storage/loot/LootParams.java`
//! - `net/minecraft/world/level/storage/loot/LootContext.java`

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use rivet_registry::Identifier;
use rivet_util::random::{
    LegacyPositionalRandomFactory, LegacyRandomSource, PositionalRandomFactory, RandomSource,
    SingleThreadedRandomSource, XoroshiroPositionalRandomFactory, XoroshiroRandomSource,
};

use super::functions::LootItemFunction;
use super::item_stack::{DEFAULT_MAX_STACK_SIZE, LootItemStack};
use super::parameters::{
    ContextKey, ContextKeySet, ContextMap, ContextMapBuilder, ContextMapError, ContextValue,
};
use super::predicates::LootItemCondition;
use super::table::LootTable;

/// Dynamic drop callback (`LootParams.DynamicDrop`).
pub type DynamicDropFn = Rc<dyn Fn(&mut dyn FnMut(LootItemStack))>;

/// Polymorphic Paper-faithful [`RandomSource`] used by [`LootContext`].
///
/// Wraps the exact RNG implementations from `rivet_util::random` without
/// dynamic dispatch overhead on hot calls.
pub enum LootRandom {
    Legacy(LegacyRandomSource),
    Xoroshiro(XoroshiroRandomSource),
    SingleThreaded(SingleThreadedRandomSource),
    /// Shared tick-thread-owned RNG stream (used for persistent named random
    /// sequences and the level's default RNG so successive unseeded rolls
    /// advance the stream in place, matching Paper's `RandomSequences` and
    /// `ServerLevel.getRandom()`).
    Shared(Rc<RefCell<LootRandom>>),
}

impl std::fmt::Debug for LootRandom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy(_) => f.write_str("LootRandom::Legacy(..)"),
            Self::Xoroshiro(_) => f.write_str("LootRandom::Xoroshiro(..)"),
            Self::SingleThreaded(_) => f.write_str("LootRandom::SingleThreaded(..)"),
            Self::Shared(_) => f.write_str("LootRandom::Shared(..)"),
        }
    }
}

/// Seed specification for a named random sequence registered in [`LootRegistry`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomSequenceSeed {
    Legacy(i64),
    Xoroshiro(i64),
    Xoroshiro128(i64, i64),
    SingleThreaded(i64),
}

impl RandomSequenceSeed {
    /// Instantiates a fresh [`LootRandom`] from this seed specification.
    #[must_use]
    pub fn instantiate(self) -> LootRandom {
        match self {
            Self::Legacy(seed) => LootRandom::Legacy(LegacyRandomSource::new(seed)),
            Self::Xoroshiro(seed) => LootRandom::Xoroshiro(XoroshiroRandomSource::new(seed)),
            Self::Xoroshiro128(lo, hi) => {
                LootRandom::Xoroshiro(XoroshiroRandomSource::new_lo_hi(lo, hi))
            }
            Self::SingleThreaded(seed) => {
                LootRandom::SingleThreaded(SingleThreadedRandomSource::new(seed))
            }
        }
    }
}

/// Polymorphic [`PositionalRandomFactory`] paired with [`LootRandom`].
pub enum LootPositionalRandomFactory {
    Legacy(LegacyPositionalRandomFactory),
    Xoroshiro(XoroshiroPositionalRandomFactory),
}

impl std::fmt::Debug for LootPositionalRandomFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Legacy(_) => f.write_str("LootPositionalRandomFactory::Legacy(..)"),
            Self::Xoroshiro(_) => f.write_str("LootPositionalRandomFactory::Xoroshiro(..)"),
        }
    }
}

impl PositionalRandomFactory for LootPositionalRandomFactory {
    type Output = LootRandom;

    fn at(&self, x: i32, y: i32, z: i32) -> Self::Output {
        match self {
            Self::Legacy(f) => LootRandom::Legacy(f.at(x, y, z)),
            Self::Xoroshiro(f) => LootRandom::Xoroshiro(f.at(x, y, z)),
        }
    }

    fn from_hash_of(&self, name: &str) -> Self::Output {
        match self {
            Self::Legacy(f) => LootRandom::Legacy(f.from_hash_of(name)),
            Self::Xoroshiro(f) => LootRandom::Xoroshiro(f.from_hash_of(name)),
        }
    }

    fn from_seed(&self, seed: i64) -> Self::Output {
        match self {
            Self::Legacy(f) => LootRandom::Legacy(f.from_seed(seed)),
            Self::Xoroshiro(f) => LootRandom::Xoroshiro(f.from_seed(seed)),
        }
    }

    fn parity_config_string(&self, out: &mut String) {
        match self {
            Self::Legacy(f) => f.parity_config_string(out),
            Self::Xoroshiro(f) => f.parity_config_string(out),
        }
    }
}

impl RandomSource for LootRandom {
    type Positional = LootPositionalRandomFactory;

    fn fork(&mut self) -> Self {
        match self {
            Self::Legacy(r) => Self::Legacy(r.fork()),
            Self::Xoroshiro(r) => Self::Xoroshiro(r.fork()),
            Self::SingleThreaded(r) => Self::SingleThreaded(r.fork()),
            Self::Shared(r) => r.borrow_mut().fork(),
        }
    }

    fn fork_positional(&mut self) -> Self::Positional {
        match self {
            Self::Legacy(r) => LootPositionalRandomFactory::Legacy(r.fork_positional()),
            Self::Xoroshiro(r) => LootPositionalRandomFactory::Xoroshiro(r.fork_positional()),
            Self::SingleThreaded(r) => LootPositionalRandomFactory::Legacy(r.fork_positional()),
            Self::Shared(r) => r.borrow_mut().fork_positional(),
        }
    }

    fn set_seed(&mut self, seed: i64) {
        match self {
            Self::Legacy(r) => r.set_seed(seed),
            Self::Xoroshiro(r) => r.set_seed(seed),
            Self::SingleThreaded(r) => r.set_seed(seed),
            Self::Shared(r) => r.borrow_mut().set_seed(seed),
        }
    }

    fn next_int(&mut self) -> i32 {
        match self {
            Self::Legacy(r) => r.next_int(),
            Self::Xoroshiro(r) => r.next_int(),
            Self::SingleThreaded(r) => r.next_int(),
            Self::Shared(r) => r.borrow_mut().next_int(),
        }
    }

    fn next_int_bound(&mut self, bound: i32) -> i32 {
        match self {
            Self::Legacy(r) => r.next_int_bound(bound),
            Self::Xoroshiro(r) => r.next_int_bound(bound),
            Self::SingleThreaded(r) => r.next_int_bound(bound),
            Self::Shared(r) => r.borrow_mut().next_int_bound(bound),
        }
    }

    fn next_long(&mut self) -> i64 {
        match self {
            Self::Legacy(r) => r.next_long(),
            Self::Xoroshiro(r) => r.next_long(),
            Self::SingleThreaded(r) => r.next_long(),
            Self::Shared(r) => r.borrow_mut().next_long(),
        }
    }

    fn next_boolean(&mut self) -> bool {
        match self {
            Self::Legacy(r) => r.next_boolean(),
            Self::Xoroshiro(r) => r.next_boolean(),
            Self::SingleThreaded(r) => r.next_boolean(),
            Self::Shared(r) => r.borrow_mut().next_boolean(),
        }
    }

    fn next_float(&mut self) -> f32 {
        match self {
            Self::Legacy(r) => r.next_float(),
            Self::Xoroshiro(r) => r.next_float(),
            Self::SingleThreaded(r) => r.next_float(),
            Self::Shared(r) => r.borrow_mut().next_float(),
        }
    }

    fn next_double(&mut self) -> f64 {
        match self {
            Self::Legacy(r) => r.next_double(),
            Self::Xoroshiro(r) => r.next_double(),
            Self::SingleThreaded(r) => r.next_double(),
            Self::Shared(r) => r.borrow_mut().next_double(),
        }
    }

    fn next_gaussian(&mut self) -> f64 {
        match self {
            Self::Legacy(r) => r.next_gaussian(),
            Self::Xoroshiro(r) => r.next_gaussian(),
            Self::SingleThreaded(r) => r.next_gaussian(),
            Self::Shared(r) => r.borrow_mut().next_gaussian(),
        }
    }
}

/// Registry/resolver providing loot tables, predicates, item modifiers, item
/// tags, and named random sequences in deterministic insertion order.
#[derive(Debug, Clone)]
pub struct LootRegistry {
    tables: IndexMap<Identifier, LootTable>,
    conditions: IndexMap<Identifier, LootItemCondition>,
    functions: IndexMap<Identifier, LootItemFunction>,
    item_tags: IndexMap<Identifier, Vec<Identifier>>,
    item_max_stack_sizes: IndexMap<Identifier, i32>,
    random_sequences: IndexMap<Identifier, Rc<RefCell<LootRandom>>>,
    default_random: Rc<RefCell<LootRandom>>,
}

impl Default for LootRegistry {
    fn default() -> Self {
        Self {
            tables: IndexMap::new(),
            conditions: IndexMap::new(),
            functions: IndexMap::new(),
            item_tags: IndexMap::new(),
            item_max_stack_sizes: IndexMap::new(),
            random_sequences: IndexMap::new(),
            default_random: Rc::new(RefCell::new(LootRandom::Legacy(LegacyRandomSource::new(0)))),
        }
    }
}

impl LootRegistry {
    /// Creates an empty [`LootRegistry`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a loot table by identifier.
    pub fn register_table(&mut self, id: Identifier, table: LootTable) -> &mut Self {
        self.tables.insert(id, table);
        self
    }

    /// Registers a loot condition (predicate) by identifier.
    pub fn register_condition(
        &mut self,
        id: Identifier,
        condition: LootItemCondition,
    ) -> &mut Self {
        self.conditions.insert(id, condition);
        self
    }

    /// Registers a loot function (item modifier) by identifier.
    pub fn register_function(&mut self, id: Identifier, function: LootItemFunction) -> &mut Self {
        self.functions.insert(id, function);
        self
    }

    /// Registers an ordered item tag list for [`super::entries::LootPoolEntryContainer::Tag`].
    pub fn register_item_tag(&mut self, tag: Identifier, items: Vec<Identifier>) -> &mut Self {
        self.item_tags.insert(tag, items);
        self
    }

    /// Registers a per-item maximum stack size (`Item.getDefaultMaxStackSize()`),
    /// e.g. `16` for `minecraft:ender_pearl` or `1` for unstackable equipment.
    pub fn register_item_max_stack_size(
        &mut self,
        item: Identifier,
        max_stack_size: i32,
    ) -> &mut Self {
        self.item_max_stack_sizes.insert(item, max_stack_size);
        self
    }

    /// Registers a named random sequence (`ServerLevel.getRandomSequence(Identifier)`).
    ///
    /// The sequence's RNG state persists and advances across successive unseeded
    /// loot rolls, matching Paper's `RandomSequences`.
    pub fn register_random_sequence(
        &mut self,
        id: Identifier,
        seed: RandomSequenceSeed,
    ) -> &mut Self {
        self.random_sequences
            .insert(id, Rc::new(RefCell::new(seed.instantiate())));
        self
    }

    /// Replaces the fallback level random stream (`ServerLevel.getRandom()`).
    pub fn set_default_random(&mut self, random: LootRandom) -> &mut Self {
        self.default_random = Rc::new(RefCell::new(random));
        self
    }

    /// Looks up a loot table by identifier.
    #[must_use]
    pub fn get_table(&self, id: &Identifier) -> Option<&LootTable> {
        self.tables.get(id)
    }

    /// Looks up a condition by identifier.
    #[must_use]
    pub fn get_condition(&self, id: &Identifier) -> Option<&LootItemCondition> {
        self.conditions.get(id)
    }

    /// Looks up a function by identifier.
    #[must_use]
    pub fn get_function(&self, id: &Identifier) -> Option<&LootItemFunction> {
        self.functions.get(id)
    }

    /// Looks up an item tag's items in deterministic order.
    #[must_use]
    pub fn get_item_tag(&self, tag: &Identifier) -> Option<&[Identifier]> {
        self.item_tags.get(tag).map(Vec::as_slice)
    }

    /// Returns the registered max stack size for `item`, defaulting to
    /// [`DEFAULT_MAX_STACK_SIZE`] (`64`).
    #[must_use]
    pub fn item_max_stack_size(&self, item: &Identifier) -> i32 {
        self.item_max_stack_sizes
            .get(item)
            .copied()
            .unwrap_or(DEFAULT_MAX_STACK_SIZE)
    }

    /// Returns a handle to a registered random sequence whose state advances in
    /// place across successive lookups, matching `ServerLevel.getRandomSequence`.
    #[must_use]
    pub fn get_random_sequence(&self, id: &Identifier) -> Option<LootRandom> {
        self.random_sequences
            .get(id)
            .map(|seq| LootRandom::Shared(Rc::clone(seq)))
    }

    /// Returns a handle to the persistent fallback level random stream
    /// (`ServerLevel.getRandom()`).
    #[must_use]
    pub fn default_random(&self) -> LootRandom {
        LootRandom::Shared(Rc::clone(&self.default_random))
    }
}

/// `net.minecraft.world.level.storage.loot.LootParams`.
#[derive(Clone)]
pub struct LootParams {
    params: ContextMap,
    dynamic_drops: IndexMap<Identifier, DynamicDropFn>,
    luck: f32,
    split_overstacked_loot: bool,
    disabled_items: IndexSet<Identifier>,
    scores: IndexMap<(String, String), i32>,
}

impl std::fmt::Debug for LootParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LootParams")
            .field("params", &self.params)
            .field(
                "dynamic_drops",
                &self.dynamic_drops.keys().collect::<Vec<_>>(),
            )
            .field("luck", &self.luck)
            .field("split_overstacked_loot", &self.split_overstacked_loot)
            .field("disabled_items", &self.disabled_items)
            .field("scores", &self.scores)
            .finish()
    }
}

impl LootParams {
    /// Starts a [`LootParamsBuilder`].
    #[must_use]
    pub fn builder() -> LootParamsBuilder {
        LootParamsBuilder::new()
    }

    /// `LootParams.contextMap()`.
    #[must_use]
    pub fn params(&self) -> &ContextMap {
        &self.params
    }

    /// `LootParams.getLuck()`.
    #[must_use]
    pub fn luck(&self) -> f32 {
        self.luck
    }

    /// Paper `GlobalConfiguration.get().misc.splitOverstackedLoot` (default `true`).
    #[must_use]
    pub fn split_overstacked_loot(&self) -> bool {
        self.split_overstacked_loot
    }

    /// `ItemStack.isItemEnabled(level.enabledFeatures())`.
    #[must_use]
    pub fn is_item_enabled(&self, item: &Identifier) -> bool {
        !self.disabled_items.contains(item)
    }

    /// `LootParams.addDynamicDrops(Identifier, Consumer<ItemStack>)`.
    pub fn add_dynamic_drops(&self, location: &Identifier, output: &mut dyn FnMut(LootItemStack)) {
        if let Some(drop_fn) = self.dynamic_drops.get(location) {
            drop_fn(output);
        }
    }

    /// Scoreboard score lookup for [`super::providers::NumberProvider::Score`].
    #[must_use]
    pub fn get_score(&self, holder: &str, objective: &str) -> Option<i32> {
        self.scores
            .get(&(holder.to_string(), objective.to_string()))
            .copied()
    }
}

/// Builder for [`LootParams`], mirroring `LootParams.Builder`.
pub struct LootParamsBuilder {
    params: ContextMapBuilder,
    dynamic_drops: IndexMap<Identifier, DynamicDropFn>,
    luck: f32,
    split_overstacked_loot: bool,
    disabled_items: IndexSet<Identifier>,
    scores: IndexMap<(String, String), i32>,
}

impl Default for LootParamsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LootParamsBuilder {
    /// Creates a new builder with `luck = 0.0` and `split_overstacked_loot = true`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            params: ContextMap::builder(),
            dynamic_drops: IndexMap::new(),
            luck: 0.0,
            split_overstacked_loot: true,
            disabled_items: IndexSet::new(),
            scores: IndexMap::new(),
        }
    }

    /// `Builder.withParameter(ContextKey, T)`.
    pub fn with_parameter(&mut self, key: ContextKey, value: ContextValue) -> &mut Self {
        self.params.with_parameter(key, value);
        self
    }

    /// `Builder.withOptionalParameter(ContextKey, @Nullable T)`.
    pub fn with_optional_parameter(
        &mut self,
        key: ContextKey,
        value: Option<ContextValue>,
    ) -> &mut Self {
        self.params.with_optional_parameter(key, value);
        self
    }

    /// `Builder.withLuck(float)`.
    pub fn with_luck(&mut self, luck: f32) -> &mut Self {
        self.luck = luck;
        self
    }

    /// `Builder.withDynamicDrop(Identifier, DynamicDrop)` — returns `Err` if
    /// `name` was already registered (matching Paper's `IllegalStateException`).
    pub fn with_dynamic_drop(
        &mut self,
        name: Identifier,
        drop: DynamicDropFn,
    ) -> Result<&mut Self, String> {
        if self.dynamic_drops.contains_key(&name) {
            return Err(format!("Duplicate dynamic drop '{name}'"));
        }
        self.dynamic_drops.insert(name, drop);
        Ok(self)
    }

    /// Configures Paper's `splitOverstackedLoot` setting (default `true`).
    pub fn with_split_overstacked_loot(&mut self, split: bool) -> &mut Self {
        self.split_overstacked_loot = split;
        self
    }

    /// Marks an item identifier as disabled by feature flags.
    pub fn with_disabled_item(&mut self, item: Identifier) -> &mut Self {
        self.disabled_items.insert(item);
        self
    }

    /// Sets a scoreboard score for `(holder, objective)` used by `ScoreboardValue`.
    pub fn with_score(
        &mut self,
        holder: impl Into<String>,
        objective: impl Into<String>,
        score: i32,
    ) -> &mut Self {
        self.scores.insert((holder.into(), objective.into()), score);
        self
    }

    /// `Builder.create(ContextKeySet)`.
    pub fn create(self, key_set: &ContextKeySet) -> Result<LootParams, ContextMapError> {
        let params = self.params.create(key_set)?;
        Ok(LootParams {
            params,
            dynamic_drops: self.dynamic_drops,
            luck: self.luck,
            split_overstacked_loot: self.split_overstacked_loot,
            disabled_items: self.disabled_items,
            scores: self.scores,
        })
    }
}

/// `LootContext.VisitedEntry<?>` — runtime recursion breadcrumb keyed by
/// element kind and identity (table pointer/id, condition id, or function id).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum VisitedEntry {
    TablePtr(usize),
    TableId(Identifier),
    ConditionId(Identifier),
    FunctionId(Identifier),
}

/// `net.minecraft.world.level.storage.loot.LootContext`.
pub struct LootContext<'a> {
    params: LootParams,
    random: LootRandom,
    resolver: &'a LootRegistry,
    visited_elements: IndexSet<VisitedEntry>,
}

impl<'a> LootContext<'a> {
    /// Starts a [`LootContextBuilder`] with `params` and `resolver`.
    #[must_use]
    pub fn builder(params: LootParams, resolver: &'a LootRegistry) -> LootContextBuilder<'a> {
        LootContextBuilder {
            params,
            resolver,
            random: None,
        }
    }

    /// `LootContext.hasParameter(ContextKey)`.
    #[must_use]
    pub fn has_parameter(&self, key: &ContextKey) -> bool {
        self.params.params().has(key)
    }

    /// `LootContext.getOptionalParameter(ContextKey)`.
    #[must_use]
    pub fn get_optional_parameter(&self, key: &ContextKey) -> Option<&ContextValue> {
        self.params.params().get_optional(key)
    }

    /// `LootContext.getParameter(ContextKey)`.
    #[must_use]
    pub fn get_parameter(&self, key: &ContextKey) -> &ContextValue {
        self.params.params().get_or_throw(key)
    }

    /// Underlying [`LootParams`].
    #[must_use]
    pub fn params(&self) -> &LootParams {
        &self.params
    }

    /// `LootContext.getLuck()`.
    #[must_use]
    pub fn luck(&self) -> f32 {
        self.params.luck()
    }

    /// `LootContext.getRandom()` (mutable borrow for RNG consumption).
    pub fn random_mut(&mut self) -> &mut LootRandom {
        &mut self.random
    }

    /// `LootContext.getResolver()`.
    #[must_use]
    pub fn resolver(&self) -> &'a LootRegistry {
        self.resolver
    }

    /// `LootContext.addDynamicDrops(Identifier, Consumer<ItemStack>)`.
    pub fn add_dynamic_drops(&self, location: &Identifier, output: &mut dyn FnMut(LootItemStack)) {
        self.params.add_dynamic_drops(location, output);
    }

    /// `LootContext.pushVisitedElement(VisitedEntry)` — returns `true` if newly
    /// inserted, `false` if already active (cycle detected).
    pub fn push_visited_element(&mut self, entry: VisitedEntry) -> bool {
        self.visited_elements.insert(entry)
    }

    /// `LootContext.popVisitedElement(VisitedEntry)`.
    pub fn pop_visited_element(&mut self, entry: &VisitedEntry) {
        self.visited_elements.shift_remove(entry);
    }

    /// `LootContext.hasVisitedElement(VisitedEntry)`.
    #[must_use]
    pub fn has_visited_element(&self, entry: &VisitedEntry) -> bool {
        self.visited_elements.contains(entry)
    }
}

/// Builder for [`LootContext`], mirroring `LootContext.Builder`.
pub struct LootContextBuilder<'a> {
    params: LootParams,
    resolver: &'a LootRegistry,
    random: Option<LootRandom>,
}

impl<'a> LootContextBuilder<'a> {
    /// `Builder.withOptionalRandomSeed(long seed)` — seed `0` leaves the
    /// existing random source untouched; nonzero creates `LegacyRandomSource::new(seed)`.
    #[must_use]
    pub fn with_optional_random_seed(mut self, seed: i64) -> Self {
        if seed != 0 {
            self.random = Some(LootRandom::Legacy(LegacyRandomSource::new(seed)));
        }
        self
    }

    /// `Builder.withOptionalRandomSource(RandomSource)`.
    #[must_use]
    pub fn with_optional_random_source(mut self, random: Option<LootRandom>) -> Self {
        if let Some(r) = random {
            self.random = Some(r);
        }
        self
    }

    /// `Builder.withOptionalRandomSeed(long, RandomSource)` — Paper overload:
    /// nonzero seed wins, otherwise uses fallback random source.
    #[must_use]
    pub fn with_optional_random_seed_or_source(mut self, seed: i64, fallback: LootRandom) -> Self {
        if seed == 0 {
            self.random = Some(fallback);
        } else {
            self.random = Some(LootRandom::Legacy(LegacyRandomSource::new(seed)));
        }
        self
    }

    /// `Builder.create(Optional<Identifier> randomSequenceKey)` — Paper's exact
    /// random selection precedence:
    /// 1. Explicit random source (from seed or source setter).
    /// 2. Named random sequence from `resolver` if `random_sequence_key` is `Some`.
    /// 3. Fallback `default_random` (representing `level.getRandom()`).
    #[must_use]
    pub fn create_with_default(
        self,
        random_sequence_key: Option<&Identifier>,
        default_random: LootRandom,
    ) -> LootContext<'a> {
        let random = self
            .random
            .or_else(|| random_sequence_key.and_then(|id| self.resolver.get_random_sequence(id)))
            .unwrap_or(default_random);

        LootContext {
            params: self.params,
            random,
            resolver: self.resolver,
            visited_elements: IndexSet::new(),
        }
    }

    /// `Builder.create(Optional<Identifier>)` falling back to `resolver.default_random()`
    /// (`ServerLevel.getRandom()`) so successive unseeded rolls advance the stream.
    #[must_use]
    pub fn create(self, random_sequence_key: Option<&Identifier>) -> LootContext<'a> {
        let default_random = self.resolver.default_random();
        self.create_with_default(random_sequence_key, default_random)
    }
}
