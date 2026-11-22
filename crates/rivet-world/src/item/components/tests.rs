//! Focused Paper-faithful unit tests for leaf item component value families
//! and codecs (JSON & NBT round-trips, exact defaults/ordering, math semantics,
//! and controlled negative/unknown cases).

use super::*;
use crate::effect::{MobEffectId, MobEffectInstance};
use crate::enchantment::{EquipmentSlot, EquipmentSlotGroup};
use crate::item::component_type::DataComponentTypeId;
use crate::item::item_id::ItemId;
use crate::item::recursive_components::ItemStackTemplate;
use indexmap::{IndexMap, IndexSet};
use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::core::BlockPos;
use rivet_registry::core::GlobalPos;
use rivet_registry::registries::{DIMENSION, Level};
use rivet_registry::{Identifier, ResourceKey};
use rivet_serialization::{Codec, JsonOps};
use rivet_text::component::Component;
use serde_json::json;
use std::sync::Arc;

fn id(path: &str) -> Identifier {
    Identifier::with_default_namespace(path)
}

fn round_trip_json<T: Clone + PartialEq + std::fmt::Debug>(
    codec: &Arc<dyn Codec<T, JsonOps>>,
    value: &T,
) -> serde_json::Value {
    let encoded = codec
        .encode_start(&JsonOps::INSTANCE, value)
        .result()
        .cloned()
        .expect("json encode should succeed");
    let decoded = codec
        .parse(&JsonOps::INSTANCE, &encoded)
        .result()
        .cloned()
        .expect("json decode should succeed");
    assert_eq!(&decoded, value, "JSON round-trip mismatch");
    encoded
}

fn round_trip_nbt<T: Clone + PartialEq + std::fmt::Debug>(
    codec: &Arc<dyn Codec<T, NbtOps>>,
    value: &T,
) -> Tag {
    let encoded = codec
        .encode_start(&NbtOps::instance(), value)
        .result()
        .cloned()
        .expect("nbt encode should succeed");
    let decoded = codec
        .parse(&NbtOps::instance(), &encoded)
        .result()
        .cloned()
        .expect("nbt decode should succeed");
    assert_eq!(&decoded, value, "NBT round-trip mismatch");
    encoded
}

// ---------------------------------------------------------------------------
// Core: CustomData, TypedEntityData, Filterable, HolderSetRef
// ---------------------------------------------------------------------------

#[test]
fn custom_data_immutability_matching_and_round_trip() {
    let mut tag = CompoundTag::new();
    tag.put_string("owner", "steve");
    tag.put_int("level", 70_000);

    let cd = CustomData::of(&tag);
    // Mutating original tag must not affect CustomData.
    tag.put_int("level", 99);
    assert_eq!(cd.tag().get_int("level"), Some(70_000));

    // update() returns a new copy without mutating the original.
    let updated = cd.update(|t| {
        t.put_boolean("shiny", true);
    });
    assert!(!cd.tag().contains("shiny"));
    assert_eq!(updated.tag().get_boolean("shiny"), Some(true));

    // matched_by uses subset semantics (NbtUtils.compareNbt).
    let mut expected = CompoundTag::new();
    expected.put_string("owner", "steve");
    assert!(updated.matched_by(&expected));
    expected.put_string("owner", "alex");
    assert!(!updated.matched_by(&expected));

    round_trip_json(&custom_data_codec(), &updated);
    round_trip_nbt(&custom_data_codec(), &updated);
}

