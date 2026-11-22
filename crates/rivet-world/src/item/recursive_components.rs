//! Recursive component SCC members:
//! - `ItemStackTemplate` (`net.minecraft.world.item.ItemStackTemplate`)
//! - `ItemContainerContents` (`net.minecraft.world.item.component.ItemContainerContents`)
//! - `BundleContents` (`net.minecraft.world.item.component.BundleContents`)
//! - `ChargedProjectiles` (`net.minecraft.world.item.component.ChargedProjectiles`)

use std::sync::Arc;

use rivet_serialization::{Codec, DataResult, DynamicOps, codec};

use crate::item::component_map::{DataComponentGetter, DataComponentMap};
use crate::item::component_patch::{
    DataComponentPatch, PatchedDataComponentMap, data_component_patch_codec,
};
use crate::item::component_type::DataComponentTypeId;
use crate::item::item_id::{ABSOLUTE_MAX_STACK_SIZE, ItemHolder, ItemId, ItemLike, item_codec};
use crate::item::item_stack::{ItemInstance, ItemStack, validate_components};

/// `ItemContainerContents.MAX_SIZE` (256).
pub const CONTAINER_MAX_SIZE: usize = 256;

/// `BundleContents.MAX_ITEMS` (256).
pub const BUNDLE_MAX_ITEMS: usize = 256;

/// `ChargedProjectiles.MAX_PROJECTILES` (1024).
pub const CHARGED_PROJECTILES_MAX: usize = 1024;

/// Exact rational fraction `(numerator, denominator)` used for Paper's
/// `BundleContents` weight arithmetic (Apache Commons `Fraction`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fraction {
    pub numerator: i64,
    pub denominator: i64,
}

impl Fraction {
    pub const ZERO: Self = Self {
        numerator: 0,
        denominator: 1,
    };
    pub const ONE: Self = Self {
        numerator: 1,
        denominator: 1,
    };
    pub const BUNDLE_IN_BUNDLE_WEIGHT: Self = Self {
        numerator: 1,
        denominator: 16,
    };

    fn from_i128_reduced(numerator: i128, denominator: i128) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        let (n, d) = if denominator < 0 {
            (numerator.checked_neg()?, denominator.checked_neg()?)
        } else {
            (numerator, denominator)
        };
        if n == 0 {
            return Some(Self::ZERO);
        }
        let g = gcd_u128(n.unsigned_abs(), d.unsigned_abs()) as i128;
        let rn = i64::try_from(n / g).ok()?;
        let rd = i64::try_from(d / g).ok()?;
        Some(Self {
            numerator: rn,
            denominator: rd,
        })
    }

    pub fn new(numerator: i64, denominator: i64) -> Self {
        assert!(denominator != 0, "Fraction denominator must not be zero");
        Self::from_i128_reduced(i128::from(numerator), i128::from(denominator))
            .expect("Fraction overflow")
    }

    pub fn checked_mul_int(self, factor: i64) -> Option<Self> {
        let n = i128::from(self.numerator).checked_mul(i128::from(factor))?;
        Self::from_i128_reduced(n, i128::from(self.denominator))
    }

    pub fn mul_int(self, factor: i64) -> Self {
        self.checked_mul_int(factor)
            .expect("Fraction multiplication overflow")
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        if self.numerator == 0 {
            return Some(other);
        }
        if other.numerator == 0 {
            return Some(self);
        }
        let d1 = i128::from(self.denominator);
        let d2 = i128::from(other.denominator);
        let g = gcd_u128(d1.unsigned_abs(), d2.unsigned_abs()) as i128;
        let d1_red = d1 / g;
        let d2_red = d2 / g;
        let lhs = i128::from(self.numerator).checked_mul(d2_red)?;
        let rhs = i128::from(other.numerator).checked_mul(d1_red)?;
        let num = lhs.checked_add(rhs)?;
        let den = d1_red.checked_mul(d2)?;
        Self::from_i128_reduced(num, den)
    }

    pub fn cmp_one(self) -> std::cmp::Ordering {
        self.numerator.cmp(&self.denominator)
    }
}

