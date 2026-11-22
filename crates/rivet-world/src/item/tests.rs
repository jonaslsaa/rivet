use std::collections::HashSet;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_serialization::JsonOps;

use crate::item::{
    ABSOLUTE_MAX_STACK_SIZE, BUNDLE_MAX_ITEMS, BundleContents, CHARGED_PROJECTILES_MAX,
    CONTAINER_MAX_SIZE, ChargedProjectiles, DEFAULT_MAX_STACK_SIZE, DataComponentGetter,
    DataComponentMap, DataComponentPatch, DataComponentTypeId, DataComponentValue, Fraction,
    ItemContainerContents, ItemHolder, ItemId, ItemInstance, ItemProperties, ItemStack,
    ItemStackTemplate, ItemStackTypeAndComponentsKey, PatchedDataComponentMap, Rarity,
    bundle_contents_codec, charged_projectiles_codec, data_component_map_codec,
    data_component_patch_codec, item_container_contents_codec, item_stack_codec,
    item_stack_optional_codec, item_stack_template_codec, validate_components,
};

fn stone() -> ItemId {
    ItemId::from_name("minecraft:stone").expect("minecraft:stone must exist in generated registry")
}

fn granite() -> ItemId {
    ItemId::from_name("minecraft:granite")
        .expect("minecraft:granite must exist in generated registry")
}

fn bundle_item() -> ItemId {
    ItemId::from_name("minecraft:bundle")
        .expect("minecraft:bundle must exist in generated registry")
}

fn arrow() -> ItemId {
    ItemId::from_name("minecraft:arrow").expect("minecraft:arrow must exist in generated registry")
}

#[test]
fn registry_seam_item_and_component_identity() {
    assert_eq!(ItemId::AIR.id(), 0);
    assert!(ItemId::AIR.is_air());
    assert_eq!(ItemId::AIR.name(), "minecraft:air");
    const { assert!(ItemId::COUNT > 1000) };

    let s = stone();
    assert!(!s.is_air());
    assert_eq!(ItemId::from_id(s.id()), Some(s));
    assert_eq!(
        ItemId::from_identifier(&Identifier::with_default_namespace("stone")),
        Some(s)
    );
    assert_eq!(ItemId::from_name("minecraft:nonexistent_item_xyz"), None);

    // ItemHolder uses identity equality based on ItemId.
    let h1 = ItemHolder::new(s);
    let h2 = ItemHolder::new(s);
    let h3 = ItemHolder::new(granite());
    assert_eq!(h1, h2);
    assert_ne!(h1, h3);

    // DataComponentTypeId registry lookup and transient/swap metadata.
    assert_eq!(
        DataComponentTypeId::from_name("minecraft:max_stack_size"),
        Some(DataComponentTypeId::MAX_STACK_SIZE)
    );
    assert_eq!(
        DataComponentTypeId::from_name("minecraft:damage"),
        Some(DataComponentTypeId::DAMAGE)
    );
    assert!(DataComponentTypeId::CREATIVE_SLOT_LOCK.is_transient());
    assert!(!DataComponentTypeId::MAX_STACK_SIZE.is_transient());
    assert!(DataComponentTypeId::DAMAGE.ignore_swap_animation());
    assert!(!DataComponentTypeId::CUSTOM_NAME.ignore_swap_animation());
}

#[test]
fn item_properties_defaults_and_durability_validator() {
    let map = ItemProperties::new()
        .stacks_to(16)
        .rarity(Rarity::Rare)
        .build_for(stone())
        .expect("valid stackable item");
    assert_eq!(map.get_i32(DataComponentTypeId::MAX_STACK_SIZE), Some(16));
    assert_eq!(
        map.get(DataComponentTypeId::RARITY),
        Some(&DataComponentValue::Rarity(Rarity::Rare))
    );

    // Valid durable item (max_stack_size == 1).
    let tool = ItemProperties::new()
        .durability(250)
        .build_for(stone())
        .expect("valid durable item");
    assert_eq!(tool.get_i32(DataComponentTypeId::MAX_STACK_SIZE), Some(1));
    assert_eq!(tool.get_i32(DataComponentTypeId::MAX_DAMAGE), Some(250));
    assert_eq!(tool.get_i32(DataComponentTypeId::DAMAGE), Some(0));

    // Controlled negative: durability + stackable (> 1) must be rejected.
    let err = ItemProperties::new()
        .durability(100)
        .stacks_to(16)
        .build_for(stone())
        .expect_err("durability + stackable must fail");
    assert!(err.contains("durability") || err.contains("damage"));
}