#[test]
fn typed_entity_data_strips_and_restores_id_and_rejects_missing_id() {
    let id_codec_json = crate::item::codec_from_fns::<Identifier, JsonOps, _, _>(
        "id",
        |v, ops| rivet_serialization::DataResult::success(ops.create_string(v.to_string())),
        crate::item::components::decode_identifier,
    );
    let id_codec_nbt = crate::item::codec_from_fns::<Identifier, NbtOps, _, _>(
        "id",
        |v, ops| rivet_serialization::DataResult::success(ops.create_string(v.to_string())),
        crate::item::components::decode_identifier,
    );

    let mut raw = CompoundTag::new();
    raw.put_string("id", "minecraft:creeper");
    raw.put_int("Fuse", 70_000);

    let ted = TypedEntityData::of(id("pig"), &raw);
    assert!(!ted.contains("id"), "constructor must strip 'id' from tag");
    assert!(ted.contains("Fuse"));

    let json_val = round_trip_json(&typed_entity_data_codec(id_codec_json.clone()), &ted);
    assert_eq!(json_val["id"], json!("minecraft:pig"));
    assert_eq!(json_val["Fuse"], json!(70_000));
    round_trip_nbt(&typed_entity_data_codec(id_codec_nbt), &ted);

    // Controlled negative: missing "id" field fails decode.
    let bad = json!({"Fuse": 30});
    assert!(
        typed_entity_data_codec(id_codec_json)
            .parse(&JsonOps::INSTANCE, &bad)
            .is_error()
    );
}

#[test]
fn filterable_decodes_bare_and_full_forms() {
    let str_codec = crate::item::codec_from_fns::<String, JsonOps, _, _>(
        "str",
        |v, ops| rivet_serialization::DataResult::success(ops.create_string(v.clone())),
        |ops, input| ops.get_string_value(input),
    );
    let fc = filterable_codec(str_codec);

    // Bare string decodes as passThrough.
    let bare = fc
        .parse(&JsonOps::INSTANCE, &json!("hello"))
        .result()
        .cloned()
        .expect("bare string should decode");
    assert_eq!(bare, Filterable::pass_through("hello".to_string()));
    assert_eq!(bare.get(false), "hello");
    assert_eq!(bare.get(true), "hello");

    // Full object with filtered variant.
    let full = Filterable::new("badword".to_string(), Some("#######".to_string()));
    assert_eq!(full.get(false), "badword");
    assert_eq!(full.get(true), "#######");
    round_trip_json(&fc, &full);
}

#[test]
fn holder_set_ref_tag_single_and_list_forms() {
    let jc = holder_set_ref_codec::<JsonOps>();
    let nc = holder_set_ref_codec::<NbtOps>();

    let tag = HolderSetRef::tag(id("is_fire"));
    assert_eq!(round_trip_json(&jc, &tag), json!("#minecraft:is_fire"));
    round_trip_nbt(&nc, &tag);

    // Single string decodes as 1-element Direct list.
    let single = jc
        .parse(&JsonOps::INSTANCE, &json!("minecraft:stone"))
        .result()
        .cloned()
        .expect("single id should decode");
    assert_eq!(single, HolderSetRef::single(id("stone")));

    let list = HolderSetRef::direct(vec![id("stone"), id("dirt")]);
    round_trip_json(&jc, &list);
    round_trip_nbt(&nc, &list);

    // Controlled negative: invalid identifier in tag.
    assert!(
        jc.parse(&JsonOps::INSTANCE, &json!("#INVALID!TAG"))
            .is_error()
    );
}

// ---------------------------------------------------------------------------
// Display: TooltipDisplay, ItemLore, CustomModelData, DyedItemColor, Map*
// ---------------------------------------------------------------------------

#[test]
fn tooltip_display_preserves_insertion_order_and_defaults() {
    let jc = tooltip_display_codec::<JsonOps>();
    let nc = tooltip_display_codec::<NbtOps>();

    let default_val: TooltipDisplay = jc
        .parse(&JsonOps::INSTANCE, &json!({}))
        .result()
        .cloned()
        .expect("empty map decodes to default");
    assert_eq!(default_val, TooltipDisplay::default());
    assert!(default_val.shows(DataComponentTypeId::LORE));

    // Insertion order preserved: UNBREAKABLE then ENCHANTMENTS then LORE.
    let td = TooltipDisplay::default()
        .with_hidden(DataComponentTypeId::UNBREAKABLE, true)
        .with_hidden(DataComponentTypeId::ENCHANTMENTS, true)
        .with_hidden(DataComponentTypeId::LORE, true);
    assert!(!td.shows(DataComponentTypeId::UNBREAKABLE));
    assert!(td.shows(DataComponentTypeId::CUSTOM_NAME));

    let encoded = round_trip_json(&jc, &td);
    assert_eq!(
        encoded["hidden_components"],
        json!([
            "minecraft:unbreakable",
            "minecraft:enchantments",
            "minecraft:lore"
        ])
    );
    round_trip_nbt(&nc, &td);

    // Removing preserves order of remaining entries.
    let td2 = td.with_hidden(DataComponentTypeId::ENCHANTMENTS, false);
    let order: Vec<_> = td2.hidden_components.iter().copied().collect();
    assert_eq!(
        order,
        vec![DataComponentTypeId::UNBREAKABLE, DataComponentTypeId::LORE]
    );

    // hide_tooltip=true hides everything.
    let all_hidden = TooltipDisplay::new(true, IndexSet::new());
    assert!(!all_hidden.shows(DataComponentTypeId::CUSTOM_NAME));

    // Controlled negative: unknown component type id fails.
    assert!(
        jc.parse(
            &JsonOps::INSTANCE,
            &json!({"hidden_components": ["minecraft:does_not_exist"]})
        )
        .is_error()
    );
}