impl std::ops::Add for Fraction {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        self.checked_add(other).expect("Fraction addition overflow")
    }
}

fn gcd_u128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a.max(1)
}

/// Port of `net.minecraft.world.item.ItemStackTemplate`.
///
/// Immutable validated blueprint of a non-empty `ItemStack` (`item != AIR`,
/// `count >= 1`), used inside `ItemContainerContents`, `BundleContents`,
/// `ChargedProjectiles`, loot tables, and recipes.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct ItemStackTemplate {
    item: ItemHolder,
    count: i32,
    components: DataComponentPatch,
}

impl ItemStackTemplate {
    /// Constructs a validated `ItemStackTemplate`, mirroring the Paper
    /// canonical constructor (`count != 0` and `!item.is(Items.AIR)`).
    pub fn try_new(
        item: ItemHolder,
        count: i32,
        components: DataComponentPatch,
    ) -> Result<Self, String> {
        if count <= 0 {
            return Err("Item must have positive count".to_string());
        }
        if item.is(ItemId::AIR) {
            return Err("Item must not be minecraft:air".to_string());
        }
        Ok(Self {
            item,
            count,
            components,
        })
    }

    /// Convenience constructor for a single item with an empty patch.
    pub fn of_item(item: impl ItemLike) -> Result<Self, String> {
        Self::try_new(item.as_item().holder(), 1, DataComponentPatch::empty())
    }

    /// Convenience constructor for `count` items with an empty patch.
    pub fn of_item_count(item: impl ItemLike, count: i32) -> Result<Self, String> {
        Self::try_new(item.as_item().holder(), count, DataComponentPatch::empty())
    }

    /// Port of `ItemStackTemplate.fromNonEmptyStack(ItemStack)`.
    pub fn from_non_empty_stack(stack: &ItemStack) -> Result<Self, String> {
        if stack.is_empty() {
            return Err("Stack must not be empty".to_string());
        }
        Ok(Self {
            item: stack.type_holder(),
            count: stack.count(),
            components: stack.components_patch(),
        })
    }

    /// Returns the item holder.
    #[inline]
    pub fn item_holder(&self) -> &ItemHolder {
        &self.item
    }

    /// Returns the `ItemId`.
    #[inline]
    pub fn item(&self) -> ItemId {
        self.item.value()
    }

    /// Returns the component patch.
    #[inline]
    pub fn components_patch(&self) -> &DataComponentPatch {
        &self.components
    }

    /// Port of `ItemStackTemplate.withCount(int)`.
    pub fn with_count(&self, count: i32) -> Result<Self, String> {
        if count == self.count {
            return Ok(self.clone());
        }
        Self::try_new(self.item.clone(), count, self.components.clone())
    }

    /// Effective `PatchedDataComponentMap` for this template.
    pub fn effective_components(&self) -> PatchedDataComponentMap {
        PatchedDataComponentMap::from_patch(self.item.components().clone(), &self.components)
    }

    /// Port of `ItemStackTemplate.create()`.
    ///
    /// Materializes a validated `ItemStack`; returns `ItemStack::empty()` if
    /// component validation fails (matching Paper's `create()`).
    pub fn create(&self) -> ItemStack {
        let patched = self.effective_components();
        if validate_components(&patched).is_error() {
            return ItemStack::empty();
        }
        ItemStack::from_holder_and_patch(self.item.clone(), self.count, &self.components)
    }

    /// Port of `ItemStackTemplate.apply(DataComponentPatch)`.
    pub fn apply(&self, extra_patch: &DataComponentPatch) -> ItemStack {
        let mut stack = self.create();
        if stack.is_empty() {
            return stack;
        }
        stack.apply_components(extra_patch);
        if validate_components(stack.patched_components()).is_error() {
            return ItemStack::empty();
        }
        stack
    }

