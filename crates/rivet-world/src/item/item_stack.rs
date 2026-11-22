//! Port of `net.minecraft.world.item.ItemInstance`,
//! `net.minecraft.world.item.ItemStack`, and
//! `net.minecraft.world.item.ItemStackLinkedSet`.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rivet_serialization::{Codec, DataResult, DynamicOps, codec};

use crate::item::component_map::{DataComponentGetter, DataComponentMap};
use crate::item::component_patch::{
    DataComponentPatch, PatchedDataComponentMap, data_component_patch_codec,
};
use crate::item::component_type::{DataComponentTypeId, DataComponentValue};
use crate::item::item_id::{ABSOLUTE_MAX_STACK_SIZE, ItemHolder, ItemId, ItemLike, item_codec};
use crate::item::recursive_components::{
    BUNDLE_MAX_ITEMS, CHARGED_PROJECTILES_MAX, CONTAINER_MAX_SIZE,
};

/// Port of `net.minecraft.world.item.ItemInstance`.
///
/// Common read-only adapter implemented by both `ItemStack` and
/// `ItemStackTemplate`, intended for downstream consumers (inventory,
/// entities, loot, persistence, protocol).
pub trait ItemInstance: DataComponentGetter {
    const FIELD_ID: &'static str = "id";
    const FIELD_COUNT: &'static str = "count";
    const FIELD_COMPONENTS: &'static str = "components";

    /// Returns the item registry identity.
    fn item_id(&self) -> ItemId;

    /// Returns the effective item count.
    fn count(&self) -> i32;

    /// `ItemInstance.getMaxStackSize()` — `getOrDefault(MAX_STACK_SIZE, 1)`.
    fn max_stack_size(&self) -> i32 {
        self.get_i32_or(DataComponentTypeId::MAX_STACK_SIZE, 1)
    }

    /// `ItemInstance.is(Item)`.
    fn is(&self, item: ItemId) -> bool {
        self.item_id() == item
    }
}

/// Port of `net.minecraft.world.item.ItemStack`.
///
/// Preserves Paper 26.2 semantics:
/// - `ItemStack::empty()` sentinel (`item == None`, `count == 0`).
/// - `is_empty()` is `true` for the sentinel, `minecraft:air`, or `count <= 0`.
/// - `count()` returns `0` when empty; `set_count` stores the raw count
///   without normalizing; `grow`/`shrink` use Java wrapping `i32` addition.
/// - `PatchedDataComponentMap` carries copy-on-write component overrides over
///   the item's prototype `DataComponentMap`.
/// - `matches`, `is_same_item`, `is_same_item_same_components`, and
///   `ItemStackLinkedSet` hashing follow Paper's exact rules.
#[derive(Clone, Debug)]
pub struct ItemStack {
    /// `None` represents Paper's `ItemStack.EMPTY` sentinel (`this.item == null`).
    item: Option<ItemHolder>,
    count: i32,
    components: PatchedDataComponentMap,
}

impl Default for ItemStack {
    fn default() -> Self {
        Self::empty()
    }
}

impl ItemStack {
    pub const FIELD_ID: &'static str = "id";
    pub const FIELD_COUNT: &'static str = "count";
    pub const FIELD_COMPONENTS: &'static str = "components";

    /// Port of `ItemStack.EMPTY`.
    pub fn empty() -> Self {
        Self {
            item: None,
            count: 0,
            components: PatchedDataComponentMap::new(DataComponentMap::empty()),
        }
    }

    /// Port of `new ItemStack(ItemLike)`.
    pub fn of(item: impl ItemLike) -> Self {
        Self::with_count(item, 1)
    }

    /// Port of `new ItemStack(ItemLike, int)`.
    pub fn with_count(item: impl ItemLike, count: i32) -> Self {
        let holder = item.as_item().holder();
        Self::from_holder(holder, count)
    }

    /// Port of `new ItemStack(Holder<Item>, int)`.
    pub fn from_holder(holder: ItemHolder, count: i32) -> Self {
        let components = PatchedDataComponentMap::new(holder.components().clone());
        Self {
            item: Some(holder),
            count,
            components,
        }
    }