#[test]
fn item_lore_round_trip_and_256_line_limit() {
    let lore = ItemLore::empty()
        .with_line_added(Component::literal("First line"))
        .unwrap()
        .with_line_added(Component::literal("Second line"))
        .unwrap();
    round_trip_json(&item_lore_codec(), &lore);
    round_trip_nbt(&item_lore_codec(), &lore);

    // Exactly 256 lines allowed; 257 rejected.
    let max_lines: Vec<Component> = (0..LORE_MAX_LINES)
        .map(|i| Component::literal(&format!("line {i}")))
        .collect();
    let full = ItemLore::try_new(max_lines).expect("256 lines must succeed");
    assert!(
        full.with_line_added(Component::literal("overflow"))
            .is_err()
    );

    let too_many = serde_json::Value::Array(vec![json!("x"); LORE_MAX_LINES + 1]);
    assert!(
        item_lore_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &too_many)
            .is_error()
    );
}

#[test]
fn custom_model_data_indexed_bounds_and_rgb_float_decode() {
    let cmd = CustomModelData::new(
        vec![1.5, -2.0],
        vec![true, false],
        vec!["variant_a".to_string()],
        vec![0xFF112233_u32 as i32],
    );
    assert_eq!(cmd.get_float(-1), None);
    assert_eq!(cmd.get_float(0), Some(1.5));
    assert_eq!(cmd.get_float(2), None);
    assert_eq!(cmd.get_flag(1), Some(false));
    assert_eq!(cmd.get_string(0), Some("variant_a"));
    assert_eq!(cmd.get_color(0), Some(0xFF112233_u32 as i32));

    round_trip_json(&custom_model_data_codec(), &cmd);
    round_trip_nbt(&custom_model_data_codec(), &cmd);

    // Float RGB triple in colors list decodes via ARGB.colorFromFloat(1.0F, r, g, b).
    let parsed = custom_model_data_codec::<JsonOps>()
        .parse(&JsonOps::INSTANCE, &json!({"colors": [[1.0, 0.0, 0.5]]}))
        .result()
        .cloned()
        .expect("float RGB triple should decode");
    assert_eq!(parsed.get_color(0), Some(0xFFFF_007F_u32 as i32));
}

#[test]
fn dyed_item_color_blending_and_codec() {
    assert_eq!(LEATHER_COLOR, -6265536);
    assert_eq!(
        DyedItemColor::new(0x0011_2233).opaque_rgb(),
        0xFF11_2233_u32 as i32
    );

    // Single white dye (0xF9FFFE) on undyed armor preserves exact channel values.
    let dyed = DyedItemColor::apply_dyes(None, &[0xFFF9_FFFE_u32 as i32]).unwrap();
    assert_eq!(dyed.rgb, 0x00F9_FFFE);

    // Applying red (0xB02E26) then blue (0x3C44AA) matches Paper's integer math.
    let blended =
        DyedItemColor::apply_dyes(None, &[0xFFB0_2E26_u32 as i32, 0xFF3C_44AA_u32 as i32]).unwrap();
    // r_total = 176 + 60 = 236 -> 118; g_total = 46 + 68 = 114 -> 57; b_total = 38 + 170 = 208 -> 104
    // intensity_total = 176 + 170 = 346 -> avg = 173.0; result_intensity = 118.0
    // red = (118 * 173.0 / 118.0) as i32 = 173 (0xAD)
    // green = (57 * 173.0 / 118.0) as i32 = 83 (0x53)
    // blue = (104 * 173.0 / 118.0) as i32 = 152 (0x98)
    assert_eq!(blended.rgb, (173 << 16) | (83 << 8) | 152);

    // Empty dye list with no current color returns None.
    assert_eq!(DyedItemColor::apply_dyes(None, &[]), None);

    round_trip_json(&dyed_item_color_codec(), &blended);
    round_trip_nbt(&dyed_item_color_codec(), &blended);
}

