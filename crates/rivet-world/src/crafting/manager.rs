//! `RecipeMap`, `RecipePropertySet`, `SelectableRecipe`, `RecipeManager`,
//! `CachedCheck`, and `RecipeCache`.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.item.crafting.RecipeMap`
//! - `net.minecraft.world.item.crafting.RecipePropertySet`
//! - `net.minecraft.world.item.crafting.SelectableRecipe`
//! - `net.minecraft.world.item.crafting.RecipeManager`
//! - `net.minecraft.world.item.crafting.RecipeCache`

use std::collections::BTreeMap;

use indexmap::{IndexMap, IndexSet};
use rivet_registry::Identifier;
use thiserror::Error;

use super::display::{RecipeDisplayEntry, RecipeDisplayId, SlotDisplay};
use super::ingredient::Ingredient;
use super::input::{CraftingInput, RecipeInput, SingleRecipeInput, SmithingRecipeInput};
use super::item::{ItemId, ItemStack};
use super::recipe_type::RecipeType;
use super::recipes::{CookingKind, Recipe, RecipeHolder, RecipeKey};

/// Errors returned by `RecipeMap` / `RecipeManager` operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecipeMapError {
    #[error("Duplicate recipe id: {0}")]
    DuplicateId(String),
}

/// `net.minecraft.world.item.crafting.RecipePropertySet`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RecipePropertySet {
    items: IndexSet<ItemId>,
}

impl RecipePropertySet {
    /// Standard vanilla `RecipePropertySet` keys (`RecipePropertySet.java`).
    pub const SMITHING_BASE: &'static str = "smithing_base";
    pub const SMITHING_TEMPLATE: &'static str = "smithing_template";
    pub const SMITHING_ADDITION: &'static str = "smithing_addition";
    pub const FURNACE_INPUT: &'static str = "furnace_input";
    pub const BLAST_FURNACE_INPUT: &'static str = "blast_furnace_input";
    pub const SMOKER_INPUT: &'static str = "smoker_input";
    pub const CAMPFIRE_INPUT: &'static str = "campfire_input";

    pub fn empty() -> Self {
        Self {
            items: IndexSet::new(),
        }
    }

    /// `RecipePropertySet.create(Collection<Ingredient>)`.
    pub fn create<'a, I>(ingredients: I) -> Self
    where
        I: IntoIterator<Item = &'a Ingredient>,
    {
        let mut items = IndexSet::new();
        for ing in ingredients {
            for item in ing.items() {
                items.insert(item);
            }
        }
        Self { items }
    }

    /// `RecipePropertySet.test(ItemStack)`.
    pub fn test(&self, stack: &ItemStack) -> bool {
        !stack.is_empty() && self.items.contains(&stack.item())
    }

    pub fn items(&self) -> &IndexSet<ItemId> {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// `net.minecraft.world.item.crafting.SelectableRecipe`.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectableRecipe {
    pub option_display: SlotDisplay,
    pub recipe: Option<RecipeHolder>,
}

/// `net.minecraft.world.item.crafting.SelectableRecipe.SingleInputEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct SingleInputEntry {
    pub input: Ingredient,
    pub recipe: SelectableRecipe,
}

/// `net.minecraft.world.item.crafting.SelectableRecipe.SingleInputSet`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SingleInputSet {
    entries: Vec<SingleInputEntry>,
}

impl SingleInputSet {
    pub const fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn new(entries: Vec<SingleInputEntry>) -> Self {
        Self { entries }
    }

    /// `SingleInputSet.acceptsInput(ItemStack)`.
    pub fn accepts_input(&self, input: &ItemStack) -> bool {
        self.entries.iter().any(|e| e.input.test(input))
    }

