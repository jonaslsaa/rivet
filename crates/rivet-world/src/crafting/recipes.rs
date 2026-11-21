//! Concrete recipe types and `RecipeHolder`.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.item.crafting.Recipe`
//! - `net.minecraft.world.item.crafting.RecipeHolder`
//! - `net.minecraft.world.item.crafting.CraftingRecipe`
//! - `net.minecraft.world.item.crafting.ShapedRecipe`
//! - `net.minecraft.world.item.crafting.ShapelessRecipe`
//! - `net.minecraft.world.item.crafting.AbstractCookingRecipe`
//! - `net.minecraft.world.item.crafting.SmeltingRecipe`
//! - `net.minecraft.world.item.crafting.BlastingRecipe`
//! - `net.minecraft.world.item.crafting.SmokingRecipe`
//! - `net.minecraft.world.item.crafting.CampfireCookingRecipe`
//! - `net.minecraft.world.item.crafting.StonecutterRecipe`
//! - `net.minecraft.world.item.crafting.SmithingRecipe`
//! - `net.minecraft.world.item.crafting.SmithingTransformRecipe`
//! - `net.minecraft.world.item.crafting.SmithingTrimRecipe`

use rivet_registry::{Identifier, ResourceKey};

use super::display::{RecipeDisplay, SlotDisplay};
use super::ingredient::{Ingredient, PlacementInfo};
use super::input::{CraftingInput, RecipeInput, SingleRecipeInput, SmithingRecipeInput};
use super::item::{ItemId, ItemStack, ItemStackTemplate};
use super::pattern::ShapedRecipePattern;
use super::recipe_type::{
    CommonInfo, CookingBookCategory, CookingBookInfo, CraftingBookCategory, CraftingBookInfo,
    RecipeBookCategory, RecipeSerializerId, RecipeType,
};
use super::stacked_contents::StackedItemContents;

/// Phantom marker type for `ResourceKey<RecipeMarker>` (`ResourceKey<Recipe<?>>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecipeMarker;

/// Canonical `ResourceKey<Recipe<?>>` alias.
pub type RecipeKey = ResourceKey<RecipeMarker>;

/// Construct a `RecipeKey` in the `minecraft:recipe` registry.
pub fn recipe_key(id: Identifier) -> RecipeKey {
    let registry_key =
        ResourceKey::create_registry_key(Identifier::with_default_namespace("recipe"));
    ResourceKey::create(&registry_key, id)
}

/// `net.minecraft.world.item.crafting.RecipeHolder<T>`.
#[derive(Debug, Clone, PartialEq)]
pub struct RecipeHolder {
    pub id: RecipeKey,
    pub value: Recipe,
}

impl RecipeHolder {
    pub const fn new(id: RecipeKey, value: Recipe) -> Self {
        Self { id, value }
    }
}

/// Default cooking times in ticks (`SmeltingRecipe`, `BlastingRecipe`,
/// `SmokingRecipe`, `CampfireCookingRecipe`).
pub const DEFAULT_SMELTING_TIME: i32 = 200;
pub const DEFAULT_BLASTING_TIME: i32 = 100;
pub const DEFAULT_SMOKING_TIME: i32 = 100;
pub const DEFAULT_CAMPFIRE_COOKING_TIME: i32 = 100;

/// Cooking recipe kind distinguishing the 4 furnace/campfire recipe subclasses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CookingKind {
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
}

impl CookingKind {
    pub const fn recipe_type(self) -> RecipeType {
        match self {
            Self::Smelting => RecipeType::Smelting,
            Self::Blasting => RecipeType::Blasting,
            Self::Smoking => RecipeType::Smoking,
            Self::CampfireCooking => RecipeType::CampfireCooking,
        }
    }

    pub const fn serializer(self) -> RecipeSerializerId {
        match self {
            Self::Smelting => RecipeSerializerId::Smelting,
            Self::Blasting => RecipeSerializerId::Blasting,
            Self::Smoking => RecipeSerializerId::Smoking,
            Self::CampfireCooking => RecipeSerializerId::CampfireCooking,
        }
    }

    pub const fn default_cooking_time(self) -> i32 {
        match self {
            Self::Smelting => DEFAULT_SMELTING_TIME,
            Self::Blasting => DEFAULT_BLASTING_TIME,
            Self::Smoking => DEFAULT_SMOKING_TIME,
            Self::CampfireCooking => DEFAULT_CAMPFIRE_COOKING_TIME,
        }
    }