#[test]
fn data_component_map_ordering_composite_and_filter() {
    let mut base_b = DataComponentMap::builder();
    base_b
        .set(
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentValue::Int(64),
        )
        .set(
            DataComponentTypeId::RARITY,
            DataComponentValue::Rarity(Rarity::Common),
        );
    let base = base_b.build().unwrap();

    // Insertion order is preserved deterministically.
    let keys: Vec<_> = base.key_set().into_iter().collect();
    assert_eq!(
        keys,
        vec![
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentTypeId::RARITY
        ]
    );

    let mut overlay_b = DataComponentMap::builder();
    overlay_b
        .set(
            DataComponentTypeId::RARITY,
            DataComponentValue::Rarity(Rarity::Epic),
        )
        .set(
            DataComponentTypeId::CUSTOM_NAME,
            DataComponentValue::Text("Excalibur".to_string()),
        );
    let overlay = overlay_b.build().unwrap();

    let combined = DataComponentMap::composite(&base, &overlay);
    assert_eq!(combined.len(), 3);
    assert_eq!(
        combined.get(DataComponentTypeId::RARITY),
        Some(&DataComponentValue::Rarity(Rarity::Epic))
    );
    assert_eq!(
        combined.get(DataComponentTypeId::CUSTOM_NAME),
        Some(&DataComponentValue::Text("Excalibur".to_string()))
    );

    let filtered = combined.filter(|k| k != DataComponentTypeId::MAX_STACK_SIZE);
    assert_eq!(filtered.len(), 2);
    assert!(!filtered.has(DataComponentTypeId::MAX_STACK_SIZE));
}

#[test]
fn data_component_patch_display_split_and_forget() {
    let mut pb = DataComponentPatch::builder();
    pb.set(DataComponentTypeId::DAMAGE, DataComponentValue::Int(7))
        .remove(DataComponentTypeId::UNBREAKABLE)
        .set(
            DataComponentTypeId::CUSTOM_NAME,
            DataComponentValue::Text("Pick".to_string()),
        );
    let patch = pb.build();

    assert_eq!(patch.len(), 3);
    assert!(!patch.is_empty());
    assert_eq!(
        patch.get(DataComponentTypeId::DAMAGE),
        Some(Some(&DataComponentValue::Int(7)))
    );
    assert_eq!(patch.get(DataComponentTypeId::UNBREAKABLE), Some(None));
    assert_eq!(patch.get(DataComponentTypeId::LORE), None);

    // Paper's exact DataComponentPatch.toString() order & syntax.
    assert_eq!(
        patch.to_string(),
        "{minecraft:damage=>7, !minecraft:unbreakable, minecraft:custom_name=>Pick}"
    );

    let split = patch.split();
    assert_eq!(split.added.len(), 2);
    assert_eq!(split.removed.len(), 1);
    assert!(split.removed.contains(&DataComponentTypeId::UNBREAKABLE));

    let forgotten = patch.forget(|k| k == DataComponentTypeId::DAMAGE);
    assert_eq!(forgotten.len(), 2);
    assert_eq!(forgotten.get(DataComponentTypeId::DAMAGE), None);
}

