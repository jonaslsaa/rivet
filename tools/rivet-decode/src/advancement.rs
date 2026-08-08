//! Canonical re-serialization of the `update_advancements` (130) packet body.
//!
//! Paper serializes the advancement added/removed/progress collections from
//! per-boot `HashMap`/`HashSet` order (`ClientboundUpdateAdvancementsPacket`),
//! so a capture can vary across boots. On top of the existing list/criteria
//! sorting this module structurally canonicalizes the advancement **display**
//! payloads (issue #221), the part the join fixture's canonicalizer once
//! preserved verbatim:
//!
//! - the two NBT components (`title`, `description`) re-emit compound fields
//!   sorted by name, because a `Component` serializes to an NBT compound whose
//!   field order depends on the DFU record map iteration;
//! - the `DataComponentPatch` (item icon components) re-emits the positive
//!   entries sorted by component type id and the negative entries sorted the
//!   same way, because the patch is a fastutil `Reference2ObjectMap` whose
//!   order is not stable across JVM processes (`DataComponentPatch.encode`).
//!
//! No value is fabricated: a component id outside the registered 26.2 range, a
//! non-empty patch with a value that does not fit its wire shape, a duplicate id
//! in a patch, or a display payload with unexpected bytes makes the whole
//! canonicalization fail (`None`), and the caller keeps the raw body. For a capture that carries no
//! advancement display data (the pinned join fixture) that means the display is
//! never rewritten or invented; the body can still differ from a raw capture in
//! the pre-existing non-display ways — added/removed/progress sorting and
//! criteria-name sorting. The fixture's `unlock_right_away` criterion is
//! obtained=true with an already-zero instant, so obtained instants pass
//! through unchanged rather than being rewritten.
//!
//! The value dispatch is pinned to protocol 776 and the 26.2 component set.
//! The components whose network form is NBT — `Component` title/description and
//! `custom_name`/`item_name` via `tagCodec`, `CustomData`
//! `custom_data`/`bucket_entity_data` via `COMPOUND_TAG`, the `ItemLore` list,
//! and the `TypedEntityData` `entity_data`/`block_entity_data` — are
//! canonicalized structurally (NBT compound field order). Every other
//! registered component value is copied byte-exact: the harness walks the
//! component's exact wire shape ([`Shape`] / the `skip_*` primitives) to bound
//! it, then preserves the raw bytes verbatim, so a display icon carrying, say,
//! an `item_model` identifier or a `max_stack_size` VarInt still canonicalizes
//! (patch entries sorted by id) instead of failing the whole advancement. Only
//! an id outside the registered 26.2 range, a duplicate positive/negative id in
//! a patch, or a value that does not fit its shape fails canonicalization.
//!
//! The display path is proven by synthetic display-bearing bodies in
//! `tools/rivet-capture/src/normalize.rs`; issue #269 tracks exercising it via
//! a real-boot capture that carries a display-bearing advancement.

use crate::frame;
use crate::nbt::{Nbt, read_nbt, write_nbt};

fn read_string(body: &[u8], off: &mut usize) -> Option<String> {
    let len = frame::read_varint(body, off)?;
    if len < 0 {
        return None;
    }
    let s = std::str::from_utf8(body.get(*off..*off + len as usize)?)
        .ok()?
        .to_owned();
    *off += len as usize;
    Some(s)
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    frame::write_varint(out, s.len() as i32);
    out.extend_from_slice(s.as_bytes());
}

/// The pinned 26.2 component type name for a network registry id, restricted to
/// exactly the NBT-shaped components `read_component_value` can bound (a bare
/// NBT tag, an `ItemLore` NBT list, or a `TypedEntityData` type-id + NBT tag).
/// Only these values get semantic canonicalization; every other registered
/// component's value (a scalar VarInt/Int, an id-mapper enum, an
/// `ItemEnchantments`/`PotionContents`/`ItemAttributeModifiers` composite) is
/// not NBT-shaped and is instead preserved byte-exact via `skip_component_value`.
fn component_type_name(id: u32) -> Option<&'static str> {
    match id {
        0 => Some("minecraft:custom_data"),
        6 => Some("minecraft:custom_name"),
        9 => Some("minecraft:item_name"),
        11 => Some("minecraft:lore"),
        58 => Some("minecraft:entity_data"),
        59 => Some("minecraft:bucket_entity_data"),
        60 => Some("minecraft:block_entity_data"),
        _ => None,
    }
}

/// Read one component value and return its canonical bytes, for the pinned
/// 26.2 components whose network value is a bare NBT payload.
/// Supported shapes (canonicalized, never fabricated):
///   - a single NBT tag: `custom_data`, `custom_name`, `item_name`,
///     `bucket_entity_data` — a `CustomData`/`Component` value
///     (`tagCodec`/`COMPOUND_TAG`), re-emitted as NBT;
///   - a list of NBT tags: `lore` — an `ItemLore` (`list(256)` of `tagCodec`);
///   - a typed NBT tag: `entity_data`, `block_entity_data` — a
///     `TypedEntityData` (`[type registry id VarInt][compound tag]`), with the
///     type id preserved verbatim.
///
/// A value that does not match the component's enforced shape (e.g. a
/// non-compound tag where the codec is `COMPOUND_TAG`) is refused (`None`),
/// matching Java's `DecoderException`. Values for every other component are
/// handled by `skip_component_value` in the caller.
fn read_component_value(body: &[u8], off: &mut usize, name: &str) -> Option<Vec<u8>> {
    match name {
        "minecraft:custom_data"          // CustomData.STREAM_CODEC = COMPOUND_TAG
        | "minecraft:bucket_entity_data" // CustomData.STREAM_CODEC = COMPOUND_TAG
        => {
            // ByteBufCodecs.COMPOUND_TAG requires the value be a CompoundTag;
            // Java throws DecoderException("Not a compound tag: ...") otherwise.
            let value = read_nbt(body, off)?;
            if !matches!(value, Nbt::Compound(_)) {
                return None;
            }
            let mut out = Vec::with_capacity(body.len() / 8);
            write_nbt(&mut out, &value);
            Some(out)
        }
        "minecraft:custom_name" // ComponentSerialization.STREAM_CODEC = tagCodec
        | "minecraft:item_name" // ComponentSerialization.STREAM_CODEC = tagCodec
        => {
            // tagCodec accepts any non-End tag (a Component serializes to an NBT
            // compound; null/End throws but any other tag type parses).
            let value = read_nbt(body, off)?;
            if matches!(value, Nbt::End) {
                return None;
            }
            let mut out = Vec::with_capacity(body.len() / 8);
            write_nbt(&mut out, &value);
            Some(out)
        }
        "minecraft:lore" => {
            // ItemLore.STREAM_CODEC = list(256) of tagCodec.
            let count = frame::read_varint(body, off)?;
            if count < 0 {
                return None;
            }
            let mut out = Vec::with_capacity(body.len() / 4);
            frame::write_varint(&mut out, count);
            for _ in 0..count {
                let line = read_nbt(body, off)?;
                if matches!(line, Nbt::End) {
                    return None;
                }
                write_nbt(&mut out, &line);
            }
            Some(out)
        }
        "minecraft:entity_data" | "minecraft:block_entity_data" => {
            // TypedEntityData.streamCodec = [type registry id VarInt][COMPOUND_TAG].
            let type_id = frame::read_varint(body, off)?;
            if type_id < 0 {
                return None;
            }
            let value = read_nbt(body, off)?;
            if !matches!(value, Nbt::Compound(_)) {
                return None;
            }
            let mut out = Vec::with_capacity(body.len() / 4);
            frame::write_varint(&mut out, type_id);
            write_nbt(&mut out, &value);
            Some(out)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Byte-exact preservation of every OTHER registered component value.
//
// Each non-NBT component's network value is a self-delimiting composition of the
// primitives below (every length is VarInt-prefixed and bounded), so the harness
// can walk the exact wire shape to find the value's end and copy it verbatim.
// This is what lets a display icon carrying, e.g., an `item_model` Identifier or
// a `max_stack_size` VarInt canonicalize (entries sorted by id) instead of
// failing the whole advancement. The shapes are pinned to DataComponents.java
// registration order on protocol 776; the NBT-shaped ids (0, 6, 9, 11, 58, 59,
// 60) are NOT listed here — they go through [`read_component_value`] above.
// ---------------------------------------------------------------------------

/// The wire shape of a registered 26.2 component value that is copied verbatim.
#[derive(Clone, Copy)]
enum Shape {
    /// A scalar, registry-id, holder-registry or id-mapper VarInt
    /// (`ByteBufCodecs.VAR_INT` / `registry` / `holderRegistry` / `idMapper`).
    VarInt,
    /// No bytes (`Unit`).
    Unit,
    /// One byte (`ByteBufCodecs.BOOL`).
    Bool,
    /// Four big-endian bytes (`ByteBufCodecs.FLOAT`).
    Float,
    /// Four big-endian bytes (`ByteBufCodecs.INT`).
    Int,
    /// `[VarInt byte length][UTF-8 bytes]` (`Identifier` / `Utf8String`).
    Utf8String,
    /// A bare NBT tag (`fromCodecWithRegistries` fallback for components without
    /// a `networkSynchronized` stream codec, e.g. `intangible_projectile`).
    NbtTag,
    /// `ByteBufCodecs.holderSet(registry)` — `[VarInt count-1][named|holders]`.
    HolderSet,
    /// `SoundEvent.STREAM_CODEC` — `holder(SOUND_EVENT, [Identifier][?Float])`.
    SoundEvent,
    /// `UseEffects` — `[Bool][Bool][Float]`.
    UseEffects,
    /// `FoodProperties.DIRECT_STREAM_CODEC` — `[VarInt][Float][Bool]`.
    Food,
    /// `Weapon` — `[VarInt][Float]`.
    Weapon,
    /// `AttackRange` — `[Float ×6]` (minReach, maxReach, minCreativeReach,
    /// maxCreativeReach, hitboxMargin, mobFactor).
    AttackRange,
    /// `SwingAnimation` — `[SwingAnimationType idMapper][VarInt]`.
    SwingAnimation,
    /// `UseCooldown` — `[Float][?Identifier]`.
    UseCooldown,
    /// `DamageResistant` — `holderSet(DAMAGE_TYPE)`.
    DamageResistant,
    /// `Repairable` — `holderSet(ITEM)`.
    Repairable,
    /// `ItemEnchantments` — `map(holderRegistry(ENCHANTMENT), VarInt)`.
    Enchantments,
    /// `CustomModelData` — `[list Float][list Bool][list String][list Int]`.
    CustomModelData,
    /// `TooltipDisplay` — `[Bool][set DataComponentType]`.
    TooltipDisplay,
    /// `Tool` — `[list Rule][Float][VarInt][Bool]`.
    Tool,
    /// `Consumable` — `[Float][ItemUseAnimation idMapper][SoundEvent][Bool][list ConsumeEffect]`.
    Consumable,
    /// `ItemStackTemplate` — `[Item holderRegistry][count VarInt][patch]`.
    ItemStackTemplate,
    /// `Equippable`.
    Equippable,
    /// `DeathProtection` — `list(ConsumeEffect)`.
    DeathProtection,
    /// `BlocksAttacks`.
    BlocksAttacks,
    /// `PiercingWeapon`.
    PiercingWeapon,
    /// `KineticWeapon`.
    KineticWeapon,
    /// `ArmorTrim` — `[TrimMaterial holder][TrimPattern holder]`.
    ArmorTrim,
    /// `LodestoneTracker` — `[?GlobalPos][Bool]`.
    LodestoneTracker,
    /// `FireworkExplosion`.
    FireworkExplosion,
    /// `Fireworks` — `[VarInt][list(256) FireworkExplosion]`.
    Fireworks,
    /// `ResolvableProfile` — `[either(GameProfile, Partial)][PlayerSkin.Patch]`.
    Profile,
    /// `JukeboxPlayable` — `holder(JUKEBOX_SONG, direct)`.
    JukeboxSong,
    /// `InstrumentComponent` — `holder(INSTRUMENT, direct)`.
    Instrument,
    /// `ProvidesTrimMaterial` — `holder(TRIM_MATERIAL, direct)`.
    TrimMaterial,
    /// `BannerPatternLayers` — `list(Layer)`.
    BannerPatterns,
    /// `PotDecorations` — `list(4)(registry ITEM)`.
    PotDecorations,
    /// `BlockItemStateProperties` — `map(String, String)`.
    BlockState,
    /// `Bees` — `list(Occupant)`.
    Bees,
    /// `ItemContainerContents` — `list(256)(?ItemStackTemplate)`.
    Container,
    /// `PotionContents`.
    PotionContents,
    /// `SuspiciousStewEffects`.
    SuspiciousStewEffects,
    /// `WritableBookContent`.
    WritableBookContent,
    /// `WrittenBookContent`.
    WrittenBookContent,
    /// `AdventureModePredicate` (`can_place_on`/`can_break`) — `list(BlockPredicate)`.
    BlockPredicateList,
    /// `ItemAttributeModifiers` — `list(Entry)`.
    AttributeModifiers,
    /// `ChargedProjectiles` — `list(1024)(ItemStackTemplate)`.
    ChargedProjectiles,
    /// `BundleContents` — `list(256)(ItemStackTemplate)`.
    BundleContents,
    /// `PaintingVariant` — `holder(PAINTING_VARIANT, direct)`.
    PaintingVariant,
}

/// The 26.2 `DATA_COMPONENT_TYPE` registry id -> wire shape, for every component
/// this harness copies byte-exact. Ids follow `DataComponents.java` registration
/// order (0-based). The NBT-shaped ids handled by [`read_component_value`] are
/// absent, and an id outside the registered range is `None` (honest failure).
fn component_shape(id: u32) -> Option<Shape> {
    use Shape::*;
    Some(match id {
        1 | 2 | 3 | 8 | 12 | 19 | 31 | 41 | 43 | 46 | 48 | 63 | 73 | 82 | 83 | 84 | 85 | 86
        | 87 | 88 | 89 | 90 | 91 | 92 | 93 | 94 | 95 | 96 | 97 | 98 | 99 | 100 | 101 | 102
        | 104 | 105 | 106 | 107 | 108 | 109 | 110 => VarInt,
        4 | 20 | 34 => Unit,
        21 => Bool,
        7 | 52 => Float,
        44 | 45 => Int,
        10 | 35 | 71 => Utf8String,
        22 | 47 | 57 | 66 | 79 | 80 => NbtTag,
        27 => DamageResistant,
        33 => Repairable,
        65 => HolderSet,
        81 => SoundEvent,
        5 => UseEffects,
        23 => Food,
        29 => Weapon,
        30 => AttackRange,
        40 => SwingAnimation,
        26 => UseCooldown,
        13 | 42 => Enchantments,
        17 => CustomModelData,
        18 => TooltipDisplay,
        28 => Tool,
        24 => Consumable,
        25 | 78 => ItemStackTemplate,
        32 => Equippable,
        36 => DeathProtection,
        37 => BlocksAttacks,
        38 => PiercingWeapon,
        39 => KineticWeapon,
        56 => ArmorTrim,
        67 => LodestoneTracker,
        68 => FireworkExplosion,
        69 => Fireworks,
        70 => Profile,
        64 => JukeboxSong,
        61 => Instrument,
        62 => TrimMaterial,
        72 => BannerPatterns,
        74 => PotDecorations,
        76 => BlockState,
        77 => Bees,
        75 => Container,
        51 => PotionContents,
        53 => SuspiciousStewEffects,
        54 => WritableBookContent,
        55 => WrittenBookContent,
        14 | 15 => BlockPredicateList,
        16 => AttributeModifiers,
        49 => ChargedProjectiles,
        50 => BundleContents,
        103 => PaintingVariant,
        _ => return None,
    })
}

/// A `(&[u8], &mut usize) -> Option<()>` skip function. Using a bare fn pointer
/// (rather than a generic `Fn`) keeps every element HRTB-clean so composite
/// skips compose without lifetime annotation; elements that need a recursion
/// depth take it as an explicit parameter instead of capturing.
type SkipFn = fn(&[u8], &mut usize) -> Option<()>;

fn skip_bytes(body: &[u8], off: &mut usize, n: usize) -> Option<()> {
    frame::read_bytes(body, off, n)?;
    Some(())
}

fn skip_varint(body: &[u8], off: &mut usize) -> Option<()> {
    frame::read_varint(body, off)?;
    Some(())
}

fn skip_bool(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 1)
}

fn skip_float(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 4)
}

fn skip_int(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 4)
}

