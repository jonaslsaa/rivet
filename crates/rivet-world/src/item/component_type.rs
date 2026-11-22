//! Port of `net.minecraft.core.component.DataComponentType`,
//! `net.minecraft.core.component.DataComponents` identity seam, and
//! `net.minecraft.core.component.TypedDataComponent`.
//!
//! All 111 vanilla data component types are sourced from
//! `rivet_registry::generated::registries::{DATA_COMPONENT_TYPE_BY_ID, DATA_COMPONENT_TYPE_BY_NAME}`.

use std::fmt;
use std::hash::{Hash, Hasher};

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::generated::registries::{
    DATA_COMPONENT_TYPE_BY_ID, DATA_COMPONENT_TYPE_BY_NAME,
};
use rivet_registry::identifier::identifier_codec;
use rivet_serialization::{DataResult, DynamicOps, Unit, codec, extra_codecs};

use crate::item::component_patch::PatchedDataComponentMap;
use crate::item::recursive_components::{
    BundleContents, ChargedProjectiles, ItemContainerContents, bundle_contents_codec,
    charged_projectiles_codec, item_container_contents_codec,
};

/// Compact identity handle for a `minecraft:data_component_type` entry.
///
/// Equality and hashing are strictly by registry identity (`u16` ID), matching
/// Paper's `Reference2ObjectMap` / identity-keyed `DataComponentType<?>` maps.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DataComponentTypeId(pub u16);

impl DataComponentTypeId {
    /// Total number of registered vanilla data component types.
    pub const COUNT: usize = DATA_COMPONENT_TYPE_BY_ID.len();

    // Core built-in component IDs matching `DATA_COMPONENT_TYPE_BY_ID` (verified by unit test).
    pub const CUSTOM_DATA: Self = Self(0);
    pub const MAX_STACK_SIZE: Self = Self(1);
    pub const MAX_DAMAGE: Self = Self(2);
    pub const DAMAGE: Self = Self(3);
    pub const UNBREAKABLE: Self = Self(4);
    pub const USE_EFFECTS: Self = Self(5);
    pub const CUSTOM_NAME: Self = Self(6);
    pub const MINIMUM_ATTACK_CHARGE: Self = Self(7);
    pub const DAMAGE_TYPE: Self = Self(8);
    pub const ITEM_NAME: Self = Self(9);
    pub const ITEM_MODEL: Self = Self(10);
    pub const LORE: Self = Self(11);
    pub const RARITY: Self = Self(12);
    pub const ENCHANTMENTS: Self = Self(13);
    pub const CAN_PLACE_ON: Self = Self(14);
    pub const CAN_BREAK: Self = Self(15);
    pub const ATTRIBUTE_MODIFIERS: Self = Self(16);
    pub const CUSTOM_MODEL_DATA: Self = Self(17);
    pub const TOOLTIP_DISPLAY: Self = Self(18);
    pub const REPAIR_COST: Self = Self(19);
    pub const CREATIVE_SLOT_LOCK: Self = Self(20);
    pub const ENCHANTMENT_GLINT_OVERRIDE: Self = Self(21);
    pub const INTANGIBLE_PROJECTILE: Self = Self(22);
    pub const FOOD: Self = Self(23);
    pub const CONSUMABLE: Self = Self(24);
    pub const USE_REMAINDER: Self = Self(25);
    pub const USE_COOLDOWN: Self = Self(26);
    pub const DAMAGE_RESISTANT: Self = Self(27);
    pub const TOOL: Self = Self(28);
    pub const WEAPON: Self = Self(29);
    pub const ENCHANTABLE: Self = Self(31);
    pub const EQUIPPABLE: Self = Self(32);
    pub const REPAIRABLE: Self = Self(33);
    pub const GLIDER: Self = Self(34);
    pub const TOOLTIP_STYLE: Self = Self(35);
    pub const ADDITIONAL_TRADE_COST: Self = Self(41);
    pub const STORED_ENCHANTMENTS: Self = Self(42);
    pub const MAP_POST_PROCESSING: Self = Self(48);
    pub const CHARGED_PROJECTILES: Self = Self(49);
    pub const BUNDLE_CONTENTS: Self = Self(50);
    pub const ENTITY_DATA: Self = Self(58);
    pub const BLOCK_ENTITY_DATA: Self = Self(60);
    pub const CONTAINER: Self = Self(75);
    pub const BLOCK_STATE: Self = Self(76);
    pub const BEES: Self = Self(77);
    pub const LOCK: Self = Self(79);
    pub const CONTAINER_LOOT: Self = Self(80);
    pub const BREAK_SOUND: Self = Self(81);

