//! Port of `net.minecraft.world.level.storage.loot.LootTable`.
//!
//! Java source (Paper 26.2):
//! - `net/minecraft/world/level/storage/loot/LootTable.java`

use rivet_registry::Identifier;
use rivet_util::mth;
use rivet_util::random::RandomSource;
use rivet_util::util::shuffle;

use super::context::{LootContext, LootParams, LootRandom, LootRegistry, VisitedEntry};
use super::functions::LootItemFunction;
use super::item_stack::{LootContainer, LootItemStack};
use super::parameters::{ContextKeySet, LootContextParamSets};
use super::pool::LootPool;
use super::validation::{Validatable, ValidationContext, validate_list};

/// `LootTable.RANDOMIZE_SEED` (`0L`).
pub const RANDOMIZE_SEED: i64 = 0;

/// `net.minecraft.world.level.storage.loot.LootTable`.
#[derive(Debug, Clone, PartialEq)]
pub struct LootTable {
    param_set: ContextKeySet,
    random_sequence: Option<Identifier>,
    pools: Vec<LootPool>,
    functions: Vec<LootItemFunction>,
}

impl LootTable {
    /// `LootTable.EMPTY`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            param_set: LootContextParamSets::empty().clone(),
            random_sequence: None,
            pools: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Starts a [`LootTableBuilder`] (`LootTable.lootTable()`).
    #[must_use]
    pub fn builder() -> LootTableBuilder {
        LootTableBuilder::new()
    }

    /// `LootTable.getParamSet()`.
    #[must_use]
    pub fn param_set(&self) -> &ContextKeySet {
        &self.param_set
    }

    /// Optional named random sequence identifier.
    #[must_use]
    pub fn random_sequence(&self) -> Option<&Identifier> {
        self.random_sequence.as_ref()
    }

    /// Pools in list order.
    #[must_use]
    pub fn pools(&self) -> &[LootPool] {
        &self.pools
    }

    /// Table-level item functions in list order.
    #[must_use]
    pub fn functions(&self) -> &[LootItemFunction] {
        &self.functions
    }

    /// `LootTable.createStackSplitter(ServerLevel, Consumer<ItemStack>)`.
    ///
    /// Preserves Paper's `fixes.splitOverstackedLoot` toggle and feature-flag
    /// check (`isItemEnabled`).
    pub fn split_and_emit(
        params: &LootParams,
        result: LootItemStack,
        output: &mut dyn FnMut(LootItemStack),
    ) {
        if !params.is_item_enabled(result.item()) {
            return;
        }
        let skip_splitter = !params.split_overstacked_loot();
        let max_stack = result.max_stack_size().max(1);
        if skip_splitter || result.count() < max_stack {
            output(result);
        } else {
            let mut count = result.count();
            while count > 0 {
                let copy = result.copy_with_count(max_stack.min(count));
                count = count.wrapping_sub(copy.count());
                output(copy);
            }
        }
    }

    /// Internal raw execution with an optional table identifier for
    /// breadcrumb-based cycle detection (when referenced by ID via
    /// [`LootRegistry`], uses [`VisitedEntry::TableId`]; otherwise uses
    /// [`VisitedEntry::TablePtr`]).
    pub(crate) fn get_random_items_raw_by_id(
        &self,
        table_id: Option<&Identifier>,
        context: &mut LootContext<'_>,
        output: &mut dyn FnMut(LootItemStack, &mut LootContext<'_>),
    ) {
        let breadcrumb = match table_id {
            Some(id) => VisitedEntry::TableId(id.clone()),
            None => VisitedEntry::TablePtr((self as *const Self) as usize),
        };

        if context.push_visited_element(breadcrumb.clone()) {
            let functions = &self.functions;
            let mut decorated = |stack: LootItemStack, ctx: &mut LootContext<'_>| {
                let transformed = LootItemFunction::apply_all(functions, stack, ctx);
                output(transformed, ctx);
            };

            for pool in &self.pools {
                pool.add_random_items(&mut decorated, context);
            }

            context.pop_visited_element(&breadcrumb);
        }
    }

    /// `LootTable.getRandomItemsRaw(LootContext, Consumer<ItemStack>)`.
    pub fn get_random_items_raw(
        &self,
        context: &mut LootContext<'_>,
        output: &mut dyn FnMut(LootItemStack),
    ) {
        self.get_random_items_raw_by_id(None, context, &mut |stack, _| output(stack));
    }