    /// `SingleInputSet.selectByInput(ItemStack)` — preserves entry order.
    pub fn select_by_input(&self, input: &ItemStack) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter(|e| e.input.test(input))
                .cloned()
                .collect(),
        }
    }

    pub fn entries(&self) -> &[SingleInputEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// `net.minecraft.world.item.crafting.RecipeManager.ServerDisplayInfo`.
#[derive(Debug, Clone, PartialEq)]
pub struct ServerDisplayInfo {
    pub display: RecipeDisplayEntry,
    pub parent: RecipeHolder,
}

/// `net.minecraft.world.item.crafting.RecipeMap`.
///
/// Preserves deterministic insertion order per recipe type and globally by key,
/// rejects duplicate IDs, supports removal, and matches inputs in insertion
/// order (returning no matches for empty inputs).
#[derive(Debug, Clone, Default)]
pub struct RecipeMap {
    by_key: IndexMap<RecipeKey, RecipeHolder>,
    by_type: IndexMap<RecipeType, Vec<RecipeKey>>,
}

impl RecipeMap {
    pub fn empty() -> Self {
        Self::default()
    }

    /// `RecipeMap.create(Iterable<RecipeHolder<?>>)`.
    pub fn create<I>(recipes: I) -> Result<Self, RecipeMapError>
    where
        I: IntoIterator<Item = RecipeHolder>,
    {
        let mut map = Self::empty();
        for holder in recipes {
            map.add_recipe(holder)?;
        }
        Ok(map)
    }

    /// Paper `RecipeMap.addRecipe(RecipeHolder<?>)` — rejects duplicate keys.
    pub fn add_recipe(&mut self, holder: RecipeHolder) -> Result<(), RecipeMapError> {
        if self.by_key.contains_key(&holder.id) {
            return Err(RecipeMapError::DuplicateId(holder.id.to_string()));
        }
        let recipe_type = holder.value.recipe_type();
        let key = holder.id.clone();
        self.by_key.insert(key.clone(), holder);
        self.by_type.entry(recipe_type).or_default().push(key);
        Ok(())
    }

    /// Paper `RecipeMap.removeRecipe(ResourceKey<Recipe<?>>)`.
    pub fn remove_recipe(&mut self, key: &RecipeKey) -> bool {
        let Some(removed) = self.by_key.shift_remove(key) else {
            return false;
        };
        let recipe_type = removed.value.recipe_type();
        if let Some(keys) = self.by_type.get_mut(&recipe_type) {
            keys.retain(|k| k != key);
        }
        true
    }

    pub fn by_key(&self, key: &RecipeKey) -> Option<&RecipeHolder> {
        self.by_key.get(key)
    }

    pub fn values(&self) -> impl Iterator<Item = &RecipeHolder> {
        self.by_key.values()
    }

    pub fn by_type(&self, recipe_type: RecipeType) -> Vec<&RecipeHolder> {
        self.by_type
            .get(&recipe_type)
            .map(|keys| keys.iter().filter_map(|k| self.by_key.get(k)).collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.by_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_key.is_empty()
    }

    /// Return all matching crafting recipes in insertion order (empty for empty input).
    pub fn get_crafting_recipes_for<'a>(&'a self, input: &CraftingInput) -> Vec<&'a RecipeHolder> {
        if input.is_empty() {
            return Vec::new();
        }
        self.by_type(RecipeType::Crafting)
            .into_iter()
            .filter(|h| h.value.matches_crafting(input))
            .collect()
    }

    /// Return all matching single-input recipes (cooking or stonecutting) in insertion order.
    pub fn get_single_recipes_for<'a>(
        &'a self,
        recipe_type: RecipeType,
        input: &SingleRecipeInput,
    ) -> Vec<&'a RecipeHolder> {
        if input.is_empty() {
            return Vec::new();
        }
        self.by_type(recipe_type)
            .into_iter()
            .filter(|h| h.value.matches_single(input))
            .collect()
    }

    /// Return all matching smithing recipes in insertion order.
    pub fn get_smithing_recipes_for<'a>(
        &'a self,
        input: &SmithingRecipeInput,
    ) -> Vec<&'a RecipeHolder> {
        if input.is_empty() {
            return Vec::new();
        }
        self.by_type(RecipeType::Smithing)
            .into_iter()
            .filter(|h| h.value.matches_smithing(input))
            .collect()
    }
}