fn skip_long(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 8)
}

fn skip_double(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 8)
}

fn skip_uuid(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 16)
}

/// Skip `[VarInt byte length][UTF-8 bytes]` (`Utf8String` / `Identifier`).
fn skip_utf8(body: &[u8], off: &mut usize) -> Option<()> {
    let len = frame::read_varint(body, off)?;
    if len < 0 {
        return None;
    }
    skip_bytes(body, off, len as usize)
}

/// Skip one NBT tag (`[type byte][payload]`), reusing the strict parser so a
/// malformed tag (negative array length, truncated payload) fails the patch.
fn skip_nbt(body: &[u8], off: &mut usize) -> Option<()> {
    let _ = read_nbt(body, off)?;
    Some(())
}

/// Skip a `tagCodec` value — any non-`End` NBT tag (a `Component`).
fn skip_component_tag(body: &[u8], off: &mut usize) -> Option<()> {
    let value = read_nbt(body, off)?;
    if matches!(value, Nbt::End) {
        return None;
    }
    Some(())
}

/// `[Bool present][value]`.
fn skip_optional(body: &[u8], off: &mut usize, value: SkipFn) -> Option<()> {
    let present = *body.get(*off)?;
    *off += 1;
    if present != 0 {
        value(body, off)?;
    }
    Some(())
}

/// `[VarInt count][count × value]`. A negative count fails (Java's encoder never
/// writes one; the harness's other canonicalizers reject them the same way).
fn skip_list(body: &[u8], off: &mut usize, elem: SkipFn) -> Option<()> {
    let count = frame::read_varint(body, off)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        elem(body, off)?;
    }
    Some(())
}

/// `[VarInt count][count × (key, value)]`.
fn skip_map(body: &[u8], off: &mut usize, key: SkipFn, value: SkipFn) -> Option<()> {
    let count = frame::read_varint(body, off)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        key(body, off)?;
        value(body, off)?;
    }
    Some(())
}

/// `ByteBufCodecs.either(left, right)` — `[readBoolean][left|right]`. Java's
/// `readBoolean()` is `readByte() != 0`, so any nonzero byte selects the left
/// codec and a zero byte selects the right one.
fn skip_either(body: &[u8], off: &mut usize, left: SkipFn, right: SkipFn) -> Option<()> {
    let which = *body.get(*off)?;
    *off += 1;
    if which != 0 {
        left(body, off)
    } else {
        right(body, off)
    }
}

/// `ByteBufCodecs.holder(registry, direct)` — `[VarInt id][id == 0 ? direct : nothing]`.
fn skip_holder(body: &[u8], off: &mut usize, direct: SkipFn) -> Option<()> {
    let id = frame::read_varint(body, off)?;
    if id == 0 {
        direct(body, off)?;
    }
    Some(())
}

/// `ByteBufCodecs.holderSet(registry)` — `[VarInt count-1][count == -1 ? named : holders]`.
/// Java reads `VarInt.read - 1`: a value of 0 means a named set (`[Identifier]`),
/// a positive value is `count` direct holder ids, and a negative value other than
/// -1 is an empty direct set.
fn skip_holder_set(body: &[u8], off: &mut usize) -> Option<()> {
    let raw = frame::read_varint(body, off)?;
    if raw == 0 {
        return skip_utf8(body, off); // TagKey identifier
    }
    if raw < 0 {
        return Some(()); // Java: empty direct set
    }
    for _ in 0..raw.saturating_sub(1) {
        skip_varint(body, off)?;
    }
    Some(())
}

// -- nested composite shapes ------------------------------------------------

fn skip_sound_event(body: &[u8], off: &mut usize) -> Option<()> {
    // SoundEvent.STREAM_CODEC = holder(SOUND_EVENT, [Identifier][?Float]).
    skip_holder(body, off, |b, o| {
        skip_utf8(b, o)?;
        skip_optional(b, o, skip_float)
    })
}

fn skip_use_effects(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bool(body, off)?;
    skip_bool(body, off)?;
    skip_float(body, off)
}

fn skip_food(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?;
    skip_float(body, off)?;
    skip_bool(body, off)
}

fn skip_weapon(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?;
    skip_float(body, off)
}

fn skip_attack_range(body: &[u8], off: &mut usize) -> Option<()> {
    // AttackRange.STREAM_CODEC = six ByteBufCodecs.FLOAT (minReach, maxReach,
    // minCreativeReach, maxCreativeReach, hitboxMargin, mobFactor).
    for _ in 0..6 {
        skip_float(body, off)?;
    }
    Some(())
}

fn skip_swing_animation(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?;
    skip_varint(body, off)
}

fn skip_use_cooldown(body: &[u8], off: &mut usize) -> Option<()> {
    skip_float(body, off)?;
    skip_optional(body, off, skip_utf8)
}

fn skip_enchantments(body: &[u8], off: &mut usize) -> Option<()> {
    skip_map(body, off, skip_varint, skip_varint)
}

fn skip_custom_model_data(body: &[u8], off: &mut usize) -> Option<()> {
    skip_list(body, off, skip_float)?;
    skip_list(body, off, skip_bool)?;
    skip_list(body, off, skip_utf8)?;
    skip_list(body, off, skip_int)
}

fn skip_tooltip_display(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bool(body, off)?;
    skip_list(body, off, skip_varint) // set of DataComponentType registry ids
}

fn skip_tool_rule(body: &[u8], off: &mut usize) -> Option<()> {
    skip_holder_set(body, off)?;
    skip_optional(body, off, skip_float)?;
    skip_optional(body, off, skip_bool)
}

fn skip_tool(body: &[u8], off: &mut usize) -> Option<()> {
    skip_list(body, off, skip_tool_rule)?;
    skip_float(body, off)?;
    skip_varint(body, off)?;
    skip_bool(body, off)
}

/// `MobEffectInstance.Details` — `[VarInt][VarInt][Bool][Bool][Bool][?Details]`.
fn skip_effect_details(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    if depth > 8 {
        return None;
    }
    skip_varint(body, off)?; // amplifier
    skip_varint(body, off)?; // duration
    skip_bool(body, off)?; // ambient
    skip_bool(body, off)?; // showParticles
    skip_bool(body, off)?; // showIcon
    // hiddenEffect: [Bool present][recursive Details].
    let present = *body.get(*off)?;
    *off += 1;
    if present != 0 {
        skip_effect_details(body, off, depth + 1)?;
    }
    Some(())
}

fn skip_mob_effect_instance(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // holderRegistry(MOB_EFFECT)
    skip_effect_details(body, off, 0)
}

/// `ConsumeEffect.STREAM_CODEC` — `[registry id][subtype by id]`. The five
/// registered subtype ids follow `ConsumeEffect.Type` registration order.
fn skip_consume_effect(body: &[u8], off: &mut usize) -> Option<()> {
    match frame::read_varint(body, off)? {
        0 => {
            // ApplyStatusEffectsConsumeEffect: [list(MobEffectInstance)][Float].
            skip_list(body, off, skip_mob_effect_instance)?;
            skip_float(body, off)
        }
        1 => skip_holder_set(body, off), // RemoveStatusEffectsConsumeEffect: holderSet(MOB_EFFECT)
        2 => Some(()),                   // ClearAllStatusEffectsConsumeEffect: unit
        3 => skip_float(body, off),      // TeleportRandomlyConsumeEffect
        4 => skip_sound_event(body, off), // PlaySoundConsumeEffect
        _ => None,
    }
}

