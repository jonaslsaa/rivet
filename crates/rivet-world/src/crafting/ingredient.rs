//! Recipe ingredients and placement info.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.item.crafting.Ingredient`
//! - `net.minecraft.world.item.crafting.PlacementInfo`

use rivet_registry::Identifier;
use thiserror::Error;

use super::display::SlotDisplay;
use super::item::{ItemId, ItemStack, ItemStackTemplate};

/// Validation errors when constructing an [`Ingredient`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum IngredientError {
    #[error("Ingredient cannot be empty")]
    Empty,
    #[error("Ingredient must not contain minecraft:air")]
    ContainsAir,
}

/// `net.minecraft.world.item.crafting.Ingredient`.
///
/// Matches Paper 26.2 semantics:
/// - Non-empty and never contains `minecraft:air`.
/// - Supports item lists (direct holder set), tag-backed item sets, and
///   Paper's exact-`ItemStack` choice (`isExact()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ingredient {
    /// Direct list of accepted items (`HolderSet.direct(...)`).
    Items(Vec<ItemId>),
    /// Tag-backed ingredient with its tag key and resolved item list.
    Tag { tag: Identifier, items: Vec<ItemId> },
    /// Paper exact-stack ingredient (`Ingredient.ofStacks(...)`).
    Exact(Vec<ItemStack>),
}

impl Ingredient {
    /// Construct a single-item ingredient (`Ingredient.of(ItemLike)`).
    pub fn of_item(item: ItemId) -> Result<Self, IngredientError> {
        Self::of_items(vec![item])
    }

    /// Construct a multi-item ingredient (`Ingredient.of(ItemLike...)`).
    ///
    /// Mirrors Paper's constructor check: rejects empty direct lists and any
    /// list containing `Items.AIR`.
    pub fn of_items(items: Vec<ItemId>) -> Result<Self, IngredientError> {
        if items.is_empty() {
            return Err(IngredientError::Empty);
        }
        if items.iter().any(|item| item.is_air()) {
            return Err(IngredientError::ContainsAir);
        }
        Ok(Self::Items(items))
    }

    /// Construct a tag-backed ingredient (`Ingredient.of(HolderSet<Item>)`).
    pub fn of_tag(tag: Identifier, items: Vec<ItemId>) -> Result<Self, IngredientError> {
        if items.iter().any(|item| item.is_air()) {
            return Err(IngredientError::ContainsAir);
        }
        Ok(Self::Tag { tag, items })
    }

    /// Construct a Paper exact-stack ingredient (`Ingredient.ofStacks(...)`).
    pub fn of_exact_stacks(stacks: Vec<ItemStack>) -> Result<Self, IngredientError> {
        if stacks.is_empty() {
            return Err(IngredientError::Empty);
        }
        if stacks.iter().any(ItemStack::is_empty) {
            return Err(IngredientError::ContainsAir);
        }
        Ok(Self::Exact(stacks))
    }

    /// Paper `Ingredient.isExact()`.
    pub const fn is_exact(&self) -> bool {
        matches!(self, Self::Exact(_))
    }