#[test]
fn map_components_round_trip_and_order() {
    assert_eq!(MapItemColor::DEFAULT.rgb, 4_603_950);
    round_trip_json(&map_item_color_codec(), &MapItemColor::new(0x123456));
    round_trip_nbt(&map_item_color_codec(), &MapItemColor::new(0x123456));

    assert_eq!(MapPostProcessing::by_id(99), MapPostProcessing::Lock);
    assert_eq!(MapPostProcessing::by_id(1), MapPostProcessing::Scale);
    round_trip_json(&map_post_processing_codec(), &MapPostProcessing::Scale);
    round_trip_nbt(&map_post_processing_codec(), &MapPostProcessing::Lock);

    let decs = MapDecorations::empty()
        .with_decoration(
            "home".to_string(),
            MapDecorationEntry::new(id("red_x"), 10.5, -20.25, 180.0),
        )
        .with_decoration(
            "temple".to_string(),
            MapDecorationEntry::new(id("mansion"), 500.0, 250.0, 90.0),
        );
    round_trip_json(&map_decorations_codec(), &decs);
    round_trip_nbt(&map_decorations_codec(), &decs);
}

// ---------------------------------------------------------------------------
// Combat: Weapon, AttackRange, SwingAnimation, DamageResistant, BlocksAttacks,
//         PiercingWeapon, KineticWeapon, Tool, Enchantable, Repairable
// ---------------------------------------------------------------------------

#[test]
fn combat_weapons_and_ranges_defaults_and_negatives() {
    // Weapon
    let w = Weapon::try_new(2, AXE_DISABLES_BLOCKING_FOR_SECONDS).unwrap();
    round_trip_json(&weapon_codec(), &w);
    round_trip_nbt(&weapon_codec(), &w);
    assert!(
        weapon_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!({"item_damage_per_attack": -1}))
            .is_error()
    );

    // AttackRange
    let ar: AttackRange = attack_range_codec::<JsonOps>()
        .parse(&JsonOps::INSTANCE, &json!({}))
        .result()
        .copied()
        .unwrap();
    assert_eq!(ar, AttackRange::DEFAULT);
    assert_eq!(ar.effective_max_range(false), 3.0);
    assert_eq!(ar.effective_max_range(true), 5.0);
    assert!(
        attack_range_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!({"hitbox_margin": 1.5}))
            .is_error()
    );

    // SwingAnimation
    let sa: SwingAnimation = swing_animation_codec::<JsonOps>()
        .parse(&JsonOps::INSTANCE, &json!({}))
        .result()
        .copied()
        .unwrap();
    assert_eq!(sa, SwingAnimation::DEFAULT);
    round_trip_json(
        &swing_animation_codec(),
        &SwingAnimation::try_new(SwingAnimationType::Stab, 12).unwrap(),
    );
    assert!(
        swing_animation_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!({"duration": 0}))
            .is_error()
    );

    // DamageResistant
    let dr = DamageResistant::new(HolderSetRef::tag(id("is_explosion")));
    round_trip_json(&damage_resistant_codec(), &dr);
    round_trip_nbt(&damage_resistant_codec(), &dr);
}