fn skip_consumable(body: &[u8], off: &mut usize) -> Option<()> {
    skip_float(body, off)?; // consumeSeconds
    skip_varint(body, off)?; // ItemUseAnimation idMapper
    skip_sound_event(body, off)?; // sound
    skip_bool(body, off)?; // hasConsumeParticles
    skip_list(body, off, skip_consume_effect)
}

/// `AttributeModifier` — `[Identifier][Double][Operation idMapper]`.
fn skip_attribute_modifier(body: &[u8], off: &mut usize) -> Option<()> {
    skip_utf8(body, off)?;
    skip_double(body, off)?;
    skip_varint(body, off)
}

fn skip_item_stack_template(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    // ItemStackTemplate = [Item holderRegistry][count VarInt][DataComponentPatch].
    skip_varint(body, off)?;
    skip_varint(body, off)?;
    skip_patch_value(body, off, depth)
}

/// The `DataComponentPatch` VALUE (the entry's type id has already been read):
/// `[positive VarInt][negative VarInt][positive entries][negative entries]`.
fn skip_patch_value(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    let positive = frame::read_varint(body, off)?;
    let negative = frame::read_varint(body, off)?;
    if positive < 0 || negative < 0 {
        return None;
    }
    for _ in 0..positive {
        let id = frame::read_varint(body, off)?;
        if id < 0 {
            return None;
        }
        skip_any_component_value(body, off, id as u32, depth + 1)?;
    }
    for _ in 0..negative {
        skip_varint(body, off)?;
    }
    Some(())
}

fn skip_equippable(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // EquipmentSlot idMapper
    skip_sound_event(body, off)?; // equipSound
    skip_optional(body, off, skip_utf8)?; // assetId (ResourceKey)
    skip_optional(body, off, skip_utf8)?; // cameraOverlay (Identifier)
    skip_optional(body, off, skip_holder_set)?; // allowedEntities holderSet(ENTITY_TYPE)
    skip_bool(body, off)?; // dispensable
    skip_bool(body, off)?; // swappable
    skip_bool(body, off)?; // damageOnHurt
    skip_bool(body, off)?; // equipOnInteract
    skip_bool(body, off)?; // canBeSheared
    skip_sound_event(body, off) // shearingSound
}

fn skip_death_protection(body: &[u8], off: &mut usize) -> Option<()> {
    skip_list(body, off, skip_consume_effect)
}

fn skip_damage_reduction(body: &[u8], off: &mut usize) -> Option<()> {
    skip_float(body, off)?; // horizontalBlockingAngle
    skip_optional(body, off, skip_holder_set)?; // type holderSet(DAMAGE_TYPE)
    skip_float(body, off)?; // base
    skip_float(body, off) // factor
}

fn skip_item_damage_function(body: &[u8], off: &mut usize) -> Option<()> {
    skip_float(body, off)?; // threshold
    skip_float(body, off)?; // base
    skip_float(body, off) // factor
}

fn skip_blocks_attacks(body: &[u8], off: &mut usize) -> Option<()> {
    skip_float(body, off)?; // blockDelaySeconds
    skip_float(body, off)?; // disableCooldownScale
    skip_list(body, off, skip_damage_reduction)?; // damageReductions
    skip_item_damage_function(body, off)?; // itemDamage
    skip_optional(body, off, skip_holder_set)?; // bypassedBy holderSet(DAMAGE_TYPE)
    skip_optional(body, off, skip_sound_event)?; // blockSound
    skip_optional(body, off, skip_sound_event) // disableSound
}

fn skip_piercing_weapon(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bool(body, off)?;
    skip_bool(body, off)?;
    skip_optional(body, off, skip_sound_event)?;
    skip_optional(body, off, skip_sound_event)
}

fn skip_kinetic_condition(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // maxDurationTicks
    skip_float(body, off)?; // minSpeed
    skip_float(body, off) // minRelativeSpeed
}

fn skip_kinetic_weapon(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // contactCooldownTicks
    skip_varint(body, off)?; // delayTicks
    skip_optional(body, off, skip_kinetic_condition)?; // dismountConditions
    skip_optional(body, off, skip_kinetic_condition)?; // knockbackConditions
    skip_optional(body, off, skip_kinetic_condition)?; // damageConditions
    skip_float(body, off)?; // forwardMovement
    skip_float(body, off)?; // damageMultiplier
    skip_optional(body, off, skip_sound_event)?; // sound
    skip_optional(body, off, skip_sound_event) // hitSound
}

fn skip_material_asset_group(body: &[u8], off: &mut usize) -> Option<()> {
    // MaterialAssetGroup = [AssetInfo STRING_UTF8][map(ResourceKey, AssetInfo)].
    skip_utf8(body, off)?;
    skip_map(body, off, skip_utf8, skip_utf8)
}

fn skip_trim_material_holder(body: &[u8], off: &mut usize) -> Option<()> {
    // holder(TRIM_MATERIAL, [MaterialAssetGroup][Component tag]).
    skip_holder(body, off, |b, o| {
        skip_material_asset_group(b, o)?;
        skip_component_tag(b, o)
    })
}

fn skip_trim_pattern_holder(body: &[u8], off: &mut usize) -> Option<()> {
    // holder(TRIM_PATTERN, [Identifier][Component tag][Bool]).
    skip_holder(body, off, |b, o| {
        skip_utf8(b, o)?;
        skip_component_tag(b, o)?;
        skip_bool(b, o)
    })
}

fn skip_armor_trim(body: &[u8], off: &mut usize) -> Option<()> {
    skip_trim_material_holder(body, off)?;
    skip_trim_pattern_holder(body, off)
}

/// `GlobalPos` — `[ResourceKey(DIMENSION) Identifier][BlockPos long]`.
fn skip_global_pos(body: &[u8], off: &mut usize) -> Option<()> {
    skip_utf8(body, off)?;
    skip_long(body, off)
}

fn skip_lodestone_tracker(body: &[u8], off: &mut usize) -> Option<()> {
    skip_optional(body, off, skip_global_pos)?;
    skip_bool(body, off)
}

fn skip_firework_explosion(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // Shape idMapper
    skip_list(body, off, skip_int)?; // colors
    skip_list(body, off, skip_int)?; // fadeColors
    skip_bool(body, off)?; // hasTrail
    skip_bool(body, off) // hasTwinkle
}

fn skip_fireworks(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // flightDuration
    skip_list(body, off, skip_firework_explosion)
}

fn skip_game_profile_properties(body: &[u8], off: &mut usize) -> Option<()> {
    // [VarInt count][count × ([String name][String value][?String signature])].
    let count = frame::read_varint(body, off)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        skip_utf8(body, off)?;
        skip_utf8(body, off)?;
        skip_optional(body, off, skip_utf8)?;
    }
    Some(())
}

fn skip_game_profile(body: &[u8], off: &mut usize) -> Option<()> {
    skip_bytes(body, off, 16)?; // UUID
    skip_utf8(body, off)?; // PLAYER_NAME
    skip_game_profile_properties(body, off)
}

fn skip_resolvable_partial(body: &[u8], off: &mut usize) -> Option<()> {
    skip_optional(body, off, skip_utf8)?; // name
    skip_optional(body, off, skip_uuid)?; // id
    skip_game_profile_properties(body, off)
}

fn skip_player_skin_patch(body: &[u8], off: &mut usize) -> Option<()> {
    // [ResourceTexture? ×3][PlayerModelType Bool?] — ResourceTexture = Identifier.
    skip_optional(body, off, skip_utf8)?;
    skip_optional(body, off, skip_utf8)?;
    skip_optional(body, off, skip_utf8)?;
    skip_optional(body, off, skip_bool)
}

fn skip_profile(body: &[u8], off: &mut usize) -> Option<()> {
    skip_either(body, off, skip_game_profile, skip_resolvable_partial)?;
    skip_player_skin_patch(body, off)
}

fn skip_instrument_holder(body: &[u8], off: &mut usize) -> Option<()> {
    // holder(INSTRUMENT, [SoundEvent][Float][Float][Component tag]).
    skip_holder(body, off, |b, o| {
        skip_sound_event(b, o)?;
        skip_float(b, o)?;
        skip_float(b, o)?;
        skip_component_tag(b, o)
    })
}

fn skip_jukebox_song_holder(body: &[u8], off: &mut usize) -> Option<()> {
    // holder(JUKEBOX_SONG, [SoundEvent][Component tag][Float][VarInt]).
    skip_holder(body, off, |b, o| {
        skip_sound_event(b, o)?;
        skip_component_tag(b, o)?;
        skip_float(b, o)?;
        skip_varint(b, o)
    })
}

fn skip_banner_pattern_holder(body: &[u8], off: &mut usize) -> Option<()> {
    // holder(BANNER_PATTERN, [Identifier][STRING_UTF8]).
    skip_holder(body, off, |b, o| {
        skip_utf8(b, o)?;
        skip_utf8(b, o)
    })
}

fn skip_banner_patterns(body: &[u8], off: &mut usize) -> Option<()> {
    // list(Layer = [BannerPattern holder][DyeColor idMapper]).
    skip_list(body, off, |b, o| {
        skip_banner_pattern_holder(b, o)?;
        skip_varint(b, o)
    })
}

fn skip_pot_decorations(body: &[u8], off: &mut usize) -> Option<()> {
    skip_list(body, off, skip_varint)
}

fn skip_block_state(body: &[u8], off: &mut usize) -> Option<()> {
    skip_map(body, off, skip_utf8, skip_utf8)
}

/// `TypedEntityData` — `[type registry id VarInt][COMPOUND_TAG]`.
fn skip_typed_entity_data(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?;
    skip_nbt(body, off)
}

fn skip_beehive_occupant(body: &[u8], off: &mut usize) -> Option<()> {
    skip_typed_entity_data(body, off)?;
    skip_varint(body, off)?; // ticksInHive
    skip_varint(body, off) // minTicksInHive
}

fn skip_bees(body: &[u8], off: &mut usize) -> Option<()> {
    skip_list(body, off, skip_beehive_occupant)
}

fn skip_container(body: &[u8], off: &mut usize) -> Option<()> {
    // list(256)(?ItemStackTemplate).
    skip_list(body, off, skip_optional_item_stack)
}

fn skip_potion_contents(body: &[u8], off: &mut usize) -> Option<()> {
    skip_optional(body, off, skip_varint)?; // potion holderRegistry(POTION)
    skip_optional(body, off, skip_int)?; // customColor
    skip_list(body, off, skip_mob_effect_instance)?; // customEffects
    skip_optional(body, off, skip_utf8) // customName
}

fn skip_suspicious_stew_effects(body: &[u8], off: &mut usize) -> Option<()> {
    // list(Entry = [MobEffect holderRegistry][VarInt duration]).
    skip_list(body, off, |b, o| {
        skip_varint(b, o)?;
        skip_varint(b, o)
    })
}

fn skip_filterable(body: &[u8], off: &mut usize, value: SkipFn) -> Option<()> {
    value(body, off)?;
    skip_optional(body, off, value)
}

/// `Filterable(STRING_UTF8)` — a plain string plus an optional filtered copy.
fn skip_filterable_utf8(body: &[u8], off: &mut usize) -> Option<()> {
    skip_filterable(body, off, skip_utf8)
}

/// `Filterable(ComponentSerialization.STREAM_CODEC)`.
fn skip_filterable_component(body: &[u8], off: &mut usize) -> Option<()> {
    skip_filterable(body, off, skip_component_tag)
}

/// A depth-0 `ItemStackTemplate` (no deeper patch nesting is ever valid here).
fn skip_item_stack_leaf(body: &[u8], off: &mut usize) -> Option<()> {
    skip_item_stack_template(body, off, 0)
}