#[test]
fn patched_map_normalization_cow_and_iteration_order() {
    let mut proto_b = DataComponentMap::builder();
    proto_b
        .set(
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentValue::Int(64),
        )
        .set(
            DataComponentTypeId::RARITY,
            DataComponentValue::Rarity(Rarity::Common),
        )
        .set(DataComponentTypeId::REPAIR_COST, DataComponentValue::Int(0));
    let proto = proto_b.build().unwrap();

    let mut map = PatchedDataComponentMap::new(proto.clone());
    assert!(map.as_patch().is_empty());

    // Setting a value equal to prototype normalizes away (no patch entry).
    let prev = map.set(
        DataComponentTypeId::MAX_STACK_SIZE,
        DataComponentValue::Int(64),
    );
    assert_eq!(prev, Some(DataComponentValue::Int(64)));
    assert!(map.as_patch().is_empty());

    // Removing a component absent from prototype is a no-op.
    assert_eq!(map.remove(DataComponentTypeId::CUSTOM_NAME), None);
    assert!(map.as_patch().is_empty());

    // Copy-on-write isolation: mutating clone does not affect original.
    let mut cloned = map.copy();
    cloned.set(
        DataComponentTypeId::CUSTOM_NAME,
        DataComponentValue::Text("Named".to_string()),
    );
    cloned.remove(DataComponentTypeId::REPAIR_COST);
    assert!(!map.has(DataComponentTypeId::CUSTOM_NAME));
    assert!(map.has(DataComponentTypeId::REPAIR_COST));
    assert!(cloned.has(DataComponentTypeId::CUSTOM_NAME));
    assert!(!cloned.has(DataComponentTypeId::REPAIR_COST));
    assert_eq!(cloned.len(), 3);

    // Edge-ordering test: PatchedDataComponentMap emits patched non-removed entries
    // before unchanged prototype entries (matching Paper's PatchedDataComponentMap.iterator()).
    let iter_keys: Vec<_> = cloned
        .typed_iter()
        .into_iter()
        .map(|t| t.component_type)
        .collect();
    assert_eq!(
        iter_keys,
        vec![
            DataComponentTypeId::CUSTOM_NAME,
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentTypeId::RARITY,
        ]
    );

    // Equality & java_hash_code agree with an independently constructed map.
    let rebuilt = PatchedDataComponentMap::from_patch(proto, &cloned.as_patch());
    assert_eq!(cloned, rebuilt);
    assert_eq!(cloned.java_hash_code(), rebuilt.java_hash_code());
}

#[test]
fn item_stack_empty_count_split_damage_and_matching() {
    let empty = ItemStack::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.count(), 0);
    assert_eq!(empty.item_id(), ItemId::AIR);

    // Air with positive count is still empty and reports count 0.
    let air_stack = ItemStack::with_count(ItemId::AIR, 10);
    assert!(air_stack.is_empty());
    assert_eq!(air_stack.count(), 0);

    // Non-air with count <= 0 is empty and reports count 0, while set_count stores raw value.
    let mut stack = ItemStack::with_count(stone(), 5);
    assert!(!stack.is_empty());
    assert_eq!(stack.count(), 5);
    stack.set_count(-3);
    assert!(stack.is_empty());
    assert_eq!(stack.count(), 0);
    stack.set_count(10);
    assert_eq!(stack.count(), 10);

    // Split semantics.
    let split = stack.split(4);
    assert_eq!(split.count(), 4);
    assert_eq!(stack.count(), 6);
    let over_split = stack.split(99);
    assert_eq!(over_split.count(), 6);
    assert!(stack.is_empty());

    // Wrapping grow/shrink semantics.
    let mut wrap = ItemStack::with_count(stone(), i32::MAX);
    wrap.grow(1);
    assert!(wrap.is_empty()); // wrapped to i32::MIN <= 0

    // Damage & stackability semantics.
    let tool_proto = ItemProperties::new()
        .durability(10)
        .build_for(stone())
        .unwrap();
    let mut tool = ItemStack::from_holder(ItemHolder::with_components(stone(), tool_proto), 1);
    assert!(tool.is_damageable_item());
    assert!(!tool.is_damaged());
    assert!(!tool.is_stackable());
    assert_eq!(tool.max_damage(), 10);
    assert_eq!(tool.damage_value(), 0);

    tool.set_damage_value(9);
    assert!(tool.is_damaged());
    assert!(!tool.is_broken());
    assert!(tool.next_damage_will_break());

    // Clamped to max_damage when over-damaged.
    tool.set_damage_value(999);
    assert_eq!(tool.damage_value(), 10);
    assert!(tool.is_broken());

    // Matching & ItemStackTypeAndComponentsKey (count-independent).
    let mut a = ItemStack::with_count(stone(), 1);
    a.set(
        DataComponentTypeId::CUSTOM_NAME,
        DataComponentValue::Text("Rock".to_string()),
    );
    let mut b = ItemStack::with_count(stone(), 32);
    b.set(
        DataComponentTypeId::CUSTOM_NAME,
        DataComponentValue::Text("Rock".to_string()),
    );
    assert!(!ItemStack::matches(&a, &b)); // different counts
    assert!(ItemStack::is_same_item(&a, &b));
    assert!(ItemStack::is_same_item_same_components(&a, &b));

    let mut set = HashSet::new();
    set.insert(ItemStackTypeAndComponentsKey(a.clone()));
    assert!(set.contains(&ItemStackTypeAndComponentsKey(b.clone())));
    assert_eq!(
        ItemStack::hash_item_and_components(&a),
        ItemStack::hash_item_and_components(&b)
    );
}