#[test]
fn blocks_attacks_damage_math_and_round_trip() {
    let default_ba: BlocksAttacks = blocks_attacks_codec::<JsonOps>()
        .parse(&JsonOps::INSTANCE, &json!({}))
        .result()
        .cloned()
        .unwrap();
    assert_eq!(default_ba, BlocksAttacks::default());
    assert_eq!(default_ba.block_delay_ticks(), 0);

    // ItemDamageFunction.apply: below threshold -> 0; at/above -> floor(base + factor * dealt)
    let f = ItemDamageFunction::try_new(3.0, 1.0, 0.5).unwrap();
    assert_eq!(f.apply(2.9), 0);
    assert_eq!(f.apply(3.0), 2); // floor(1.0 + 1.5) = 2
    assert_eq!(f.apply(5.0), 3); // floor(1.0 + 2.5) = 3

    // DamageReduction.resolve
    let r = DamageReduction::default_reduction();
    assert_eq!(r.resolve(true, 10.0, 0.0), 10.0);
    assert_eq!(r.resolve(true, 10.0, std::f64::consts::PI), 0.0);
    assert_eq!(r.resolve(false, 10.0, 0.0), 0.0);

    let custom = BlocksAttacks {
        block_delay_seconds: 0.25,
        disable_cooldown_scale: 0.5,
        damage_reductions: vec![
            DamageReduction::try_new(
                120.0,
                Some(HolderSetRef::tag(id("is_projectile"))),
                2.0,
                0.5,
            )
            .unwrap(),
        ],
        item_damage: f,
        bypassed_by: Some(id("bypasses_shield")),
        block_sound: Some(id("item.shield.block")),
        disabled_sound: Some(id("item.shield.break")),
    };
    assert_eq!(custom.block_delay_ticks(), 5);
    round_trip_json(&blocks_attacks_codec(), &custom);
    round_trip_nbt(&blocks_attacks_codec(), &custom);
}

#[test]
fn piercing_kinetic_tool_enchantable_and_repairable() {
    let pw = PiercingWeapon {
        deals_knockback: false,
        dismounts: true,
        sound: Some(id("item.trident.throw")),
        hit_sound: None,
    };
    round_trip_json(&piercing_weapon_codec(), &pw);
    round_trip_nbt(&piercing_weapon_codec(), &pw);

    let cond = KineticWeaponCondition::try_new(20, 0.5, 0.25).unwrap();
    assert!(cond.test(20, 0.5, 0.25, 1.0));
    assert!(!cond.test(21, 0.5, 0.25, 1.0));
    assert!(!cond.test(10, 0.4, 0.25, 1.0));

    let kw = KineticWeapon {
        contact_cooldown_ticks: 5,
        delay_ticks: 2,
        dismount_conditions: Some(cond),
        knockback_conditions: None,
        damage_conditions: Some(cond),
        forward_movement: 0.5,
        damage_multiplier: 1.5,
        sound: Some(id("item.mace.smash_air")),
        hit_sound: None,
    };
    round_trip_json(&kinetic_weapon_codec(), &kw);
    round_trip_nbt(&kinetic_weapon_codec(), &kw);

    // Tool: first matching rule wins.
    let tool = Tool::new(
        vec![
            ToolRule::mines_and_drops(HolderSetRef::single(id("cobweb")), 15.0),
            ToolRule::override_speed(HolderSetRef::single(id("cobweb")), 2.0),
            ToolRule::denies_drops(HolderSetRef::single(id("obsidian"))),
        ],
        1.0,
        2,
        false,
    );
    assert_eq!(tool.mining_speed_for_direct(&id("cobweb")), 15.0);
    assert_eq!(tool.mining_speed_for_direct(&id("dirt")), 1.0);
    assert!(tool.is_correct_for_drops_direct(&id("cobweb")));
    assert!(!tool.is_correct_for_drops_direct(&id("obsidian")));
    round_trip_json(&tool_codec(), &tool);
    round_trip_nbt(&tool_codec(), &tool);

    // Enchantable & Repairable
    let ench = Enchantable::try_new(15).unwrap();
    round_trip_json(&enchantable_codec(), &ench);
    round_trip_nbt(&enchantable_codec(), &ench);
    assert!(
        enchantable_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!({"value": 0}))
            .is_error()
    );

    let rep = Repairable::new(HolderSetRef::tag(id("planks")));
    round_trip_json(&repairable_codec(), &rep);
    round_trip_nbt(&repairable_codec(), &rep);
}

