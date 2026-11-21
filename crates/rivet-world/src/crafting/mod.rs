//! `net.minecraft.world.item.crafting` + `net.minecraft.stats.ServerRecipeBook`
//! (issue #463, M3).
//!
//! Vertical foundation slice faithful to Paper 26.2 (`0a993450f129c4942c2a9ed45ba047412b4667cf`):
//!
//! - [`item`] — `ItemId` (backed by generated `minecraft:item` tables),
//!   `ItemStack`, `ItemStackTemplate`, and standard crafting remainders.
//! - [`recipe_type`] — `RecipeType`, `RecipeSerializerId`,
//!   `CraftingBookCategory`, `CookingBookCategory`, `RecipeBookCategory`,
//!   `RecipeBookType`, and `CommonInfo` / book-info records.
//! - [`ingredient`] — `Ingredient` (direct, tag, Paper exact-stack, air/empty
//!   validation, optional-ingredient matching) and `PlacementInfo`.
//! - [`input`] — `RecipeInput`, `CraftingInput` (with `of_positioned`
//!   bounding-box trimming), `SingleRecipeInput`, and `SmithingRecipeInput`.
//! - [`stacked_contents`] — augmenting-path bipartite matching for shapeless
//!   recipes and recipe-book craftability checks.
//! - [`pattern`] — `ShapedRecipePattern` (3x3 bounds, `shrink`, symmetry,
//!   flipped-before-unflipped matching).
//! - [`display`] — `SlotDisplay` (all 11 vanilla types), `RecipeDisplay` (all 5
//!   vanilla types), `RecipeDisplayId`, and `RecipeDisplayEntry`.
//! - [`recipes`] — `ShapedRecipe`, `ShapelessRecipe`, `CookingRecipe`
//!   (smelting/blasting/smoking/campfire), `StonecutterRecipe`,
//!   `SmithingTransformRecipe` (with Paper's `copy_data_components` flag),
//!   `SmithingTrimRecipe`, `Recipe`, `RecipeHolder`, and
//!   `default_crafting_remainder`.
//! - [`manager`] — `RecipeMap`, `RecipePropertySet`, `SelectableRecipe` /
//!   `SingleInputSet`, `RecipeManager` (last-match priority, display unpacking,
//!   synchronized property sets), `CachedCheck`, and `RecipeCache`.
//! - [`recipe_book`] — `TypeSettings`, `RecipeBookSettings`, and
//!   `ServerRecipeBook` (known/highlight sets, add/remove/initial entries,
//!   pack/load_untrusted).
//! - [`json`] — Vanilla 26.2 datapack JSON recipe loader.
//!
//! Protocol boundary: `ClientboundUpdateRecipesPacket` wire encoding and join
//! emission are owned by issue #87; this module owns the gameplay value types
//! and runtime logic only.

pub mod display;
pub mod ingredient;
pub mod input;
pub mod item;
pub mod json;
pub mod manager;
pub mod pattern;
pub mod recipe_book;
pub mod recipe_type;
pub mod recipes;
pub mod stacked_contents;

#[cfg(test)]
mod tests;

pub use display::{RecipeDisplay, RecipeDisplayEntry, RecipeDisplayId, SlotDisplay};
pub use ingredient::{Ingredient, IngredientError, PlacementInfo};
pub use input::{
    CraftingInput, PositionedCraftingInput, RecipeInput, SingleRecipeInput, SmithingRecipeInput,
};
pub use item::{ItemId, ItemStack, ItemStackTemplate};
pub use json::{RecipeJsonError, parse_recipe_json};
pub use manager::{
    CachedCheck, RecipeCache, RecipeManager, RecipeMap, RecipeMapError, RecipePropertySet,
    SelectableRecipe, ServerDisplayInfo, SingleInputEntry, SingleInputSet,
};
pub use pattern::{PatternError, ShapedRecipePattern};
pub use recipe_book::{
    PackedServerRecipeBook, RecipeBookAddEntry, RecipeBookSettings, ServerRecipeBook, TypeSettings,
};
pub use recipe_type::{
    CommonInfo, CookingBookCategory, CookingBookInfo, CraftingBookCategory, CraftingBookInfo,
    RecipeBookCategory, RecipeBookType, RecipeSerializerId, RecipeType,
};
pub use recipes::{
    CookingKind, CookingRecipe, DEFAULT_BLASTING_TIME, DEFAULT_CAMPFIRE_COOKING_TIME,
    DEFAULT_SMELTING_TIME, DEFAULT_SMOKING_TIME, Recipe, RecipeHolder, RecipeKey, RecipeMarker,
    ShapedRecipe, ShapelessRecipe, SmithingTransformRecipe, SmithingTrimRecipe, StonecutterRecipe,
    default_crafting_remainder, recipe_key,
};
pub use stacked_contents::StackedItemContents;