    /// Port of `new ItemStack(Holder<Item>, int, DataComponentPatch)`.
    pub fn from_holder_and_patch(
        holder: ItemHolder,
        count: i32,
        patch: &DataComponentPatch,
    ) -> Self {
        let components = PatchedDataComponentMap::from_patch(holder.components().clone(), patch);
        Self {
            item: Some(holder),
            count,
            components,
        }
    }

    /// Port of `new ItemStack(ItemLike, int, DataComponentPatch)`.
    pub fn with_patch(item: impl ItemLike, count: i32, patch: &DataComponentPatch) -> Self {
        Self::from_holder_and_patch(item.as_item().holder(), count, patch)
    }

    /// Port of `ItemStack.isEmpty()`.
    ///
    /// Returns `true` for the `EMPTY` sentinel, `minecraft:air`, or `count <= 0`.
    #[inline]
    pub fn is_empty(&self) -> bool {
        match &self.item {
            None => true,
            Some(holder) => holder.is(ItemId::AIR) || self.count <= 0,
        }
    }

    /// Port of `ItemStack.getItem()`.
    ///
    /// Returns `ItemId::AIR` when empty, matching Paper's `isEmpty() ? Items.AIR : this.item.value()`.
    #[inline]
    pub fn item(&self) -> ItemId {
        if self.is_empty() {
            ItemId::AIR
        } else {
            self.item.as_ref().map_or(ItemId::AIR, ItemHolder::value)
        }
    }

    /// Port of `ItemStack.typeHolder()` (`getItemHolder()`).
    pub fn type_holder(&self) -> ItemHolder {
        if self.is_empty() {
            ItemId::AIR.holder()
        } else {
            self.item.clone().unwrap_or_else(|| ItemId::AIR.holder())
        }
    }

    /// Port of `ItemStack.is(Item)`.
    #[inline]
    pub fn is_item(&self, item: ItemId) -> bool {
        self.item() == item
    }

    /// Port of `ItemStack.getCount()` / `count()`.
    #[inline]
    pub fn get_count(&self) -> i32 {
        if self.is_empty() { 0 } else { self.count }
    }

    /// Port of `ItemStack.setCount(int)`.
    ///
    /// Stores the raw count without clamping/normalizing, matching Paper.
    #[inline]
    pub fn set_count(&mut self, count: i32) {
        self.count = count;
    }

    /// Port of `ItemStack.grow(int)` using Java wrapping `i32` addition.
    #[inline]
    pub fn grow(&mut self, amount: i32) {
        let next = self.get_count().wrapping_add(amount);
        self.set_count(next);
    }

    /// Port of `ItemStack.shrink(int)` (`this.grow(-amount)`).
    #[inline]
    pub fn shrink(&mut self, amount: i32) {
        self.grow(amount.wrapping_neg());
    }

    /// Port of `ItemStack.limitSize(int)`.
    pub fn limit_size(&mut self, max_stack_size: i32) {
        if !self.is_empty() && self.get_count() > max_stack_size {
            self.set_count(max_stack_size);
        }
    }

    /// Port of `ItemStack.split(int)`.
    ///
    /// Takes `min(amount, getCount())`, returns a copy with that count, and
    /// shrinks `self` by that amount.
    pub fn split(&mut self, amount: i32) -> ItemStack {
        let taken = amount.min(self.get_count());
        let split_stack = self.copy_with_count(taken);
        self.shrink(taken);
        split_stack
    }

    /// Port of `ItemStack.copy()`.
    pub fn copy(&self) -> ItemStack {
        if self.is_empty() {
            return ItemStack::empty();
        }
        ItemStack {
            item: self.item.clone(),
            count: self.count,
            components: self.components.copy(),
        }
    }

    /// Port of `ItemStack.copyWithCount(int)`.
    pub fn copy_with_count(&self, count: i32) -> ItemStack {
        if self.is_empty() {
            return ItemStack::empty();
        }
        let mut copy = self.copy();
        copy.set_count(count);
        copy
    }

    /// Port of `ItemStack.copyAndClear()`.
    pub fn copy_and_clear(&mut self) -> ItemStack {
        if self.is_empty() {
            return ItemStack::empty();
        }
        let copy = self.copy();
        self.set_count(0);
        copy
    }

    // -------------------------------------------------------------------------
    // Components API
    // -------------------------------------------------------------------------