#[test]
fn codecs_roundtrip_nbt_and_json() {
    let nbt_ops = NbtOps::instance();
    let json_ops = JsonOps::new(false);

    // 1. DataComponentMap codec omits transient components.
    let mut custom = CompoundTag::new();
    custom.put_int("level", 3);
    let mut map_b = DataComponentMap::builder();
    map_b
        .set(
            DataComponentTypeId::CUSTOM_DATA,
            DataComponentValue::CustomData(custom),
        )
        .set(
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentValue::Int(16),
        )
        .set(
            DataComponentTypeId::CREATIVE_SLOT_LOCK,
            DataComponentValue::Unit,
        );
    let map = map_b.build().unwrap();

    let nbt_codec = data_component_map_codec::<NbtOps>();
    let encoded = nbt_codec
        .encode_start(&nbt_ops, &map)
        .result()
        .cloned()
        .expect("encode map");
    let decoded = nbt_codec
        .parse(&nbt_ops, &encoded)
        .result()
        .cloned()
        .expect("decode map");
    assert_eq!(decoded.len(), 2);
    assert!(!decoded.has(DataComponentTypeId::CREATIVE_SLOT_LOCK));
    assert_eq!(
        decoded.get_i32(DataComponentTypeId::MAX_STACK_SIZE),
        Some(16)
    );

    // 2. DataComponentPatch codec roundtrip with removals (`!`).
    let mut pb = DataComponentPatch::builder();
    pb.set(
        DataComponentTypeId::CUSTOM_NAME,
        DataComponentValue::Text("Hello".to_string()),
    )
    .remove(DataComponentTypeId::RARITY);
    let patch = pb.build();
    let patch_json_codec = data_component_patch_codec::<JsonOps>();
    let encoded_patch = patch_json_codec
        .encode_start(&json_ops, &patch)
        .result()
        .cloned()
        .expect("encode patch json");
    let decoded_patch = patch_json_codec
        .parse(&json_ops, &encoded_patch)
        .result()
        .cloned()
        .expect("decode patch json");
    assert_eq!(decoded_patch, patch);

    // 3. ItemStack codec and OPTIONAL_CODEC roundtrip.
    let mut stack = ItemStack::with_count(stone(), 12);
    stack.set(
        DataComponentTypeId::CUSTOM_NAME,
        DataComponentValue::Text("Cobble".to_string()),
    );
    let stack_nbt_codec = item_stack_codec::<NbtOps>();
    let enc_stack = stack_nbt_codec
        .encode_start(&nbt_ops, &stack)
        .result()
        .cloned()
        .expect("encode stack");
    let dec_stack = stack_nbt_codec
        .parse(&nbt_ops, &enc_stack)
        .result()
        .cloned()
        .expect("decode stack");
    assert!(ItemStack::matches(&stack, &dec_stack));

    // OPTIONAL_CODEC encodes empty as {} and decodes {} as EMPTY.
    let opt_codec = item_stack_optional_codec::<JsonOps>();
    let enc_empty = opt_codec
        .encode_start(&json_ops, &ItemStack::empty())
        .result()
        .cloned()
        .expect("encode empty optional");
    let dec_empty = opt_codec
        .parse(&json_ops, &enc_empty)
        .result()
        .cloned()
        .expect("decode empty optional");
    assert!(dec_empty.is_empty());
}

