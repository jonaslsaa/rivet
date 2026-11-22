//! Recipe input containers: `RecipeInput`, `CraftingInput`, `SingleRecipeInput`,
//! and `SmithingRecipeInput`.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.item.crafting.RecipeInput`
//! - `net.minecraft.world.item.crafting.CraftingInput`
//! - `net.minecraft.world.item.crafting.SingleRecipeInput`
//! - `net.minecraft.world.item.crafting.SmithingRecipeInput`

use super::item::ItemStack;

/// `net.minecraft.world.item.crafting.RecipeInput`.
pub trait RecipeInput {
    fn get_item(&self, index: usize) -> &ItemStack;
    fn size(&self) -> usize;

    fn is_empty(&self) -> bool {
        (0..self.size()).all(|i| self.get_item(i).is_empty())
    }
}

/// `net.minecraft.world.item.crafting.CraftingInput`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CraftingInput {
    width: i32,
    height: i32,
    items: Vec<ItemStack>,
    ingredient_count: i32,
}

/// `net.minecraft.world.item.crafting.CraftingInput.Positioned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PositionedCraftingInput {
    pub input: CraftingInput,
    pub left: i32,
    pub top: i32,
}

impl PositionedCraftingInput {
    pub const fn empty() -> Self {
        Self {
            input: CraftingInput::empty(),
            left: 0,
            top: 0,
        }
    }
}

impl CraftingInput {
    /// `CraftingInput.EMPTY`.
    pub const fn empty() -> Self {
        Self {
            width: 0,
            height: 0,
            items: Vec::new(),
            ingredient_count: 0,
        }
    }

    /// `CraftingInput.of(int width, int height, List<ItemStack> items)`.
    pub fn of(width: i32, height: i32, items: Vec<ItemStack>) -> Self {
        Self::of_positioned(width, height, items).input
    }

    /// `CraftingInput.ofPositioned(int width, int height, List<ItemStack> items)`.
    ///
    /// Computes the tight non-empty bounding box, trims empty border rows and
    /// columns, preserves `(left, top)`, and counts non-empty stacks.
    pub fn of_positioned(
        width: i32,
        height: i32,
        items: Vec<ItemStack>,
    ) -> PositionedCraftingInput {
        if width <= 0 || height <= 0 {
            return PositionedCraftingInput::empty();
        }
        assert_eq!(
            items.len(),
            (width as usize) * (height as usize),
            "CraftingInput items length must equal width * height"
        );

        let mut left = width - 1;
        let mut right = 0;
        let mut top = height - 1;
        let mut bottom = 0;

        for y in 0..height {
            let mut row_empty = true;
            for x in 0..width {
                let stack = &items[(x + y * width) as usize];
                if !stack.is_empty() {
                    left = left.min(x);
                    right = right.max(x);
                    row_empty = false;
                }
            }
            if !row_empty {
                top = top.min(y);
                bottom = bottom.max(y);
            }
        }

        let trimmed_w = right - left + 1;
        let trimmed_h = bottom - top + 1;
        if trimmed_w <= 0 || trimmed_h <= 0 {
            return PositionedCraftingInput::empty();
        }

        if trimmed_w == width && trimmed_h == height {
            let ingredient_count = items.iter().filter(|s| !s.is_empty()).count() as i32;
            return PositionedCraftingInput {
                input: Self {
                    width,
                    height,
                    items,
                    ingredient_count,
                },
                left,
                top,
            };
        }

        let mut trimmed = Vec::with_capacity((trimmed_w * trimmed_h) as usize);
        let mut ingredient_count = 0_i32;
        for y in 0..trimmed_h {
            for x in 0..trimmed_w {
                let stack = items[((x + left) + (y + top) * width) as usize].clone();
                if !stack.is_empty() {
                    ingredient_count = ingredient_count.wrapping_add(1);
                }
                trimmed.push(stack);
            }
        }

        PositionedCraftingInput {
            input: Self {
                width: trimmed_w,
                height: trimmed_h,
                items: trimmed,
                ingredient_count,
            },
            left,
            top,
        }
    }

    pub const fn width(&self) -> i32 {
        self.width
    }

    pub const fn height(&self) -> i32 {
        self.height
    }

    pub const fn ingredient_count(&self) -> i32 {
        self.ingredient_count
    }

    pub fn items(&self) -> &[ItemStack] {
        &self.items
    }

    /// `CraftingInput.getItem(int x, int y)`.
    pub fn get_item_xy(&self, x: i32, y: i32) -> &ItemStack {
        &self.items[(x + y * self.width) as usize]
    }
}

impl RecipeInput for CraftingInput {
    fn get_item(&self, index: usize) -> &ItemStack {
        &self.items[index]
    }

    fn size(&self) -> usize {
        self.items.len()
    }

    fn is_empty(&self) -> bool {
        self.ingredient_count == 0
    }
}

/// `net.minecraft.world.item.crafting.SingleRecipeInput`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleRecipeInput {
    pub item: ItemStack,
}

impl SingleRecipeInput {
    pub const fn new(item: ItemStack) -> Self {
        Self { item }
    }
}

impl RecipeInput for SingleRecipeInput {
    fn get_item(&self, index: usize) -> &ItemStack {
        assert_eq!(index, 0, "No item for index {index}");
        &self.item
    }

    fn size(&self) -> usize {
        1
    }

    fn is_empty(&self) -> bool {
        self.item.is_empty()
    }
}

/// `net.minecraft.world.item.crafting.SmithingRecipeInput`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmithingRecipeInput {
    pub template: ItemStack,
    pub base: ItemStack,
    pub addition: ItemStack,
}

impl SmithingRecipeInput {
    pub const fn new(template: ItemStack, base: ItemStack, addition: ItemStack) -> Self {
        Self {
            template,
            base,
            addition,
        }
    }
}

impl RecipeInput for SmithingRecipeInput {
    fn get_item(&self, index: usize) -> &ItemStack {
        match index {
            0 => &self.template,
            1 => &self.base,
            2 => &self.addition,
            _ => panic!("Recipe does not contain slot {index}"),
        }
    }

    fn size(&self) -> usize {
        3
    }

    fn is_empty(&self) -> bool {
        self.template.is_empty() && self.base.is_empty() && self.addition.is_empty()
    }
}