/// `net.minecraft.world.item.crafting.RecipeManager`.
#[derive(Debug, Clone)]
pub struct RecipeManager {
    recipes: RecipeMap,
    property_sets: IndexMap<Identifier, RecipePropertySet>,
    stonecutter_recipes: SingleInputSet,
    all_displays: Vec<ServerDisplayInfo>,
    recipe_to_display: IndexMap<RecipeKey, Vec<ServerDisplayInfo>>,
    /// Generation counter incremented on every recipe reload/mutation so
    /// `RecipeCache` can detect manager invalidation.
    generation: u64,
}

impl Default for RecipeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RecipeManager {
    pub fn new() -> Self {
        Self {
            recipes: RecipeMap::empty(),
            property_sets: IndexMap::new(),
            stonecutter_recipes: SingleInputSet::empty(),
            all_displays: Vec::new(),
            recipe_to_display: IndexMap::new(),
            generation: 0,
        }
    }

    /// Build a `RecipeManager` from a collection of `RecipeHolder`s and run
    /// `finalize_recipe_loading()`.
    pub fn from_recipes<I>(recipes: I) -> Result<Self, RecipeMapError>
    where
        I: IntoIterator<Item = RecipeHolder>,
    {
        let mut manager = Self::new();
        manager.recipes = RecipeMap::create(recipes)?;
        manager.finalize_recipe_loading();
        Ok(manager)
    }

    /// Paper `RecipeManager.addRecipe(RecipeHolder<?>)`.
    pub fn add_recipe(&mut self, holder: RecipeHolder) -> Result<(), RecipeMapError> {
        self.recipes.add_recipe(holder)?;
        self.finalize_recipe_loading();
        Ok(())
    }

    /// Paper `RecipeManager.removeRecipe(ResourceKey<Recipe<?>>)`.
    pub fn remove_recipe(&mut self, key: &RecipeKey) -> bool {
        let removed = self.recipes.remove_recipe(key);
        if removed {
            self.finalize_recipe_loading();
        }
        removed
    }

