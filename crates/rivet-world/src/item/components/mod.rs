//! Port of Paper 26.2 leaf item data-component value families and codecs
//! (`net.minecraft.world.item.component.*`, `net.minecraft.world.food.FoodProperties`,
//! `net.minecraft.server.network.Filterable`, `net.minecraft.world.item.consume_effects.*`,
//! `net.minecraft.world.item.enchantment.{Enchantable, Repairable}`).
//!
//! These value types sit above the recursive I1 `ItemStack`/`DataComponents` SCC
//! (issue #431) and build on the existing `rivet_world::item` identity and
//! `ItemStackTemplate` seams.

pub mod combat;
pub mod consumable;
pub mod core;
pub mod display;
pub mod world_and_books;

#[cfg(test)]
mod tests;

pub use combat::{
    AXE_DISABLES_BLOCKING_FOR_SECONDS, AttackRange, BlocksAttacks, DamageReduction,
    DamageResistant, Enchantable, ItemDamageFunction, KineticWeapon, KineticWeaponCondition,
    PiercingWeapon, Repairable, SwingAnimation, SwingAnimationType, Tool, ToolRule, Weapon,
    attack_range_codec, blocks_attacks_codec, damage_reduction_codec, damage_resistant_codec,
    enchantable_codec, item_damage_function_codec, kinetic_weapon_codec,
    kinetic_weapon_condition_codec, piercing_weapon_codec, repairable_codec, swing_animation_codec,
    swing_animation_type_codec, tool_codec, tool_rule_codec, weapon_codec,
};
pub use consumable::{
    ApplyStatusEffectsConsumeEffect, ClearAllStatusEffectsConsumeEffect, Consumable, ConsumeEffect,
    DEFAULT_CONSUME_SECONDS, DEFAULT_STEW_DURATION, DeathProtection, FoodProperties,
    ItemUseAnimation, MAX_OMINOUS_BOTTLE_AMPLIFIER, MIN_OMINOUS_BOTTLE_AMPLIFIER,
    OminousBottleAmplifier, PlaySoundConsumeEffect, RemoveStatusEffectsConsumeEffect,
    SuspiciousStewEffects, SuspiciousStewEntry, TeleportRandomlyConsumeEffect, UseCooldown,
    UseEffects, UseRemainder, consumable_codec, consume_effect_codec, death_protection_codec,
    food_properties_codec, item_use_animation_codec, ominous_bottle_amplifier_codec,
    suspicious_stew_effects_codec, use_cooldown_codec, use_effects_codec, use_remainder_codec,
};
pub use core::{
    CustomData, Filterable, HolderRef, HolderSetRef, SoundEvent, TypedEntityData,
    custom_data_codec, filterable_codec, holder_ref_codec, holder_set_ref_codec, sound_event_codec,
    sound_event_direct_codec, typed_entity_data_codec,
};
pub use display::{
    CustomModelData, DyedItemColor, ItemLore, LEATHER_COLOR, LORE_MAX_LINES,
    MAP_ITEM_COLOR_DEFAULT, MapDecorationEntry, MapDecorations, MapItemColor, MapPostProcessing,
    TooltipDisplay, custom_model_data_codec, dyed_item_color_codec, item_lore_codec,
    map_decoration_entry_codec, map_decorations_codec, map_item_color_codec, tooltip_display_codec,
};
pub use world_and_books::{
    AttributeModifierDisplay, AttributeModifierEntry, AttributeModifierSpec, AttributeOperation,
    BOOK_MAX_GENERATION, BOOK_PAGE_EDIT_LENGTH, BOOK_PAGE_LENGTH, BOOK_TITLE_LENGTH,
    BOOK_TITLE_MAX_LENGTH, BlockItemStateProperties, FIREWORKS_MAX_EXPLOSIONS, FireworkExplosion,
    FireworkShape, Fireworks, Instrument, InstrumentComponent, ItemAttributeModifiers,
    LodestoneTracker, MaterialAssetGroup, ProvidesTrimMaterial, SeededContainerLoot,
    SulfurCubeContent, TrimMaterial, WRITABLE_BOOK_MAX_PAGES, WritableBookContent,
    WrittenBookContent, attribute_modifier_display_codec, attribute_modifier_entry_codec,
    attribute_operation_codec, block_item_state_properties_codec, firework_explosion_codec,
    firework_shape_codec, fireworks_codec, instrument_component_codec, instrument_direct_codec,
    item_attribute_modifiers_codec, lodestone_tracker_codec, provides_trim_material_codec,
    seeded_container_loot_codec, sulfur_cube_content_codec, trim_material_direct_codec,
    writable_book_content_codec, written_book_content_codec,
};

use crate::item::codec_from_fns;
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::Identifier;
use rivet_serialization::float_format::java_float_compare;
use rivet_serialization::{Codec, DataResult, DynamicOps};
use rivet_util::mth;
use std::cmp::Ordering;
use std::sync::Arc;

