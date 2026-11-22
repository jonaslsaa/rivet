//! Vanilla Minecraft 26.2 JSON recipe loader.
//!
//! Deserializes standard datapack recipe JSONs (`crafting_shaped`,
//! `crafting_shapeless`, `smelting`, `blasting`, `smoking`,
//! `campfire_cooking`, `stonecutting`, `smithing_transform`, `smithing_trim`,
//! and special crafting serializers) into [`Recipe`] values.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use rivet_registry::Identifier;
use serde_json::Value;
use thiserror::Error;

use super::ingredient::{Ingredient, IngredientError};
use super::item::{ItemId, ItemStackTemplate};
use super::pattern::{PatternError, ShapedRecipePattern};
use super::recipe_type::{
    CommonInfo, CookingBookCategory, CookingBookInfo, CraftingBookCategory, CraftingBookInfo,
    RecipeSerializerId,
};
use super::recipes::{
    CookingKind, CookingRecipe, DyeRecipe, ImbueRecipe, Recipe, ShapedRecipe, ShapelessRecipe,
    SmithingTransformRecipe, SmithingTrimRecipe, StonecutterRecipe, TransmuteRecipe,
};

/// Errors when parsing a vanilla recipe JSON document.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RecipeJsonError {
    #[error("Recipe JSON must be an object")]
    NotAnObject,
    #[error("Missing required field '{0}'")]
    MissingField(&'static str),
    #[error("Invalid field '{field}': {message}")]
    InvalidField {
        field: &'static str,
        message: String,
    },
    #[error("Unknown recipe serializer type '{0}'")]
    UnknownSerializer(String),
    #[error("Unknown item '{0}'")]
    UnknownItem(String),
    #[error(transparent)]
    Ingredient(#[from] IngredientError),
    #[error(transparent)]
    Pattern(#[from] PatternError),
}

/// Parse a vanilla recipe JSON object into a [`Recipe`].
///
/// `tag_resolver` resolves `#namespace:tag` references into their item IDs.
pub fn parse_recipe_json<F>(value: &Value, tag_resolver: &F) -> Result<Recipe, RecipeJsonError>
where
    F: Fn(&Identifier) -> Vec<ItemId>,
{
    let obj = value.as_object().ok_or(RecipeJsonError::NotAnObject)?;
    let type_str = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RecipeJsonError::MissingField("type"))?;

    let serializer = RecipeSerializerId::from_name(type_str)
        .ok_or_else(|| RecipeJsonError::UnknownSerializer(type_str.to_owned()))?;

    let show_notification = obj
        .get("show_notification")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let common_info = CommonInfo { show_notification };
    let group = obj
        .get("group")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();

    match serializer {
        RecipeSerializerId::CraftingShaped => {
            let category = parse_crafting_category(obj.get("category"))?;
            let pattern_rows = parse_string_list(obj.get("pattern"), "pattern")?;
            let key_obj = obj
                .get("key")
                .and_then(Value::as_object)
                .ok_or(RecipeJsonError::MissingField("key"))?;
            let mut key = IndexMap::new();
            for (k, v) in key_obj {
                let mut chars = k.chars();
                let ch = chars.next().ok_or_else(|| RecipeJsonError::InvalidField {
                    field: "key",
                    message: "empty key symbol".to_owned(),
                })?;
                if chars.next().is_some() {
                    return Err(RecipeJsonError::InvalidField {
                        field: "key",
                        message: format!("key symbol '{k}' must be a single character"),
                    });
                }
                key.insert(ch, parse_ingredient(v, tag_resolver)?);
            }
            let pattern = ShapedRecipePattern::of(&key, &pattern_rows)?;
            let result = parse_result_template(obj.get("result"))?;
            Ok(Recipe::Shaped(ShapedRecipe {
                common_info,
                book_info: CraftingBookInfo { category, group },
                pattern,
                result,
            }))
        }
        RecipeSerializerId::CraftingShapeless => {
            let category = parse_crafting_category(obj.get("category"))?;
            let ing_arr = obj
                .get("ingredients")
                .and_then(Value::as_array)
                .ok_or(RecipeJsonError::MissingField("ingredients"))?;
            if ing_arr.is_empty() || ing_arr.len() > 9 {
                return Err(RecipeJsonError::InvalidField {
                    field: "ingredients",
                    message: format!(
                        "shapeless recipe must have between 1 and 9 ingredients, got {}",
                        ing_arr.len()
                    ),
                });
            }
            let mut ingredients = Vec::with_capacity(ing_arr.len());
            for item in ing_arr {
                ingredients.push(parse_ingredient(item, tag_resolver)?);
            }
            let result = parse_result_template(obj.get("result"))?;
            Ok(Recipe::Shapeless(ShapelessRecipe {
                common_info,
                book_info: CraftingBookInfo { category, group },
                ingredients,
                result,
            }))
        }
        RecipeSerializerId::Smelting
        | RecipeSerializerId::Blasting
        | RecipeSerializerId::Smoking
        | RecipeSerializerId::CampfireCooking => {
            let kind = match serializer {
                RecipeSerializerId::Smelting => CookingKind::Smelting,
                RecipeSerializerId::Blasting => CookingKind::Blasting,
                RecipeSerializerId::Smoking => CookingKind::Smoking,
                RecipeSerializerId::CampfireCooking => CookingKind::CampfireCooking,
                _ => unreachable!(),
            };
            let category = parse_cooking_category(obj.get("category"))?;
            let ing_val = obj
                .get("ingredient")
                .ok_or(RecipeJsonError::MissingField("ingredient"))?;
            let ingredient = parse_ingredient(ing_val, tag_resolver)?;
            let result = parse_result_template(obj.get("result"))?;
            let experience = obj
                .get("experience")
                .and_then(Value::as_f64)
                .map_or(0.0_f32, |v| v as f32);
            let cooking_time = obj
                .get("cookingtime")
                .and_then(Value::as_i64)
                .map_or(kind.default_cooking_time(), |v| v as i32);
            Ok(Recipe::Cooking(CookingRecipe {
                kind,
                common_info,
                book_info: CookingBookInfo { category, group },
                ingredient,
                result,
                experience,
                cooking_time,
            }))
        }
        RecipeSerializerId::Stonecutting => {
            let ing_val = obj
                .get("ingredient")
                .ok_or(RecipeJsonError::MissingField("ingredient"))?;
            let ingredient = parse_ingredient(ing_val, tag_resolver)?;
            let result = parse_result_template(obj.get("result"))?;
            Ok(Recipe::Stonecutter(StonecutterRecipe {
                common_info,
                ingredient,
                result,
            }))
        }
        RecipeSerializerId::SmithingTransform => {
            let template = obj
                .get("template")
                .map(|v| parse_ingredient(v, tag_resolver))
                .transpose()?;
            let base_val = obj
                .get("base")
                .ok_or(RecipeJsonError::MissingField("base"))?;
            let base = parse_ingredient(base_val, tag_resolver)?;
            let addition = obj
                .get("addition")
                .map(|v| parse_ingredient(v, tag_resolver))
                .transpose()?;
            let result = parse_result_template(obj.get("result"))?;
            let copy_data_components = obj
                .get("copy_data_components")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            Ok(Recipe::SmithingTransform(SmithingTransformRecipe {
                common_info,
                template,
                base,
                addition,
                result,
                copy_data_components,
            }))
        }
        RecipeSerializerId::SmithingTrim => {
            let template = parse_ingredient(
                obj.get("template")
                    .ok_or(RecipeJsonError::MissingField("template"))?,
                tag_resolver,
            )?;
            let base = parse_ingredient(
                obj.get("base")
                    .ok_or(RecipeJsonError::MissingField("base"))?,
                tag_resolver,
            )?;
            let addition = parse_ingredient(
                obj.get("addition")
                    .ok_or(RecipeJsonError::MissingField("addition"))?,
                tag_resolver,
            )?;
            let pattern_str = obj
                .get("pattern")
                .and_then(Value::as_str)
                .ok_or(RecipeJsonError::MissingField("pattern"))?;
            let pattern = parse_identifier(pattern_str, "pattern")?;
            Ok(Recipe::SmithingTrim(SmithingTrimRecipe {
                common_info,
                template,
                base,
                addition,
                pattern,
            }))
        }
        RecipeSerializerId::CraftingDye => {
            let category = parse_crafting_category(obj.get("category"))?;
            let target = parse_ingredient(
                obj.get("target")
                    .ok_or(RecipeJsonError::MissingField("target"))?,
                tag_resolver,
            )?;
            let dye = parse_ingredient(
                obj.get("dye").ok_or(RecipeJsonError::MissingField("dye"))?,
                tag_resolver,
            )?;
            let result = parse_result_template(obj.get("result"))?;
            Ok(Recipe::Dye(DyeRecipe {
                common_info,
                book_info: CraftingBookInfo { category, group },
                target,
                dye,
                result,
            }))
        }
        RecipeSerializerId::CraftingImbue => {
            let category = parse_crafting_category(obj.get("category"))?;
            let source = parse_ingredient(
                obj.get("source")
                    .ok_or(RecipeJsonError::MissingField("source"))?,
                tag_resolver,
            )?;
            let material = parse_ingredient(
                obj.get("material")
                    .ok_or(RecipeJsonError::MissingField("material"))?,
                tag_resolver,
            )?;
            let result = parse_result_template(obj.get("result"))?;
            Ok(Recipe::Imbue(ImbueRecipe {
                common_info,
                book_info: CraftingBookInfo { category, group },
                source,
                material,
                result,
            }))
        }
        RecipeSerializerId::CraftingTransmute => {
            let category = parse_crafting_category(obj.get("category"))?;
            let input = parse_ingredient(
                obj.get("input")
                    .ok_or(RecipeJsonError::MissingField("input"))?,
                tag_resolver,
            )?;
            let material = parse_ingredient(
                obj.get("material")
                    .ok_or(RecipeJsonError::MissingField("material"))?,
                tag_resolver,
            )?;
            let (min_material_count, max_material_count) =
                parse_material_count(obj.get("material_count"))?;
            let result = parse_result_template(obj.get("result"))?;
            let add_material_count_to_result = match obj.get("add_material_count_to_result") {
                None => false,
                Some(v) => v.as_bool().ok_or_else(|| RecipeJsonError::InvalidField {
                    field: "add_material_count_to_result",
                    message: "expected boolean".to_owned(),
                })?,
            };
            Ok(Recipe::Transmute(TransmuteRecipe {
                common_info,
                book_info: CraftingBookInfo { category, group },
                input,
                material,
                min_material_count,
                max_material_count,
                result,
                add_material_count_to_result,
            }))
        }
        other => {
            let category = parse_crafting_category(obj.get("category"))?;
            Ok(Recipe::Special {
                serializer: other,
                category,
            })
        }
    }
}

fn parse_crafting_category(value: Option<&Value>) -> Result<CraftingBookCategory, RecipeJsonError> {
    let Some(val) = value else {
        return Ok(CraftingBookCategory::Misc);
    };
    let s = val.as_str().ok_or_else(|| RecipeJsonError::InvalidField {
        field: "category",
        message: "expected string".to_owned(),
    })?;
    CraftingBookCategory::from_name(s).ok_or_else(|| RecipeJsonError::InvalidField {
        field: "category",
        message: format!("unknown crafting category '{s}'"),
    })
}

fn parse_cooking_category(value: Option<&Value>) -> Result<CookingBookCategory, RecipeJsonError> {
    let Some(val) = value else {
        return Ok(CookingBookCategory::Misc);
    };
    let s = val.as_str().ok_or_else(|| RecipeJsonError::InvalidField {
        field: "category",
        message: "expected string".to_owned(),
    })?;
    CookingBookCategory::from_name(s).ok_or_else(|| RecipeJsonError::InvalidField {
        field: "category",
        message: format!("unknown cooking category '{s}'"),
    })
}

fn parse_string_list(
    value: Option<&Value>,
    field: &'static str,
) -> Result<Vec<String>, RecipeJsonError> {
    let arr = value
        .and_then(Value::as_array)
        .ok_or(RecipeJsonError::MissingField(field))?;
    let mut out = Vec::with_capacity(arr.len());
    for v in arr {
        let s = v.as_str().ok_or_else(|| RecipeJsonError::InvalidField {
            field,
            message: "expected string element".to_owned(),
        })?;
        out.push(s.to_owned());
    }
    Ok(out)
}

fn parse_material_count(value: Option<&Value>) -> Result<(i32, i32), RecipeJsonError> {
    match value {
        None => Ok((1, 1)),
        Some(Value::Number(n)) => {
            let raw = n.as_i64().ok_or_else(|| RecipeJsonError::InvalidField {
                field: "material_count",
                message: "expected integer in 1..=8".to_owned(),
            })?;
            if !(1..=8).contains(&raw) {
                return Err(RecipeJsonError::InvalidField {
                    field: "material_count",
                    message: format!("material_count {raw} out of valid range 1..=8"),
                });
            }
            Ok((raw as i32, raw as i32))
        }
        Some(Value::Object(map)) => {
            let min = match map.get("min") {
                None => 1_i64,
                Some(v) => v.as_i64().ok_or_else(|| RecipeJsonError::InvalidField {
                    field: "material_count.min",
                    message: "expected integer in 1..=8".to_owned(),
                })?,
            };
            let max = match map.get("max") {
                None => 8_i64,
                Some(v) => v.as_i64().ok_or_else(|| RecipeJsonError::InvalidField {
                    field: "material_count.max",
                    message: "expected integer in 1..=8".to_owned(),
                })?,
            };
            if !(1..=8).contains(&min) || !(1..=8).contains(&max) || min > max {
                return Err(RecipeJsonError::InvalidField {
                    field: "material_count",
                    message: format!("invalid material_count bounds min={min} max={max}"),
                });
            }
            Ok((min as i32, max as i32))
        }
        Some(_) => Err(RecipeJsonError::InvalidField {
            field: "material_count",
            message: "expected integer or {min, max} bounds object".to_owned(),
        }),
    }
}

fn parse_identifier(raw: &str, field: &'static str) -> Result<Identifier, RecipeJsonError> {
    Identifier::try_parse_result(raw)
        .map_err(|err| RecipeJsonError::InvalidField {
            field,
            message: format!("invalid identifier '{raw}': {err}"),
        })?
        .ok_or_else(|| RecipeJsonError::InvalidField {
            field,
            message: format!("invalid identifier '{raw}'"),
        })
}

/// Parse a Minecraft 26.2 `Ingredient` JSON value:
/// - `"#minecraft:planks"` -> tag ingredient
/// - `"minecraft:stick"` -> single-item ingredient
/// - `["minecraft:oak_planks", "minecraft:birch_planks"]` -> direct item list
fn parse_ingredient<F>(value: &Value, tag_resolver: &F) -> Result<Ingredient, RecipeJsonError>
where
    F: Fn(&Identifier) -> Vec<ItemId>,
{
    match value {
        Value::String(s) => {
            if let Some(tag_str) = s.strip_prefix('#') {
                let tag = parse_identifier(tag_str, "ingredient")?;
                let items = tag_resolver(&tag);
                Ok(Ingredient::of_tag(tag, items)?)
            } else {
                let item =
                    ItemId::from_name(s).ok_or_else(|| RecipeJsonError::UnknownItem(s.clone()))?;
                Ok(Ingredient::of_item(item)?)
            }
        }
        Value::Array(arr) => {
            let mut items = Vec::with_capacity(arr.len());
            for elem in arr {
                let s = elem.as_str().ok_or_else(|| RecipeJsonError::InvalidField {
                    field: "ingredient",
                    message: "expected item string in ingredient list".to_owned(),
                })?;
                let item = ItemId::from_name(s)
                    .ok_or_else(|| RecipeJsonError::UnknownItem(s.to_owned()))?;
                items.push(item);
            }
            Ok(Ingredient::of_items(items)?)
        }
        _ => Err(RecipeJsonError::InvalidField {
            field: "ingredient",
            message: "expected item string, '#tag', or list of item strings".to_owned(),
        }),
    }
}

fn parse_result_template(value: Option<&Value>) -> Result<ItemStackTemplate, RecipeJsonError> {
    let val = value.ok_or(RecipeJsonError::MissingField("result"))?;
    match val {
        Value::String(s) => {
            let item =
                ItemId::from_name(s).ok_or_else(|| RecipeJsonError::UnknownItem(s.clone()))?;
            if item.is_air() {
                return Err(RecipeJsonError::InvalidField {
                    field: "result",
                    message: "result item must not be air".to_owned(),
                });
            }
            Ok(ItemStackTemplate::new(item, 1))
        }
        Value::Object(map) => {
            let id_str = map
                .get("id")
                .and_then(Value::as_str)
                .ok_or(RecipeJsonError::MissingField("result.id"))?;
            let item = ItemId::from_name(id_str)
                .ok_or_else(|| RecipeJsonError::UnknownItem(id_str.to_owned()))?;
            if item.is_air() {
                return Err(RecipeJsonError::InvalidField {
                    field: "result.id",
                    message: "result item must not be air".to_owned(),
                });
            }
            let count = match map.get("count") {
                None => 1_i32,
                Some(count_val) => {
                    let raw = count_val
                        .as_i64()
                        .ok_or_else(|| RecipeJsonError::InvalidField {
                            field: "result.count",
                            message: "expected integer count in 1..=99".to_owned(),
                        })?;
                    if !(1..=99).contains(&raw) {
                        return Err(RecipeJsonError::InvalidField {
                            field: "result.count",
                            message: format!("count {raw} out of valid range 1..=99"),
                        });
                    }
                    raw as i32
                }
            };
            let mut components = BTreeMap::new();
            let mut removed_components = BTreeSet::new();
            if let Some(comp_val) = map.get("components") {
                let comp_obj =
                    comp_val
                        .as_object()
                        .ok_or_else(|| RecipeJsonError::InvalidField {
                            field: "result.components",
                            message: "expected object".to_owned(),
                        })?;
                for (k, v) in comp_obj {
                    if let Some(stripped) = k.strip_prefix('!') {
                        let comp_id = parse_identifier(stripped, "result.components")?;
                        components.remove(&comp_id);
                        removed_components.insert(comp_id);
                    } else {
                        let comp_id = parse_identifier(k, "result.components")?;
                        let serialized = match v {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        removed_components.remove(&comp_id);
                        components.insert(comp_id, serialized);
                    }
                }
            }
            Ok(ItemStackTemplate::with_component_patch(
                item,
                count,
                components,
                removed_components,
            ))
        }
        _ => Err(RecipeJsonError::InvalidField {
            field: "result",
            message: "expected item id string or object {id, count}".to_owned(),
        }),
    }
}