    /// Returns the underlying `PatchedDataComponentMap` (empty prototype when
    /// this stack is empty).
    pub fn patched_components(&self) -> &PatchedDataComponentMap {
        &self.components
    }

    /// Port of `ItemStack.getComponents()` as an immutable `DataComponentMap`.
    ///
    /// Returns `DataComponentMap::empty()` for an empty stack.
    pub fn immutable_components(&self) -> DataComponentMap {
        if self.is_empty() {
            DataComponentMap::empty()
        } else {
            self.components.to_immutable_map()
        }
    }

    /// Port of `ItemStack.getPrototype()`.
    pub fn prototype(&self) -> DataComponentMap {
        if self.is_empty() {
            DataComponentMap::empty()
        } else {
            self.components.prototype().clone()
        }
    }

    /// Port of `ItemStack.getComponentsPatch()`.
    pub fn components_patch(&self) -> DataComponentPatch {
        if self.is_empty() {
            DataComponentPatch::empty()
        } else {
            self.components.as_patch()
        }
    }

    /// Port of `ItemStack.hasNonDefault(DataComponentType<?>)`.
    pub fn has_non_default(&self, component_type: DataComponentTypeId) -> bool {
        !self.is_empty() && self.components.has_non_default(component_type)
    }

    /// Port of `ItemStack.set(DataComponentType<T>, T)`.
    pub fn set(
        &mut self,
        component_type: DataComponentTypeId,
        value: DataComponentValue,
    ) -> Option<DataComponentValue> {
        self.components.set(component_type, value)
    }

    /// Port of `ItemStack.set(DataComponentType<T>, @Nullable T)`.
    pub fn set_opt(
        &mut self,
        component_type: DataComponentTypeId,
        value: Option<DataComponentValue>,
    ) -> Option<DataComponentValue> {
        self.components.set_opt(component_type, value)
    }

    /// Port of `ItemStack.remove(DataComponentType<T>)`.
    pub fn remove(&mut self, component_type: DataComponentTypeId) -> Option<DataComponentValue> {
        self.components.remove(component_type)
    }

    /// Port of `ItemStack.applyComponents(DataComponentPatch)`.
    pub fn apply_components(&mut self, patch: &DataComponentPatch) {
        self.components.apply_patch(patch);
    }

    /// Port of `ItemStack.applyComponentsAndValidate(DataComponentPatch)`.
    pub fn apply_components_and_validate(&mut self, patch: &DataComponentPatch) -> DataResult<()> {
        self.components.apply_patch(patch);
        validate_components(&self.components)
    }

    /// Port of `ItemStack.applyComponents(DataComponentMap)`.
    pub fn apply_component_map(&mut self, map: &DataComponentMap) {
        self.components.set_all(map);
    }

    /// Port of `ItemStack.isComponentsPatchEmpty()`.
    pub fn is_components_patch_empty(&self) -> bool {
        self.components_patch().is_empty()
    }

    // -------------------------------------------------------------------------
    // Stackability & Damage semantics (Paper exact)
    // -------------------------------------------------------------------------

    /// Port of `ItemStack.isStackable()`.
    ///
    /// `getMaxStackSize() > 1 && (!isDamageableItem() || !isDamaged())`
    pub fn is_stackable(&self) -> bool {
        self.max_stack_size() > 1 && (!self.is_damageable_item() || !self.is_damaged())
    }

    /// Port of `ItemStack.isDamageableItem()`.
    ///
    /// `has(MAX_DAMAGE) && !has(UNBREAKABLE) && has(DAMAGE)`
    pub fn is_damageable_item(&self) -> bool {
        self.has(DataComponentTypeId::MAX_DAMAGE)
            && !self.has(DataComponentTypeId::UNBREAKABLE)
            && self.has(DataComponentTypeId::DAMAGE)
    }

    /// Port of `ItemStack.isDamaged()`.
    pub fn is_damaged(&self) -> bool {
        self.is_damageable_item() && self.damage_value() > 0
    }

    /// Port of `ItemStack.getMaxDamage()`.
    pub fn max_damage(&self) -> i32 {
        self.get_i32_or(DataComponentTypeId::MAX_DAMAGE, 0)
    }