#[test]
fn recursive_components_container_bundle_and_projectiles() {
    let nbt_ops = NbtOps::instance();
    let json_ops = JsonOps::new(false);

    // ItemContainerContents preserves sparse slots and trims trailing empties.
    let s1 = ItemStack::with_count(stone(), 4);
    let s2 = ItemStack::with_count(granite(), 2);
    let container = ItemContainerContents::from_items(&[
        ItemStack::empty(),
        s1.clone(),
        ItemStack::empty(),
        s2.clone(),
        ItemStack::empty(),
    ])
    .expect("valid container");
    assert_eq!(container.slots().len(), 4);
    assert!(container.slots()[0].is_none());
    assert!(container.slots()[1].is_some());
    assert!(container.slots()[2].is_none());
    assert!(container.slots()[3].is_some());

    let container_codec = item_container_contents_codec::<NbtOps>();
    let enc = container_codec
        .encode_start(&nbt_ops, &container)
        .result()
        .cloned()
        .expect("encode container");
    let dec = container_codec
        .parse(&nbt_ops, &enc)
        .result()
        .cloned()
        .expect("decode container");
    assert_eq!(dec, container);

    // BundleContents exact Fraction weights (32/64 + 32/64 == 1).
    let half_stone = ItemStackTemplate::of_item_count(stone(), 32).unwrap();
    let half_granite = ItemStackTemplate::of_item_count(granite(), 32).unwrap();
    let full_bundle = BundleContents::try_new(vec![half_stone, half_granite]).unwrap();
    assert_eq!(full_bundle.weight(), Fraction::ONE);
    assert!(full_bundle.validate().is_success());

    // Nested bundle weight: 1/16 + inner_weight.
    let inner_item = ItemStackTemplate::of_item_count(stone(), 16).unwrap();
    let inner_bundle = BundleContents::try_new(vec![inner_item]).unwrap();
    let mut inner_pb = DataComponentPatch::builder();
    inner_pb.set(
        DataComponentTypeId::BUNDLE_CONTENTS,
        DataComponentValue::BundleContents(inner_bundle),
    );
    let nested_bundle_tmpl =
        ItemStackTemplate::try_new(bundle_item().holder(), 1, inner_pb.build()).unwrap();
    let outer_bundle = BundleContents::try_new(vec![nested_bundle_tmpl]).unwrap();
    // 1/16 + 16/64 = 1/16 + 4/16 = 5/16.
    assert_eq!(outer_bundle.weight(), Fraction::new(5, 16));
    assert!(outer_bundle.validate().is_success());

    let bundle_codec = bundle_contents_codec::<JsonOps>();
    let enc_b = bundle_codec
        .encode_start(&json_ops, &outer_bundle)
        .result()
        .cloned()
        .expect("encode bundle");
    let dec_b = bundle_codec
        .parse(&json_ops, &enc_b)
        .result()
        .cloned()
        .expect("decode bundle");
    assert_eq!(dec_b, outer_bundle);

    // ChargedProjectiles filters empties and roundtrips.
    let proj = ChargedProjectiles::of_non_empty(&[
        ItemStack::empty(),
        ItemStack::with_count(arrow(), 1),
        ItemStack::empty(),
    ]);
    assert_eq!(proj.len(), 1);
    assert!(proj.contains(arrow()));
    assert!(!proj.contains(stone()));

    let proj_codec = charged_projectiles_codec::<NbtOps>();
    let enc_p = proj_codec
        .encode_start(&nbt_ops, &proj)
        .result()
        .cloned()
        .expect("encode projectiles");
    let dec_p = proj_codec
        .parse(&nbt_ops, &enc_p)
        .result()
        .cloned()
        .expect("decode projectiles");
    assert_eq!(dec_p, proj);

    // ItemStackTemplate codec roundtrip.
    let tmpl = ItemStackTemplate::of_item_count(stone(), 5).unwrap();
    let tmpl_codec = item_stack_template_codec::<JsonOps>();
    let enc_t = tmpl_codec
        .encode_start(&json_ops, &tmpl)
        .result()
        .cloned()
        .expect("encode template");
    let dec_t = tmpl_codec
        .parse(&json_ops, &enc_t)
        .result()
        .cloned()
        .expect("decode template");
    assert_eq!(dec_t, tmpl);
}