/// `[?ItemStackTemplate]` (`Optional<ItemStackTemplate>`).
fn skip_optional_item_stack(body: &[u8], off: &mut usize) -> Option<()> {
    skip_optional(body, off, skip_item_stack_leaf)
}

/// `RangedMatcher` — `[?String min][?String max]`.
fn skip_ranged_matcher(body: &[u8], off: &mut usize) -> Option<()> {
    skip_optional(body, off, skip_utf8)?;
    skip_optional(body, off, skip_utf8)
}

fn skip_writable_book_content(body: &[u8], off: &mut usize) -> Option<()> {
    // list(100)(Filterable(STRING_UTF8(1024))).
    skip_list(body, off, skip_filterable_utf8)
}

fn skip_written_book_content(body: &[u8], off: &mut usize) -> Option<()> {
    skip_filterable(body, off, skip_utf8)?; // title Filterable(string(32))
    skip_utf8(body, off)?; // author
    skip_varint(body, off)?; // generation
    skip_list(body, off, skip_filterable_component)?; // pages
    skip_bool(body, off) // resolved
}

fn skip_state_properties_predicate(body: &[u8], off: &mut usize) -> Option<()> {
    // list(PropertyMatcher = [STRING_UTF8 name][ValueMatcher]).
    skip_list(body, off, |b, o| {
        skip_utf8(b, o)?;
        // ValueMatcher = either(ExactMatcher STRING_UTF8, RangedMatcher [?String][?String]).
        skip_either(b, o, skip_utf8, skip_ranged_matcher)
    })
}

fn skip_nbt_predicate(body: &[u8], off: &mut usize) -> Option<()> {
    skip_nbt(body, off) // COMPOUND_TAG
}

/// `TypedDataComponent` — `[DataComponentType registry VarInt][value]`, where the
/// value uses the component's own stream codec (recursing into
/// [`skip_any_component_value`]).
fn skip_typed_data_component(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    let id = frame::read_varint(body, off)?;
    if id < 0 {
        return None;
    }
    skip_any_component_value(body, off, id as u32, depth + 1)
}

fn skip_data_component_exact_predicate(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    // list(64)(TypedDataComponent) — inlined because the element takes `depth`.
    let count = frame::read_varint(body, off)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        skip_typed_data_component(body, off, depth)?;
    }
    Some(())
}

fn skip_data_component_predicate(body: &[u8], off: &mut usize, _depth: usize) -> Option<()> {
    // list(64)(Single). Single = [Type either][value]. The Type either reads
    // [readBoolean][registry varint]: a nonzero byte selects a concrete
    // DATA_COMPONENT_PREDICATE_TYPE, a zero byte a DATA_COMPONENT_TYPE (AnyValue).
    // The value for BOTH branches is a fromCodecWithRegistries NBT tag — a
    // concrete predicate's `singleStreamCodec` serializes the whole predicate to
    // one tag, and an AnyValueType's `unitCodec` encodes to an empty compound —
    // so every element is [bool][varint][NBT tag], fully bounded by the NBT
    // parser (no depth recursion). An End tag is refused like Java's tagCodec.
    let count = frame::read_varint(body, off)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        skip_bool(body, off)?; // either flag (nonzero = concrete predicate type)
        skip_varint(body, off)?; // predicate-type or component-type registry id
        let value = read_nbt(body, off)?;
        if matches!(value, Nbt::End) {
            return None;
        }
    }
    Some(())
}

fn skip_data_component_matchers(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    skip_data_component_exact_predicate(body, off, depth)?;
    skip_data_component_predicate(body, off, depth)
}

fn skip_block_predicate(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    skip_optional(body, off, skip_holder_set)?; // blocks
    skip_optional(body, off, skip_state_properties_predicate)?; // properties
    skip_optional(body, off, skip_nbt_predicate)?; // nbt
    skip_data_component_matchers(body, off, depth) // components
}

/// `AdventureModePredicate` (`can_place_on`/`can_break`) — `list(BlockPredicate)`.
fn skip_block_predicate_list(body: &[u8], off: &mut usize, depth: usize) -> Option<()> {
    // list(BlockPredicate) — inlined because the element takes `depth`.
    let count = frame::read_varint(body, off)?;
    if count < 0 {
        return None;
    }
    for _ in 0..count {
        skip_block_predicate(body, off, depth)?;
    }
    Some(())
}

fn skip_attribute_modifiers_entry(body: &[u8], off: &mut usize) -> Option<()> {
    skip_varint(body, off)?; // Attribute holderRegistry
    skip_attribute_modifier(body, off)?; // modifier
    skip_varint(body, off)?; // EquipmentSlotGroup idMapper
    // Display = Type idMapper (0=Default, 1=Hidden, 2=OverrideText) then dispatch.
    match frame::read_varint(body, off)? {
        0 | 1 => Some(()),
        2 => skip_component_tag(body, off), // OverrideText = [Component tag]
        _ => None,
    }
}

fn skip_attribute_modifiers(body: &[u8], off: &mut usize) -> Option<()> {
    skip_list(body, off, skip_attribute_modifiers_entry)
}

fn skip_charged_projectiles(body: &[u8], off: &mut usize) -> Option<()> {
    // list(1024)(ItemStackTemplate) — the elements are NOT Optional (only
    // ItemContainerContents wraps its ItemStackTemplates in an Optional).
    skip_list(body, off, skip_item_stack_leaf)
}

fn skip_bundle_contents(body: &[u8], off: &mut usize) -> Option<()> {
    // list(256)(ItemStackTemplate) — the elements are NOT Optional.
    skip_list(body, off, skip_item_stack_leaf)
}

fn skip_painting_variant_holder(body: &[u8], off: &mut usize) -> Option<()> {
    // holder(PAINTING_VARIANT, [VarInt][VarInt][Identifier][?Component][?Component]).
    skip_holder(body, off, |b, o| {
        skip_varint(b, o)?;
        skip_varint(b, o)?;
        skip_utf8(b, o)?;
        skip_optional(b, o, skip_component_tag)?;
        skip_optional(b, o, skip_component_tag)
    })
}

/// Bound `shape`'s wire value, advancing `*off` past it. The depth cap guards
/// against pathological nesting through `ItemStackTemplate`/`DataComponentMatchers`
/// recursion on hostile input (Java tracks codec depth the same way).
fn skip_shape(body: &[u8], off: &mut usize, shape: Shape, depth: usize) -> Option<()> {
    use Shape::*;
    if depth > 8 {
        return None;
    }
    match shape {
        VarInt => skip_varint(body, off),
        Unit => Some(()),
        Bool => skip_bool(body, off),
        Float => skip_float(body, off),
        Int => skip_int(body, off),
        Utf8String => skip_utf8(body, off),
        NbtTag => skip_nbt(body, off),
        HolderSet => skip_holder_set(body, off),
        SoundEvent => skip_sound_event(body, off),
        UseEffects => skip_use_effects(body, off),
        Food => skip_food(body, off),
        Weapon => skip_weapon(body, off),
        AttackRange => skip_attack_range(body, off),
        SwingAnimation => skip_swing_animation(body, off),
        UseCooldown => skip_use_cooldown(body, off),
        DamageResistant => skip_holder_set(body, off),
        Repairable => skip_holder_set(body, off),
        Enchantments => skip_enchantments(body, off),
        CustomModelData => skip_custom_model_data(body, off),
        TooltipDisplay => skip_tooltip_display(body, off),
        Tool => skip_tool(body, off),
        Consumable => skip_consumable(body, off),
        ItemStackTemplate => skip_item_stack_template(body, off, depth),
        Equippable => skip_equippable(body, off),
        DeathProtection => skip_death_protection(body, off),
        BlocksAttacks => skip_blocks_attacks(body, off),
        PiercingWeapon => skip_piercing_weapon(body, off),
        KineticWeapon => skip_kinetic_weapon(body, off),
        ArmorTrim => skip_armor_trim(body, off),
        LodestoneTracker => skip_lodestone_tracker(body, off),
        FireworkExplosion => skip_firework_explosion(body, off),
        Fireworks => skip_fireworks(body, off),
        Profile => skip_profile(body, off),
        JukeboxSong => skip_jukebox_song_holder(body, off),
        Instrument => skip_instrument_holder(body, off),
        TrimMaterial => skip_trim_material_holder(body, off),
        BannerPatterns => skip_banner_patterns(body, off),
        PotDecorations => skip_pot_decorations(body, off),
        BlockState => skip_block_state(body, off),
        Bees => skip_bees(body, off),
        Container => skip_container(body, off),
        PotionContents => skip_potion_contents(body, off),
        SuspiciousStewEffects => skip_suspicious_stew_effects(body, off),
        WritableBookContent => skip_writable_book_content(body, off),
        WrittenBookContent => skip_written_book_content(body, off),
        BlockPredicateList => skip_block_predicate_list(body, off, depth),
        AttributeModifiers => skip_attribute_modifiers(body, off),
        ChargedProjectiles => skip_charged_projectiles(body, off),
        BundleContents => skip_bundle_contents(body, off),
        PaintingVariant => skip_painting_variant_holder(body, off),
    }
}

/// Bound one registered component's wire value (the entry's type id has already
/// been read), advancing `*off` past it. `None` for an id outside the registered
/// range or a value that does not fit its shape.
fn skip_component_value(body: &[u8], off: &mut usize, id: u32, depth: usize) -> Option<()> {
    let shape = component_shape(id)?;
    skip_shape(body, off, shape, depth)
}

/// Consume one registered component's wire value (the entry's type id has
/// already been read), dispatching the NBT-shaped ids (0/6/9/11/58/59/60) —
/// which are not expressible as a [`Shape`] — to the shared reader, and every
/// other registered id to its byte-exact shape walker. Used where a composite
/// embeds a `DataComponentPatch` (an `ItemStackTemplate`-holding component) or a
/// `TypedDataComponent`, so a nested `custom_name`/`custom_data`/... value is
/// skipped instead of failing the whole canonicalization.
fn skip_any_component_value(body: &[u8], off: &mut usize, id: u32, depth: usize) -> Option<()> {
    match component_type_name(id) {
        Some(name) => {
            let _ = read_component_value(body, off, name)?;
            Some(())
        }
        None => skip_component_value(body, off, id, depth),
    }
}

/// One positive patch entry: the component type id plus the canonical value.
struct PatchEntry {
    type_id: u32,
    value: Vec<u8>,
}

/// Canonicalize a `DataComponentPatch` at `*off`. The caller passes the offset
/// at the very start of the patch value (before the counts); this function owns
/// and consumes the leading `positive` and `negative` count VarInts, then the
/// `positive` entries (`[type id VarInt][value]`) and `negative` entries
/// (`[type id VarInt]`), advancing `*off` past all of them. It returns the
/// canonical re-serialization with the positive entries sorted by type id and
/// the negatives sorted the same way.
///
/// A positive entry whose component is NBT-shaped is semantically canonicalized
/// (NBT compound field order); every other registered component's value is
/// copied byte-exact. A duplicate id within the positives or the negatives, a
/// negative count, an id outside the registered 26.2 range, or a value that does
/// not fit its shape fails the whole patch (`None`).
fn canon_data_component_patch_body(body: &[u8], off: &mut usize) -> Option<Vec<u8>> {
    let positive = frame::read_varint(body, off)?;
    let negative = frame::read_varint(body, off)?;
    if positive < 0 || negative < 0 {
        return None;
    }
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::with_capacity(positive as usize);
    for _ in 0..positive {
        let type_id = frame::read_varint(body, off)?;
        if type_id < 0 || !seen.insert(type_id) {
            return None; // Java's map.put silently overwrites; a duplicate is not honest
        }
        let value = match component_type_name(type_id as u32) {
            Some(name) => read_component_value(body, off, name)?,
            None => {
                let start = *off;
                skip_component_value(body, off, type_id as u32, 0)?;
                body[start..*off].to_vec()
            }
        };
        entries.push(PatchEntry {
            type_id: type_id as u32,
            value,
        });
    }
    let mut negatives = Vec::with_capacity(negative as usize);
    for _ in 0..negative {
        let type_id = frame::read_varint(body, off)?;
        if type_id < 0 || !seen.insert(type_id) {
            return None;
        }
        negatives.push(type_id as u32);
    }
    entries.sort_by_key(|e| e.type_id);
    negatives.sort_unstable();

    let mut out = Vec::with_capacity(body.len() / 2);
    frame::write_varint(&mut out, entries.len() as i32);
    frame::write_varint(&mut out, negatives.len() as i32);
    for e in &entries {
        frame::write_varint(&mut out, e.type_id as i32);
        out.extend_from_slice(&e.value);
    }
    for t in &negatives {
        frame::write_varint(&mut out, *t as i32);
    }
    Some(out)
}

