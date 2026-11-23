//! Shared `BaseContainerBlockEntity` / `RandomizableContainerBlockEntity` state
//! and `ContainerHelper` NBT item serialization helpers.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.level.block.entity.BaseContainerBlockEntity`
//! - `net.minecraft.world.LockCode`
//! - `net.minecraft.world.RandomizableContainer` (`tryLoadLootTable` / `trySaveLootTable`)
//! - `com.destroystokyo.paper.loottable.PaperLootableInventoryData` (`loadNbt` / `saveNbt`)
//! - `net.minecraft.world.ContainerHelper` (`saveAllItems` / `loadAllItems`)

use std::sync::{Arc, OnceLock};

use rivet_nbt::{
    compound_tag::CompoundTag, list_tag::ListTag, nbt_ops::NbtOps,
    string_tag_visitor::StringTagVisitor, tag::Tag, tag_parser::TagParser,
};
use rivet_registry::Identifier;
use rivet_serialization::codec::Codec;
use rivet_text::{Component, component_serialization};

use crate::inventory::{ComponentMap, DEFAULT_ITEM_MAX_STACK_SIZE, InventoryItemStack, ItemId};

/// Prefix used to store non-string NBT component tags losslessly inside
/// `ComponentMap` (`BTreeMap<String, String>`) until #431's typed
/// `DataComponentPatch` lands.
const SNBT_COMPONENT_PREFIX: &str = "\0snbt:";

/// `DataComponents.MAX_STACK_SIZE` patch key.
const MAX_STACK_SIZE_COMPONENT: &str = "minecraft:max_stack_size";

/// Encode an NBT component value into its `ComponentMap` string form: string
/// tags are stored verbatim, every other tag as prefixed SNBT.
pub(crate) fn encode_component_tag(tag: &Tag) -> String {
    match tag {
        Tag::String(s) => s.value.clone(),
        other => format!(
            "{SNBT_COMPONENT_PREFIX}{}",
            StringTagVisitor::to_string(other)
        ),
    }
}

/// Decode a `ComponentMap` value back to its NBT tag (inverse of
/// [`encode_component_tag`]).
pub(crate) fn decode_component_tag(value: &str) -> Tag {
    if let Some(snbt) = value.strip_prefix(SNBT_COMPONENT_PREFIX)
        && let Ok(parsed) = TagParser::create(NbtOps).parse_fully(snbt)
    {
        return parsed;
    }
    Tag::String(rivet_nbt::string_tag::StringTag::value_of(value.to_owned()))
}

/// `Item.getDefaultMaxStackSize()` for an inventory item id.
///
/// Uses the crafting item seam's per-item stack-size table (`1`/`16`/`64`),
/// the shared source the furnace recipe results use as well; the generated
/// item prototypes do not carry per-item `max_stack_size` yet.
pub fn default_max_stack_size(item: ItemId) -> i32 {
    crate::crafting::ItemId::from_raw(item.0).map_or(
        DEFAULT_ITEM_MAX_STACK_SIZE,
        crate::crafting::ItemId::default_max_stack_size,
    )
}

/// `ItemStack.getMaxStackSize()` for `item` carrying `components`: the
/// `minecraft:max_stack_size` patch entry when present, else the item's
/// prototype default.
pub fn max_stack_size_with_components(item: ItemId, components: &ComponentMap) -> i32 {
    components
        .get(MAX_STACK_SIZE_COMPONENT)
        .and_then(|v| match decode_component_tag(v) {
            Tag::Int(i) => Some(i.value),
            _ => v.parse::<i32>().ok(),
        })
        .unwrap_or_else(|| default_max_stack_size(item))
}

/// `new ItemStack(item, count)` with the item's prototype max stack size.
pub fn item_stack(item: ItemId, count: i32) -> InventoryItemStack {
    InventoryItemStack::with_max_stack_size(item, count, default_max_stack_size(item))
}

/// Paper `net.minecraft.world.LockCode` (`record LockCode(ItemPredicate predicate)`).
///
/// The `ItemPredicate` is held as its encoded `ItemPredicate.CODEC` compound
/// (e.g. `{components:{"minecraft:custom_name":"Key"}}`) until an
/// `ItemPredicate` port exists; `None` is `LockCode.NO_LOCK`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LockCode {
    predicate: Option<CompoundTag>,
}

impl LockCode {
    /// `LockCode.NO_LOCK`.
    pub const fn no_lock() -> Self {
        Self { predicate: None }
    }