// ---------------------------------------------------------------------------
// Consumable: FoodProperties, Consumable, ConsumeEffect, DeathProtection,
//             SuspiciousStewEffects, UseCooldown, UseEffects, UseRemainder,
//             OminousBottleAmplifier
// ---------------------------------------------------------------------------

#[test]
fn consumable_timing_effects_and_round_trips() {
    let food = FoodProperties::try_new(4, 2.4, true).unwrap();
    round_trip_json(&food_properties_codec(), &food);
    round_trip_nbt(&food_properties_codec(), &food);

    let speed = MobEffectInstance::full(MobEffectId::Speed, 200, 1, false, true, true, None);
    let cons = Consumable {
        consume_seconds: DEFAULT_CONSUME_SECONDS,
        animation: ItemUseAnimation::Drink,
        sound: id("entity.generic.drink"),
        has_consume_particles: false,
        on_consume_effects: vec![
            ConsumeEffect::ApplyEffects(
                ApplyStatusEffectsConsumeEffect::try_new(vec![speed], 0.5).unwrap(),
            ),
            ConsumeEffect::RemoveEffects(RemoveStatusEffectsConsumeEffect {
                effects: HolderSetRef::single(id("poison")),
            }),
            ConsumeEffect::ClearAllEffects(ClearAllStatusEffectsConsumeEffect),
            ConsumeEffect::TeleportRandomly(TeleportRandomlyConsumeEffect::try_new(8.0).unwrap()),
            ConsumeEffect::PlaySound(PlaySoundConsumeEffect {
                sound: id("entity.enderman.teleport"),
            }),
        ],
    };
    assert_eq!(cons.consume_ticks(), 32);
    // wait_ticks = (32 * 0.21875) as i32 = 7.
    // remaining = 24 -> used = 8 > 7 and 24 % 4 == 0 -> true.
    // remaining = 28 -> used = 4 <= 7 -> false.
    assert!(cons.should_emit_particles_and_sounds(24));
    assert!(!cons.should_emit_particles_and_sounds(25));
    assert!(!cons.should_emit_particles_and_sounds(28));

    round_trip_json(&consumable_codec(), &cons);
    round_trip_nbt(&consumable_codec(), &cons);

    // Controlled negative: unknown consume effect type.
    assert!(
        consume_effect_codec::<JsonOps>()
            .parse(
                &JsonOps::INSTANCE,
                &json!({"type": "minecraft:unknown_effect"})
            )
            .is_error()
    );

    // DeathProtection
    let dp = DeathProtection::new(vec![ConsumeEffect::ClearAllEffects(
        ClearAllStatusEffectsConsumeEffect,
    )]);
    round_trip_json(&death_protection_codec(), &dp);
    round_trip_nbt(&death_protection_codec(), &dp);

    // SuspiciousStewEffects
    let stew = SuspiciousStewEffects::empty()
        .with_effect_added(SuspiciousStewEntry::new(
            id("night_vision"),
            DEFAULT_STEW_DURATION,
        ))
        .with_effect_added(SuspiciousStewEntry::new(id("jump_boost"), 100));
    round_trip_json(&suspicious_stew_effects_codec(), &stew);
    round_trip_nbt(&suspicious_stew_effects_codec(), &stew);

    // UseCooldown, UseEffects, UseRemainder, OminousBottleAmplifier
    let cd = UseCooldown::try_new(1.5, Some(id("ender_pearl"))).unwrap();
    assert_eq!(cd.ticks(), 30);
    round_trip_json(&use_cooldown_codec(), &cd);
    round_trip_nbt(&use_cooldown_codec(), &cd);
    assert!(
        use_cooldown_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!({"seconds": 0.0}))
            .is_error()
    );

    let ue = UseEffects::try_new(true, false, 0.75).unwrap();
    round_trip_json(&use_effects_codec(), &ue);
    round_trip_nbt(&use_effects_codec(), &ue);

    let rem = UseRemainder::new(
        ItemStackTemplate::of_item(ItemId::from_identifier(&id("bowl")).unwrap()).unwrap(),
    );
    round_trip_json(&use_remainder_codec(), &rem);
    round_trip_nbt(&use_remainder_codec(), &rem);

    let amp = OminousBottleAmplifier::try_new(4).unwrap();
    round_trip_json(&ominous_bottle_amplifier_codec(), &amp);
    round_trip_nbt(&ominous_bottle_amplifier_codec(), &amp);
    assert!(
        ominous_bottle_amplifier_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!(5))
            .is_error()
    );
}

