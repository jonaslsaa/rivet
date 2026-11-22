//! Shaped recipe pattern parsing, shrinking, symmetry, and matching.
//!
//! Mirrors Paper 26.2 `net.minecraft.world.item.crafting.ShapedRecipePattern`.

use indexmap::{IndexMap, IndexSet};
use thiserror::Error;

use super::ingredient::Ingredient;
use super::input::CraftingInput;

/// Validation errors when constructing a [`ShapedRecipePattern`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PatternError {
    #[error("Invalid pattern: empty pattern not allowed")]
    EmptyPattern,
    #[error("Invalid pattern: too many rows, 3 is maximum")]
    TooManyRows,
    #[error("Invalid pattern: too many columns, 3 is maximum")]
    TooManyColumns,
    #[error("Invalid pattern: each row must be the same width")]
    UnevenRows,
    #[error("Pattern references symbol '{0}' but it's not defined in the key")]
    UndefinedSymbol(char),
    #[error("Key defines symbols that aren't used in pattern: {0:?}")]
    UnusedSymbols(Vec<char>),
    #[error("Invalid key entry: ' ' is a reserved symbol")]
    SpaceInKey,
    #[error("Pattern must contain at least one ingredient")]
    AllEmpty,
}

/// `net.minecraft.world.item.crafting.ShapedRecipePattern`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapedRecipePattern {
    width: i32,
    height: i32,
    ingredients: Vec<Option<Ingredient>>,
    ingredient_count: i32,
    symmetrical: bool,
}

impl ShapedRecipePattern {
    pub const MAX_WIDTH: usize = 3;
    pub const MAX_HEIGHT: usize = 3;

    /// `ShapedRecipePattern.of(Map<Character, Ingredient> key, List<String> pattern)`.
    pub fn of(key: &IndexMap<char, Ingredient>, pattern: &[String]) -> Result<Self, PatternError> {
        if key.contains_key(&' ') {
            return Err(PatternError::SpaceInKey);
        }
        if pattern.is_empty() {
            return Err(PatternError::EmptyPattern);
        }
        if pattern.len() > Self::MAX_HEIGHT {
            return Err(PatternError::TooManyRows);
        }

        let first_len = pattern[0].chars().count();
        if first_len > Self::MAX_WIDTH {
            return Err(PatternError::TooManyColumns);
        }
        for row in pattern {
            if row.chars().count() != first_len {
                return Err(PatternError::UnevenRows);
            }
        }

        let shrunk = shrink(pattern);
        if shrunk.is_empty() {
            return Err(PatternError::AllEmpty);
        }

        let width = shrunk[0].chars().count() as i32;
        let height = shrunk.len() as i32;
        let mut ingredients = Vec::with_capacity((width * height) as usize);
        let mut used_symbols: IndexSet<char> = IndexSet::new();
        let mut ingredient_count = 0_i32;

        for row in &shrunk {
            for ch in row.chars() {
                if ch == ' ' {
                    ingredients.push(None);
                } else {
                    let ing = key
                        .get(&ch)
                        .ok_or(PatternError::UndefinedSymbol(ch))?
                        .clone();
                    used_symbols.insert(ch);
                    ingredients.push(Some(ing));
                    ingredient_count = ingredient_count.wrapping_add(1);
                }
            }
        }

        let unused: Vec<char> = key
            .keys()
            .copied()
            .filter(|ch| !used_symbols.contains(ch))
            .collect();
        if !unused.is_empty() {
            return Err(PatternError::UnusedSymbols(unused));
        }

        let symmetrical = is_symmetrical(width, height, &ingredients);

        Ok(Self {
            width,
            height,
            ingredients,
            ingredient_count,
            symmetrical,
        })
    }

    pub const fn width(&self) -> i32 {
        self.width
    }

    pub const fn height(&self) -> i32 {
        self.height
    }

    pub fn ingredients(&self) -> &[Option<Ingredient>] {
        &self.ingredients
    }

    pub const fn ingredient_count(&self) -> i32 {
        self.ingredient_count
    }

    pub const fn is_symmetrical(&self) -> bool {
        self.symmetrical
    }

    /// `ShapedRecipePattern.matches(CraftingInput)`.
    ///
    /// Mirrors Paper exactly:
    /// 1. `input.ingredientCount() != this.ingredientCount` -> false
    /// 2. `input.width() == this.width && input.height() == this.height`
    /// 3. If `!symmetrical`, tries horizontally flipped first, then unflipped.
    pub fn matches(&self, input: &CraftingInput) -> bool {
        if input.ingredient_count() != self.ingredient_count {
            return false;
        }
        if input.width() == self.width && input.height() == self.height {
            if !self.symmetrical && self.matches_oriented(input, true) {
                return true;
            }
            if self.matches_oriented(input, false) {
                return true;
            }
        }
        false
    }

    fn matches_oriented(&self, input: &CraftingInput, mirrored: bool) -> bool {
        for y in 0..self.height {
            for x in 0..self.width {
                let pattern_x = if mirrored { self.width - x - 1 } else { x };
                let ingredient = &self.ingredients[(pattern_x + y * self.width) as usize];
                let stack = input.get_item_xy(x, y);
                if !Ingredient::test_optional(ingredient.as_ref(), stack) {
                    return false;
                }
            }
        }
        true
    }
}

/// `ShapedRecipePattern.shrink(List<String>)` — trims leading/trailing empty
/// rows and columns.
fn shrink(pattern: &[String]) -> Vec<String> {
    let mut left = usize::MAX;
    let mut right = 0usize;
    let mut top = 0usize;
    let mut bottom = 0usize;

    for (row_idx, row) in pattern.iter().enumerate() {
        let first = first_non_empty(row);
        let last = last_non_empty(row);
        if first == row.chars().count() {
            if bottom == 0 {
                top += 1;
            }
        } else {
            left = left.min(first);
            right = right.max(last);
            bottom = row_idx + 1;
        }
    }

    if bottom <= top || left == usize::MAX {
        return Vec::new();
    }

    pattern[top..bottom]
        .iter()
        .map(|row| row.chars().skip(left).take(right - left + 1).collect())
        .collect()
}

fn first_non_empty(row: &str) -> usize {
    row.chars()
        .position(|c| c != ' ')
        .unwrap_or_else(|| row.chars().count())
}

fn last_non_empty(row: &str) -> usize {
    let chars: Vec<char> = row.chars().collect();
    for i in (0..chars.len()).rev() {
        if chars[i] != ' ' {
            return i;
        }
    }
    0
}

/// Mirrors `Util.isSymmetrical(width, height, ingredients)`.
fn is_symmetrical(width: i32, height: i32, ingredients: &[Option<Ingredient>]) -> bool {
    for y in 0..height {
        for x in 0..(width / 2) {
            let left = &ingredients[(x + y * width) as usize];
            let right = &ingredients[((width - 1 - x) + y * width) as usize];
            if left != right {
                return false;
            }
        }
    }
    true
}