    /// Port of `ItemStackTemplate.validate()`.
    pub fn validate(&self) -> DataResult<()> {
        let patched = self.effective_components();
        validate_components(&patched).flat_map(|_| {
            let max_stack = patched
                .get_i32(DataComponentTypeId::MAX_STACK_SIZE)
                .unwrap_or(1);
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

impl DataComponentGetter for ItemStackTemplate {
    fn get(&self, component_type: DataComponentTypeId) -> Option<&crate::item::DataComponentValue> {
        match self.components.get(component_type) {
            Some(opt) => opt,
            None => self.item.components().get(component_type),
        }
    }
}

impl ItemInstance for ItemStackTemplate {
    fn item_id(&self) -> ItemId {
        self.item.value()
    }

    fn count(&self) -> i32 {
        self.count
    }
}

/// Port of `ItemStackTemplate.CODEC` (`id`, `count` defaulting to 1 in `1..=99`,
/// `components` defaulting to empty).
pub fn item_stack_template_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ItemStackTemplate, Ops>> {
    crate::item::codec_from_fns(
        "ItemStackTemplate",
        |tmpl: &ItemStackTemplate, ops: &Ops| {
            let id_val = match crate::item::data_result_into(
                item_codec::<Ops>().encode_start(ops, &tmpl.item()),
            ) {
                Ok(v) => v,
                Err(err) => return DataResult::error(err),
            };
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string(ItemStack::FIELD_ID.to_string()),
                id_val,
            )];
            if tmpl.count != 1 {
                let count_val = match crate::item::data_result_into(
                    codec::int_range::<Ops>(1, ABSOLUTE_MAX_STACK_SIZE)
                        .encode_start(ops, &tmpl.count),
                ) {
                    Ok(v) => v,
                    Err(err) => return DataResult::error(err),
                };
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string(ItemStack::FIELD_COUNT.to_string()),
                    count_val,
                ));
            }
            let effective = tmpl.effective_components();
            if effective.has(DataComponentTypeId::MAX_DAMAGE)
                && effective.get_i32_or(DataComponentTypeId::MAX_STACK_SIZE, 1) > 1
            {
                return DataResult::error("Item cannot be both damageable and stackable");
            }
            let standard = tmpl.item().default_components();
            let patch = if tmpl.item_holder().components() == &standard {
                tmpl.components_patch().clone()
            } else {
                crate::item::item_stack::patch_relative_to_standard_prototype(
                    tmpl.item(),
                    &effective,
                )
            };
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
            // Also accept single-item string form (`"minecraft:stone"`) when decoding.
            if crate::item::data_result_into(ops.get_string_value(input)).is_ok() {
                return item_codec::<Ops>().parse(ops, input).flat_map(|item| {
                    match ItemStackTemplate::try_new(item.holder(), 1, DataComponentPatch::empty())
                    {
                        Ok(t) => DataResult::success(t),
                        Err(e) => DataResult::error(e),
                    }
                });
            }

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
            match ItemStackTemplate::try_new(item.holder(), count, patch) {
                Ok(tmpl) => {
                    let effective = tmpl.effective_components();
                    if effective.has(DataComponentTypeId::MAX_DAMAGE)
                        && effective.get_i32_or(DataComponentTypeId::MAX_STACK_SIZE, 1) > 1
                    {
                        return DataResult::error("Item cannot be both damageable and stackable");
                    }
                    DataResult::success(tmpl)
                }
                Err(err) => DataResult::error(err),
            }
        },
    )
}

/// Port of `net.minecraft.world.item.component.ItemContainerContents`.
///
/// Bounded list (at most 256 slots) of `Option<ItemStackTemplate>`, with
/// trailing empty slots trimmed (`fromItems`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct ItemContainerContents {
    items: Vec<Option<ItemStackTemplate>>,
}

impl ItemContainerContents {
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    /// Constructs from a slot list, rejecting sizes > 256.
    pub fn try_from_slots(items: Vec<Option<ItemStackTemplate>>) -> Result<Self, String> {
        if items.len() > CONTAINER_MAX_SIZE {
            return Err(format!(
                "Got {} items, but maximum is {}",
                items.len(),
                CONTAINER_MAX_SIZE
            ));
        }
        Ok(Self { items })
    }