    /// Constructs a `DataComponentTypeId` from a raw registry ID if valid.
    #[inline]
    pub const fn from_id(id: u16) -> Option<Self> {
        if (id as usize) < DATA_COMPONENT_TYPE_BY_ID.len() {
            Some(Self(id))
        } else {
            None
        }
    }

    /// Looks up a component type by namespaced name (`"minecraft:damage"`) or
    /// default-namespace path (`"damage"`).
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(&id) = DATA_COMPONENT_TYPE_BY_NAME.get(name) {
            return Some(Self(id));
        }
        let parsed = Identifier::try_parse(name)?;
        Self::from_identifier(&parsed)
    }

    /// Looks up a component type by `Identifier`.
    pub fn from_identifier(id: &Identifier) -> Option<Self> {
        let full = id.to_string();
        DATA_COMPONENT_TYPE_BY_NAME
            .get(full.as_str())
            .copied()
            .map(Self)
    }

    /// Returns the registered namespaced string (e.g. `"minecraft:damage"`).
    #[inline]
    pub fn name(self) -> Option<&'static str> {
        DATA_COMPONENT_TYPE_BY_ID.get(self.0 as usize).copied()
    }

    /// Returns the `Identifier` for this component type.
    pub fn identifier(self) -> Option<Identifier> {
        self.name().map(Identifier::parse)
    }

    /// Returns the raw numeric registry ID.
    #[inline]
    pub const fn id(self) -> u16 {
        self.0
    }

    /// Iterates over every generated vanilla data component type in ID order.
    pub fn iter() -> impl ExactSizeIterator<Item = DataComponentTypeId> {
        (0..DATA_COMPONENT_TYPE_BY_ID.len() as u16).map(DataComponentTypeId)
    }

    /// Port of `DataComponentType.isTransient()`.
    ///
    /// In Paper 26.2 (`DataComponents.java`), exactly three built-in component
    /// types have no persistent codec (`codec() == null`):
    /// - `minecraft:creative_slot_lock`
    /// - `minecraft:additional_trade_cost`
    /// - `minecraft:map_post_processing`
    #[inline]
    pub const fn is_transient(self) -> bool {
        matches!(
            self,
            Self::CREATIVE_SLOT_LOCK | Self::ADDITIONAL_TRADE_COST | Self::MAP_POST_PROCESSING
        )
    }

    /// Port of `DataComponentType.ignoreSwapAnimation()`.
    ///
    /// In Paper 26.2 (`DataComponents.java`), only `minecraft:damage` sets
    /// `ignoreSwapAnimation()`.
    #[inline]
    pub const fn ignore_swap_animation(self) -> bool {
        self.0 == Self::DAMAGE.0
    }

    /// Java-compatible identity hash code.
    #[inline]
    pub const fn java_hash_code(self) -> i32 {
        self.0 as i32
    }
}

impl fmt::Display for DataComponentTypeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "<unregistered:{}>", self.0),
        }
    }
}

/// Port of `net.minecraft.world.item.Rarity`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

impl Rarity {
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Common => "common",
            Self::Uncommon => "uncommon",
            Self::Rare => "rare",
            Self::Epic => "epic",
        }
    }

    pub fn from_serialized_name(s: &str) -> Option<Self> {
        match s {
            "common" => Some(Self::Common),
            "uncommon" => Some(Self::Uncommon),
            "rare" => Some(Self::Rare),
            "epic" => Some(Self::Epic),
            _ => None,
        }
    }

    pub const fn java_hash_code(self) -> i32 {
        self as i32
    }
}

impl fmt::Display for Rarity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.serialized_name())
    }
}