/// Canonicalize a `DisplayInfo` value and return its canonical bytes:
/// `[title NBT][description NBT][icon][frame VarInt][flags int][bg?][x float][y float]`.
/// `icon` is an `ItemStackTemplate` = `[item VarInt][count VarInt][patch]`.
fn canon_display_info(body: &[u8], off: &mut usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(body.len() / 2);

    let title = read_nbt(body, off)?;
    write_nbt(&mut out, &title);
    let description = read_nbt(body, off)?;
    write_nbt(&mut out, &description);

    // icon: ItemStackTemplate = [item VarInt][count VarInt][DataComponentPatch].
    let item = frame::read_varint(body, off)?;
    let count = frame::read_varint(body, off)?;
    if item < 0 || count < 0 {
        return None;
    }
    frame::write_varint(&mut out, item);
    frame::write_varint(&mut out, count);
    let patch = canon_data_component_patch_body(body, off)?;
    out.extend_from_slice(&patch);

    let frame_v = frame::read_varint(body, off)?;
    if frame_v < 0 {
        return None;
    }
    frame::write_varint(&mut out, frame_v);
    let flags = frame::read_i32(body, off)?;
    out.extend_from_slice(&flags.to_be_bytes());
    if flags & 1 != 0 {
        let bg = read_string(body, off)?;
        write_string(&mut out, &bg);
    }
    let xy = frame::read_bytes(body, off, 8)?;
    out.extend_from_slice(xy);
    Some(out)
}

/// Canonicalize one advancement value at `*off` and return `(id, raw_bytes)`,
/// where `raw_bytes` is the canonical `[id][parent?][display?][requirements]`
/// `[telemetry]`. A display payload is structurally canonicalized; an absent
/// display (the pinned join fixture) is passed through with no display payload
/// fabricated or rewritten.
fn canon_advancement_value(body: &[u8], off: &mut usize) -> Option<(String, Vec<u8>)> {
    let id = read_string(body, off)?;
    let mut out = Vec::with_capacity(body.len() / 4);
    write_string(&mut out, &id);

    if *body.get(*off)? != 0 {
        *off += 1;
        let parent = read_string(body, off)?;
        out.push(1);
        write_string(&mut out, &parent);
    } else {
        *off += 1;
        out.push(0);
    }

    if *body.get(*off)? != 0 {
        *off += 1;
        let display = canon_display_info(body, off)?;
        out.push(1);
        out.extend_from_slice(&display);
    } else {
        *off += 1;
        out.push(0);
    }

    // requirements: [VarInt groups][group × ([VarInt names][names])] — inner
    // sets are order-insensitive, so re-emit them sorted (as before #221).
    let group_count = frame::read_varint(body, off)?;
    if group_count < 0 {
        return None;
    }
    let mut groups: Vec<Vec<String>> = Vec::with_capacity(group_count as usize);
    for _ in 0..group_count {
        let name_count = frame::read_varint(body, off)?;
        if name_count < 0 {
            return None;
        }
        let mut names = Vec::with_capacity(name_count as usize);
        for _ in 0..name_count {
            names.push(read_string(body, off)?);
        }
        names.sort();
        groups.push(names);
    }
    let telemetry = *body.get(*off)?;
    *off += 1;

    frame::write_varint(&mut out, groups.len() as i32);
    for names in &groups {
        frame::write_varint(&mut out, names.len() as i32);
        for name in names {
            write_string(&mut out, name);
        }
    }
    out.push(telemetry);
    Some((id, out))
}