    /// `new LockCode(ItemPredicate)` from the predicate's encoded compound.
    pub fn new(predicate: CompoundTag) -> Self {
        Self {
            predicate: Some(predicate),
        }
    }

    /// Whether this lock is `LockCode.NO_LOCK`.
    pub fn is_unlocked(&self) -> bool {
        self.predicate.is_none()
    }

    /// Encoded `ItemPredicate` compound if locked.
    pub fn predicate(&self) -> Option<&CompoundTag> {
        self.predicate.as_ref()
    }

    /// `LockCode.addToTag(ValueOutput)` — `if (this != NO_LOCK) store("lock", CODEC, this)`.
    pub fn save(&self, output: &mut CompoundTag) {
        if let Some(predicate) = &self.predicate {
            output.put("lock".into(), Tag::Compound(predicate.clone()));
        }
    }

    /// `LockCode.fromTag(ValueInput)` — `read("lock", CODEC).orElse(NO_LOCK)`.
    ///
    /// `ItemPredicate.CODEC` is a record codec, so only a compound decodes; any
    /// other tag type fails to parse and yields `NO_LOCK`.
    pub fn load(input: &CompoundTag) -> Self {
        match input.get_compound("lock") {
            Some(predicate) => Self::new(predicate.clone()),
            None => Self::no_lock(),
        }
    }
}

/// Paper `PaperLootableInventoryData` persisted under `Paper.LootableData`.
#[derive(Debug, Clone, PartialEq)]
pub struct PaperLootableData {
    pub last_fill: i64,
    pub next_refill: i64,
    pub num_refills: i32,
    /// `lootedPlayers` entries (`{UUID:[I;...], Time:L}`) kept as their encoded
    /// list in load order.
    pub looted_players: ListTag,
}

impl Default for PaperLootableData {
    fn default() -> Self {
        Self {
            last_fill: -1,
            next_refill: -1,
            num_refills: 0,
            looted_players: ListTag::new(),
        }
    }
}

impl PaperLootableData {
    const ROOT: &'static str = "Paper.LootableData";

    /// `PaperLootableInventoryData.loadNbt(ValueInput)`.
    pub fn load(&mut self, input: &CompoundTag) {
        let empty = CompoundTag::new();
        let data = input.get_compound(Self::ROOT).unwrap_or(&empty);
        self.last_fill = data.get_long_or("lastFill", -1);
        self.next_refill = data.get_long_or("nextRefill", -1);
        self.num_refills = data.get_int_or("numRefills", 0);
        if let Some(list) = data.get_list("lootedPlayers")
            && !list.list.is_empty()
        {
            self.looted_players = list.clone();
        }
    }

    /// `PaperLootableInventoryData.saveNbt(ValueOutput)` — writes the child only
    /// when it is non-empty.
    pub fn save(&self, output: &mut CompoundTag) {
        let mut data = CompoundTag::new();
        if self.next_refill != -1 {
            data.put_long("nextRefill", self.next_refill);
        }
        if self.last_fill != -1 {
            data.put_long("lastFill", self.last_fill);
        }
        if self.num_refills != 0 {
            data.put_int("numRefills", self.num_refills);
        }
        if !self.looted_players.list.is_empty() {
            data.put(
                "lootedPlayers".into(),
                Tag::List(self.looted_players.clone()),
            );
        }
        if !data.is_empty() {
            output.put(Self::ROOT.into(), Tag::Compound(data));
        }
    }
}

/// Shared `ComponentSerialization.CODEC` over `NbtOps` (built once).
fn component_codec() -> &'static Arc<dyn Codec<Component, NbtOps>> {
    static CODEC: OnceLock<Arc<dyn Codec<Component, NbtOps>>> = OnceLock::new();
    CODEC.get_or_init(component_serialization::codec::<NbtOps>)
}

/// Shared state for Paper `BaseContainerBlockEntity` and
/// `RandomizableContainerBlockEntity` subclasses.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BaseContainerState {
    pub lock_key: LockCode,
    pub custom_name: Option<Component>,
    pub loot_table: Option<Identifier>,
    pub loot_table_seed: i64,
    /// Paper `RandomizableContainerBlockEntity.lootableData`.
    pub lootable_data: PaperLootableData,
    changed_count: u64,
}

impl BaseContainerState {
    /// Create a fresh unlocked, unnamed container state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the block entity dirty (`BlockEntity.setChanged()`).
    pub fn set_changed(&mut self) {
        self.changed_count = self.changed_count.wrapping_add(1);
    }