#[test]
fn controlled_negative_validations_and_codec_rejections() {
    let nbt_ops = NbtOps::instance();
    let stack_codec = item_stack_codec::<NbtOps>();

    // 1. Air item rejected by ItemStack::CODEC.
    let mut air_tag = CompoundTag::new();
    air_tag.put_string("id", "minecraft:air");
    air_tag.put_int("count", 1);
    assert!(
        stack_codec
            .parse(&nbt_ops, &Tag::Compound(air_tag))
            .is_error()
    );

    // 2. Count outside 1..=99 rejected by ItemStack::CODEC.
    for bad_count in [0, -1, ABSOLUTE_MAX_STACK_SIZE + 1] {
        let mut tag = CompoundTag::new();
        tag.put_string("id", "minecraft:stone");
        tag.put_int("count", bad_count);
        assert!(
            stack_codec.parse(&nbt_ops, &Tag::Compound(tag)).is_error(),
            "count {bad_count} must be rejected"
        );
    }

    // 3. Unknown component and transient component rejected on decode.
    let map_codec = data_component_map_codec::<NbtOps>();
    let mut bad_map = CompoundTag::new();
    bad_map.put_int("minecraft:does_not_exist", 1);
    assert!(
        map_codec
            .parse(&nbt_ops, &Tag::Compound(bad_map))
            .is_error()
    );

    let mut transient_map = CompoundTag::new();
    transient_map.put(
        "minecraft:creative_slot_lock".to_string(),
        Tag::Compound(CompoundTag::new()),
    );
    assert!(
        map_codec
            .parse(&nbt_ops, &Tag::Compound(transient_map))
            .is_error()
    );

    // 4. Count exceeding max_stack_size rejected by validate_strict.
    let mut oversized = ItemStack::with_count(stone(), DEFAULT_MAX_STACK_SIZE + 1);
    assert!(oversized.validate_strict().is_error());
    oversized.set_count(DEFAULT_MAX_STACK_SIZE);
    assert!(oversized.validate_strict().is_success());

    // 5. Durability + stackable (> 1) rejected by validate_components.
    let mut bad_b = DataComponentMap::builder();
    bad_b
        .set(
            DataComponentTypeId::MAX_STACK_SIZE,
            DataComponentValue::Int(16),
        )
        .set(DataComponentTypeId::MAX_DAMAGE, DataComponentValue::Int(50))
        .set(DataComponentTypeId::DAMAGE, DataComponentValue::Int(1));
    let bad_components = bad_b.build().unwrap();
    assert!(validate_components(&bad_components).is_error());

    // 6. Over-capacity BundleContents (> 1 weight) rejected.
    let t1 = ItemStackTemplate::of_item_count(stone(), 64).unwrap();
    let t2 = ItemStackTemplate::of_item_count(granite(), 1).unwrap();
    let overfull = BundleContents::try_new(vec![t1.clone(), t2]).unwrap();
    assert!(overfull.validate().is_error());

    // 7. Bound limits enforced on container, bundle, and charged projectiles.
    assert!(ItemContainerContents::try_from_slots(vec![None; CONTAINER_MAX_SIZE + 1]).is_err());
    assert!(BundleContents::try_new(vec![t1.clone(); BUNDLE_MAX_ITEMS + 1]).is_err());
    assert!(ChargedProjectiles::try_new(vec![t1; CHARGED_PROJECTILES_MAX + 1]).is_err());

    // 8. ItemStackTemplate rejects air and non-positive count.
    assert!(
        ItemStackTemplate::try_new(ItemId::AIR.holder(), 1, DataComponentPatch::empty()).is_err()
    );
    assert!(ItemStackTemplate::of_item_count(stone(), 0).is_err());
}
