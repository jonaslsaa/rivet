//! Shared `BaseContainerBlockEntity` / `RandomizableContainerBlockEntity` state
//! and `ContainerHelper` NBT item serialization helpers.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.level.block.entity.BaseContainerBlockEntity`
//! - `net.minecraft.world.ContainerHelper` (`saveAllItems` / `loadAllItems`)

use rivet_nbt::{
    compound_tag::CompoundTag, list_tag::ListTag, nbt_ops::NbtOps,
    string_tag_visitor::StringTagVisitor, tag::Tag, tag_parser::TagParser,
};
use rivet_registry::Identifier;

use crate::inventory::{ComponentMap, DEFAULT_ITEM_MAX_STACK_SIZE, InventoryItemStack, ItemId};

/// Prefix used to store non-string NBT component tags losslessly inside
/// `ComponentMap` (`BTreeMap<String, String>`) until #431's typed
/// `DataComponentPatch` lands.
const SNBT_COMPONENT_PREFIX: &str = "\0snbt:";

/// Paper `net.minecraft.world.LockCode` seam for `BaseContainerBlockEntity`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LockCode {
    key: Option<String>,
}

impl LockCode {
    /// `LockCode.NO_LOCK`.
    pub const fn no_lock() -> Self {
        Self { key: None }
    }

    /// Construct a lock with a key string.
    pub fn new(key: impl Into<String>) -> Self {
        let key = key.into();
        if key.is_empty() {
            Self::no_lock()
        } else {
            Self { key: Some(key) }
        }
    }

    /// Whether this lock is unlocked (`NO_LOCK`).
    pub fn is_unlocked(&self) -> bool {
        self.key.is_none()
    }

    /// Key string if locked.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// `LockCode.addToTag(ValueOutput)`.
    pub fn save(&self, output: &mut CompoundTag) {
        if let Some(key) = &self.key {
            output.put_string("lock", key);
        }
    }

    /// `LockCode.fromTag(ValueInput)`.
    pub fn load(input: &CompoundTag) -> Self {
        match input.get_string("lock") {
            Some(key) if !key.is_empty() => Self {
                key: Some(key.to_string()),
            },
            _ => Self::no_lock(),
        }
    }
}

/// Shared state for Paper `BaseContainerBlockEntity` and
/// `RandomizableContainerBlockEntity` subclasses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BaseContainerState {
    pub lock_key: LockCode,
    pub custom_name: Option<String>,
    pub loot_table: Option<Identifier>,
    pub loot_table_seed: i64,
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
        if let Some(name) = &self.custom_name {
            output.put_string("CustomName", name);
        }
    }

    /// `BaseContainerBlockEntity.loadAdditional(ValueInput)`.
    pub fn load_base(&mut self, input: &CompoundTag) {
        self.lock_key = LockCode::load(input);
        self.custom_name = input.get_string("CustomName").cloned();
    }

    /// `RandomizableContainer.trySaveLootTable(ValueOutput)`.
    ///
    /// Returns `true` if a loot table was present and written (in which case
    /// Paper skips writing `Items`).
    pub fn try_save_loot_table(&self, output: &mut CompoundTag) -> bool {
        let Some(loot_table) = &self.loot_table else {
            return false;
        };
        output.put_string("LootTable", &loot_table.to_string());
        if self.loot_table_seed != 0 {
            output.put_long("LootTableSeed", self.loot_table_seed);
        }
        true
    }

    /// `RandomizableContainer.tryLoadLootTable(ValueInput)`.
    ///
    /// Returns `true` if a loot table was present and loaded (in which case
    /// Paper skips loading `Items`).
    pub fn try_load_loot_table(&mut self, input: &CompoundTag) -> bool {
        let Some(id_str) = input.get_string("LootTable") else {
            self.loot_table = None;
            self.loot_table_seed = 0;
            return false;
        };
        let Some(id) = Identifier::try_parse_result(id_str).ok().flatten() else {
            self.loot_table = None;
            self.loot_table_seed = 0;
            return false;
        };
        self.loot_table = Some(id);
        self.loot_table_seed = input.get_long_or("LootTableSeed", 0);
        true
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
        if stack.get_max_stack_size() != DEFAULT_ITEM_MAX_STACK_SIZE {
            entry.put_int("max_stack_size", stack.get_max_stack_size());
        }
        if !stack.components().is_empty() {
            let mut comp = CompoundTag::new();
            let parser = TagParser::create(NbtOps);
            for (k, v) in stack.components() {
                if let Some(snbt) = v.strip_prefix(SNBT_COMPONENT_PREFIX)
                    && let Ok(parsed) = parser.parse_fully(snbt)
                {
                    comp.put(k.clone(), parsed);
                } else {
                    comp.put_string(k, v);
                }
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
        let slot = entry.get_byte_or("Slot", -1) as u8 as usize;
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
        let max_stack_size = entry.get_int_or("max_stack_size", DEFAULT_ITEM_MAX_STACK_SIZE);
        let mut stack = InventoryItemStack::with_max_stack_size(item_id, count, max_stack_size);
        if let Some(comp_tag) = entry.get_compound("components") {
            let mut map = ComponentMap::new();
            for (k, v) in &comp_tag.tags {
                match v {
                    Tag::String(s) => {
                        map.insert(k.clone(), s.value.clone());
                    }
                    other => {
                        let encoded = format!(
                            "{SNBT_COMPONENT_PREFIX}{}",
                            StringTagVisitor::to_string(other)
                        );
                        map.insert(k.clone(), encoded);
                    }
                }
            }
            stack = stack.with_components(map);
        }
        items[slot] = stack;
    }
}