    /// `LootTable.getRandomItems(LootContext, Consumer<ItemStack>)`.
    pub fn get_random_items_with_context(
        &self,
        context: &mut LootContext<'_>,
        output: &mut dyn FnMut(LootItemStack),
    ) {
        self.get_random_items_raw_by_id(None, context, &mut |stack, ctx| {
            Self::split_and_emit(ctx.params(), stack, output);
        });
    }

    /// `LootTable.getRandomItems(LootParams, long optionalLootTableSeed, Consumer<ItemStack>)`.
    pub fn get_random_items_with_seed_into(
        &self,
        params: LootParams,
        resolver: &LootRegistry,
        optional_loot_table_seed: i64,
        output: &mut dyn FnMut(LootItemStack),
    ) {
        let mut context = LootContext::builder(params, resolver)
            .with_optional_random_seed(optional_loot_table_seed)
            .create(self.random_sequence.as_ref());
        self.get_random_items_with_context(&mut context, output);
    }

    /// `LootTable.getRandomItems(LootParams, long optionalLootTableSeed)` -> `Vec<LootItemStack>`.
    #[must_use]
    pub fn get_random_items_with_seed(
        &self,
        params: LootParams,
        resolver: &LootRegistry,
        optional_loot_table_seed: i64,
    ) -> Vec<LootItemStack> {
        let mut result = Vec::new();
        self.get_random_items_with_seed_into(
            params,
            resolver,
            optional_loot_table_seed,
            &mut |stack| result.push(stack),
        );
        result
    }

    /// `LootTable.getRandomItems(LootParams, RandomSource)` -> `Vec<LootItemStack>`.
    #[must_use]
    pub fn get_random_items_with_random(
        &self,
        params: LootParams,
        resolver: &LootRegistry,
        random_source: LootRandom,
    ) -> Vec<LootItemStack> {
        let mut context = LootContext::builder(params, resolver)
            .with_optional_random_source(Some(random_source))
            .create(self.random_sequence.as_ref());
        let mut result = Vec::new();
        self.get_random_items_with_context(&mut context, &mut |stack| result.push(stack));
        result
    }

    /// `LootTable.getRandomItems(LootParams)` -> `Vec<LootItemStack>`.
    #[must_use]
    pub fn get_random_items(
        &self,
        params: LootParams,
        resolver: &LootRegistry,
    ) -> Vec<LootItemStack> {
        let mut context =
            LootContext::builder(params, resolver).create(self.random_sequence.as_ref());
        let mut result = Vec::new();
        self.get_random_items_with_context(&mut context, &mut |stack| result.push(stack));
        result
    }

    /// `LootTable.fill(Container, LootParams, long optionalRandomSeed)` —
    /// rolls items, collects and shuffles available empty slots, runs
    /// `shuffleAndSplitItems`, and places stacks from the tail of `available_slots`.
    pub fn fill(
        &self,
        container: &mut dyn LootContainer,
        params: LootParams,
        resolver: &LootRegistry,
        optional_random_seed: i64,
    ) {
        let mut context = LootContext::builder(params, resolver)
            .with_optional_random_seed(optional_random_seed)
            .create(self.random_sequence.as_ref());
        self.fill_with_context(container, &mut context);
    }

    /// `LootTable.fill(Container, LootParams, RandomSource)` using an explicit [`LootContext`].
    pub fn fill_with_context(
        &self,
        container: &mut dyn LootContainer,
        context: &mut LootContext<'_>,
    ) {
        let mut item_stacks = Vec::new();
        self.get_random_items_with_context(context, &mut |stack| item_stacks.push(stack));

        let random = context.random_mut();
        let mut available_slots = Self::get_available_slots(container, random);
        Self::shuffle_and_split_items(&mut item_stacks, available_slots.len(), random);

        for item_stack in item_stacks {
            let Some(slot) = available_slots.pop() else {
                return;
            };
            if item_stack.is_empty() {
                container.set_item(slot, LootItemStack::empty());
            } else {
                container.set_item(slot, item_stack);
            }
        }
    }