    /// Port of `ItemStack.getDamageValue()`.
    ///
    /// `Mth.clamp(getOrDefault(DAMAGE, 0), 0, getMaxDamage())`
    pub fn damage_value(&self) -> i32 {
        let max_d = self.max_damage();
        let raw = self.get_i32_or(DataComponentTypeId::DAMAGE, 0);
        clamp_i32(raw, 0, max_d)
    }

    /// Port of `ItemStack.setDamageValue(int)`.
    pub fn set_damage_value(&mut self, value: i32) {
        let max_d = self.max_damage();
        let clamped = clamp_i32(value, 0, max_d);
        self.set(
            DataComponentTypeId::DAMAGE,
            DataComponentValue::Int(clamped),
        );
    }

    /// Port of `ItemStack.isBroken()`.
    pub fn is_broken(&self) -> bool {
        self.is_damageable_item() && self.damage_value() >= self.max_damage()
    }

    /// Port of `ItemStack.nextDamageWillBreak()`.
    pub fn next_damage_will_break(&self) -> bool {
        self.is_damageable_item() && self.damage_value().wrapping_add(1) >= self.max_damage()
    }

    /// Returns `true` if this item can be placed inside container items like
    /// shulker boxes or bundles (`Item.canFitInsideContainerItems()`).
    pub fn can_fit_inside_container_items(&self) -> bool {
        let name = self.item().name();
        !name.ends_with("shulker_box")
    }

    // -------------------------------------------------------------------------
    // Paper static equality & matching helpers
    // -------------------------------------------------------------------------

    /// Port of `ItemStack.matches(ItemStack, ItemStack)`.
    pub fn matches(a: &ItemStack, b: &ItemStack) -> bool {
        a.get_count() == b.get_count() && Self::is_same_item_same_components(a, b)
    }

    /// Port of `ItemStack.isSameItem(ItemStack, ItemStack)`.
    pub fn is_same_item(a: &ItemStack, b: &ItemStack) -> bool {
        a.is_item(b.item())
    }

    /// Port of `ItemStack.isSameItemSameComponents(ItemStack, ItemStack)`.
    pub fn is_same_item_same_components(a: &ItemStack, b: &ItemStack) -> bool {
        if !a.is_item(b.item()) {
            return false;
        }
        if a.is_empty() && b.is_empty() {
            return true;
        }
        a.components == b.components
    }

    /// Port of `ItemStack.matchesIgnoringComponents(a, b, Predicate<DataComponentType<?>>)`.
    pub fn matches_ignoring_components<F>(a: &ItemStack, b: &ItemStack, mut ignore: F) -> bool
    where
        F: FnMut(DataComponentTypeId) -> bool,
    {
        if a.get_count() != b.get_count() || !a.is_item(b.item()) {
            return false;
        }
        if a.is_empty() && b.is_empty() {
            return true;
        }
        let a_map = a.immutable_components();
        let b_map = b.immutable_components();
        for (k, v) in a_map.iter() {
            if ignore(k) {
                continue;
            }
            if b_map.get(k) != Some(v) {
                return false;
            }
        }
        for (k, _) in b_map.iter() {
            if ignore(k) {
                continue;
            }
            if !a_map.has(k) {
                return false;
            }
        }
        true
    }

    /// Port of `ItemStack.hashItemAndComponents(ItemStack)`.
    ///
    /// Matches `ItemStackLinkedSet` hashing: includes item identity and
    /// effective components, excludes count.
    pub fn hash_item_and_components(stack: &ItemStack) -> i32 {
        let result = 31i32.wrapping_add(stack.item().java_hash_code());
        let comp_hash = if stack.is_empty() {
            0
        } else {
            stack.components.java_hash_code()
        };
        result.wrapping_mul(31).wrapping_add(comp_hash)
    }

    /// Validates this stack's count and components (`ItemStack.validateStrict`).
    pub fn validate_strict(&self) -> DataResult<()> {
        if self.is_empty() {
            return DataResult::error("ItemStack must not be empty");
        }
        validate_components(&self.components).flat_map(|()| {
            let max_stack = self.max_stack_size();
            if self.count > max_stack {
                DataResult::error(format!(
                    "Item stack with stack size of {} was larger than maximum: {}",
                    self.count, max_stack
                ))
            } else {
                DataResult::success(())
            }
        })
    }
}