    /// Port of `ItemContainerContents.fromItems(List<ItemStack>)`.
    ///
    /// Trims trailing empty stacks; intermediate empty slots remain `None`.
    pub fn from_items(stacks: &[ItemStack]) -> Result<Self, String> {
        let last_non_empty = stacks.iter().rposition(|s| !s.is_empty());
        let Some(last_idx) = last_non_empty else {
            return Ok(Self::empty());
        };
        let len = last_idx + 1;
        if len > CONTAINER_MAX_SIZE {
            return Err(format!(
                "Got {len} items, but maximum is {CONTAINER_MAX_SIZE}"
            ));
        }
        let mut items = Vec::with_capacity(len);
        for stack in &stacks[..len] {
            if stack.is_empty() {
                items.push(None);
            } else {
                items.push(Some(ItemStackTemplate::from_non_empty_stack(stack)?));
            }
        }
        Ok(Self { items })
    }

    /// Returns all slots (including intermediate `None` holes).
    #[inline]
    pub fn slots(&self) -> &[Option<ItemStackTemplate>] {
        &self.items
    }

    /// Returns `true` if there are no stored items.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterates over non-empty item templates in slot order (`nonEmptyItems()`).
    pub fn non_empty_items(&self) -> impl Iterator<Item = &ItemStackTemplate> {
        self.items.iter().flatten()
    }

    /// Copies stored templates into `target` inventory slots (`copyInto`).
    pub fn copy_into(&self, target: &mut [ItemStack]) {
        for (idx, slot) in target.iter_mut().enumerate() {
            *slot = self
                .items
                .get(idx)
                .and_then(|opt| opt.as_ref())
                .map(ItemStackTemplate::create)
                .unwrap_or_else(ItemStack::empty);
        }
    }
}

/// Port of `ItemContainerContents.CODEC` (sparse list of `{slot: 0..=255, item: ItemStackTemplate}`).
pub fn item_container_contents_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ItemContainerContents, Ops>> {
    crate::item::codec_from_fns(
        "ItemContainerContents",
        |contents: &ItemContainerContents, ops: &Ops| {
            let tmpl_codec = item_stack_template_codec::<Ops>();
            let mut list = Vec::new();
            for (slot_idx, slot) in contents.items.iter().enumerate() {
                if let Some(tmpl) = slot {
                    let encoded_item =
                        match crate::item::data_result_into(tmpl_codec.encode_start(ops, tmpl)) {
                            Ok(v) => v,
                            Err(err) => return DataResult::error(err),
                        };
                    let entry = ops.create_map(vec![
                        rivet_serialization::Pair::of(
                            ops.create_string("slot".to_string()),
                            ops.create_int(slot_idx as i32),
                        ),
                        rivet_serialization::Pair::of(
                            ops.create_string("item".to_string()),
                            encoded_item,
                        ),
                    ]);
                    list.push(entry);
                }
            }
            DataResult::success(ops.create_list(list))
        },
        |ops: &Ops, input: &Ops::Output| {
            let tmpl_codec = item_stack_template_codec::<Ops>();
            let raw_list = match crate::item::data_result_into(ops.get_stream(input)) {
                Ok(l) => l,
                Err(err) => return DataResult::error(err),
            };
            if raw_list.len() > CONTAINER_MAX_SIZE {
                return DataResult::error(format!(
                    "Container has {} entries, maximum is {}",
                    raw_list.len(),
                    CONTAINER_MAX_SIZE
                ));
            }
            let mut parsed: Vec<(usize, ItemStackTemplate)> = Vec::with_capacity(raw_list.len());
            let mut max_slot: Option<usize> = None;
            for entry in &raw_list {
                let map = match crate::item::data_result_into(ops.get_map(entry)) {
                    Ok(m) => m,
                    Err(err) => return DataResult::error(err),
                };
                let Some(slot_raw) = map.get_string("slot") else {
                    return DataResult::error("Missing 'slot' in ItemContainerContents entry");
                };
                let slot_i32 = match crate::item::data_result_into(
                    codec::int_range::<Ops>(0, (CONTAINER_MAX_SIZE - 1) as i32)
                        .parse(ops, &slot_raw),
                ) {
                    Ok(s) => s,
                    Err(err) => return DataResult::error(err),
                };
                let Some(item_raw) = map.get_string("item") else {
                    return DataResult::error("Missing 'item' in ItemContainerContents entry");
                };
                let tmpl = match crate::item::data_result_into(tmpl_codec.parse(ops, &item_raw)) {
                    Ok(t) => t,
                    Err(err) => return DataResult::error(err),
                };
                let slot = slot_i32 as usize;
                max_slot = Some(max_slot.map_or(slot, |cur| cur.max(slot)));
                parsed.push((slot, tmpl));
            }
            let Some(last) = max_slot else {
                return DataResult::success(ItemContainerContents::empty());
            };
            let mut slots = vec![None; last + 1];
            for (slot, tmpl) in parsed {
                slots[slot] = Some(tmpl);
            }
            match ItemContainerContents::try_from_slots(slots) {
                Ok(c) => DataResult::success(c),
                Err(err) => DataResult::error(err),
            }
        },
    )
}