    /// Paper `RecipeManager.clearRecipes()`.
    pub fn clear_recipes(&mut self) {
        self.recipes = RecipeMap::empty();
        self.finalize_recipe_loading();
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn recipes(&self) -> &RecipeMap {
        &self.recipes
    }

    pub fn by_key(&self, key: &RecipeKey) -> Option<&RecipeHolder> {
        self.recipes.by_key(key)
    }

    pub fn property_set(&self, id: &Identifier) -> Option<&RecipePropertySet> {
        self.property_sets.get(id)
    }

    pub fn synchronized_property_sets(&self) -> &IndexMap<Identifier, RecipePropertySet> {
        &self.property_sets
    }

    pub fn stonecutter_recipes(&self) -> &SingleInputSet {
        &self.stonecutter_recipes
    }

    pub fn all_displays(&self) -> &[ServerDisplayInfo] {
        &self.all_displays
    }

    /// `RecipeManager.getRecipeFromDisplay(RecipeDisplayId)`.
    pub fn recipe_from_display(&self, id: RecipeDisplayId) -> Option<&ServerDisplayInfo> {
        if id.0 < 0 {
            return None;
        }
        self.all_displays.get(id.0 as usize)
    }

    /// `RecipeManager.listDisplaysForRecipe(ResourceKey<Recipe<?>>)`.
    pub fn list_displays_for_recipe(&self, key: &RecipeKey) -> Vec<&RecipeDisplayEntry> {
        self.recipe_to_display
            .get(key)
            .map(|list| list.iter().map(|info| &info.display).collect())
            .unwrap_or_default()
    }

    /// Paper `RecipeManager.getRecipeFor(RecipeType.CRAFTING, CraftingInput, Level)`.
    ///
    /// Returns the **last** matching recipe (Paper gives later recipes priority).
    pub fn get_crafting_recipe_for(&self, input: &CraftingInput) -> Option<&RecipeHolder> {
        self.recipes
            .get_crafting_recipes_for(input)
            .into_iter()
            .last()
    }

    /// Paper `RecipeManager.getRecipeFor(type, input, level, @Nullable ResourceKey<Recipe<?>> lastRecipe)`.
    ///
    /// Checks `last_recipe` hint first; falls back to full scan returning the last match.
    pub fn get_crafting_recipe_for_with_hint(
        &self,
        input: &CraftingInput,
        last_recipe: Option<&RecipeKey>,
    ) -> Option<&RecipeHolder> {
        if input.is_empty() {
            return None;
        }
        if let Some(key) = last_recipe
            && let Some(holder) = self.recipes.by_key(key)
            && holder.value.recipe_type() == RecipeType::Crafting
            && holder.value.matches_crafting(input)
        {
            return Some(holder);
        }
        self.get_crafting_recipe_for(input)
    }

    /// Paper `RecipeManager.getRecipeFor(cooking_or_stonecutting_type, SingleRecipeInput, Level)`.
    ///
    /// Returns the **last** matching recipe.
    pub fn get_single_recipe_for(
        &self,
        recipe_type: RecipeType,
        input: &SingleRecipeInput,
    ) -> Option<&RecipeHolder> {
        self.recipes
            .get_single_recipes_for(recipe_type, input)
            .into_iter()
            .last()
    }

    /// Hint-accelerated single-input lookup (`CachedCheck` fast path).
    pub fn get_single_recipe_for_with_hint(
        &self,
        recipe_type: RecipeType,
        input: &SingleRecipeInput,
        last_recipe: Option<&RecipeKey>,
    ) -> Option<&RecipeHolder> {
        if input.is_empty() {
            return None;
        }
        if let Some(key) = last_recipe
            && let Some(holder) = self.recipes.by_key(key)
            && holder.value.recipe_type() == recipe_type
            && holder.value.matches_single(input)
        {
            return Some(holder);
        }
        self.get_single_recipe_for(recipe_type, input)
    }

    /// Paper `RecipeManager.getRecipeFor(RecipeType.SMITHING, SmithingRecipeInput, Level)`.
    ///
    /// Returns the **last** matching recipe.
    pub fn get_smithing_recipe_for(&self, input: &SmithingRecipeInput) -> Option<&RecipeHolder> {
        self.recipes
            .get_smithing_recipes_for(input)
            .into_iter()
            .last()
    }

    /// `RecipeManager.finalizeRecipeLoading(FeatureFlagSet)`.
    ///
    /// Recomputes:
    /// - Synchronized `RecipePropertySet`s (smithing + 4 cooking inputs),
    /// - Ordered `SingleInputSet` for stonecutter recipes,
    /// - Deterministic `RecipeDisplayId`s and `RecipeDisplayEntry`s grouped by
    ///   group string (matching `RecipeManager.unpackRecipeInfo`).
    pub fn finalize_recipe_loading(&mut self) {
        self.generation = self.generation.wrapping_add(1);

        let mut smithing_base = Vec::new();
        let mut smithing_template = Vec::new();
        let mut smithing_addition = Vec::new();
        let mut furnace_input = Vec::new();
        let mut blast_furnace_input = Vec::new();
        let mut smoker_input = Vec::new();
        let mut campfire_input = Vec::new();
        let mut stonecutter_entries = Vec::new();

        for holder in self.recipes.values() {
            match &holder.value {
                Recipe::Cooking(c) => match c.kind {
                    CookingKind::Smelting => furnace_input.push(c.ingredient.clone()),
                    CookingKind::Blasting => blast_furnace_input.push(c.ingredient.clone()),
                    CookingKind::Smoking => smoker_input.push(c.ingredient.clone()),
                    CookingKind::CampfireCooking => campfire_input.push(c.ingredient.clone()),
                },
                Recipe::Stonecutter(sc) => {
                    stonecutter_entries.push(SingleInputEntry {
                        input: sc.ingredient.clone(),
                        recipe: SelectableRecipe {
                            option_display: sc.option_display(),
                            recipe: Some(holder.clone()),
                        },
                    });
                }
                Recipe::SmithingTransform(st) => {
                    smithing_base.push(st.base.clone());
                    if let Some(t) = &st.template {
                        smithing_template.push(t.clone());
                    }
                    if let Some(a) = &st.addition {
                        smithing_addition.push(a.clone());
                    }
                }
                Recipe::SmithingTrim(st) => {
                    smithing_base.push(st.base.clone());
                    smithing_template.push(st.template.clone());
                    smithing_addition.push(st.addition.clone());
                }
                Recipe::Shaped(_) | Recipe::Shapeless(_) | Recipe::Special { .. } => {}
            }
        }

        let mut property_sets = IndexMap::new();
        for (name, ingredients) in [
            (RecipePropertySet::SMITHING_BASE, &smithing_base),
            (RecipePropertySet::SMITHING_TEMPLATE, &smithing_template),
            (RecipePropertySet::SMITHING_ADDITION, &smithing_addition),
            (RecipePropertySet::FURNACE_INPUT, &furnace_input),
            (RecipePropertySet::BLAST_FURNACE_INPUT, &blast_furnace_input),
            (RecipePropertySet::SMOKER_INPUT, &smoker_input),
            (RecipePropertySet::CAMPFIRE_INPUT, &campfire_input),
        ] {
            property_sets.insert(
                Identifier::with_default_namespace(name),
                RecipePropertySet::create(ingredients.iter()),
            );
        }
        self.property_sets = property_sets;
        self.stonecutter_recipes = SingleInputSet::new(stonecutter_entries);

        // `RecipeManager.unpackRecipeInfo`
        let mut all_displays = Vec::new();
        let mut recipe_to_display: IndexMap<RecipeKey, Vec<ServerDisplayInfo>> = IndexMap::new();
        let mut group_ids: BTreeMap<String, i32> = BTreeMap::new();
        let mut next_group_id = 0_i32;
        let mut next_display_id = 0_i32;

        for holder in self.recipes.values() {
            if holder.value.is_special() {
                continue;
            }
            let group_name = holder.value.group();
            let group_id = if group_name.is_empty() {
                None
            } else {
                let id = *group_ids.entry(group_name.to_owned()).or_insert_with(|| {
                    let current = next_group_id;
                    next_group_id = next_group_id.wrapping_add(1);
                    current
                });
                Some(id)
            };

            let placement = holder.value.placement_info();
            let crafting_requirements = if placement.is_impossible_to_place() {
                None
            } else {
                Some(placement.ingredients().to_vec())
            };
            let category = holder.value.recipe_book_category();

            for display in holder.value.display() {
                let id = RecipeDisplayId(next_display_id);
                next_display_id = next_display_id.wrapping_add(1);
                let entry = RecipeDisplayEntry {
                    id,
                    display,
                    group: group_id,
                    category,
                    crafting_requirements: crafting_requirements.clone(),
                };
                let info = ServerDisplayInfo {
                    display: entry,
                    parent: holder.clone(),
                };
                all_displays.push(info.clone());
                recipe_to_display
                    .entry(holder.id.clone())
                    .or_default()
                    .push(info);
            }
        }

        self.all_displays = all_displays;
        self.recipe_to_display = recipe_to_display;
    }
}

/// `RecipeManager.CachedCheck<I, T>` — remembers the last matched `RecipeKey`
/// for fast repeated lookups (e.g. furnace/smoker tick loops).
#[derive(Debug, Clone)]
pub struct CachedCheck {
    recipe_type: RecipeType,
    last_recipe: Option<RecipeKey>,
}

impl CachedCheck {
    pub const fn new(recipe_type: RecipeType) -> Self {
        Self {
            recipe_type,
            last_recipe: None,
        }
    }