/// Java `Mth.clamp(int value, int min, int max)`:
/// returns `min` when `value < min`, `max` when `value > max`, else `value`.
/// Note: when `max < min` (e.g. `max_damage == 0` with negative damage),
/// `Math.min(Math.max(value, min), max)` matches Paper's `Mth.clamp`.
#[inline]
fn clamp_i32(value: i32, min: i32, max: i32) -> i32 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

impl DataComponentGetter for ItemStack {
    fn get(&self, component_type: DataComponentTypeId) -> Option<&DataComponentValue> {
        if self.is_empty() {
            return None;
        }
        self.components.get(component_type)
    }
}

impl ItemInstance for ItemStack {
    #[inline]
    fn item_id(&self) -> ItemId {
        self.item()
    }

    #[inline]
    fn count(&self) -> i32 {
        self.get_count()
    }
}

impl fmt::Display for ItemStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.get_count(), self.item())
    }
}

/// Wrapper key implementing Paper's `ItemStackLinkedSet.TYPE_AND_TAG` strategy:
/// compares and hashes by `ItemStack::is_same_item_same_components` and
/// `ItemStack::hash_item_and_components` (ignoring count).
#[derive(Clone, Debug)]
pub struct ItemStackTypeAndComponentsKey(pub ItemStack);

impl PartialEq for ItemStackTypeAndComponentsKey {
    fn eq(&self, other: &Self) -> bool {
        ItemStack::is_same_item_same_components(&self.0, &other.0)
    }
}

impl Eq for ItemStackTypeAndComponentsKey {}

impl Hash for ItemStackTypeAndComponentsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        ItemStack::hash_item_and_components(&self.0).hash(state);
    }
}

/// Port of `ItemStack.validateComponents(DataComponentMap)`.
///
/// Enforces:
/// 1. `"Item cannot be both damageable and stackable"` when `MAX_DAMAGE` is
///    present and `MAX_STACK_SIZE > 1`.
/// 2. Container, bundle, and charged-projectile size/weight invariants.
pub fn validate_components<G: DataComponentGetter>(components: &G) -> DataResult<()> {
    if components.has(DataComponentTypeId::MAX_DAMAGE)
        && components.get_i32_or(DataComponentTypeId::MAX_STACK_SIZE, 1) > 1
    {
        return DataResult::error("Item cannot be both damageable and stackable");
    }
    if let Some(container) = components
        .get(DataComponentTypeId::CONTAINER)
        .and_then(DataComponentValue::as_container)
        && container.slots().len() > CONTAINER_MAX_SIZE
    {
        return DataResult::error(format!(
            "Container has {} items, maximum is {}",
            container.slots().len(),
            CONTAINER_MAX_SIZE
        ));
    }
    if let Some(bundle) = components
        .get(DataComponentTypeId::BUNDLE_CONTENTS)
        .and_then(DataComponentValue::as_bundle_contents)
    {
        if bundle.len() > BUNDLE_MAX_ITEMS {
            return DataResult::error(format!(
                "Bundle has {} items, maximum is {}",
                bundle.len(),
                BUNDLE_MAX_ITEMS
            ));
        }
        if let Err(err) = crate::item::data_result_into(bundle.validate()) {
            return DataResult::error(err);
        }
    }
    if let Some(proj) = components
        .get(DataComponentTypeId::CHARGED_PROJECTILES)
        .and_then(DataComponentValue::as_charged_projectiles)
        && proj.len() > CHARGED_PROJECTILES_MAX
    {
        return DataResult::error(format!(
            "ChargedProjectiles has {} items, maximum is {}",
            proj.len(),
            CHARGED_PROJECTILES_MAX
        ));
    }
    DataResult::success(())
}

/// Computes the `DataComponentPatch` representing `components` relative to
/// `item`'s standard registry prototype so codecs preserve custom prototype
/// defaults across save/load round trips.
pub(crate) fn patch_relative_to_standard_prototype(
    item: ItemId,
    components: &PatchedDataComponentMap,
) -> DataComponentPatch {
    let standard = item.default_components();
    if components.prototype() == &standard {
        return components.as_patch();
    }
    let mut builder = DataComponentPatch::builder();
    for k in components.key_set() {
        if let Some(v) = components.get(k)
            && standard.get(k) != Some(v)
        {
            builder.set(k, v.clone());
        }
    }
    for (k, _) in standard.iter() {
        if !components.has(k) {
            builder.remove(k);
        }
    }
    builder.build()
}