/// Helper to read a required map field from `DynamicOps::get_map`.
pub(crate) fn get_map_field<Ops: DynamicOps>(
    ops: &Ops,
    map: &dyn rivet_serialization::MapLike<Ops::Output>,
    key: &str,
) -> Option<Ops::Output> {
    let key_val = ops.create_string(key.to_string());
    map.get(&key_val)
}

/// Helper to encode an `Identifier` via `DynamicOps::create_string`.
pub(crate) fn encode_identifier<Ops: DynamicOps>(id: &Identifier, ops: &Ops) -> Ops::Output {
    ops.create_string(id.to_string().to_string())
}

/// Helper to decode an `Identifier` from a string value.
pub(crate) fn decode_identifier<Ops: DynamicOps>(
    ops: &Ops,
    input: &Ops::Output,
) -> DataResult<Identifier> {
    ops.get_string_value(input).flat_map(|s| {
        Identifier::try_parse(&s)
            .map(DataResult::success)
            .unwrap_or_else(|| DataResult::error(format!("Not a valid resource location: {s}")))
    })
}

/// `ExtraCodecs.RGB_COLOR_CODEC`: accepts either a packed integer RGB color or
/// a 3-element float list `[r, g, b]` in `[0.0, 1.0]` converted via
/// `ARGB.colorFromFloat(1.0F, r, g, b)`. Encodes as a packed integer.
pub fn rgb_color_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    codec_from_fns(
        "rgb_color",
        |v: &i32, ops: &Ops| DataResult::success(ops.create_int(*v)),
        |ops: &Ops, input: &Ops::Output| {
            let num = ops.get_number_value(input);
            if let Some(n) = num.result() {
                return DataResult::success(n.int_value());
            }
            let list = ops.get_stream(input);
            if let Some(items) = list.result() {
                if items.len() != 3 {
                    return DataResult::error(format!(
                        "RGB float list must have 3 elements, got {}",
                        items.len()
                    ));
                }
                let r = match ops.get_number_value(&items[0]).result() {
                    Some(n) => n.float_value(),
                    None => return DataResult::error("RGB list element 0 is not a number"),
                };
                let g = match ops.get_number_value(&items[1]).result() {
                    Some(n) => n.float_value(),
                    None => return DataResult::error("RGB list element 1 is not a number"),
                };
                let b = match ops.get_number_value(&items[2]).result() {
                    Some(n) => n.float_value(),
                    None => return DataResult::error("RGB list element 2 is not a number"),
                };
                return DataResult::success(argb_color_from_float(1.0, r, g, b));
            }
            DataResult::error("Expected RGB integer or 3-element float list")
        },
    )
}

/// Port of `net.minecraft.util.ARGB.colorFromFloat(float a, float r, float g, float b)`.
pub(crate) fn argb_color_from_float(a: f32, r: f32, g: f32, b: f32) -> i32 {
    argb_color(
        as_8_bit_channel(a),
        as_8_bit_channel(r),
        as_8_bit_channel(g),
        as_8_bit_channel(b),
    )
}

/// Port of `net.minecraft.util.ARGB.as8BitChannel(float)`.
#[inline]
pub(crate) fn as_8_bit_channel(value: f32) -> i32 {
    mth::floor(value * 255.0)
}

/// Port of `net.minecraft.util.ARGB.color(int a, int r, int g, int b)`.
#[inline]
pub(crate) fn argb_color(a: i32, r: i32, g: i32, b: i32) -> i32 {
    ((a & 0xFF) << 24) | ((r & 0xFF) << 16) | ((g & 0xFF) << 8) | (b & 0xFF)
}

/// Port of `net.minecraft.util.ARGB.opaque(int color)`.
#[inline]
pub(crate) fn argb_opaque(color: i32) -> i32 {
    color | (0xFF00_0000_u32 as i32)
}

/// Port of `net.minecraft.util.ARGB.red(int color)`.
#[inline]
pub(crate) fn argb_red(color: i32) -> i32 {
    (color >> 16) & 0xFF
}

/// Port of `net.minecraft.util.ARGB.green(int color)`.
#[inline]
pub(crate) fn argb_green(color: i32) -> i32 {
    (color >> 8) & 0xFF
}

/// Port of `net.minecraft.util.ARGB.blue(int color)`.
#[inline]
pub(crate) fn argb_blue(color: i32) -> i32 {
    color & 0xFF
}