// ---------------------------------------------------------------------------
// World & Books: LodestoneTracker, SeededContainerLoot,
//                BlockItemStateProperties, WritableBookContent,
//                WrittenBookContent, Fireworks, ItemAttributeModifiers,
//                SulfurCubeContent, InstrumentComponent, ProvidesTrimMaterial
// ---------------------------------------------------------------------------

#[test]
fn lodestone_loot_and_block_state_properties() {
    let gp = GlobalPos::of(
        ResourceKey::<Level>::create(&DIMENSION, id("overworld")),
        BlockPos::new(100, 64, -200),
    );
    let lt = LodestoneTracker::new(Some(gp), false);
    round_trip_json(&lodestone_tracker_codec(), &lt);
    round_trip_nbt(&lodestone_tracker_codec(), &lt);

    let loot = SeededContainerLoot::new(id("chests/simple_dungeon"), 123456789);
    round_trip_json(&seeded_container_loot_codec(), &loot);
    round_trip_nbt(&seeded_container_loot_codec(), &loot);

    let mut map = IndexMap::new();
    map.insert("facing".to_string(), "north".to_string());
    let props = BlockItemStateProperties::new(map).with_property("lit", "true");
    assert_eq!(props.get("facing"), Some("north"));
    assert_eq!(props.get("lit"), Some("true"));
    round_trip_json(&block_item_state_properties_codec(), &props);
    round_trip_nbt(&block_item_state_properties_codec(), &props);
}

#[test]
fn books_limits_generation_and_round_trip() {
    let writable = WritableBookContent::try_new(vec![
        Filterable::pass_through("Page 1".to_string()),
        Filterable::new("Raw 2".to_string(), Some("Clean 2".to_string())),
    ])
    .unwrap();
    assert_eq!(writable.get_pages(false), vec!["Page 1", "Raw 2"]);
    assert_eq!(writable.get_pages(true), vec!["Page 1", "Clean 2"]);
    round_trip_json(&writable_book_content_codec(), &writable);
    round_trip_nbt(&writable_book_content_codec(), &writable);

    // Controlled negative: > 100 pages rejected.
    let too_many_pages =
        vec![Filterable::pass_through("p".to_string()); WRITABLE_BOOK_MAX_PAGES + 1];
    assert!(WritableBookContent::try_new(too_many_pages).is_err());

    // Controlled negative: page > 1024 UTF-16 code units rejected.
    let long_page = "a".repeat(BOOK_PAGE_EDIT_LENGTH + 1);
    assert!(WritableBookContent::try_new(vec![Filterable::pass_through(long_page)]).is_err());

    let written = WrittenBookContent::try_new(
        Filterable::pass_through("My Book".to_string()),
        "Steve".to_string(),
        0,
        vec![Filterable::pass_through(Component::literal("Chapter 1"))],
        true,
    )
    .unwrap();
    let copy1 = written.try_craft_copy().expect("gen 0 -> 1");
    assert_eq!(copy1.generation, 1);
    let copy2 = copy1.try_craft_copy().expect("gen 1 -> 2");
    assert_eq!(copy2.generation, 2);
    assert!(
        copy2.try_craft_copy().is_none(),
        "gen >= 2 cannot be copied"
    );

    round_trip_json(&written_book_content_codec(), &written);
    round_trip_nbt(&written_book_content_codec(), &written);

    // Controlled negative: generation out of 0..=3 or title > 32 chars.
    assert!(
        written_book_content_codec::<JsonOps>()
            .parse(
                &JsonOps::INSTANCE,
                &json!({"title": "ok", "author": "a", "generation": 4})
            )
            .is_error()
    );
    assert!(
        written_book_content_codec::<JsonOps>()
            .parse(
                &JsonOps::INSTANCE,
                &json!({"title": "x".repeat(33), "author": "a"})
            )
            .is_error()
    );
}