/// Port of `net.minecraft.world.item.component.BundleContents`.
///
/// Bounded list (at most 256) of `ItemStackTemplate`s with exact rational
/// `weight()` matching Paper (`1/16` overhead for nested bundles, `1` for
/// non-empty beehives, otherwise `1 / maxStackSize`).
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct BundleContents {
    items: Vec<ItemStackTemplate>,
}

impl BundleContents {
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    pub fn try_new(items: Vec<ItemStackTemplate>) -> Result<Self, String> {
        if items.len() > BUNDLE_MAX_ITEMS {
            return Err(format!(
                "Got {} items in bundle, maximum is {}",
                items.len(),
                BUNDLE_MAX_ITEMS
            ));
        }
        Ok(Self { items })
    }

    #[inline]
    pub fn items(&self) -> &[ItemStackTemplate] {
        &self.items
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Port of `BundleContents.getWeight(ItemInstance)`, returning a
    /// `DataResult` error on rational overflow (`"Excessive total bundle weight"`).
    pub fn item_weight_checked(item: &impl ItemInstance) -> DataResult<Fraction> {
        if let Some(nested) = item
            .get(DataComponentTypeId::BUNDLE_CONTENTS)
            .and_then(|v| v.as_bundle_contents())
        {
            return nested.weight_checked().flat_map(|w| {
                match Fraction::BUNDLE_IN_BUNDLE_WEIGHT.checked_add(w) {
                    Some(total) => DataResult::success(total),
                    None => DataResult::error("Excessive total bundle weight"),
                }
            });
        }
        if let Some(bees) = item
            .get(DataComponentTypeId::BEES)
            .and_then(|v| v.as_bees())
            && !bees.is_empty()
        {
            return DataResult::success(Fraction::ONE);
        }
        DataResult::success(Fraction::new(1, i64::from(item.max_stack_size().max(1))))
    }

    /// Port of `BundleContents.getWeight(ItemInstance)`.
    pub fn item_weight(item: &impl ItemInstance) -> Fraction {
        Self::item_weight_checked(item)
            .result()
            .copied()
            .expect("Excessive total bundle weight")
    }

    /// Port of `BundleContents.weight()` returning `DataResult<Fraction>`.
    pub fn weight_checked(&self) -> DataResult<Fraction> {
        let mut total = Fraction::ZERO;
        for tmpl in &self.items {
            let item_w = match crate::item::data_result_into(Self::item_weight_checked(tmpl)) {
                Ok(w) => w,
                Err(err) => return DataResult::error(err),
            };
            let Some(scaled) = item_w.checked_mul_int(i64::from(tmpl.count())) else {
                return DataResult::error("Excessive total bundle weight");
            };
            let Some(next) = total.checked_add(scaled) else {
                return DataResult::error("Excessive total bundle weight");
            };
            total = next;
        }
        DataResult::success(total)
    }

    /// Port of `BundleContents.weight()`.
    pub fn weight(&self) -> Fraction {
        self.weight_checked()
            .result()
            .copied()
            .expect("Excessive total bundle weight")
    }

    /// Port of `BundleContents.canItemBeInBundle(ItemStack)`.
    pub fn can_item_be_in_bundle(stack: &ItemStack) -> bool {
        !stack.is_empty() && stack.can_fit_inside_container_items()
    }

    /// Validates that total weight does not overflow or exceed `1` (full bundle capacity).
    pub fn validate(&self) -> DataResult<()> {
        if self.items.len() > BUNDLE_MAX_ITEMS {
            return DataResult::error(format!(
                "Bundle has {} items, maximum is {}",
                self.items.len(),
                BUNDLE_MAX_ITEMS
            ));
        }
        self.weight_checked().flat_map(|w| {
            if w.cmp_one().is_gt() {
                DataResult::error("Bundle weight exceeds capacity (1)")
            } else {
                DataResult::success(())
            }
        })
    }
}

/// Port of `BundleContents.CODEC` (`ItemStackTemplate.CODEC.sizeLimitedListOf(256).xmap(...)`).
pub fn bundle_contents_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<BundleContents, Ops>> {
    let list_codec = codec::list_with_range(
        item_stack_template_codec::<Ops>(),
        0,
        BUNDLE_MAX_ITEMS as i32,
    );
    codec::flat_xmap(
        list_codec,
        Arc::new(
            |items: &Vec<ItemStackTemplate>| match BundleContents::try_new(items.clone()) {
                Ok(b) => DataResult::success(b),
                Err(err) => DataResult::error(err),
            },
        ),
        Arc::new(|bundle: &BundleContents| DataResult::success(bundle.items.clone())),
    )
}

/// Port of `net.minecraft.world.item.component.ChargedProjectiles`.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Default)]
pub struct ChargedProjectiles {
    items: Vec<ItemStackTemplate>,
}

impl ChargedProjectiles {
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    pub fn try_new(items: Vec<ItemStackTemplate>) -> Result<Self, String> {
        if items.len() > CHARGED_PROJECTILES_MAX {
            return Err(format!(
                "Got {} charged projectiles, maximum is {}",
                items.len(),
                CHARGED_PROJECTILES_MAX
            ));
        }
        Ok(Self { items })
    }

