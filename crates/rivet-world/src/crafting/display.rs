//! Recipe and slot display types.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.item.crafting.display.SlotDisplay`
//! - `net.minecraft.world.item.crafting.display.SlotDisplays`
//! - `net.minecraft.world.item.crafting.display.RecipeDisplay`
//! - `net.minecraft.world.item.crafting.display.RecipeDisplays`
//! - `net.minecraft.world.item.crafting.display.RecipeDisplayId`
//! - `net.minecraft.world.item.crafting.display.RecipeDisplayEntry`

use rivet_registry::Identifier;

use super::ingredient::Ingredient;
use super::item::{ItemId, ItemStack, ItemStackTemplate};
use super::recipe_type::RecipeBookCategory;

/// `net.minecraft.world.item.crafting.display.RecipeDisplayId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RecipeDisplayId(pub i32);

/// `net.minecraft.world.item.crafting.display.SlotDisplay` — all 11 vanilla
/// display variants from `SlotDisplays.bootstrap`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotDisplay {
    Empty,
    AnyFuel,
    WithAnyPotion {
        display: Box<SlotDisplay>,
    },
    OnlyWithComponent {
        source: Box<SlotDisplay>,
        component: Identifier,
    },
    Item(ItemId),
    ItemStack(ItemStackTemplate),
    Tag(Identifier),
    Dyed {
        dye: Box<SlotDisplay>,
        target: Box<SlotDisplay>,
    },
    SmithingTrim {
        base: Box<SlotDisplay>,
        material: Box<SlotDisplay>,
        pattern: Identifier,
    },
    WithRemainder {
        input: Box<SlotDisplay>,
        remainder: Box<SlotDisplay>,
    },
    Composite(Vec<SlotDisplay>),
}

impl SlotDisplay {
    /// Vanilla registry path in `minecraft:slot_display`.
    pub const fn type_path(&self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::AnyFuel => "any_fuel",
            Self::WithAnyPotion { .. } => "with_any_potion",
            Self::OnlyWithComponent { .. } => "only_with_component",
            Self::Item(_) => "item",
            Self::ItemStack(_) => "item_stack",
            Self::Tag(_) => "tag",
            Self::Dyed { .. } => "dyed",
            Self::SmithingTrim { .. } => "smithing_trim",
            Self::WithRemainder { .. } => "with_remainder",
            Self::Composite(_) => "composite",
        }
    }

    /// Resolve concrete `ItemStack` previews for this slot display.
    ///
    /// Tags are resolved via `tag_resolver` when provided.
    pub fn resolve_for_stacks<F>(&self, tag_resolver: &F) -> Vec<ItemStack>
    where
        F: Fn(&Identifier) -> Vec<ItemId>,
    {
        match self {
            Self::Empty | Self::AnyFuel => Vec::new(),
            Self::Item(item) => vec![ItemStack::new(*item, 1)],
            Self::ItemStack(template) => vec![template.create()],
            Self::Tag(tag) => tag_resolver(tag)
                .into_iter()
                .map(|id| ItemStack::new(id, 1))
                .collect(),
            Self::Composite(children) => children
                .iter()
                .flat_map(|c| c.resolve_for_stacks(tag_resolver))
                .collect(),
            Self::WithRemainder { input, .. } => input.resolve_for_stacks(tag_resolver),
            Self::WithAnyPotion { display } => display.resolve_for_stacks(tag_resolver),
            Self::OnlyWithComponent { source, .. } => source.resolve_for_stacks(tag_resolver),
            Self::Dyed { target, .. } => target.resolve_for_stacks(tag_resolver),
            Self::SmithingTrim { base, .. } => base.resolve_for_stacks(tag_resolver),
        }
    }
}

/// `net.minecraft.world.item.crafting.display.RecipeDisplay` — all 5 vanilla
/// recipe display variants from `RecipeDisplays.bootstrap`.
#[derive(Debug, Clone, PartialEq)]
pub enum RecipeDisplay {
    ShapelessCrafting {
        ingredients: Vec<SlotDisplay>,
        result: SlotDisplay,
        crafting_station: SlotDisplay,
    },
    ShapedCrafting {
        width: i32,
        height: i32,
        ingredients: Vec<SlotDisplay>,
        result: SlotDisplay,
        crafting_station: SlotDisplay,
    },
    Furnace {
        ingredient: SlotDisplay,
        fuel: SlotDisplay,
        result: SlotDisplay,
        crafting_station: SlotDisplay,
        duration: i32,
        experience: f32,
    },
    Stonecutter {
        input: SlotDisplay,
        result: SlotDisplay,
        crafting_station: SlotDisplay,
    },
    Smithing {
        template: SlotDisplay,
        base: SlotDisplay,
        addition: SlotDisplay,
        result: SlotDisplay,
        crafting_station: SlotDisplay,
    },
}

impl RecipeDisplay {
    /// Vanilla registry path in `minecraft:recipe_display`.
    pub const fn type_path(&self) -> &'static str {
        match self {
            Self::ShapelessCrafting { .. } => "crafting_shapeless",
            Self::ShapedCrafting { .. } => "crafting_shaped",
            Self::Furnace { .. } => "furnace",
            Self::Stonecutter { .. } => "stonecutter",
            Self::Smithing { .. } => "smithing",
        }
    }

    /// `RecipeDisplay.result()`.
    pub fn result(&self) -> &SlotDisplay {
        match self {
            Self::ShapelessCrafting { result, .. }
            | Self::ShapedCrafting { result, .. }
            | Self::Furnace { result, .. }
            | Self::Stonecutter { result, .. }
            | Self::Smithing { result, .. } => result,
        }
    }

    /// `RecipeDisplay.craftingStation()`.
    pub fn crafting_station(&self) -> &SlotDisplay {
        match self {
            Self::ShapelessCrafting {
                crafting_station, ..
            }
            | Self::ShapedCrafting {
                crafting_station, ..
            }
            | Self::Furnace {
                crafting_station, ..
            }
            | Self::Stonecutter {
                crafting_station, ..
            }
            | Self::Smithing {
                crafting_station, ..
            } => crafting_station,
        }
    }
}

/// `net.minecraft.world.item.crafting.display.RecipeDisplayEntry`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeDisplayEntry {
    pub id: RecipeDisplayId,
    pub display: RecipeDisplay,
    pub group: Option<i32>,
    pub category: RecipeBookCategory,
    pub crafting_requirements: Option<Vec<Ingredient>>,
}

impl RecipeDisplayEntry {
    /// `RecipeDisplayEntry.resultItems(ContextMap)`.
    pub fn result_items<F>(&self, tag_resolver: &F) -> Vec<ItemStack>
    where
        F: Fn(&Identifier) -> Vec<ItemId>,
    {
        self.display.result().resolve_for_stacks(tag_resolver)
    }
}