    pub fn last_recipe(&self) -> Option<&RecipeKey> {
        self.last_recipe.as_ref()
    }

    /// Look up a crafting recipe using the cached hint.
    pub fn get_crafting<'a>(
        &mut self,
        input: &CraftingInput,
        manager: &'a RecipeManager,
    ) -> Option<&'a RecipeHolder> {
        debug_assert_eq!(self.recipe_type, RecipeType::Crafting);
        let result = manager.get_crafting_recipe_for_with_hint(input, self.last_recipe.as_ref());
        if let Some(holder) = result {
            self.last_recipe = Some(holder.id.clone());
        }
        result
    }

    /// Look up a single-input recipe (cooking / stonecutting) using the cached hint.
    pub fn get_single<'a>(
        &mut self,
        input: &SingleRecipeInput,
        manager: &'a RecipeManager,
    ) -> Option<&'a RecipeHolder> {
        let result = manager.get_single_recipe_for_with_hint(
            self.recipe_type,
            input,
            self.last_recipe.as_ref(),
        );
        if let Some(holder) = result {
            self.last_recipe = Some(holder.id.clone());
        }
        result
    }
}

/// `net.minecraft.world.item.crafting.RecipeCache` — fixed-capacity LRU
/// move-to-front cache for crafting inputs, invalidated when the
/// `RecipeManager` generation changes.
#[derive(Debug, Clone)]
pub struct RecipeCache {
    entries: Vec<Option<RecipeCacheEntry>>,
    cached_generation: Option<u64>,
}