/// Typed component value carrier for `DataComponentMap`, `DataComponentPatch`,
/// and `ItemStack`.
///
/// Core item-stack, durability, identity, and recursive SCC components carry
/// strongly-typed Rust values. Additional leaf data-driven components preserve
/// exact `Tag` payloads via `RawNbt` so all 111 generated component types
/// round-trip losslessly.
#[derive(Clone, Debug)]
pub enum DataComponentValue {
    /// `minecraft:custom_data` (`CustomData`).
    CustomData(CompoundTag),
    /// Integer-valued components (`max_stack_size`, `max_damage`, `damage`, `repair_cost`, etc.).
    Int(i32),
    /// Float-valued components (`minimum_attack_charge`, `potion_duration_scale`, etc.).
    Float(f32),
    /// Boolean-valued components (`enchantment_glint_override`, etc.).
    Bool(bool),
    /// Unit-valued marker components (`unbreakable`, `creative_slot_lock`, `glider`, `intangible_projectile`).
    Unit,
    /// Identifier-valued components (`item_model`, `tooltip_style`, `break_sound`, etc.).
    Identifier(Identifier),
    /// Text/string-valued components (`custom_name`, `item_name`).
    Text(String),
    /// `minecraft:rarity`.
    Rarity(Rarity),
    /// `minecraft:container` (`ItemContainerContents`) — recursive SCC member.
    Container(ItemContainerContents),
    /// `minecraft:bundle_contents` (`BundleContents`) — recursive SCC member.
    BundleContents(BundleContents),
    /// `minecraft:charged_projectiles` (`ChargedProjectiles`) — recursive SCC member.
    ChargedProjectiles(ChargedProjectiles),
    /// `minecraft:bees` (`Bees`) — list of bee entity entries (used by `BundleContents::get_weight`).
    Bees(Vec<CompoundTag>),
    /// Generic lossless NBT value for leaf component types not yet specialized.
    RawNbt(Tag),
}

impl PartialEq for DataComponentValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::CustomData(a), Self::CustomData(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            (Self::Float(a), Self::Float(b)) => a.to_bits() == b.to_bits(),
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Unit, Self::Unit) => true,
            (Self::Identifier(a), Self::Identifier(b)) => a == b,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Rarity(a), Self::Rarity(b)) => a == b,
            (Self::Container(a), Self::Container(b)) => a == b,
            (Self::BundleContents(a), Self::BundleContents(b)) => a == b,
            (Self::ChargedProjectiles(a), Self::ChargedProjectiles(b)) => a == b,
            (Self::Bees(a), Self::Bees(b)) => a == b,
            (Self::RawNbt(a), Self::RawNbt(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for DataComponentValue {}

fn hash_compound<H: Hasher>(tag: &CompoundTag, state: &mut H) {
    // Order-independent hash matching `CompoundTag` equality (`HashMap`/`BTreeMap`).
    let mut acc: u64 = 0;
    for (k, v) in tag.entry_set() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        k.hash(&mut h);
        hash_tag(v, &mut h);
        acc = acc.wrapping_add(h.finish());
    }
    acc.hash(state);
}

fn hash_tag<H: Hasher>(tag: &Tag, state: &mut H) {
    tag.id().hash(state);
    match tag {
        Tag::End(_) => {}
        Tag::Byte(t) => t.value.hash(state),
        Tag::Short(t) => t.value.hash(state),
        Tag::Int(t) => t.value.hash(state),
        Tag::Long(t) => t.value.hash(state),
        Tag::Float(t) => t.value.to_bits().hash(state),
        Tag::Double(t) => t.value.to_bits().hash(state),
        Tag::ByteArray(t) => t.data.hash(state),
        Tag::String(t) => t.value.hash(state),
        Tag::List(t) => {
            t.list.len().hash(state);
            for item in &t.list {
                hash_tag(item, state);
            }
        }
        Tag::Compound(c) => hash_compound(c, state),
        Tag::IntArray(t) => t.data.hash(state),
        Tag::LongArray(t) => t.data.hash(state),
    }
}

impl Hash for DataComponentValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::CustomData(tag) => hash_compound(tag, state),
            Self::Int(v) => v.hash(state),
            Self::Float(v) => v.to_bits().hash(state),
            Self::Bool(v) => v.hash(state),
            Self::Unit => {}
            Self::Identifier(id) => id.hash(state),
            Self::Text(s) => s.hash(state),
            Self::Rarity(r) => r.hash(state),
            Self::Container(c) => c.hash(state),
            Self::BundleContents(b) => b.hash(state),
            Self::ChargedProjectiles(p) => p.hash(state),
            Self::Bees(bees) => {
                bees.len().hash(state);
                for bee in bees {
                    hash_compound(bee, state);
                }
            }
            Self::RawNbt(tag) => hash_tag(tag, state),
        }
    }
}