#[test]
fn fireworks_unsigned_byte_and_limits() {
    let exp = FireworkExplosion {
        shape: FireworkShape::Creeper,
        colors: vec![0xFF0000, 0x00FF00],
        fade_colors: vec![0x0000FF],
        has_trail: true,
        has_twinkle: false,
    };
    let fw = Fireworks::try_new(200, vec![exp]).unwrap();
    round_trip_json(&fireworks_codec(), &fw);
    round_trip_nbt(&fireworks_codec(), &fw);

    // Controlled negative: flight_duration > 255 rejected.
    assert!(
        fireworks_codec::<JsonOps>()
            .parse(&JsonOps::INSTANCE, &json!({"flight_duration": 256}))
            .is_error()
    );
}

#[test]
fn attribute_modifiers_replacement_and_operation_order() {
    let attack_damage = id("attack_damage");
    let m1 = AttributeModifierSpec::new(id("base_attack"), 5.0, AttributeOperation::AddValue);
    let m2 =
        AttributeModifierSpec::new(id("bonus_base"), 0.5, AttributeOperation::AddMultipliedBase);
    let m3 = AttributeModifierSpec::new(
        id("bonus_total"),
        0.2,
        AttributeOperation::AddMultipliedTotal,
    );

    let mods = ItemAttributeModifiers::empty()
        .with_modifier_added(attack_damage.clone(), m1, EquipmentSlotGroup::Mainhand)
        .with_modifier_added(attack_damage.clone(), m2, EquipmentSlotGroup::Mainhand)
        .with_modifier_added(attack_damage.clone(), m3, EquipmentSlotGroup::Mainhand);

    // Paper compute order on base = 10.0:
    // +5.0 -> 15.0
    // +0.5 * 10.0 = +5.0 -> 20.0
    // +0.2 * 20.0 = +4.0 -> 24.0
    assert!((mods.compute(&attack_damage, 10.0, EquipmentSlot::Mainhand) - 24.0).abs() < 1e-9);
    // Non-matching slot leaves base unchanged.
    assert!((mods.compute(&attack_damage, 10.0, EquipmentSlot::Offhand) - 10.0).abs() < 1e-9);

    // Replacement removes old (attribute, id) and appends new one at the end.
    let replaced = mods.with_modifier_added(
        attack_damage.clone(),
        AttributeModifierSpec::new(id("base_attack"), 2.0, AttributeOperation::AddValue),
        EquipmentSlotGroup::Mainhand,
    );
    assert_eq!(replaced.modifiers.len(), 3);
    assert_eq!(replaced.modifiers[2].modifier.id, id("base_attack"));

    // Display override round-trips cleanly.
    let mut with_display = replaced;
    with_display.modifiers[0].display =
        AttributeModifierDisplay::OverrideText(Box::new(Component::literal("+50% Base")));
    with_display.modifiers[1].display = AttributeModifierDisplay::Hidden;
    round_trip_json(&item_attribute_modifiers_codec(), &with_display);
    round_trip_nbt(&item_attribute_modifiers_codec(), &with_display);
}

#[test]
fn sulfur_cube_instrument_and_trim_material() {
    let sc = SulfurCubeContent::new(
        ItemStackTemplate::of_item(ItemId::from_identifier(&id("water_bucket")).unwrap()).unwrap(),
    );
    round_trip_json(&sulfur_cube_content_codec(), &sc);
    round_trip_nbt(&sulfur_cube_content_codec(), &sc);

    let inst = InstrumentComponent::new(id("ponder_goat_horn"));
    round_trip_json(&instrument_component_codec(), &inst);
    round_trip_nbt(&instrument_component_codec(), &inst);

    let trim = ProvidesTrimMaterial::new(id("gold"));
    round_trip_json(&provides_trim_material_codec(), &trim);
    round_trip_nbt(&provides_trim_material_codec(), &trim);
}