    pub fn furnace_icon(self) -> ItemId {
        let name = match self {
            Self::Smelting => "minecraft:furnace",
            Self::Blasting => "minecraft:blast_furnace",
            Self::Smoking => "minecraft:smoker",
            Self::CampfireCooking => "minecraft:campfire",
        };
        ItemId::from_name(name).unwrap_or(ItemId::AIR)
    }

    pub const fn recipe_book_category(self, category: CookingBookCategory) -> RecipeBookCategory {
        match self {
            Self::Smelting => match category {
                CookingBookCategory::Blocks => RecipeBookCategory::FurnaceBlocks,
                CookingBookCategory::Food => RecipeBookCategory::FurnaceFood,
                CookingBookCategory::Misc => RecipeBookCategory::FurnaceMisc,
            },
            Self::Blasting => match category {
                CookingBookCategory::Blocks => RecipeBookCategory::BlastFurnaceBlocks,
                CookingBookCategory::Food | CookingBookCategory::Misc => {
                    RecipeBookCategory::BlastFurnaceMisc
                }
            },
            Self::Smoking => RecipeBookCategory::SmokerFood,
            Self::CampfireCooking => RecipeBookCategory::Campfire,
        }
    }
}

/// `net.minecraft.world.item.crafting.ShapedRecipe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapedRecipe {
    pub common_info: CommonInfo,
    pub book_info: CraftingBookInfo,
    pub pattern: ShapedRecipePattern,
    pub result: ItemStackTemplate,
}

impl ShapedRecipe {
    pub fn matches(&self, input: &CraftingInput) -> bool {
        self.pattern.matches(input)
    }