    /// Whether this ingredient currently has no matching items (can happen for
    /// an unresolved/empty tag, which makes `PlacementInfo` unplaceable).
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Items(items) | Self::Tag { items, .. } => items.is_empty(),
            Self::Exact(stacks) => stacks.is_empty(),
        }
    }

    /// All item IDs accepted by this ingredient (`Ingredient.items()`).
    pub fn items(&self) -> Vec<ItemId> {
        match self {
            Self::Items(items) | Self::Tag { items, .. } => items.clone(),
            Self::Exact(stacks) => stacks.iter().map(ItemStack::item).collect(),
        }
    }

    /// `Ingredient.test(ItemStack)`.
    pub fn test(&self, input: &ItemStack) -> bool {
        if input.is_empty() {
            return false;
        }
        match self {
            Self::Items(items) | Self::Tag { items, .. } => items.contains(&input.item()),
            Self::Exact(stacks) => stacks
                .iter()
                .any(|choice| choice.is_same_item_same_components(input)),
        }
    }

    /// `Ingredient.testOptionalIngredient(Optional<Ingredient>, ItemStack)`.
    ///
    /// Absent optional ingredient matches iff the input stack is empty.
    pub fn test_optional(ingredient: Option<&Self>, stack: &ItemStack) -> bool {
        match ingredient {
            Some(ing) => ing.test(stack),
            None => stack.is_empty(),
        }
    }

    /// `Ingredient.display()`.
    pub fn display(&self) -> SlotDisplay {
        match self {
            Self::Items(items) => {
                if items.len() == 1 {
                    SlotDisplay::Item(items[0])
                } else {
                    SlotDisplay::Composite(items.iter().copied().map(SlotDisplay::Item).collect())
                }
            }
            Self::Tag { tag, .. } => SlotDisplay::Tag(tag.clone()),
            Self::Exact(stacks) => {
                let displays: Vec<SlotDisplay> = stacks
                    .iter()
                    .map(|s| {
                        SlotDisplay::ItemStack(ItemStackTemplate::with_components(
                            s.item(),
                            s.count(),
                            s.components().clone(),
                        ))
                    })
                    .collect();
                if displays.len() == 1 {
                    displays.into_iter().next().unwrap_or(SlotDisplay::Empty)
                } else {
                    SlotDisplay::Composite(displays)
                }
            }
        }
    }

    /// `Ingredient.optionalIngredientToDisplay(Optional<Ingredient>)`.
    pub fn optional_display(ingredient: Option<&Self>) -> SlotDisplay {
        ingredient.map_or(SlotDisplay::Empty, Self::display)
    }
}

/// `net.minecraft.world.item.crafting.PlacementInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementInfo {
    ingredients: Vec<Ingredient>,
    slots_to_ingredient_index: Vec<i32>,
}

impl PlacementInfo {
    /// `PlacementInfo.EMPTY_SLOT = -1`.
    pub const EMPTY_SLOT: i32 = -1;

    /// `PlacementInfo.NOT_PLACEABLE`.
    pub const fn not_placeable() -> Self {
        Self {
            ingredients: Vec::new(),
            slots_to_ingredient_index: Vec::new(),
        }
    }

    /// `PlacementInfo.create(Ingredient)`.
    pub fn create_single(ingredient: Ingredient) -> Self {
        if ingredient.is_empty() {
            Self::not_placeable()
        } else {
            Self {
                ingredients: vec![ingredient],
                slots_to_ingredient_index: vec![0],
            }
        }
    }

    /// `PlacementInfo.create(List<Ingredient>)`.
    pub fn create(ingredients: Vec<Ingredient>) -> Self {
        if ingredients.is_empty() || ingredients.iter().any(Ingredient::is_empty) {
            return Self::not_placeable();
        }
        let slots_to_ingredient_index = (0..ingredients.len() as i32).collect();
        Self {
            ingredients,
            slots_to_ingredient_index,
        }
    }

    /// `PlacementInfo.createFromOptionals(List<Optional<Ingredient>>)`.
    ///
    /// Present ingredients get dense indices `0, 1, ...`; absent optional
    /// ingredients map to `EMPTY_SLOT` (`-1`). If any present ingredient is
    /// empty (or none are present), returns `NOT_PLACEABLE`.
    pub fn create_from_optionals(optionals: Vec<Option<Ingredient>>) -> Self {
        let mut ingredients = Vec::new();
        let mut slots_to_ingredient_index = Vec::with_capacity(optionals.len());
        for opt in optionals {
            match opt {
                Some(ing) => {
                    if ing.is_empty() {
                        return Self::not_placeable();
                    }
                    let idx = ingredients.len() as i32;
                    ingredients.push(ing);
                    slots_to_ingredient_index.push(idx);
                }
                None => {
                    slots_to_ingredient_index.push(Self::EMPTY_SLOT);
                }
            }
        }
        if ingredients.is_empty() {
            Self::not_placeable()
        } else {
            Self {
                ingredients,
                slots_to_ingredient_index,
            }
        }
    }

    pub fn ingredients(&self) -> &[Ingredient] {
        &self.ingredients
    }

    pub fn slots_to_ingredient_index(&self) -> &[i32] {
        &self.slots_to_ingredient_index
    }

    /// `PlacementInfo.isImpossibleToPlace()`.
    pub fn is_impossible_to_place(&self) -> bool {
        self.slots_to_ingredient_index.is_empty()
    }
}
