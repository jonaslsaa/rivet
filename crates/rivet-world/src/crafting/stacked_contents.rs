//! Paper-faithful stacked-contents bipartite matching for shapeless recipes
//! and recipe-book craftability checks.
//!
//! Mirrors `net.minecraft.world.entity.player.StackedContents` and
//! `net.minecraft.world.entity.player.StackedItemContents`.

use indexmap::IndexMap;

use super::ingredient::Ingredient;
use super::item::ItemStack;

/// `net.minecraft.world.entity.player.StackedContents` specialized for
/// crafting/recipe-book `ItemStack` matching (Paper uses an `Object2IntOpenHashMap`
/// keyed by exact `ItemStack` identity/components for exact-choice ingredients).
#[derive(Debug, Clone, Default)]
pub struct StackedItemContents {
    /// Available counts keyed by unit-count `ItemStack` (preserving insertion order).
    amounts: IndexMap<ItemStack, i32>,
}

impl StackedItemContents {
    pub fn new() -> Self {
        Self::default()
    }

    /// Account `count` units of `stack` (matching `StackedContents.account`).
    pub fn account(&mut self, stack: &ItemStack, count: i32) {
        if stack.is_empty() || count <= 0 {
            return;
        }
        let unit = stack.copy_with_count(1);
        *self.amounts.entry(unit).or_insert(0) = self
            .amounts
            .get(&stack.copy_with_count(1))
            .copied()
            .unwrap_or(0)
            .wrapping_add(count);
    }

    /// Account a single `ItemStack` with count 1 (as `CraftingInput` does for
    /// each non-empty slot in the crafting grid).
    pub fn account_simple_stack(&mut self, stack: &ItemStack) {
        self.account(stack, 1);
    }

    /// Account an `ItemStack` with its full `stack.count()` (for inventory
    /// recipe-book craftability checks).
    pub fn account_stack(&mut self, stack: &ItemStack) {
        self.account(stack, stack.count());
    }

    pub fn clear(&mut self) {
        self.amounts.clear();
    }

    /// `StackedContents.tryPick(ingredients, amount, null)` — returns `true`
    /// iff `amount` complete sets of `ingredients` can be matched against the
    /// accounted items (using exact augmenting-path bipartite matching so
    /// overlapping ingredients like `[#planks, oak_planks]` match optimally).
    pub fn can_craft(&self, ingredients: &[Ingredient], amount: i32) -> bool {
        if amount <= 0 {
            return true;
        }
        if ingredients.is_empty() {
            return false;
        }

        // Collect unique available items with count >= amount that match at
        // least one ingredient.
        let items: Vec<(ItemStack, i32)> = self
            .amounts
            .iter()
            .filter(|(stack, count)| {
                **count >= amount && ingredients.iter().any(|ing| ing.test(stack))
            })
            .map(|(s, c)| (s.clone(), *c))
            .collect();

        if items.is_empty() {
            return false;
        }

        // Each available item `j` can supply `floor(count_j / amount)` distinct
        // ingredient slots. Expand into capacity units and run Kuhn's augmenting
        // path algorithm (equivalent to Paper's `RecipePicker.dfs`).
        let mut slot_items: Vec<usize> = Vec::new();
        for (item_idx, (_, count)) in items.iter().enumerate() {
            let cap = (*count / amount) as usize;
            let capped = cap.min(ingredients.len());
            for _ in 0..capped {
                slot_items.push(item_idx);
            }
        }

        if slot_items.len() < ingredients.len() {
            return false;
        }

        // Adjacency: ingredient_idx -> list of item_idx that satisfy it.
        let mut adj: Vec<Vec<usize>> = Vec::with_capacity(ingredients.len());
        for ing in ingredients {
            let matches: Vec<usize> = items
                .iter()
                .enumerate()
                .filter_map(|(idx, (stack, _))| ing.test(stack).then_some(idx))
                .collect();
            if matches.is_empty() {
                return false;
            }
            adj.push(matches);
        }

        // Match each ingredient to one unit in `slot_items`.
        let mut match_slot: Vec<Option<usize>> = vec![None; slot_items.len()];
        for ing_idx in 0..ingredients.len() {
            let mut visited = vec![false; slot_items.len()];
            if !augment(ing_idx, &adj, &slot_items, &mut visited, &mut match_slot) {
                return false;
            }
        }
        true
    }
}

fn augment(
    ing_idx: usize,
    adj: &[Vec<usize>],
    slot_items: &[usize],
    visited: &mut [bool],
    match_slot: &mut [Option<usize>],
) -> bool {
    for (slot_idx, &item_idx) in slot_items.iter().enumerate() {
        if visited[slot_idx] || !adj[ing_idx].contains(&item_idx) {
            continue;
        }
        visited[slot_idx] = true;
        if match_slot[slot_idx].is_none()
            || augment(
                match_slot[slot_idx].unwrap(),
                adj,
                slot_items,
                visited,
                match_slot,
            )
        {
            match_slot[slot_idx] = Some(ing_idx);
            return true;
        }
    }
    false
}