    pub fn assemble(&self, _input: &CraftingInput) -> ItemStack {
        self.result.create()
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create_from_optionals(self.pattern.ingredients().to_vec())
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:crafting_table").unwrap_or(ItemId::AIR);
        vec![RecipeDisplay::ShapedCrafting {
            width: self.pattern.width(),
            height: self.pattern.height(),
            ingredients: self
                .pattern
                .ingredients()
                .iter()
                .map(|opt| Ingredient::optional_display(opt.as_ref()))
                .collect(),
            result: SlotDisplay::ItemStack(self.result.clone()),
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// `net.minecraft.world.item.crafting.ShapelessRecipe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapelessRecipe {
    pub common_info: CommonInfo,
    pub book_info: CraftingBookInfo,
    pub ingredients: Vec<Ingredient>,
    pub result: ItemStackTemplate,
}

impl ShapelessRecipe {
    /// `ShapelessRecipe.matches(CraftingInput, Level)`.
    pub fn matches(&self, input: &CraftingInput) -> bool {
        if input.ingredient_count() as usize != self.ingredients.len() {
            return false;
        }
        if input.size() == 1 && self.ingredients.len() == 1 {
            return self.ingredients[0].test(input.get_item(0));
        }
        let mut stacked = StackedItemContents::new();
        for stack in input.items() {
            if !stack.is_empty() {
                stacked.account_simple_stack(stack);
            }
        }
        stacked.can_craft(&self.ingredients, 1)
    }

    pub fn assemble(&self, _input: &CraftingInput) -> ItemStack {
        self.result.create()
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create(self.ingredients.clone())
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:crafting_table").unwrap_or(ItemId::AIR);
        vec![RecipeDisplay::ShapelessCrafting {
            ingredients: self.ingredients.iter().map(Ingredient::display).collect(),
            result: SlotDisplay::ItemStack(self.result.clone()),
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// `net.minecraft.world.item.crafting.AbstractCookingRecipe` (covers smelting,
/// blasting, smoking, and campfire cooking).
#[derive(Debug, Clone, PartialEq)]
pub struct CookingRecipe {
    pub kind: CookingKind,
    pub common_info: CommonInfo,
    pub book_info: CookingBookInfo,
    pub ingredient: Ingredient,
    pub result: ItemStackTemplate,
    pub experience: f32,
    pub cooking_time: i32,
}

impl CookingRecipe {
    pub fn matches(&self, input: &SingleRecipeInput) -> bool {
        self.ingredient.test(&input.item)
    }

    pub fn assemble(&self, _input: &SingleRecipeInput) -> ItemStack {
        self.result.create()
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create_single(self.ingredient.clone())
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        vec![RecipeDisplay::Furnace {
            ingredient: self.ingredient.display(),
            fuel: SlotDisplay::AnyFuel,
            result: SlotDisplay::ItemStack(self.result.clone()),
            crafting_station: SlotDisplay::Item(self.kind.furnace_icon()),
            duration: self.cooking_time,
            experience: self.experience,
        }]
    }
}

/// `net.minecraft.world.item.crafting.StonecutterRecipe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StonecutterRecipe {
    pub common_info: CommonInfo,
    pub ingredient: Ingredient,
    pub result: ItemStackTemplate,
}

impl StonecutterRecipe {
    pub fn matches(&self, input: &SingleRecipeInput) -> bool {
        self.ingredient.test(&input.item)
    }

    pub fn assemble(&self, _input: &SingleRecipeInput) -> ItemStack {
        self.result.create()
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create_single(self.ingredient.clone())
    }

    pub fn option_display(&self) -> SlotDisplay {
        SlotDisplay::ItemStack(self.result.clone())
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:stonecutter").unwrap_or(ItemId::AIR);
        vec![RecipeDisplay::Stonecutter {
            input: self.ingredient.display(),
            result: self.option_display(),
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// `net.minecraft.world.item.crafting.SmithingTransformRecipe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmithingTransformRecipe {
    pub common_info: CommonInfo,
    pub template: Option<Ingredient>,
    pub base: Ingredient,
    pub addition: Option<Ingredient>,
    pub result: ItemStackTemplate,
    /// Paper flag: `copyDataComponents` (default `true`).
    pub copy_data_components: bool,
}

impl SmithingTransformRecipe {
    pub fn matches(&self, input: &SmithingRecipeInput) -> bool {
        Ingredient::test_optional(self.template.as_ref(), &input.template)
            && self.base.test(&input.base)
            && Ingredient::test_optional(self.addition.as_ref(), &input.addition)
    }

    pub fn assemble(&self, input: &SmithingRecipeInput) -> ItemStack {
        if self.copy_data_components {
            self.result.create_with_original_components(&input.base)
        } else {
            self.result.create()
        }
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create_from_optionals(vec![
            self.template.clone(),
            Some(self.base.clone()),
            self.addition.clone(),
        ])
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:smithing_table").unwrap_or(ItemId::AIR);
        vec![RecipeDisplay::Smithing {
            template: Ingredient::optional_display(self.template.as_ref()),
            base: self.base.display(),
            addition: Ingredient::optional_display(self.addition.as_ref()),
            result: SlotDisplay::ItemStack(self.result.clone()),
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// `net.minecraft.world.item.crafting.SmithingTrimRecipe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmithingTrimRecipe {
    pub common_info: CommonInfo,
    pub template: Ingredient,
    pub base: Ingredient,
    pub addition: Ingredient,
    pub pattern: Identifier,
}

impl SmithingTrimRecipe {
    pub fn matches(&self, input: &SmithingRecipeInput) -> bool {
        self.template.test(&input.template)
            && self.base.test(&input.base)
            && self.addition.test(&input.addition)
    }

    pub fn assemble(&self, input: &SmithingRecipeInput) -> ItemStack {
        // Paper `SmithingTrimRecipe.applyTrim`: resolve `PROVIDES_TRIM_MATERIAL`
        // from the addition stack (component override first, then vanilla item default).
        let Some(material) = resolve_trim_material(&input.addition) else {
            return ItemStack::empty();
        };
        let mut components = input.base.components().clone();
        let trim_key = Identifier::with_default_namespace("trim");
        let new_trim = format!("{}:{material}", self.pattern);
        // Paper: if the base already has the exact same trim, return EMPTY.
        if components
            .get(&trim_key)
            .is_some_and(|existing| existing == &new_trim)
        {
            return ItemStack::empty();
        }
        components.insert(trim_key, new_trim);
        ItemStack::with_components(input.base.item(), 1, components)
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create(vec![
            self.template.clone(),
            self.base.clone(),
            self.addition.clone(),
        ])
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:smithing_table").unwrap_or(ItemId::AIR);
        vec![RecipeDisplay::Smithing {
            template: self.template.display(),
            base: self.base.display(),
            addition: self.addition.display(),
            result: SlotDisplay::SmithingTrim {
                base: Box::new(self.base.display()),
                material: Box::new(self.addition.display()),
                pattern: self.pattern.clone(),
            },
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// Unified recipe sum type covering all standard vanilla recipe categories
/// plus a `Special` marker for dynamic/special crafting recipes (e.g. armor
/// dyeing, book cloning, map extending).
#[derive(Debug, Clone, PartialEq)]
pub enum Recipe {
    Shaped(ShapedRecipe),
    Shapeless(ShapelessRecipe),
    Cooking(CookingRecipe),
    Stonecutter(StonecutterRecipe),
    SmithingTransform(SmithingTransformRecipe),
    SmithingTrim(SmithingTrimRecipe),
    Special {
        serializer: RecipeSerializerId,
        category: CraftingBookCategory,
    },
}

impl Recipe {
    pub const fn recipe_type(&self) -> RecipeType {
        match self {
            Self::Shaped(_) | Self::Shapeless(_) | Self::Special { .. } => RecipeType::Crafting,
            Self::Cooking(c) => c.kind.recipe_type(),
            Self::Stonecutter(_) => RecipeType::Stonecutting,
            Self::SmithingTransform(_) | Self::SmithingTrim(_) => RecipeType::Smithing,
        }
    }

    pub const fn serializer(&self) -> RecipeSerializerId {
        match self {
            Self::Shaped(_) => RecipeSerializerId::CraftingShaped,
            Self::Shapeless(_) => RecipeSerializerId::CraftingShapeless,
            Self::Cooking(c) => c.kind.serializer(),
            Self::Stonecutter(_) => RecipeSerializerId::Stonecutting,
            Self::SmithingTransform(_) => RecipeSerializerId::SmithingTransform,
            Self::SmithingTrim(_) => RecipeSerializerId::SmithingTrim,
            Self::Special { serializer, .. } => *serializer,
        }
    }

    /// `Recipe.isSpecial()` — special recipes are excluded from recipe-book
    /// discovery and display unpacking.
    pub const fn is_special(&self) -> bool {
        matches!(self, Self::Special { .. })
    }

    /// `Recipe.showNotification()`.
    pub const fn show_notification(&self) -> bool {
        match self {
            Self::Shaped(r) => r.common_info.show_notification,
            Self::Shapeless(r) => r.common_info.show_notification,
            Self::Cooking(r) => r.common_info.show_notification,
            Self::Stonecutter(r) => r.common_info.show_notification,
            Self::SmithingTransform(r) => r.common_info.show_notification,
            Self::SmithingTrim(r) => r.common_info.show_notification,
            Self::Special { .. } => true,
        }
    }

    /// `Recipe.group()`.
    pub fn group(&self) -> &str {
        match self {
            Self::Shaped(r) => &r.book_info.group,
            Self::Shapeless(r) => &r.book_info.group,
            Self::Cooking(r) => &r.book_info.group,
            Self::Stonecutter(_)
            | Self::SmithingTransform(_)
            | Self::SmithingTrim(_)
            | Self::Special { .. } => "",
        }
    }

    /// `Recipe.recipeBookCategory()`.
    pub const fn recipe_book_category(&self) -> RecipeBookCategory {
        match self {
            Self::Shaped(r) => crafting_to_book_category(r.book_info.category),
            Self::Shapeless(r) => crafting_to_book_category(r.book_info.category),
            Self::Special { category, .. } => crafting_to_book_category(*category),
            Self::Cooking(r) => r.kind.recipe_book_category(r.book_info.category),
            Self::Stonecutter(_) => RecipeBookCategory::Stonecutter,
            Self::SmithingTransform(_) | Self::SmithingTrim(_) => RecipeBookCategory::Smithing,
        }
    }

    /// `Recipe.placementInfo()`.
    pub fn placement_info(&self) -> PlacementInfo {
        match self {
            Self::Shaped(r) => r.placement_info(),
            Self::Shapeless(r) => r.placement_info(),
            Self::Cooking(r) => r.placement_info(),
            Self::Stonecutter(r) => r.placement_info(),
            Self::SmithingTransform(r) => r.placement_info(),
            Self::SmithingTrim(r) => r.placement_info(),
            Self::Special { .. } => PlacementInfo::not_placeable(),
        }
    }

    /// `Recipe.display()`.
    pub fn display(&self) -> Vec<RecipeDisplay> {
        match self {
            Self::Shaped(r) => r.display(),
            Self::Shapeless(r) => r.display(),
            Self::Cooking(r) => r.display(),
            Self::Stonecutter(r) => r.display(),
            Self::SmithingTransform(r) => r.display(),
            Self::SmithingTrim(r) => r.display(),
            Self::Special { .. } => Vec::new(),
        }
    }

    pub fn matches_crafting(&self, input: &CraftingInput) -> bool {
        match self {
            Self::Shaped(r) => r.matches(input),
            Self::Shapeless(r) => r.matches(input),
            _ => false,
        }
    }

    pub fn assemble_crafting(&self, input: &CraftingInput) -> Option<ItemStack> {
        match self {
            Self::Shaped(r) => Some(r.assemble(input)),
            Self::Shapeless(r) => Some(r.assemble(input)),
            _ => None,
        }
    }

    pub fn matches_single(&self, input: &SingleRecipeInput) -> bool {
        match self {
            Self::Cooking(r) => r.matches(input),
            Self::Stonecutter(r) => r.matches(input),
            _ => false,
        }
    }

    pub fn assemble_single(&self, input: &SingleRecipeInput) -> Option<ItemStack> {
        match self {
            Self::Cooking(r) => Some(r.assemble(input)),
            Self::Stonecutter(r) => Some(r.assemble(input)),
            _ => None,
        }
    }

    pub fn matches_smithing(&self, input: &SmithingRecipeInput) -> bool {
        match self {
            Self::SmithingTransform(r) => r.matches(input),
            Self::SmithingTrim(r) => r.matches(input),
            _ => false,
        }
    }

    pub fn assemble_smithing(&self, input: &SmithingRecipeInput) -> Option<ItemStack> {
        match self {
            Self::SmithingTransform(r) => Some(r.assemble(input)),
            Self::SmithingTrim(r) => Some(r.assemble(input)),
            _ => None,
        }
    }
}

/// Resolve the `DataComponents.PROVIDES_TRIM_MATERIAL` value for a smithing
/// addition stack, matching Paper `TrimMaterials.getFromIngredient`.
pub fn resolve_trim_material(addition: &ItemStack) -> Option<String> {
    if addition.is_empty() {
        return None;
    }
    let override_key = Identifier::with_default_namespace("provides_trim_material");
    if let Some(mat) = addition.components().get(&override_key)
        && !mat.is_empty()
    {
        return Some(mat.clone());
    }
    let default_mat = match addition.item().name() {
        "minecraft:quartz" => "minecraft:quartz",
        "minecraft:iron_ingot" => "minecraft:iron",
        "minecraft:netherite_ingot" => "minecraft:netherite",
        "minecraft:redstone" => "minecraft:redstone",
        "minecraft:copper_ingot" => "minecraft:copper",
        "minecraft:gold_ingot" => "minecraft:gold",
        "minecraft:emerald" => "minecraft:emerald",
        "minecraft:diamond" => "minecraft:diamond",
        "minecraft:lapis_lazuli" => "minecraft:lapis",
        "minecraft:amethyst_shard" => "minecraft:amethyst",
        "minecraft:resin_brick" => "minecraft:resin",
        _ => return None,
    };
    Some(default_mat.to_owned())
}

/// `CraftingRecipe.defaultCraftingReminder(CraftingInput)`.
///
/// Produces a remainder list with the same length as `input`, placing each
/// slot's `Item.getCraftingRemainder()` template instance (or `EMPTY`).
pub fn default_crafting_remainder(input: &CraftingInput) -> Vec<ItemStack> {
    let mut remainders = vec![ItemStack::empty(); input.size()];
    for (slot, remainder) in remainders.iter_mut().enumerate() {
        let stack = input.get_item(slot);
        if !stack.is_empty()
            && let Some(template) = stack.item().crafting_remainder()
        {
            *remainder = template.create();
        }
    }
    remainders
}

const fn crafting_to_book_category(category: CraftingBookCategory) -> RecipeBookCategory {
    match category {
        CraftingBookCategory::Building => RecipeBookCategory::CraftingBuildingBlocks,
        CraftingBookCategory::Equipment => RecipeBookCategory::CraftingEquipment,
        CraftingBookCategory::Redstone => RecipeBookCategory::CraftingRedstone,
        CraftingBookCategory::Misc => RecipeBookCategory::CraftingMisc,
    }
}