impl DataComponentValue {
    /// Returns a deterministic 32-bit Java-style hash code for this value,
    /// used by `DataComponentMap`, `PatchedDataComponentMap`, and
    /// `ItemStackLinkedSet::hash_item_and_components`.
    pub fn java_hash_code(&self) -> i32 {
        use std::collections::hash_map::DefaultHasher;
        match self {
            Self::Int(v) => *v,
            Self::Float(v) => v.to_bits() as i32,
            Self::Bool(b) => {
                if *b {
                    1231
                } else {
                    1237
                }
            }
            Self::Unit => 0,
            Self::Rarity(r) => r.java_hash_code(),
            other => {
                let mut h = DefaultHasher::new();
                other.hash(&mut h);
                let bits = h.finish();
                ((bits ^ (bits >> 32)) & 0xFFFF_FFFF) as i32
            }
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        match self {
            Self::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_custom_data(&self) -> Option<&CompoundTag> {
        match self {
            Self::CustomData(tag) => Some(tag),
            _ => None,
        }
    }

    pub fn as_identifier(&self) -> Option<&Identifier> {
        match self {
            Self::Identifier(id) => Some(id),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_rarity(&self) -> Option<Rarity> {
        match self {
            Self::Rarity(r) => Some(*r),
            _ => None,
        }
    }

    pub fn as_container(&self) -> Option<&ItemContainerContents> {
        match self {
            Self::Container(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_bundle_contents(&self) -> Option<&BundleContents> {
        match self {
            Self::BundleContents(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_charged_projectiles(&self) -> Option<&ChargedProjectiles> {
        match self {
            Self::ChargedProjectiles(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_bees(&self) -> Option<&[CompoundTag]> {
        match self {
            Self::Bees(bees) => Some(bees),
            _ => None,
        }
    }
}

impl fmt::Display for DataComponentValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CustomData(tag) => write!(f, "{tag:?}"),
            Self::Int(v) => write!(f, "{v}"),
            Self::Float(v) => write!(f, "{v}"),
            Self::Bool(v) => write!(f, "{v}"),
            Self::Unit => f.write_str("UNIT"),
            Self::Identifier(id) => write!(f, "{id}"),
            Self::Text(s) => f.write_str(s),
            Self::Rarity(r) => write!(f, "{r}"),
            Self::Container(c) => write!(f, "{c:?}"),
            Self::BundleContents(b) => write!(f, "{b:?}"),
            Self::ChargedProjectiles(p) => write!(f, "{p:?}"),
            Self::Bees(bees) => write!(f, "Bees({})", bees.len()),
            Self::RawNbt(tag) => write!(f, "{tag:?}"),
        }
    }
}

/// Port of `net.minecraft.core.component.TypedDataComponent<T>`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct TypedDataComponent {
    pub component_type: DataComponentTypeId,
    pub value: DataComponentValue,
}

impl TypedDataComponent {
    pub fn new(component_type: DataComponentTypeId, value: DataComponentValue) -> Self {
        Self {
            component_type,
            value,
        }
    }

    /// `TypedDataComponent.applyTo(PatchedDataComponentMap)`.
    pub fn apply_to(&self, components: &mut PatchedDataComponentMap) {
        components.set(self.component_type, self.value.clone());
    }

    /// `TypedDataComponent.encodeValue(DynamicOps)`.
    pub fn encode_value<Ops: DynamicOps + 'static>(&self, ops: &Ops) -> DataResult<Ops::Output> {
        if self.component_type.is_transient() {
            return DataResult::error(format!(
                "Component of type {} is not encodable",
                self.component_type
            ));
        }
        encode_component_value(self.component_type, &self.value, ops)
    }
}

impl fmt::Display for TypedDataComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}=>{}", self.component_type, self.value)
    }
}

/// Encodes a `DataComponentValue` using the Paper-faithful persistent codec for
/// `component_type`.
pub fn encode_component_value<Ops: DynamicOps + 'static>(
    component_type: DataComponentTypeId,
    value: &DataComponentValue,
    ops: &Ops,
) -> DataResult<Ops::Output> {
    if component_type.is_transient() {
        return DataResult::error(format!(
            "Component of type {component_type} is not encodable"
        ));
    }

    match component_type {
        DataComponentTypeId::MAX_STACK_SIZE => {
            let Some(v) = value.as_i32() else {
                return DataResult::error(format!("Expected integer value for {component_type}"));
            };
            codec::int_range::<Ops>(1, 99).encode_start(ops, &v)
        }
        DataComponentTypeId::MAX_DAMAGE => {
            let Some(v) = value.as_i32() else {
                return DataResult::error(format!("Expected integer value for {component_type}"));
            };
            codec::int_range::<Ops>(1, i32::MAX).encode_start(ops, &v)
        }
        DataComponentTypeId::DAMAGE | DataComponentTypeId::REPAIR_COST => {
            let Some(v) = value.as_i32() else {
                return DataResult::error(format!("Expected integer value for {component_type}"));
            };
            extra_codecs::non_negative_int_codec::<Ops>().encode_start(ops, &v)
        }
        DataComponentTypeId::UNBREAKABLE
        | DataComponentTypeId::GLIDER
        | DataComponentTypeId::INTANGIBLE_PROJECTILE => {
            if !matches!(value, DataComponentValue::Unit) {
                return DataResult::error(format!("Expected Unit value for {component_type}"));
            }
            codec::empty::<Ops>().encode_start(ops, &Unit)
        }
        DataComponentTypeId::ENCHANTMENT_GLINT_OVERRIDE => {
            let Some(b) = value.as_bool() else {
                return DataResult::error(format!("Expected boolean value for {component_type}"));
            };
            codec::bool_codec::<Ops>().encode_start(ops, &b)
        }
        DataComponentTypeId::ITEM_MODEL
        | DataComponentTypeId::TOOLTIP_STYLE
        | DataComponentTypeId::BREAK_SOUND => {
            let Some(id) = value.as_identifier() else {
                return DataResult::error(format!(
                    "Expected Identifier value for {component_type}"
                ));
            };
            identifier_codec::<Ops>().encode_start(ops, id)
        }
        DataComponentTypeId::CUSTOM_NAME | DataComponentTypeId::ITEM_NAME => {
            let Some(s) = value.as_text() else {
                return DataResult::error(format!("Expected text value for {component_type}"));
            };
            codec::string_codec::<Ops>().encode_start(ops, &s.to_string())
        }
        DataComponentTypeId::RARITY => {
            let Some(r) = value.as_rarity() else {
                return DataResult::error(format!("Expected Rarity value for {component_type}"));
            };
            codec::string_codec::<Ops>().encode_start(ops, &r.serialized_name().to_string())
        }
        DataComponentTypeId::CUSTOM_DATA => {
            let Some(compound) = value.as_custom_data() else {
                return DataResult::error(format!(
                    "Expected CompoundTag value for {component_type}"
                ));
            };
            DataResult::success(
                NbtOps::instance().convert_to(ops, &Tag::Compound(compound.clone())),
            )
        }
        DataComponentTypeId::CONTAINER => {
            let Some(container) = value.as_container() else {
                return DataResult::error(format!(
                    "Expected ItemContainerContents value for {component_type}"
                ));
            };
            item_container_contents_codec::<Ops>().encode_start(ops, container)
        }
        DataComponentTypeId::BUNDLE_CONTENTS => {
            let Some(bundle) = value.as_bundle_contents() else {
                return DataResult::error(format!(
                    "Expected BundleContents value for {component_type}"
                ));
            };
            bundle_contents_codec::<Ops>().encode_start(ops, bundle)
        }
        DataComponentTypeId::CHARGED_PROJECTILES => {
            let Some(proj) = value.as_charged_projectiles() else {
                return DataResult::error(format!(
                    "Expected ChargedProjectiles value for {component_type}"
                ));
            };
            charged_projectiles_codec::<Ops>().encode_start(ops, proj)
        }
        DataComponentTypeId::BEES => {
            let Some(bees) = value.as_bees() else {
                return DataResult::error(format!("Expected Bees value for {component_type}"));
            };
            let nbt_ops = NbtOps::instance();
            let items: Vec<Ops::Output> = bees
                .iter()
                .map(|c| nbt_ops.convert_to(ops, &Tag::Compound(c.clone())))
                .collect();
            DataResult::success(ops.create_list(items))
        }
        _ => match value {
            DataComponentValue::RawNbt(tag) => {
                DataResult::success(NbtOps::instance().convert_to(ops, tag))
            }
            DataComponentValue::Int(v) => codec::int_codec::<Ops>().encode_start(ops, v),
            DataComponentValue::Float(v) => codec::float_codec::<Ops>().encode_start(ops, v),
            DataComponentValue::Bool(v) => codec::bool_codec::<Ops>().encode_start(ops, v),
            DataComponentValue::Unit => codec::empty::<Ops>().encode_start(ops, &Unit),
            DataComponentValue::Identifier(id) => identifier_codec::<Ops>().encode_start(ops, id),
            DataComponentValue::Text(s) => codec::string_codec::<Ops>().encode_start(ops, s),
            DataComponentValue::CustomData(c) => {
                DataResult::success(NbtOps::instance().convert_to(ops, &Tag::Compound(c.clone())))
            }
            _ => DataResult::error(format!(
                "Unsupported value variant for component {component_type}"
            )),
        },
    }
}

/// Decodes a `DataComponentValue` using the Paper-faithful persistent codec for
/// `component_type`.
pub fn decode_component_value<Ops: DynamicOps + 'static>(
    component_type: DataComponentTypeId,
    ops: &Ops,
    input: &Ops::Output,
) -> DataResult<DataComponentValue> {
    if component_type.is_transient() {
        return DataResult::error(format!(
            "Component type {component_type} is transient and cannot be decoded"
        ));
    }

    match component_type {
        DataComponentTypeId::MAX_STACK_SIZE => codec::int_range::<Ops>(1, 99)
            .parse(ops, input)
            .map_owned(DataComponentValue::Int),
        DataComponentTypeId::MAX_DAMAGE => codec::int_range::<Ops>(1, i32::MAX)
            .parse(ops, input)
            .map_owned(DataComponentValue::Int),
        DataComponentTypeId::DAMAGE | DataComponentTypeId::REPAIR_COST => {
            extra_codecs::non_negative_int_codec::<Ops>()
                .parse(ops, input)
                .map_owned(DataComponentValue::Int)
        }
        DataComponentTypeId::UNBREAKABLE
        | DataComponentTypeId::GLIDER
        | DataComponentTypeId::INTANGIBLE_PROJECTILE => codec::empty::<Ops>()
            .parse(ops, input)
            .map_owned(|_| DataComponentValue::Unit),
        DataComponentTypeId::ENCHANTMENT_GLINT_OVERRIDE => codec::bool_codec::<Ops>()
            .parse(ops, input)
            .map_owned(DataComponentValue::Bool),
        DataComponentTypeId::ITEM_MODEL
        | DataComponentTypeId::TOOLTIP_STYLE
        | DataComponentTypeId::BREAK_SOUND => identifier_codec::<Ops>()
            .parse(ops, input)
            .map_owned(DataComponentValue::Identifier),
        DataComponentTypeId::CUSTOM_NAME | DataComponentTypeId::ITEM_NAME => {
            codec::string_codec::<Ops>()
                .parse(ops, input)
                .map_owned(DataComponentValue::Text)
        }
        DataComponentTypeId::RARITY => {
            codec::string_codec::<Ops>()
                .parse(ops, input)
                .flat_map(|s| match Rarity::from_serialized_name(&s) {
                    Some(r) => DataResult::success(DataComponentValue::Rarity(r)),
                    None => DataResult::error(format!("Unknown rarity: {s}")),
                })
        }
        DataComponentTypeId::CUSTOM_DATA => {
            let tag = ops.convert_to(&NbtOps::instance(), input);
            match tag {
                Tag::Compound(c) => DataResult::success(DataComponentValue::CustomData(c)),
                _ => DataResult::error("Not a compound tag for minecraft:custom_data"),
            }
        }
        DataComponentTypeId::CONTAINER => item_container_contents_codec::<Ops>()
            .parse(ops, input)
            .map_owned(DataComponentValue::Container),
        DataComponentTypeId::BUNDLE_CONTENTS => bundle_contents_codec::<Ops>()
            .parse(ops, input)
            .map_owned(DataComponentValue::BundleContents),
        DataComponentTypeId::CHARGED_PROJECTILES => charged_projectiles_codec::<Ops>()
            .parse(ops, input)
            .map_owned(DataComponentValue::ChargedProjectiles),
        DataComponentTypeId::BEES => {
            let nbt_ops = NbtOps::instance();
            ops.get_stream(input).flat_map(|items| {
                let mut bees = Vec::with_capacity(items.len());
                for item in items {
                    match ops.convert_to(&nbt_ops, &item) {
                        Tag::Compound(c) => bees.push(c),
                        _ => return DataResult::error("Expected compound tag in minecraft:bees"),
                    }
                }
                DataResult::success(DataComponentValue::Bees(bees))
            })
        }
        _ => {
            let tag = ops.convert_to(&NbtOps::instance(), input);
            DataResult::success(DataComponentValue::RawNbt(tag))
        }
    }
}