    /// Number of times `set_changed()` was called on this block entity.
    pub fn changed_count(&self) -> u64 {
        self.changed_count
    }

    /// `BaseContainerBlockEntity.saveAdditional(ValueOutput)`.
    pub fn save_base(&self, output: &mut CompoundTag) {
        self.lock_key.save(output);
        output.store_nullable("CustomName", component_codec(), self.custom_name.as_ref());
    }

    /// `BaseContainerBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_base(&mut self, input: &CompoundTag) {
        self.lock_key = LockCode::load(input);
        // `BlockEntity.parseCustomNameSafe(input, "CustomName")`.
        self.custom_name = input.read("CustomName", component_codec());
    }

    /// `RandomizableContainer.trySaveLootTable(ValueOutput)` for a
    /// `RandomizableContainerBlockEntity`.
    ///
    /// Paper's block entities always carry `lootableData`, so the method returns
    /// `false` and callers write `Items` unconditionally; it is therefore a
    /// plain writer here.
    pub fn save_loot_table(&self, output: &mut CompoundTag) {
        let Some(loot_table) = &self.loot_table else {
            return;
        };
        output.put_string("LootTable", &loot_table.to_string());
        self.lootable_data.save(output);
        if self.loot_table_seed != 0 {
            output.put_long("LootTableSeed", self.loot_table_seed);
        }
    }

    /// `RandomizableContainer.tryLoadLootTable(ValueInput)` for a
    /// `RandomizableContainerBlockEntity`.
    ///
    /// Paper's block entities always carry `lootableData`, so the method returns
    /// `false` and callers load `Items` unconditionally; it is therefore a plain
    /// reader here.
    pub fn load_loot_table(&mut self, input: &CompoundTag) {
        self.loot_table = input
            .get_string("LootTable")
            .and_then(|id| Identifier::try_parse_result(id).ok().flatten());
        if self.loot_table.is_some() {
            self.lootable_data.load(input);
        }
        self.loot_table_seed = input.get_long_or("LootTableSeed", 0);
    }
}

/// `ContainerHelper.saveAllItems(ValueOutput, NonNullList<ItemStack>, boolean)`.
pub fn save_all_items(
    output: &mut CompoundTag,
    items: &[InventoryItemStack],
    always_put_tag: bool,
) {
    let mut list = ListTag::new();
    for (slot, stack) in items.iter().enumerate() {
        if stack.is_empty() {
            continue;
        }
        let Some(item_name) = stack.item().name() else {
            continue;
        };
        let mut entry = CompoundTag::new();
        entry.put_byte("Slot", slot as i8);
        entry.put_string("id", item_name);
        entry.put_int("count", stack.get_count());
        if !stack.components().is_empty() {
            let mut comp = CompoundTag::new();
            for (k, v) in stack.components() {
                comp.put(k.clone(), decode_component_tag(v));
            }
            entry.put("components".into(), Tag::Compound(comp));
        }
        list.list.push(Tag::Compound(entry));
    }

    if !list.list.is_empty() || always_put_tag {
        output.put("Items".into(), Tag::List(list));
    }
}

/// `ContainerHelper.loadAllItems(ValueInput, NonNullList<ItemStack>)`.
///
/// Like the callers in Paper, the slots are reset first.
pub fn load_all_items(input: &CompoundTag, items: &mut [InventoryItemStack]) {
    for slot in items.iter_mut() {
        *slot = InventoryItemStack::empty();
    }

    let Some(list) = input.get_list("Items") else {
        return;
    };

    for tag in &list.list {
        let Tag::Compound(entry) = tag else {
            continue;
        };
        // `ExtraCodecs.optionalAlwaysPresentFieldOf(UNSIGNED_BYTE, "Slot", 0)`.
        let slot = entry.get_byte_or("Slot", 0) as u8 as usize;
        if slot >= items.len() {
            continue;
        }
        let Some(item_name) = entry.get_string("id") else {
            continue;
        };
        let Some(item_id) = ItemId::from_name(item_name) else {
            continue;
        };
        let count = entry.get_int_or("count", 1);
        let mut map = ComponentMap::new();
        if let Some(comp_tag) = entry.get_compound("components") {
            for (k, v) in &comp_tag.tags {
                map.insert(k.clone(), encode_component_tag(v));
            }
        }
        let max_stack_size = max_stack_size_with_components(item_id, &map);
        let mut stack = InventoryItemStack::with_max_stack_size(item_id, count, max_stack_size);
        if !map.is_empty() {
            stack = stack.with_components(map);
        }
        items[slot] = stack;
    }
}
