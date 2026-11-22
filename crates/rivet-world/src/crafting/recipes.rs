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

    fn parse_trim_component(raw: &str) -> Option<(Identifier, Identifier)> {
        if let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(raw) {
            let material = map
                .get("material")
                .and_then(serde_json::Value::as_str)
                .and_then(Identifier::try_parse)?;
            let pattern = map
                .get("pattern")
                .and_then(serde_json::Value::as_str)
                .and_then(Identifier::try_parse)?;
            return Some((pattern, material));
        }
        None
    }

    pub fn assemble(&self, input: &SmithingRecipeInput) -> ItemStack {
        // Paper `SmithingTrimRecipe.applyTrim`: resolve `PROVIDES_TRIM_MATERIAL`
        // from the addition stack (component override first, then vanilla item default).
        let Some(material_raw) = resolve_trim_material(&input.addition) else {
            return ItemStack::empty();
        };
        let Some(material) = Identifier::try_parse(&material_raw) else {
            return ItemStack::empty();
        };
        let mut components = input.base.components().clone();
        let trim_key = Identifier::with_default_namespace("trim");
        // Paper: if the base already has the exact same trim (pattern + material), return EMPTY.
        if components
            .get(&trim_key)
            .and_then(|existing| Self::parse_trim_component(existing))
            .is_some_and(|(pat, mat)| pat == self.pattern && mat == material)
        {
            return ItemStack::empty();
        }
        let new_trim = serde_json::json!({
            "material": material.to_string(),
            "pattern": self.pattern.to_string(),
        })
        .to_string();
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

/// `net.minecraft.world.item.crafting.DyeRecipe`.
#[derive(Debug, Clone, PartialEq)]
pub struct DyeRecipe {
    pub common_info: CommonInfo,
    pub book_info: CraftingBookInfo,
    pub target: Ingredient,
    pub dye: Ingredient,
    pub result: ItemStackTemplate,
}

impl DyeRecipe {
    fn has_dye_component(stack: &ItemStack) -> bool {
        stack
            .components()
            .contains_key(&Identifier::with_default_namespace("dye"))
            || stack.item().name().ends_with("_dye")
    }

    fn dye_name(stack: &ItemStack) -> &str {
        if let Some(dye) = stack
            .components()
            .get(&Identifier::with_default_namespace("dye"))
        {
            return dye.as_str();
        }
        stack
            .item()
            .name()
            .strip_prefix("minecraft:")
            .and_then(|s| s.strip_suffix("_dye"))
            .unwrap_or("white")
    }

    /// Paper `DyeColor.getTextureDiffuseColor()` RGB values (opaque 24-bit RGB).
    fn dye_texture_diffuse_rgb(name: &str) -> i32 {
        match name {
            "white" => 16_383_998,
            "orange" => 16_351_261,
            "magenta" => 13_061_821,
            "light_blue" => 3_847_130,
            "yellow" => 16_701_501,
            "lime" => 8_439_583,
            "pink" => 15_961_002,
            "gray" => 4_673_362,
            "light_gray" => 10_329_495,
            "cyan" => 1_481_884,
            "purple" => 8_991_416,
            "blue" => 3_949_738,
            "brown" => 8_606_770,
            "green" => 6_192_150,
            "red" => 11_546_150,
            "black" => 1_908_001,
            _ => 16_383_998, // DyeColor.WHITE default
        }
    }

    fn parse_dyed_color_rgb(raw: &str) -> Option<i32> {
        let trimmed = raw.trim();
        if let Ok(rgb) = trimmed.parse::<i32>() {
            return Some(rgb & 0x00FF_FFFF);
        }
        if let Some(hex) = trimmed.strip_prefix('#')
            && let Ok(rgb) = i32::from_str_radix(hex, 16)
        {
            return Some(rgb & 0x00FF_FFFF);
        }
        if let Ok(serde_json::Value::Object(map)) =
            serde_json::from_str::<serde_json::Value>(trimmed)
            && let Some(rgb) = map.get("rgb").and_then(serde_json::Value::as_i64)
        {
            return Some((rgb as i32) & 0x00FF_FFFF);
        }
        None
    }

    /// Exact port of Paper `DyedItemColor.applyDyes(DyedItemColor, List<DyeColor>)`.
    fn apply_dyes(current_rgb: Option<i32>, dye_rgbs: &[i32]) -> i32 {
        let mut red_total = 0_i32;
        let mut green_total = 0_i32;
        let mut blue_total = 0_i32;
        let mut intensity_total = 0_i32;
        let mut color_count = 0_i32;

        if let Some(rgb) = current_rgb {
            let red = (rgb >> 16) & 0xFF;
            let green = (rgb >> 8) & 0xFF;
            let blue = rgb & 0xFF;
            intensity_total = intensity_total.wrapping_add(red.max(green.max(blue)));
            red_total = red_total.wrapping_add(red);
            green_total = green_total.wrapping_add(green);
            blue_total = blue_total.wrapping_add(blue);
            color_count = color_count.wrapping_add(1);
        }

        for &color in dye_rgbs {
            let red = (color >> 16) & 0xFF;
            let green = (color >> 8) & 0xFF;
            let blue = color & 0xFF;
            intensity_total = intensity_total.wrapping_add(red.max(green.max(blue)));
            red_total = red_total.wrapping_add(red);
            green_total = green_total.wrapping_add(green);
            blue_total = blue_total.wrapping_add(blue);
            color_count = color_count.wrapping_add(1);
        }

        if color_count == 0 {
            return 0;
        }

        let mut red = red_total / color_count;
        let mut green = green_total / color_count;
        let mut blue = blue_total / color_count;
        let average_intensity = intensity_total as f32 / color_count as f32;
        let result_intensity = red.max(green.max(blue)) as f32;
        if result_intensity > 0.0 {
            red = (red as f32 * average_intensity / result_intensity) as i32;
            green = (green as f32 * average_intensity / result_intensity) as i32;
            blue = (blue as f32 * average_intensity / result_intensity) as i32;
        }
        ((red & 0xFF) << 16) | ((green & 0xFF) << 8) | (blue & 0xFF)
    }

    /// `DyeRecipe.matches(CraftingInput, Level)`.
    pub fn matches(&self, input: &CraftingInput) -> bool {
        if input.ingredient_count() < 2 {
            return false;
        }
        let mut has_target = false;
        let mut has_dyes = false;
        for slot in 0..input.size() {
            let stack = input.get_item(slot);
            if !stack.is_empty() {
                if self.target.test(stack) {
                    if has_target {
                        return false;
                    }
                    has_target = true;
                } else {
                    if !self.dye.test(stack) || !Self::has_dye_component(stack) {
                        return false;
                    }
                    has_dyes = true;
                }
            }
        }
        has_dyes && has_target
    }

    /// `DyeRecipe.assemble(CraftingInput)`.
    pub fn assemble(&self, input: &CraftingInput) -> ItemStack {
        let mut dye_rgbs = Vec::new();
        let mut target_stack: Option<&ItemStack> = None;
        for slot in 0..input.size() {
            let stack = input.get_item(slot);
            if !stack.is_empty() {
                if self.target.test(stack) {
                    if target_stack.is_some() {
                        return ItemStack::empty();
                    }
                    target_stack = Some(stack);
                } else {
                    if !self.dye.test(stack) {
                        return ItemStack::empty();
                    }
                    dye_rgbs.push(Self::dye_texture_diffuse_rgb(Self::dye_name(stack)));
                }
            }
        }
        let Some(target) = target_stack else {
            return ItemStack::empty();
        };
        if dye_rgbs.is_empty() {
            return ItemStack::empty();
        }
        let dyed_key = Identifier::with_default_namespace("dyed_color");
        let current_rgb = target
            .components()
            .get(&dyed_key)
            .and_then(|raw| Self::parse_dyed_color_rgb(raw));
        let new_rgb = Self::apply_dyes(current_rgb, &dye_rgbs);
        let mut out = self.result.create_with_original_components(target);
        out.components_mut().insert(dyed_key, new_rgb.to_string());
        out
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create(vec![self.target.clone(), self.dye.clone()])
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:crafting_table").unwrap_or(ItemId::AIR);
        let dyes_with_comp = SlotDisplay::OnlyWithComponent {
            source: Box::new(self.dye.display()),
            component: Identifier::with_default_namespace("dye"),
        };
        let target_display = self.target.display();
        vec![RecipeDisplay::ShapelessCrafting {
            ingredients: vec![target_display.clone(), dyes_with_comp.clone()],
            result: SlotDisplay::Dyed {
                dye: Box::new(dyes_with_comp),
                target: Box::new(target_display),
            },
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// `net.minecraft.world.item.crafting.ImbueRecipe`.
#[derive(Debug, Clone, PartialEq)]
pub struct ImbueRecipe {
    pub common_info: CommonInfo,
    pub book_info: CraftingBookInfo,
    pub source: Ingredient,
    pub material: Ingredient,
    pub result: ItemStackTemplate,
}

impl ImbueRecipe {
    /// `ImbueRecipe.matches(CraftingInput, Level)`.
    pub fn matches(&self, input: &CraftingInput) -> bool {
        if input.width() != 3 || input.height() != 3 || input.ingredient_count() != 9 {
            return false;
        }
        for y in 0..3 {
            for x in 0..3 {
                let stack = input.get_item_xy(x, y);
                if stack.is_empty() {
                    return false;
                }
                let ing = if x == 1 && y == 1 {
                    &self.source
                } else {
                    &self.material
                };
                if !ing.test(stack) {
                    return false;
                }
            }
        }
        true
    }

    /// `ImbueRecipe.assemble(CraftingInput)`.
    pub fn assemble(&self, input: &CraftingInput) -> ItemStack {
        let source = input.get_item_xy(1, 1);
        let mut out = self.result.create();
        let potion_key = Identifier::with_default_namespace("potion_contents");
        if let Some(potion) = source.components().get(&potion_key) {
            out.components_mut().insert(potion_key, potion.clone());
        } else {
            out.components_mut().remove(&potion_key);
        }
        out
    }

    pub fn placement_info(&self) -> PlacementInfo {
        PlacementInfo::create(vec![
            self.material.clone(),
            self.material.clone(),
            self.material.clone(),
            self.material.clone(),
            self.source.clone(),
            self.material.clone(),
            self.material.clone(),
            self.material.clone(),
            self.material.clone(),
        ])
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:crafting_table").unwrap_or(ItemId::AIR);
        let mat = self.material.display();
        let src = SlotDisplay::WithAnyPotion {
            display: Box::new(self.source.display()),
        };
        vec![RecipeDisplay::ShapedCrafting {
            width: 3,
            height: 3,
            ingredients: vec![
                mat.clone(),
                mat.clone(),
                mat.clone(),
                mat.clone(),
                src,
                mat.clone(),
                mat.clone(),
                mat.clone(),
                mat,
            ],
            result: SlotDisplay::WithAnyPotion {
                display: Box::new(SlotDisplay::ItemStack(self.result.clone())),
            },
            crafting_station: SlotDisplay::Item(station),
        }]
    }
}

/// `net.minecraft.world.item.crafting.TransmuteRecipe`.
#[derive(Debug, Clone, PartialEq)]
pub struct TransmuteRecipe {
    pub common_info: CommonInfo,
    pub book_info: CraftingBookInfo,
    pub input: Ingredient,
    pub material: Ingredient,
    pub min_material_count: i32,
    pub max_material_count: i32,
    pub result: ItemStackTemplate,
    pub add_material_count_to_result: bool,
}

impl TransmuteRecipe {
    fn compute_result_size(&self, material_count: i32) -> i32 {
        if self.add_material_count_to_result {
            material_count.wrapping_add(self.result.count)
        } else {
            self.result.count
        }
    }

    fn compute_result(&self, input_ingredient: &ItemStack, extra_count: i32) -> ItemStack {
        let out = self
            .result
            .create_with_original_components(input_ingredient);
        let count = self.result.count.wrapping_add(extra_count);
        out.copy_with_count(count)
    }

    /// `TransmuteRecipe.matches(CraftingInput, Level)`.
    pub fn matches(&self, input: &CraftingInput) -> bool {
        let min_mc = self.min_material_count;
        let max_mc = self.max_material_count;
        let count = input.ingredient_count();
        if count < min_mc + 1 || count > max_mc + 1 {
            return false;
        }
        let mut found_input: Option<&ItemStack> = None;
        let mut material_count = 0_i32;
        for slot in 0..input.size() {
            let stack = input.get_item(slot);
            if !stack.is_empty() {
                if self.input.test(stack) {
                    if found_input.is_some() {
                        return false;
                    }
                    found_input = Some(stack);
                } else {
                    if !self.material.test(stack) {
                        return false;
                    }
                    material_count += 1;
                    if material_count > max_mc {
                        return false;
                    }
                }
            }
        }
        let Some(found) = found_input else {
            return false;
        };
        if found.is_empty() || material_count < min_mc || material_count > max_mc {
            return false;
        }
        let result_count = self.compute_result_size(material_count);
        if result_count != 1 {
            return true;
        }
        let result = self.compute_result(found, 0);
        !result.is_empty() && !found.is_same_item_same_components(&result)
    }

    /// `TransmuteRecipe.assemble(CraftingInput)`.
    pub fn assemble(&self, input: &CraftingInput) -> ItemStack {
        if self.add_material_count_to_result {
            let mut material_count = 0_i32;
            let mut input_ingredient: Option<&ItemStack> = None;
            for slot in 0..input.size() {
                let stack = input.get_item(slot);
                if !stack.is_empty() {
                    if self.input.test(stack) {
                        input_ingredient = Some(stack);
                    } else if self.material.test(stack) {
                        material_count += 1;
                    }
                }
            }
            match input_ingredient {
                Some(ing) => self.compute_result(ing, material_count),
                None => ItemStack::empty(),
            }
        } else {
            for slot in 0..input.size() {
                let stack = input.get_item(slot);
                if !stack.is_empty() && self.input.test(stack) {
                    return self.compute_result(stack, 0);
                }
            }
            ItemStack::empty()
        }
    }

    pub fn placement_info(&self) -> PlacementInfo {
        let max_mc = self.max_material_count.max(1) as usize;
        let mut ingredients = Vec::with_capacity(1 + max_mc);
        ingredients.push(self.input.clone());
        for _ in 0..max_mc {
            ingredients.push(self.material.clone());
        }
        PlacementInfo::create(ingredients)
    }

    pub fn display(&self) -> Vec<RecipeDisplay> {
        let station = ItemId::from_name("minecraft:crafting_table").unwrap_or(ItemId::AIR);
        let mut displays = Vec::new();
        let material_display = self.material.display();
        for mc in self.min_material_count..=self.max_material_count {
            let mut ingredient_slots = Vec::with_capacity(1 + mc.max(0) as usize);
            ingredient_slots.push(self.input.display());
            for _ in 0..mc.max(0) {
                ingredient_slots.push(material_display.clone());
            }
            let result_count = self.compute_result_size(mc);
            let res_template = ItemStackTemplate::with_components(
                self.result.item,
                result_count,
                self.result.components.clone(),
            );
            displays.push(RecipeDisplay::ShapelessCrafting {
                ingredients: ingredient_slots,
                result: SlotDisplay::ItemStack(res_template),
                crafting_station: SlotDisplay::Item(station),
            });
        }
        displays
    }
}

/// Unified recipe sum type covering all standard vanilla recipe categories
/// plus a `Special` marker for dynamic/special crafting recipes (e.g.
/// book cloning, map extending, firework crafting).
#[derive(Debug, Clone, PartialEq)]
pub enum Recipe {
    Shaped(ShapedRecipe),
    Shapeless(ShapelessRecipe),
    Dye(DyeRecipe),
    Imbue(ImbueRecipe),
    Transmute(TransmuteRecipe),
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
            Self::Shaped(_)
            | Self::Shapeless(_)
            | Self::Dye(_)
            | Self::Imbue(_)
            | Self::Transmute(_)
            | Self::Special { .. } => RecipeType::Crafting,
            Self::Cooking(c) => c.kind.recipe_type(),
            Self::Stonecutter(_) => RecipeType::Stonecutting,
            Self::SmithingTransform(_) | Self::SmithingTrim(_) => RecipeType::Smithing,
        }
    }

    pub const fn serializer(&self) -> RecipeSerializerId {
        match self {
            Self::Shaped(_) => RecipeSerializerId::CraftingShaped,
            Self::Shapeless(_) => RecipeSerializerId::CraftingShapeless,
            Self::Dye(_) => RecipeSerializerId::CraftingDye,
            Self::Imbue(_) => RecipeSerializerId::CraftingImbue,
            Self::Transmute(_) => RecipeSerializerId::CraftingTransmute,
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
            Self::Dye(r) => r.common_info.show_notification,
            Self::Imbue(r) => r.common_info.show_notification,
            Self::Transmute(r) => r.common_info.show_notification,
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
            Self::Dye(r) => &r.book_info.group,
            Self::Imbue(r) => &r.book_info.group,
            Self::Transmute(r) => &r.book_info.group,
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
            Self::Dye(r) => crafting_to_book_category(r.book_info.category),
            Self::Imbue(r) => crafting_to_book_category(r.book_info.category),
            Self::Transmute(r) => crafting_to_book_category(r.book_info.category),
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
            Self::Dye(r) => r.placement_info(),
            Self::Imbue(r) => r.placement_info(),
            Self::Transmute(r) => r.placement_info(),
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
            Self::Dye(r) => r.display(),
            Self::Imbue(r) => r.display(),
            Self::Transmute(r) => r.display(),
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
            Self::Dye(r) => r.matches(input),
            Self::Imbue(r) => r.matches(input),
            Self::Transmute(r) => r.matches(input),
            _ => false,
        }
    }

    pub fn assemble_crafting(&self, input: &CraftingInput) -> Option<ItemStack> {
        match self {
            Self::Shaped(r) => Some(r.assemble(input)),
            Self::Shapeless(r) => Some(r.assemble(input)),
            Self::Dye(r) => Some(r.assemble(input)),
            Self::Imbue(r) => Some(r.assemble(input)),
            Self::Transmute(r) => Some(r.assemble(input)),
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