/// Decode side of `CustomData.COMPOUND_TAG_CODEC` =
/// `Codec.either(CompoundTag.CODEC, TagParser.FLATTENED_CODEC)`: a structured
/// compound (converted via `NbtOps`) or, failing that, an SNBT string that
/// parses to a compound.
pub(crate) fn decode_compound_tag<Ops: DynamicOps>(
    ops: &Ops,
    input: &Ops::Output,
) -> DataResult<CompoundTag> {
    let tag = ops.convert_to(&NbtOps::instance(), input);
    let left_err = match tag {
        Tag::Compound(c) => return DataResult::success(c),
        other => format!("Not a compound tag: {other:?}"),
    };
    match ops.get_string_value(input).result() {
        Some(snbt) => match parse_compound_fully(snbt) {
            Ok(c) => DataResult::success(c),
            Err(e) => DataResult::error(format!(
                "Failed to parse either. First: {left_err}; Second: {}",
                e.message
            )),
        },
        None => DataResult::error(format!(
            "Failed to parse either. First: {left_err}; Second: Not a string"
        )),
    }
}

/// Encode side of `CustomData.COMPOUND_TAG_CODEC` (Paper's default, non-SNBT
/// branch): convert the `CompoundTag` into `Ops::Output` via `NbtOps`.
pub(crate) fn encode_compound_tag<Ops: DynamicOps>(tag: &CompoundTag, ops: &Ops) -> Ops::Output {
    NbtOps::instance().convert_to(ops, &Tag::Compound(tag.clone()))
}

/// Validate an integer range `[min, max]` inclusive.
pub(crate) fn validate_int_range(value: i32, min: i32, max: i32, label: &str) -> DataResult<i32> {
    if value < min || value > max {
        DataResult::error(format!(
            "{label} must be within range [{min}; {max}]: {value}"
        ))
    } else {
        DataResult::success(value)
    }
}

/// `ExtraCodecs.NON_NEGATIVE_INT` (`[0, Integer.MAX_VALUE]`).
pub(crate) fn validate_non_negative_int(value: i32, label: &str) -> DataResult<i32> {
    if value < 0 {
        DataResult::error(format!("{label} must be non-negative: {value}"))
    } else {
        DataResult::success(value)
    }
}

/// `ExtraCodecs.POSITIVE_INT` (`[1, Integer.MAX_VALUE]`).
pub(crate) fn validate_positive_int(value: i32, label: &str) -> DataResult<i32> {
    if value < 1 {
        DataResult::error(format!("{label} must be positive: {value}"))
    } else {
        DataResult::success(value)
    }
}

/// `ExtraCodecs.floatRange(min, max)` / `Codec.floatRange(min, max)`: inclusive
/// range check using Java `Float.compareTo` (IEEE total order), so `-0.0 < 0.0`
/// and `NaN` sorts above every value.
pub(crate) fn validate_float_range(value: f32, min: f32, max: f32, label: &str) -> DataResult<f32> {
    if java_float_compare(value, min) == Ordering::Less
        || java_float_compare(value, max) == Ordering::Greater
    {
        DataResult::error(format!(
            "{label} must be within range [{min}; {max}]: {value}"
        ))
    } else {
        DataResult::success(value)
    }
}

/// `ExtraCodecs.POSITIVE_FLOAT`: `compareTo(0.0F) > 0 && compareTo(Float.MAX_VALUE) <= 0`.
pub(crate) fn validate_positive_float(value: f32, label: &str) -> DataResult<f32> {
    if java_float_compare(value, 0.0) != Ordering::Greater
        || java_float_compare(value, f32::MAX) == Ordering::Greater
    {
        DataResult::error(format!("{label} must be positive: {value}"))
    } else {
        DataResult::success(value)
    }
}

/// `ExtraCodecs.NON_NEGATIVE_FLOAT`: `compareTo(0.0F) >= 0 && compareTo(Float.MAX_VALUE) <= 0`.
pub(crate) fn validate_non_negative_float(value: f32, label: &str) -> DataResult<f32> {
    if java_float_compare(value, 0.0) == Ordering::Less
        || java_float_compare(value, f32::MAX) == Ordering::Greater
    {
        DataResult::error(format!("{label} must be non-negative: {value}"))
    } else {
        DataResult::success(value)
    }
}

/// Turn a validator `DataResult` into a `Result` for `?`-style composition.
pub(crate) fn result_of<T: Clone>(r: DataResult<T>) -> Result<T, String> {
    match r.result() {
        Some(v) => Ok(v.clone()),
        None => Err(r
            .error_ref()
            .map(|e| e.message().to_string())
            .unwrap_or_default()),
    }
}

/// Validate a string length in `[min, max]` using Java UTF-16 code-unit length
/// (`str::encode_utf16().count()`).
pub(crate) fn validate_string_len(
    s: &str,
    min: usize,
    max: usize,
    label: &str,
) -> Result<(), String> {
    let len = s.encode_utf16().count();
    if len < min || len > max {
        Err(format!(
            "{label} length {len} outside allowed range [{min}; {max}]"
        ))
    } else {
        Ok(())
    }
}