    /// Port of `ChargedProjectiles.ofNonEmpty(List<ItemStack>)`.
    ///
    /// Filters empty stacks and truncates to `CHARGED_PROJECTILES_MAX` (1024).
    pub fn of_non_empty(stacks: &[ItemStack]) -> Self {
        let mut items = Vec::new();
        for stack in stacks {
            if !stack.is_empty()
                && let Ok(tmpl) = ItemStackTemplate::from_non_empty_stack(stack)
            {
                items.push(tmpl);
                if items.len() >= CHARGED_PROJECTILES_MAX {
                    break;
                }
            }
        }
        Self { items }
    }

    #[inline]
    pub fn items(&self) -> &[ItemStackTemplate] {
        &self.items
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Port of `ChargedProjectiles.contains(Item)`.
    pub fn contains(&self, item: ItemId) -> bool {
        self.items.iter().any(|t| t.item() == item)
    }
}

/// Port of `ChargedProjectiles.CODEC`.
pub fn charged_projectiles_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ChargedProjectiles, Ops>> {
    let list_codec = codec::list_with_range(
        item_stack_template_codec::<Ops>(),
        0,
        CHARGED_PROJECTILES_MAX as i32,
    );
    codec::flat_xmap(
        list_codec,
        Arc::new(|items: &Vec<ItemStackTemplate>| {
            match ChargedProjectiles::try_new(items.clone()) {
                Ok(c) => DataResult::success(c),
                Err(err) => DataResult::error(err),
            }
        }),
        Arc::new(|proj: &ChargedProjectiles| DataResult::success(proj.items.clone())),
    )
}

/// Helper to build a `DataComponentMap` with an empty-safe prototype lookup.
#[allow(dead_code)]
pub(crate) fn empty_component_map() -> DataComponentMap {
    DataComponentMap::empty()
}