#[derive(Debug, Clone)]
struct RecipeCacheEntry {
    key: Vec<ItemStack>,
    width: i32,
    height: i32,
    value: Option<RecipeKey>,
}

impl RecipeCacheEntry {
    fn matches(&self, input: &CraftingInput) -> bool {
        if self.width != input.width() || self.height != input.height() {
            return false;
        }
        if self.key.len() != input.size() {
            return false;
        }
        for (idx, cached_stack) in self.key.iter().enumerate() {
            if !cached_stack.is_same_item_same_components(input.get_item(idx)) {
                return false;
            }
        }
        true
    }
}

impl RecipeCache {
    /// `new RecipeCache(int capacity)` (vanilla crafter uses capacity 10).
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: vec![None; capacity],
            cached_generation: None,
        }
    }

    /// `RecipeCache.get(ServerLevel, CraftingInput)`.
    pub fn get<'a>(
        &mut self,
        manager: &'a RecipeManager,
        input: &CraftingInput,
    ) -> Option<&'a RecipeHolder> {
        if input.is_empty() {
            return None;
        }
        self.validate_recipe_manager(manager);

        for i in 0..self.entries.len() {
            let is_hit = self.entries[i]
                .as_ref()
                .is_some_and(|entry| entry.matches(input));
            if is_hit {
                self.move_entry_to_front(i);
                let cached_key = self.entries[0].as_ref().and_then(|e| e.value.as_ref());
                return cached_key.and_then(|k| manager.by_key(k));
            }
        }

        let computed = manager.get_crafting_recipe_for(input);
        let computed_key = computed.map(|h| h.id.clone());
        self.insert(input, computed_key);
        computed
    }

    fn validate_recipe_manager(&mut self, manager: &RecipeManager) {
        if self.cached_generation != Some(manager.generation()) {
            self.cached_generation = Some(manager.generation());
            for slot in &mut self.entries {
                *slot = None;
            }
        }
    }

    fn move_entry_to_front(&mut self, index: usize) {
        if index > 0 {
            let entry = self.entries[index].take();
            for j in (1..=index).rev() {
                self.entries[j] = self.entries[j - 1].take();
            }
            self.entries[0] = entry;
        }
    }

    fn insert(&mut self, input: &CraftingInput, value: Option<RecipeKey>) {
        if self.entries.is_empty() {
            return;
        }
        let key: Vec<ItemStack> = input.items().iter().map(|s| s.copy_with_count(1)).collect();
        let last = self.entries.len() - 1;
        self.entries[last] = Some(RecipeCacheEntry {
            key,
            width: input.width(),
            height: input.height(),
            value,
        });
        self.move_entry_to_front(last);
    }
}