    /// `LootTable.shuffleAndSplitItems(ObjectArrayList<ItemStack>, int availableSlots, RandomSource)`.
    ///
    /// Preserves exact Paper RNG call ordering:
    /// 1. Partition out empty stacks (discarded) and stacks with `count > 1` (`splittable_items`).
    /// 2. While `available_slots - result.len() - splittable_items.len() > 0 && !splittable_items.is_empty()`:
    ///    - `idx = Mth.nextInt(random, 0, splittable_items.len() - 1)`
    ///    - `remove = Mth.nextInt(random, 1, item_stack.count() / 2)`
    ///    - `copy = item_stack.split(remove)`
    ///    - If `item_stack.count() > 1 && random.next_boolean()` -> push to `splittable_items`, else `result`.
    ///    - If `copy.count() > 1 && random.next_boolean()` -> push to `splittable_items`, else `result`.
    /// 3. Append remaining `splittable_items` to `result` and `Util.shuffle(result, random)`.
    pub fn shuffle_and_split_items<R: RandomSource>(
        result: &mut Vec<LootItemStack>,
        available_slots: usize,
        random: &mut R,
    ) {
        let mut splittable_items: Vec<LootItemStack> = Vec::new();
        let mut kept: Vec<LootItemStack> = Vec::with_capacity(result.len());

        for stack in result.drain(..) {
            if stack.is_empty() {
                continue;
            } else if stack.count() > 1 {
                splittable_items.push(stack);
            } else {
                kept.push(stack);
            }
        }
        *result = kept;

        while (available_slots as isize)
            - (result.len() as isize)
            - (splittable_items.len() as isize)
            > 0
            && !splittable_items.is_empty()
        {
            let pick = mth::next_int(random, 0, (splittable_items.len() - 1) as i32) as usize;
            let mut item_stack = splittable_items.remove(pick);
            let remove = mth::next_int(random, 1, item_stack.count() / 2);
            let copy = item_stack.split(remove);

            if item_stack.count() > 1 && random.next_boolean() {
                splittable_items.push(item_stack);
            } else {
                result.push(item_stack);
            }

            if copy.count() > 1 && random.next_boolean() {
                splittable_items.push(copy);
            } else {
                result.push(copy);
            }
        }

        result.append(&mut splittable_items);
        shuffle(result, random);
    }

    /// `LootTable.getAvailableSlots(Container, RandomSource)`.
    fn get_available_slots<R: RandomSource>(
        container: &dyn LootContainer,
        random: &mut R,
    ) -> Vec<usize> {
        let mut slots = Vec::new();
        for i in 0..container.container_size() {
            if container.item(i).is_empty() {
                slots.push(i);
            }
        }
        shuffle(&mut slots, random);
        slots
    }
}

impl Validatable for LootTable {
    /// `LootTable.validate(ValidationContext)` — validates `pools` then `functions`.
    fn validate(&self, context: &ValidationContext<'_>) {
        validate_list(context, "pools", &self.pools);
        validate_list(context, "functions", &self.functions);
    }
}

/// Builder for [`LootTable`], mirroring `LootTable.Builder`.
#[derive(Debug, Clone)]
pub struct LootTableBuilder {
    pools: Vec<LootPool>,
    functions: Vec<LootItemFunction>,
    param_set: ContextKeySet,
    random_sequence: Option<Identifier>,
}

impl Default for LootTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl LootTableBuilder {
    /// Creates a builder with `DEFAULT_PARAM_SET = LootContextParamSets.ALL_PARAMS`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pools: Vec::new(),
            functions: Vec::new(),
            param_set: LootContextParamSets::all_params().clone(),
            random_sequence: None,
        }
    }

    /// `Builder.withPool(LootPool)`.
    #[must_use]
    pub fn with_pool(mut self, pool: LootPool) -> Self {
        self.pools.push(pool);
        self
    }

    /// `Builder.setParamSet(ContextKeySet)`.
    #[must_use]
    pub fn set_param_set(mut self, param_set: ContextKeySet) -> Self {
        self.param_set = param_set;
        self
    }

    /// `Builder.setRandomSequence(Identifier)`.
    #[must_use]
    pub fn set_random_sequence(mut self, id: Identifier) -> Self {
        self.random_sequence = Some(id);
        self
    }

    /// `Builder.apply(LootItemFunction)`.
    #[must_use]
    pub fn apply(mut self, function: LootItemFunction) -> Self {
        self.functions.push(function);
        self
    }

    /// Builds the [`LootTable`].
    #[must_use]
    pub fn build(self) -> LootTable {
        LootTable {
            param_set: self.param_set,
            random_sequence: self.random_sequence,
            pools: self.pools,
            functions: self.functions,
        }
    }
}