/// Port of `ItemStack.CODEC` (`id`, `count` in `1..=99` defaulting to 1,
/// `components` patch defaulting to empty, validated via `validate_components`).
pub fn item_stack_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ItemStack, Ops>> {
    crate::item::codec_from_fns(
        "ItemStack",
        |stack: &ItemStack, ops: &Ops| {
            if stack.is_empty() {
                return DataResult::error("Cannot encode empty ItemStack with ItemStack.CODEC");
            }
            if let Err(err) = crate::item::data_result_into(validate_components(&stack.components))
            {
                return DataResult::error(err);
            }
            let id_val = match crate::item::data_result_into(
                item_codec::<Ops>().encode_start(ops, &stack.item()),
            ) {
                Ok(v) => v,
                Err(err) => return DataResult::error(err),
            };
            let count_val = match crate::item::data_result_into(
                codec::int_range::<Ops>(1, ABSOLUTE_MAX_STACK_SIZE)
                    .encode_start(ops, &stack.get_count()),
            ) {
                Ok(v) => v,
                Err(err) => return DataResult::error(err),
            };
            let mut entries = vec![
                rivet_serialization::Pair::of(
                    ops.create_string(ItemStack::FIELD_ID.to_string()),
                    id_val,
                ),
                rivet_serialization::Pair::of(
                    ops.create_string(ItemStack::FIELD_COUNT.to_string()),
                    count_val,
                ),
            ];
            let patch = patch_relative_to_standard_prototype(stack.item(), &stack.components);
            if !patch.is_empty() {
                let comp_val = match crate::item::data_result_into(
                    data_component_patch_codec::<Ops>().encode_start(ops, &patch),
                ) {
                    Ok(v) => v,
                    Err(err) => return DataResult::error(err),
                };
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string(ItemStack::FIELD_COMPONENTS.to_string()),
                    comp_val,
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map = match crate::item::data_result_into(ops.get_map(input)) {
                Ok(m) => m,
                Err(err) => return DataResult::error(err),
            };
            let Some(id_raw) = map.get_string(ItemStack::FIELD_ID) else {
                return DataResult::error(format!(
                    "Missing required field '{}'",
                    ItemStack::FIELD_ID
                ));
            };
            let item = match crate::item::data_result_into(item_codec::<Ops>().parse(ops, &id_raw))
            {
                Ok(i) => i,
                Err(err) => return DataResult::error(err),
            };
            let count = match map.get_string(ItemStack::FIELD_COUNT) {
                Some(count_raw) => match crate::item::data_result_into(
                    codec::int_range::<Ops>(1, ABSOLUTE_MAX_STACK_SIZE).parse(ops, &count_raw),
                ) {
                    Ok(c) => c,
                    Err(err) => return DataResult::error(err),
                },
                None => 1,
            };
            let patch = match map.get_string(ItemStack::FIELD_COMPONENTS) {
                Some(comp_raw) => match crate::item::data_result_into(
                    data_component_patch_codec::<Ops>().parse(ops, &comp_raw),
                ) {
                    Ok(p) => p,
                    Err(err) => return DataResult::error(err),
                },
                None => DataComponentPatch::empty(),
            };
            let stack = ItemStack::from_holder_and_patch(item.holder(), count, &patch);
            validate_components(&stack.components).map(|()| stack)
        },
    )
}

/// Port of `ItemStack.OPTIONAL_CODEC` — encodes empty stacks as `{}` and
/// decodes `{}` as `ItemStack::empty()`.
pub fn item_stack_optional_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ItemStack, Ops>> {
    let inner = item_stack_codec::<Ops>();
    let inner_enc = Arc::clone(&inner);
    crate::item::codec_from_fns(
        "OptionalItemStack",
        move |stack: &ItemStack, ops: &Ops| {
            if stack.is_empty() {
                DataResult::success(ops.empty_map())
            } else {
                inner_enc.encode_start(ops, stack)
            }
        },
        move |ops: &Ops, input: &Ops::Output| {
            if let Ok(entries) = crate::item::data_result_into(ops.get_map_values(input))
                && entries.is_empty()
            {
                return DataResult::success(ItemStack::empty());
            }
            inner.parse(ops, input)
        },
    )
}