/// Canonicalize an `update_advancements` (130) body: sort the added list, the
/// removed set, and the progress map (all HashMap/HashSet-backed per boot) by
/// identifier; sort each progress's criteria by criterion name; and
/// structurally canonicalize each advancement's display payload (NBT compound
/// field order + `DataComponentPatch` entry order). Obtained instants are
/// wall-clock per boot, so they are zeroed like the existing canonicalizer.
///
/// Returns `None` when the body does not parse or a display payload cannot be
/// bounded; the caller then keeps the raw body (honest non-canonicalization).
pub fn canon_update_advancements(body: &[u8]) -> Option<Vec<u8>> {
    let mut off = 0;
    let reset = *body.get(off)?;
    off += 1;

    let added_count = frame::read_varint(body, &mut off)?;
    if added_count < 0 {
        return None;
    }
    let mut added = Vec::with_capacity(added_count as usize);
    for _ in 0..added_count {
        let (id, raw) = canon_advancement_value(body, &mut off)?;
        added.push((id, raw));
    }
    added.sort_by(|a, b| a.0.cmp(&b.0));

    let removed_count = frame::read_varint(body, &mut off)?;
    if removed_count < 0 {
        return None;
    }
    let mut removed = Vec::with_capacity(removed_count as usize);
    for _ in 0..removed_count {
        let start = off;
        let id = read_string(body, &mut off)?;
        removed.push((id, body[start..off].to_vec()));
    }
    removed.sort_by(|a, b| a.0.cmp(&b.0));

    let progress_count = frame::read_varint(body, &mut off)?;
    if progress_count < 0 {
        return None;
    }
    let mut progress = Vec::with_capacity(progress_count as usize);
    for _ in 0..progress_count {
        let id = read_string(body, &mut off)?;
        // AdvancementProgress: [VarInt criteria][criteria × ([String][bool][?Instant])].
        let crit_count = frame::read_varint(body, &mut off)?;
        if crit_count < 0 {
            return None;
        }
        let mut criteria = Vec::with_capacity(crit_count as usize);
        for _ in 0..crit_count {
            let name = read_string(body, &mut off)?;
            let obtained = *body.get(off)?;
            off += 1;
            if obtained != 0 {
                frame::read_bytes(body, &mut off, 8)?;
            }
            let mut raw = Vec::with_capacity(name.len() + 16);
            write_string(&mut raw, &name);
            raw.push(obtained);
            if obtained != 0 {
                raw.extend_from_slice(&0i64.to_be_bytes()); // obtained instant -> 0
            }
            criteria.push((name, raw));
        }
        criteria.sort_by(|a, b| a.0.cmp(&b.0));
        let mut prog = Vec::with_capacity(crit_count as usize * 4 + id.len() + 4);
        write_string(&mut prog, &id);
        frame::write_varint(&mut prog, criteria.len() as i32);
        for (_, raw) in &criteria {
            prog.extend_from_slice(raw);
        }
        progress.push((id, prog));
    }
    progress.sort_by(|a, b| a.0.cmp(&b.0));

    let show = *body.get(off)?;
    if off + 1 != body.len() {
        return None;
    }

    let mut out = Vec::with_capacity(body.len());
    out.push(reset);
    frame::write_varint(&mut out, added.len() as i32);
    for (_, raw) in &added {
        out.extend_from_slice(raw);
    }
    frame::write_varint(&mut out, removed.len() as i32);
    for (_, raw) in &removed {
        out.extend_from_slice(raw);
    }
    frame::write_varint(&mut out, progress.len() as i32);
    for (_, raw) in &progress {
        out.extend_from_slice(raw);
    }
    out.push(show);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id of the first added advancement whose canonical form does not match
    /// the baseline's, or `None` when every advancement canonicalizes identically.
    /// Test-only helper: names the affected advancement when a negative test
    /// asserts that a semantic mutation is detected.
    fn first_advancement_mismatch(body: &[u8], baseline: &[u8]) -> Option<String> {
        let added = canonical_added(body)?;
        let baseline_added = canonical_added(baseline)?;
        for (id, raw) in &added {
            if baseline_added.get(id) != Some(raw) {
                return Some(id.clone());
            }
        }
        None
    }

    /// The `added` advancements of a body as `id -> canonical bytes`, sorted by id.
    fn canonical_added(body: &[u8]) -> Option<std::collections::HashMap<String, Vec<u8>>> {
        let mut off = 0;
        let _reset = *body.get(off)?;
        off += 1;
        let added_count = frame::read_varint(body, &mut off)?;
        if added_count < 0 {
            return None;
        }
        let mut added = std::collections::HashMap::with_capacity(added_count as usize);
        for _ in 0..added_count {
            let (id, raw) = canon_advancement_value(body, &mut off)?;
            added.insert(id, raw);
        }
        Some(added)
    }

    // -- wire builders (Paper-faithful: [bool] optional/display, VarInt counts,
    //    VarInt-prefixed UTF-8 identifiers, big-endian primitives) -------------

    fn nbt_str(s: &str) -> Nbt {
        Nbt::String(s.to_owned())
    }

    fn nbt_compound(fields: Vec<(&str, Nbt)>) -> Nbt {
        Nbt::Compound(fields.into_iter().map(|(n, v)| (n.to_owned(), v)).collect())
    }

    fn nbt_bytes(v: &Nbt) -> Vec<u8> {
        let mut out = Vec::new();
        write_nbt(&mut out, v);
        out
    }

    /// Raw NBT compound bytes with the fields encoded in the given order —
    /// bypasses the `write_payload` on-write sort so a test can feed a
    /// genuinely unsorted compound to the canonicalizer.
    fn raw_compound(fields: &[(&str, u8, Vec<u8>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(10); // compound tag
        for (name, type_id, payload) in fields {
            out.push(*type_id);
            out.extend_from_slice(&(name.len() as u16).to_be_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(payload);
        }
        out.push(0); // end tag
        out
    }

    /// The `[u16 len][chars]` payload of an NBT string tag (no type byte).
    fn raw_str(s: &str) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(s.len() as u16).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
        out
    }

    fn patch(pos: &[(u32, Vec<u8>)], neg: &[u32]) -> Vec<u8> {
        let mut out = Vec::new();
        frame::write_varint(&mut out, pos.len() as i32);
        frame::write_varint(&mut out, neg.len() as i32);
        for (id, value) in pos {
            frame::write_varint(&mut out, *id as i32);
            out.extend_from_slice(value);
        }
        for id in neg {
            frame::write_varint(&mut out, *id as i32);
        }
        out
    }

    fn icon(item: i32, count: i32, components: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        frame::write_varint(&mut out, item);
        frame::write_varint(&mut out, count);
        out.extend_from_slice(components);
        out
    }

    fn display(
        title: &Nbt,
        desc: &Nbt,
        icon: &[u8],
        frame_v: i32,
        flags: i32,
        position: (f32, f32),
        bg: Option<&str>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        write_nbt(&mut out, title);
        write_nbt(&mut out, desc);
        out.extend_from_slice(icon);
        frame::write_varint(&mut out, frame_v);
        out.extend_from_slice(&flags.to_be_bytes());
        if let Some(bg) = bg {
            write_string(&mut out, bg);
        }
        out.extend_from_slice(&position.0.to_be_bytes());
        out.extend_from_slice(&position.1.to_be_bytes());
        out
    }

    fn advancement(
        id: &str,
        parent: Option<&str>,
        display: Option<&[u8]>,
        groups: &[Vec<String>],
        telemetry: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        write_string(&mut out, id);
        match parent {
            Some(p) => {
                out.push(1);
                write_string(&mut out, p);
            }
            None => out.push(0),
        }
        match display {
            Some(d) => {
                out.push(1);
                out.extend_from_slice(d);
            }
            None => out.push(0),
        }
        frame::write_varint(&mut out, groups.len() as i32);
        for g in groups {
            frame::write_varint(&mut out, g.len() as i32);
            for name in g {
                write_string(&mut out, name);
            }
        }
        out.push(telemetry as u8);
        out
    }

    fn progress(id: &str, criteria: &[(&str, bool)]) -> Vec<u8> {
        let mut out = Vec::new();
        write_string(&mut out, id);
        frame::write_varint(&mut out, criteria.len() as i32);
        for (name, obtained) in criteria {
            write_string(&mut out, name);
            out.push(*obtained as u8);
            if *obtained {
                out.extend_from_slice(&42i64.to_be_bytes());
            }
        }
        out
    }

    fn body(
        reset: bool,
        added: &[Vec<u8>],
        removed: &[&str],
        progress: &[Vec<u8>],
        show: bool,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(reset as u8);
        frame::write_varint(&mut out, added.len() as i32);
        for a in added {
            out.extend_from_slice(a);
        }
        frame::write_varint(&mut out, removed.len() as i32);
        for r in removed {
            write_string(&mut out, r);
        }
        frame::write_varint(&mut out, progress.len() as i32);
        for p in progress {
            out.extend_from_slice(p);
        }
        out.push(show as u8);
        out
    }

    // -- fixtures -------------------------------------------------------------

    /// A custom_name component value: a bare NBT tag (id 6).
    fn custom_name(s: &str) -> (u32, Vec<u8>) {
        (6, nbt_bytes(&nbt_compound(vec![("text", nbt_str(s))])))
    }

    /// A custom_data component value: a compound NBT tag (id 0).
    fn custom_data(s: &str) -> (u32, Vec<u8>) {
        (0, nbt_bytes(&nbt_compound(vec![("extra", nbt_str(s))])))
    }

    /// A lore component value: `[VarInt count][count × NBT]` (id 11).
    fn lore(lines: &[&str]) -> (u32, Vec<u8>) {
        let mut out = Vec::new();
        frame::write_varint(&mut out, lines.len() as i32);
        for l in lines {
            let mut tag = Vec::new();
            write_nbt(&mut tag, &nbt_compound(vec![("text", nbt_str(l))]));
            out.extend_from_slice(&tag);
        }
        (11, out)
    }

    /// An entity_data component value: `[type registry id VarInt][compound NBT]`
    /// (id 58).
    fn entity_data(type_id: i32, tag: &Nbt) -> (u32, Vec<u8>) {
        let mut out = Vec::new();
        frame::write_varint(&mut out, type_id);
        write_nbt(&mut out, tag);
        (58, out)
    }

    /// Like `display`, but takes the title/description as PRE-ENCODED raw NBT
    /// wire bytes (e.g. `raw_compound` output). This bypasses `write_nbt`'s
    /// on-write compound-field sort so a test can feed a genuinely unsorted
    /// compound directly to the canonicalizer, independent of the writer.
    fn display_raw(
        title: &[u8],
        desc: &[u8],
        icon: &[u8],
        frame_v: i32,
        flags: i32,
        position: (f32, f32),
        bg: Option<&str>,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(title);
        out.extend_from_slice(desc);
        out.extend_from_slice(icon);
        frame::write_varint(&mut out, frame_v);
        out.extend_from_slice(&flags.to_be_bytes());
        if let Some(bg) = bg {
            write_string(&mut out, bg);
        }
        out.extend_from_slice(&position.0.to_be_bytes());
        out.extend_from_slice(&position.1.to_be_bytes());
        out
    }

    /// Like `display_for`, but for pre-encoded raw title/description bytes.
    fn display_for_raw(
        title: &[u8],
        desc: &[u8],
        components: &[(u32, Vec<u8>)],
        negatives: &[u32],
        order: &[usize],
    ) -> Vec<u8> {
        let mut entries = Vec::new();
        for &i in order {
            entries.push(components[i].clone());
        }
        display_raw(
            title,
            desc,
            &icon(926, 1, &patch(&entries, negatives)),
            0,
            0,
            (0.5, -1.25),
            None,
        )
    }

    fn display_for(
        title: &Nbt,
        desc: &Nbt,
        components: &[(u32, Vec<u8>)],
        negatives: &[u32],
        order: &[usize],
    ) -> Vec<u8> {
        let mut entries = Vec::new();
        for &i in order {
            entries.push(components[i].clone());
        }
        display(
            title,
            desc,
            &icon(926, 1, &patch(&entries, negatives)),
            0,
            0,
            (0.5, -1.25),
            None,
        )
    }

    // -- tests ----------------------------------------------------------------

    #[test]
    fn reordered_equivalent_displays_canonicalize_identically() {
        // The two bodies carry the SAME semantic display payloads, encoded with
        // genuinely different wire byte orders in FOUR independent dimensions:
        //   - the title/description NBT compound field order (unsorted
        //     [text,color] vs the canonical [color,text], built with
        //     `raw_compound` so the writer's on-write sort cannot mask it);
        //   - the DataComponentPatch positive-entry order;
        //   - the added-list order;
        //   - the progress-criteria order.
        // Every variation must collapse to one canonical byte string. This is
        // non-vacuous for the compound sort: without `write_payload` sorting
        // compound fields on emit, body_a's story:first would keep its
        // unsorted [text,color] wire bytes while body_b's would keep
        // [color,text], and the two canonical forms would differ.
        let title_unsorted =
            raw_compound(&[("text", 8, raw_str("A")), ("color", 8, raw_str("red"))]);
        let title_sorted = raw_compound(&[("color", 8, raw_str("red")), ("text", 8, raw_str("A"))]);
        let desc_unsorted =
            raw_compound(&[("text", 8, raw_str("D")), ("color", 8, raw_str("green"))]);
        let desc_sorted =
            raw_compound(&[("color", 8, raw_str("green")), ("text", 8, raw_str("D"))]);
        let zombie = nbt_compound(vec![("id", nbt_str("zombie"))]);

        // Shared component set: custom_name(6), custom_data(0), lore(11),
        // entity_data(58). All four are NBT-shaped and must canonicalize by
        // component type id regardless of entry order. The removals are ids
        // DISJOINT from the set-positive ids — Java's DataComponentPatch is one
        // map keyed by component type, so a valid patch can never both set and
        // remove the same component (a negative `map.put(type, Optional.empty())`
        // would just overwrite the positive entry).
        let components = &[
            custom_name("x"),
            custom_data("y"),
            lore(&["L1", "L2"]),
            entity_data(7, &zombie),
        ];
        let negatives = &[2u32, 3, 9];

        // body_a: story:first carries the UNSORTED title/desc compounds,
        // story:second the sorted ones. body_b swaps which advancement gets the
        // unsorted wire bytes, and additionally scrambles the patch-entry order.
        let body_a = body(
            false,
            &[
                advancement(
                    "story:second",
                    None,
                    Some(&display_for_raw(
                        &title_sorted,
                        &desc_sorted,
                        components,
                        negatives,
                        &[0, 1, 2, 3],
                    )),
                    &[vec!["a".into()]],
                    false,
                ),
                advancement(
                    "story:first",
                    None,
                    Some(&display_for_raw(
                        &title_unsorted,
                        &desc_unsorted,
                        components,
                        negatives,
                        &[0, 1, 2, 3],
                    )),
                    &[vec!["b".into()]],
                    false,
                ),
            ],
            &["story:zz"],
            &[progress(
                "story:first",
                &[("crit_b", true), ("crit_a", false)],
            )],
            true,
        );
        let body_b = body(
            false,
            &[
                advancement(
                    "story:first",
                    None,
                    Some(&display_for_raw(
                        &title_sorted,
                        &desc_sorted,
                        components,
                        negatives,
                        &[3, 2, 1, 0],
                    )),
                    &[vec!["b".into()]],
                    false,
                ),
                advancement(
                    "story:second",
                    None,
                    Some(&display_for_raw(
                        &title_unsorted,
                        &desc_unsorted,
                        components,
                        negatives,
                        &[2, 0, 3, 1],
                    )),
                    &[vec!["a".into()]],
                    false,
                ),
            ],
            &["story:zz"],
            &[progress(
                "story:first",
                &[("crit_a", false), ("crit_b", true)],
            )],
            true,
        );

        let canon_a = canon_update_advancements(&body_a).expect("body_a canonicalizes");
        let canon_b = canon_update_advancements(&body_b).expect("body_b canonicalizes");
        assert_eq!(
            canon_a, canon_b,
            "reordered-equivalent displays must canonicalize identically"
        );
        // Idempotent: canonicalizing the canonical form is a no-op.
        assert_eq!(canon_update_advancements(&canon_a), Some(canon_a.clone()));
        assert_eq!(canon_update_advancements(&canon_b), Some(canon_b.clone()));
    }

    #[test]
    fn unsorted_compound_fields_canonicalize_to_sorted_bytes() {
        // #221 headline: a Component serializes to an NBT compound whose field
        // order depends on the DFU record map iteration, so a capture can carry
        // the fields in any order. The `write_payload` writer sorts on emit, so
        // the `nbt_compound` builders cannot produce an unsorted compound —
        // hand-craft the raw wire bytes instead and assert the canonical form
        // is the sorted order (not a pass-through of the parse order). Without
        // the compound-field sort a regression would emit the unsorted input
        // verbatim and fail this assertion.
        let title_unsorted = raw_compound(&[
            ("text", 8, raw_str("A")),
            ("italic", 1, vec![1]),
            ("color", 8, raw_str("red")),
        ]);
        let title_sorted = raw_compound(&[
            ("color", 8, raw_str("red")),
            ("italic", 1, vec![1]),
            ("text", 8, raw_str("A")),
        ]);
        let desc_unsorted =
            raw_compound(&[("text", 8, raw_str("D")), ("color", 8, raw_str("green"))]);
        let desc_sorted =
            raw_compound(&[("color", 8, raw_str("green")), ("text", 8, raw_str("D"))]);

        // DisplayInfo with the title/description compounds in unsorted field
        // order on the wire: [title][description][icon item=0 count=0 patch
        // (pos=0,neg=0)][frame 0][flags 0][x y floats].
        let mut display = Vec::new();
        display.extend_from_slice(&title_unsorted);
        display.extend_from_slice(&desc_unsorted);
        display.extend_from_slice(&[0, 0, 0, 0]); // item, count, patch(0, 0)
        display.extend_from_slice(&[0]); // frame
        display.extend_from_slice(&0i32.to_be_bytes()); // flags
        display.extend_from_slice(&0.5f32.to_be_bytes());
        display.extend_from_slice(&(-1.25f32).to_be_bytes());

        let mut off = 0;
        let canon = canon_display_info(&display, &mut off)
            .expect("display with unsorted compounds canonicalizes");
        assert_eq!(off, display.len(), "whole display consumed");

        // Title and description must be re-emitted with fields sorted by name,
        // with every non-compound field passed through verbatim.
        let mut expected = Vec::new();
        expected.extend_from_slice(&title_sorted);
        expected.extend_from_slice(&desc_sorted);
        expected.extend_from_slice(&[0, 0, 0, 0]);
        expected.extend_from_slice(&[0]);
        expected.extend_from_slice(&0i32.to_be_bytes());
        expected.extend_from_slice(&0.5f32.to_be_bytes());
        expected.extend_from_slice(&(-1.25f32).to_be_bytes());
        assert_eq!(canon, expected);
    }

    #[test]
    fn semantic_mutation_is_detected_and_names_advancement() {
        let title = nbt_compound(vec![("text", nbt_str("Original"))]);
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[custom_name("x")], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let base = body(false, std::slice::from_ref(&adv), &[], &[], true);

        // Changed title text (a real semantic difference).
        let mutated_title = nbt_compound(vec![("text", nbt_str("Mutated"))]);
        let mutated = advancement(
            "story:root",
            None,
            Some(&display_for(
                &mutated_title,
                &title,
                &[custom_name("x")],
                &[],
                &[0],
            )),
            &[vec!["c".into()]],
            false,
        );
        let mutated_body = body(false, &[mutated], &[], &[], true);
        assert_eq!(
            first_advancement_mismatch(&base, &mutated_body),
            Some("story:root".to_owned())
        );

        // The mutated body must NOT canonicalize to the baseline.
        assert_ne!(
            canon_update_advancements(&base),
            canon_update_advancements(&mutated_body)
        );
    }

    #[test]
    fn no_display_body_is_honest_and_idempotent() {
        // Like the pinned join fixture, this body carries no advancement display
        // data (display byte 0). The body is built already in canonical order
        // (added/progress lists and criteria sorted, no obtained instants to
        // zero), so canonicalization is a byte-for-byte identity — no fabricated
        // display payload is invented. Note this holds only because every
        // criterion is NOT obtained: a fresh join's obtained criteria carry a
        // wall-clock instant that canonicalizes to 0, so a no-display body with
        // obtained=true would NOT be byte-identical (see
        // `progress_obtained_instants_are_zeroed`). This test deliberately uses
        // obtained=false to isolate the display-honesty claim.
        let adv = advancement("story:root", None, None, &[vec!["c".into()]], false);
        let input = body(
            true,
            std::slice::from_ref(&adv),
            &[],
            &[progress("story:root", &[("c", false)])],
            true,
        );
        let canon = canon_update_advancements(&input).expect("no-display body canonicalizes");
        assert_eq!(
            canon, input,
            "no-display body (all criteria un-obtained) must pass through byte-identically"
        );
        assert_eq!(canon_update_advancements(&canon), Some(canon.clone()));
    }

    #[test]
    fn non_nbt_component_value_is_preserved_byte_exact() {
        // item_model (id 10) is an Identifier, not an NBT payload. The harness
        // cannot semantically canonicalize its value, so it must be copied
        // byte-exact while the patch still canonicalizes (entries sorted by id).
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        // item_model is a Utf8String Identifier: [VarInt byte len][bytes].
        let value = vec![0x03, 0x61, 0x62, 0x63]; // "abc"
        let item_model = (10u32, value.clone());
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[item_model], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        let canon = canon_update_advancements(&input).expect("patch canonicalizes");
        // The single positive entry is the only one, so its value is verbatim.
        assert!(canon.windows(value.len()).any(|w| w == value));
        assert_eq!(canon_update_advancements(&canon), Some(canon.clone()));
    }

    #[test]
    fn mixed_patch_with_non_nbt_component_sorts_and_preserves() {
        // A patch with an NBT-shaped custom_name (id 6) AND a non-NBT item_model
        // (id 10) fed in reverse order must canonicalize to [6, 10] with the
        // item_model value preserved byte-exact and the custom_name value
        // semantically canonicalized. Exercised directly on the patch so the
        // assertion is the exact expected byte string, not a position search.
        let model_value = vec![0x03, 0x61, 0x62, 0x63]; // [len 3]["abc"]
        let model = (10u32, model_value.clone());
        let name = custom_name("x");
        let input_patch = patch(&[model, name.clone()], &[]); // id 10 before id 6
        let mut off = 0;
        let canon_patch =
            canon_data_component_patch_body(&input_patch, &mut off).expect("patch canonicalizes");
        assert_eq!(off, input_patch.len(), "whole patch consumed");

        // [pos=2][neg=0][id 6][name NBT][id 10][model bytes].
        let mut expected = Vec::new();
        frame::write_varint(&mut expected, 2);
        frame::write_varint(&mut expected, 0);
        frame::write_varint(&mut expected, 6);
        expected.extend_from_slice(&name.1);
        frame::write_varint(&mut expected, 10);
        expected.extend_from_slice(&model_value);
        assert_eq!(canon_patch, expected);
    }

    #[test]
    fn duplicate_patch_id_is_rejected() {
        // Java's Reference2ObjectMap.put silently overwrites a duplicate id; an
        // honest canonicalizer must reject the patch (None) rather than guess
        // which of the two values Java kept.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let dup_a = custom_name("a");
        let dup_b = custom_name("b");
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[dup_a, dup_b], &[], &[0, 1])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        assert_eq!(canon_update_advancements(&input), None);
    }

    #[test]
    fn progress_obtained_instants_are_zeroed() {
        // The pre-existing canonicalizer behavior: obtained instants are
        // wall-clock per boot, so they are zeroed. Two bodies that differ only
        // in the instant value must canonicalize identically.
        let adv = advancement("story:root", None, None, &[vec!["c".into()]], false);
        let instant_a = progress("story:root", &[("c", true)]);
        let mut instant_b = progress("story:root", &[("c", true)]);
        // The entry is [id][count][name][obtained=1][8-byte long]; the long is
        // the final 8 bytes. Flip only its least-significant byte so the two
        // bodies differ solely in the obtained instant.
        let len = instant_b.len();
        instant_b[len - 1] ^= 0x01;
        let body_a = body(false, std::slice::from_ref(&adv), &[], &[instant_a], true);
        let body_b = body(false, std::slice::from_ref(&adv), &[], &[instant_b], true);
        assert_ne!(body_a, body_b);
        assert_eq!(
            canon_update_advancements(&body_a),
            canon_update_advancements(&body_b)
        );
    }

    /// A raw `[type byte][payload]` root NBT value with a NEGATIVE payload
    /// length for `type_byte`. `length` is written big-endian as the tag's
    /// count field (i32 for arrays/list, u16 for a string).
    fn raw_negative_length(type_byte: u8, length: i32) -> Vec<u8> {
        let mut out = Vec::new();
        out.push(type_byte);
        out.extend_from_slice(&length.to_be_bytes());
        out
    }

    /// Build a `DisplayInfo` whose title is the given raw NBT bytes, with an
    /// otherwise-valid icon/frame/flags/position, so a title that refuses to
    /// parse makes `canon_display_info` return `None`.
    fn display_with_title(title: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(title);
        // description: a valid empty compound.
        out.push(10);
        out.push(0);
        // icon: item 0, count 1, patch (pos=0, neg=0).
        frame::write_varint(&mut out, 0);
        frame::write_varint(&mut out, 1);
        frame::write_varint(&mut out, 0);
        frame::write_varint(&mut out, 0);
        frame::write_varint(&mut out, 0); // frame
        out.extend_from_slice(&0i32.to_be_bytes()); // flags
        out.extend_from_slice(&0.5f32.to_be_bytes());
        out.extend_from_slice(&(-1.25f32).to_be_bytes());
        out
    }

    #[test]
    fn negative_nbt_array_list_lengths_are_rejected() {
        // Java's `NbtIo` throws a DecoderException for a negative array/list
        // size. The canonicalizer must refuse the payload (None), not coerce it
        // to zero and emit a fabricated empty array.
        for type_byte in [
            7u8, /* ByteArray */
            11,  /* IntArray */
            12,  /* LongArray */
        ] {
            let display = display_with_title(&raw_negative_length(type_byte, -1));
            let mut off = 0;
            assert_eq!(
                canon_display_info(&display, &mut off),
                None,
                "negative {type_byte} array length must fail canonicalization"
            );
        }

        // List (type 9): [elem type byte][count i32][elems].
        let mut list = Vec::new();
        list.push(9);
        list.push(8); // elem: string
        list.extend_from_slice(&(-1i32).to_be_bytes()); // count -1
        let display = display_with_title(&list);
        let mut off = 0;
        assert_eq!(
            canon_display_info(&display, &mut off),
            None,
            "negative list length must fail canonicalization"
        );
    }

    #[test]
    fn negative_length_inside_compound_is_not_a_terminator() {
        // A negative ByteArray length nested INSIDE a compound field must fail
        // the whole compound — the pre-#221 parser conflated a failed field
        // with the type-0 end tag and would have silently terminated the
        // compound, accepting wire bytes Java rejects.
        let mut title = Vec::new();
        title.push(10); // compound
        title.push(7); // field type: ByteArray
        title.extend_from_slice(&2u16.to_be_bytes()); // name len
        title.extend_from_slice(b"ab"); // name
        title.extend_from_slice(&(-1i32).to_be_bytes()); // negative length
        title.push(0); // would-be end tag
        let display = display_with_title(&title);
        let mut off = 0;
        assert_eq!(
            canon_display_info(&display, &mut off),
            None,
            "a negative array length inside a compound must fail the whole compound"
        );
    }

    #[test]
    fn background_identifier_branch_is_covered() {
        // DisplayInfo flags bit 0 (background present): the wire carries a
        // VarInt-prefixed identifier string after the flags int, before the
        // position floats. The canonical form must preserve the identifier
        // verbatim and consume the whole display.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let desc = nbt_compound(vec![("text", nbt_str("D"))]);
        let mut display = Vec::new();
        write_nbt(&mut display, &title);
        write_nbt(&mut display, &desc);
        frame::write_varint(&mut display, 0); // icon item
        frame::write_varint(&mut display, 1); // icon count
        frame::write_varint(&mut display, 0); // patch positive
        frame::write_varint(&mut display, 0); // patch negative
        frame::write_varint(&mut display, 0); // frame
        display.extend_from_slice(&1i32.to_be_bytes()); // flags: background present
        write_string(
            &mut display,
            "minecraft:textures/gui/advancements/backgrounds/adventure.png",
        );
        display.extend_from_slice(&0.5f32.to_be_bytes());
        display.extend_from_slice(&(-1.25f32).to_be_bytes());

        let mut off = 0;
        let canon = canon_display_info(&display, &mut off).expect("display canonicalizes");
        assert_eq!(off, display.len(), "whole display consumed");
        // Byte-identical to the input: everything here is already canonical
        // (single-field compounds, sorted patch, identifier passed verbatim).
        assert_eq!(canon, display);
    }

    #[test]
    fn list_tag_with_end_elem_and_positive_count_is_rejected() {
        // Java's `ListTag.loadList` throws "Missing type on ListTag" when the
        // elem type is End (0) with a positive count. The strict parser must
        // refuse the whole tag (None) rather than treat the empty `End` elem as
        // a list of nothing.
        let mut bad_title = Vec::new();
        bad_title.push(9); // list tag
        bad_title.push(0); // elem: End
        bad_title.extend_from_slice(&1i32.to_be_bytes()); // count 1 — Java rejects
        let mut bad_display = Vec::new();
        bad_display.extend_from_slice(&bad_title);
        write_nbt(
            &mut bad_display,
            &nbt_compound(vec![("text", nbt_str("D"))]),
        ); // desc
        frame::write_varint(&mut bad_display, 0); // icon item
        frame::write_varint(&mut bad_display, 1); // icon count
        frame::write_varint(&mut bad_display, 0); // patch positive
        frame::write_varint(&mut bad_display, 0); // patch negative
        frame::write_varint(&mut bad_display, 0); // frame
        bad_display.extend_from_slice(&0i32.to_be_bytes()); // flags
        bad_display.extend_from_slice(&0.5f32.to_be_bytes());
        bad_display.extend_from_slice(&(-1.25f32).to_be_bytes());

        let mut off = 0;
        assert_eq!(canon_display_info(&bad_display, &mut off), None);
    }

    #[test]
    fn compound_tag_components_enforce_compound_value() {
        // custom_data / bucket_entity_data / entity_data / block_entity_data use
        // ByteBufCodecs.COMPOUND_TAG: the value MUST be a CompoundTag, else Java
        // throws DecoderException. A custom_data whose value is a plain NBT
        // string must fail the whole advancement (None), not be re-emitted.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        // id 0 custom_data value = [type 8][len][string] — NOT a compound.
        let bad_custom_data = (0u32, nbt_bytes(&Nbt::String("nope".to_owned())));
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[bad_custom_data], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        assert_eq!(canon_update_advancements(&input), None);
    }

    #[test]
    fn modified_utf8_astral_and_nul_round_trip_through_advancement_nbt() {
        // NBT strings (String payloads and compound field names) use
        // DataInput.readUTF/writeUTF (modified UTF-8). An astral character
        // encodes as a 6-byte surrogate pair and NUL as `C0 80`; both must
        // survive a title canonicalization round-trip byte-for-byte.
        let astral = "\u{1F600}"; // U+1F600 -> ED A0 BD ED B8 80 (6 bytes)
        let title = nbt_compound(vec![("text", nbt_str(astral))]);
        let desc = nbt_compound(vec![("text", nbt_str("\0"))]);
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(
                &title,
                &desc,
                &[custom_name(astral)],
                &[],
                &[0],
            )),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        let canon = canon_update_advancements(&input).expect("astral/NUL title canonicalizes");

        // The canonical form must decode back to the same strings.
        let mut off = 0;
        let reset = *canon.get(off).unwrap();
        off += 1;
        assert_eq!(reset, 0);
        let added_count = frame::read_varint(&canon, &mut off).unwrap();
        assert_eq!(added_count, 1);
        let _id = read_string(&canon, &mut off).unwrap();
        off += 1; // parent absent
        assert_eq!(*canon.get(off).unwrap(), 1);
        off += 1; // display present
        let display = canon_display_info(&canon, &mut off).unwrap();

        // Re-parse the canonical display's two NBT components.
        let mut d_off = 0;
        let title_tag = read_nbt(&display, &mut d_off).unwrap();
        let desc_tag = read_nbt(&display, &mut d_off).unwrap();
        match (title_tag, desc_tag) {
            (Nbt::Compound(t), Nbt::Compound(d)) => {
                let t_text = t
                    .iter()
                    .find(|(n, _)| n == "text")
                    .expect("title has text field");
                let d_text = d
                    .iter()
                    .find(|(n, _)| n == "text")
                    .expect("desc has text field");
                match (&t_text.1, &d_text.1) {
                    (Nbt::String(ts), Nbt::String(ds)) => {
                        assert_eq!(ts, astral);
                        assert_eq!(ds, "\0");
                    }
                    other => panic!("expected two strings, got {other:?}"),
                }
            }
            other => panic!("expected two compounds, got {other:?}"),
        }
        // Idempotence: re-canonicalizing the canonical form is stable.
        assert_eq!(canon_update_advancements(&canon), Some(canon.clone()));
    }

    #[test]
    fn profile_either_left_branch_is_selected_by_nonzero_byte() {
        // ResolvableProfile (id 70) = ByteBufCodecs.either(GAME_PROFILE, Partial)
        // + PlayerSkin.Patch. Java's `either` decode is `readBoolean() ? left :
        // right`, i.e. a NONZERO byte selects the full GameProfile (left) and a
        // zero byte selects the Partial (right). A regression where the branch
        // test was inverted would mis-skip the value (reading 8 bytes instead of
        // 23 here) and misalign the rest of the display, so the full value must
        // round-trip byte-exact.
        let mut profile = Vec::new();
        profile.push(1u8); // either flag: nonzero -> GameProfile (left)
        profile.extend_from_slice(&[0u8; 16]); // UUID (all zero)
        profile.push(0); // name: empty
        profile.push(0); // properties: count 0
        profile.extend_from_slice(&[0u8; 4]); // PlayerSkin.Patch: four absent optionals
        assert_eq!(profile.len(), 23);

        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let entry = (70u32, profile.clone());
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[entry], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        let canon = canon_update_advancements(&input).expect("profile value canonicalizes");
        assert!(
            canon.windows(profile.len()).any(|w| w == profile),
            "profile value must be copied byte-exact"
        );
        assert_eq!(canon_update_advancements(&canon), Some(canon.clone()));
    }

    #[test]
    fn charged_and_bundle_projectile_contents_are_not_optional() {
        // ChargedProjectiles (id 49) and BundleContents (id 50) are
        // `ItemStackTemplate.STREAM_CODEC.apply(ByteBufCodecs.list(...))` — plain
        // ItemStackTemplate lists with NO Optional wrapper (only
        // ItemContainerContents 75 wraps its elements in an Optional). A
        // regression that skipped each element as `?ItemStackTemplate` would read
        // the second element's item id as an absent-present bool and truncate the
        // value, so a two-element value must round-trip byte-exact.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        for id in [49u32, 50] {
            let mut value = Vec::new();
            frame::write_varint(&mut value, 2); // list count
            for _ in 0..2 {
                frame::write_varint(&mut value, 0); // item
                frame::write_varint(&mut value, 1); // count
                frame::write_varint(&mut value, 0); // patch positive
                frame::write_varint(&mut value, 0); // patch negative
            }
            let entry = (id, value.clone());
            let adv = advancement(
                "story:root",
                None,
                Some(&display_for(&title, &title, &[entry], &[], &[0])),
                &[vec!["c".into()]],
                false,
            );
            let input = body(false, &[adv], &[], &[], true);
            let canon = canon_update_advancements(&input)
                .expect("plain ItemStackTemplate list canonicalizes");
            assert_eq!(
                canon, input,
                "{id} two-element list must be copied byte-exact (elements not Optional)"
            );
        }
    }

    #[test]
    fn data_component_predicate_reads_value_tag_for_both_branches() {
        // can_place_on (id 14) = list(BlockPredicate); each BlockPredicate carries
        // DataComponentMatchers = [exact list][partial list] where partial is a
        // list(Single) of DataComponentPredicate. Each Single = [Type either][value]
        // with the Type either reading [bool][registry varint], and the value for
        // BOTH branches is an NBT tag: a concrete predicate's
        // fromCodecWithRegistries tag, or an AnyValueType's empty-compound unit
        // (`unitCodec` maps empty() to an empty compound — never zero bytes). A
        // regression that read the value tag for only one branch would mis-skip
        // the other and truncate the can_place_on value.
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let mut value = Vec::new();
        frame::write_varint(&mut value, 1); // list(BlockPredicate) count
        value.push(0); // blocks optional: absent
        value.push(0); // properties optional: absent
        value.push(0); // nbt optional: absent
        value.push(0); // DataComponentExactPredicate: empty list
        // DataComponentPredicate: list(64)(Single).
        frame::write_varint(&mut value, 2); // two Singles
        // AnyValue branch (either flag 0 -> DATA_COMPONENT_TYPE): the value is an
        // empty-compound unit, still an NBT tag on the wire.
        value.push(0); // either flag: zero -> AnyValue (component type)
        frame::write_varint(&mut value, 6); // custom_name DATA_COMPONENT_TYPE id
        value.push(10); // value tag: empty compound
        value.push(0);
        // Concrete branch (either flag nonzero -> DATA_COMPONENT_PREDICATE_TYPE).
        value.push(1); // either flag: nonzero -> concrete predicate type
        frame::write_varint(&mut value, 0); // predicate-type registry id
        value.push(10); // value tag: empty compound
        value.push(0);
        let entry = (14u32, value.clone());
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[entry], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        let canon = canon_update_advancements(&input).expect("predicate value canonicalizes");
        assert_eq!(
            canon, input,
            "data-component predicate with concrete and AnyValue singles must be copied byte-exact"
        );
    }

    /// The NBT-shaped component set exercised inside nested patches: custom_name
    /// (6), lore (11), entity_data (58), plus a non-NBT item_model (10) that must
    /// still be bounded via its exact wire shape. Entries are fed in a non-sorted
    /// order so a naive id-ordered walk would mis-align.
    fn nested_component_patch() -> Vec<u8> {
        let zombie = nbt_compound(vec![("id", nbt_str("zombie"))]);
        let model = (10u32, vec![0x03, 0x61, 0x62, 0x63]); // [len 3]["abc"]
        patch(
            &[
                custom_name("x"),
                lore(&["L1"]),
                entity_data(7, &zombie),
                model,
            ],
            &[],
        )
    }

    /// Run `input` through the display canonicalizer and assert the display value
    /// is copied byte-exact: a nested ItemStackTemplate's DataComponentPatch is
    /// NOT re-sorted (only the icon patch at the display top level is), so a
    /// valid composite whose patch carries NBT-shaped components must pass
    /// through unchanged rather than failing the whole canonicalization.
    fn assert_composite_value_passes_byte_exact(entry: (u32, Vec<u8>)) {
        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[entry], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        let canon = canon_update_advancements(&input)
            .expect("composite with nested NBT-shaped components canonicalizes");
        assert_eq!(
            canon, input,
            "composite value with nested NBT-shaped components must be copied byte-exact"
        );
    }

    #[test]
    fn nested_patch_dispatches_nbt_shaped_components_in_composite_paths() {
        // Every ItemStackTemplate-holding component embeds DataComponentPatch
        // values, and a nested patch entry whose component is NBT-shaped
        // (custom_name 6 / lore 11 / entity_data 58) is not expressible as a
        // Shape and must be dispatched to the shared NBT reader. A regression
        // would fail the whole display canonicalization (None) and keep the raw
        // body. The three representative paths differ in how the elements are
        // wrapped:
        //   - container (75)        list(256)(?ItemStackTemplate)  — Optional
        //   - use_remainder (25)    single ItemStackTemplate        — no list
        //   - bundle_contents (50)  list(256)(ItemStackTemplate)    — plain
        let patch = nested_component_patch();

        // Container: [count][?ItemStackTemplate: present][item][count][patch].
        let mut container = Vec::new();
        frame::write_varint(&mut container, 1); // list count
        container.push(1); // Optional present
        frame::write_varint(&mut container, 0); // item
        frame::write_varint(&mut container, 1); // count
        container.extend_from_slice(&patch);
        assert_composite_value_passes_byte_exact((75u32, container));

        // UseRemainder: [item][count][patch] — a single ItemStackTemplate.
        let mut use_remainder = Vec::new();
        frame::write_varint(&mut use_remainder, 0); // item
        frame::write_varint(&mut use_remainder, 1); // count
        use_remainder.extend_from_slice(&patch);
        assert_composite_value_passes_byte_exact((25u32, use_remainder));

        // BundleContents: [count][item][count][patch] — plain, NOT Optional.
        let mut bundle = Vec::new();
        frame::write_varint(&mut bundle, 1); // list count
        frame::write_varint(&mut bundle, 0); // item
        frame::write_varint(&mut bundle, 1); // count
        bundle.extend_from_slice(&patch);
        assert_composite_value_passes_byte_exact((50u32, bundle));
    }

    #[test]
    fn attack_range_value_is_six_floats() {
        // AttackRange.STREAM_CODEC (id 30) = six ByteBufCodecs.FLOAT (minReach,
        // maxReach, minCreativeReach, maxCreativeReach, hitboxMargin, mobFactor).
        // A regression that consumed only two floats would leave the trailing
        // four misaligned and corrupt the display; the full 24-byte value must
        // round-trip byte-exact.
        let mut value = Vec::new();
        for v in [1.0f32, 4.5, 0.0, 3.0, 0.25, 1.0] {
            value.extend_from_slice(&v.to_be_bytes());
        }
        assert_eq!(value.len(), 24);

        let title = nbt_compound(vec![("text", nbt_str("T"))]);
        let entry = (30u32, value.clone());
        let adv = advancement(
            "story:root",
            None,
            Some(&display_for(&title, &title, &[entry], &[], &[0])),
            &[vec!["c".into()]],
            false,
        );
        let input = body(false, &[adv], &[], &[], true);
        let canon = canon_update_advancements(&input).expect("attack_range value canonicalizes");
        assert!(
            canon.windows(value.len()).any(|w| w == value),
            "attack_range value must be copied byte-exact"
        );
        assert_eq!(canon_update_advancements(&canon), Some(canon.clone()));
    }
}
