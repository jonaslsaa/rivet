//! Paper `net.minecraft.world.RandomizableContainer.unpackLootTable` for
//! `RandomizableContainerBlockEntity` subclasses (chest, barrel, hopper,
//! crafter).
//!
//! Paper unpacks implicitly from `isEmpty()`, `getItem(0)`, `removeItem`,
//! `removeItemNoUpdate` and `setItem`, but only while the block entity is bound
//! to a server level (`level != null && level.getServer() != null`). These
//! block entities are level-less values, so their `Container` methods take the
//! level-less branch (a no-op that keeps the loot table). Level-bound callers
//! pass the level context explicitly through
//! [`RandomizableContainer::unpack_loot_table`] before those operations.

use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::Identifier;

use crate::inventory::{ComponentMap, Container, InventoryItemStack, ItemId};
use crate::loot::{
    ContextValue, LootContainer, LootContextParamSets, LootContextParams, LootItemStack,
    LootParams, LootRandomState, LootRegistry, LootTable,
};

use super::nbt::{BaseContainerState, encode_component_tag, max_stack_size_with_components};

/// Level-scoped inputs of `RandomizableContainer.unpackLootTable(null)`.
pub struct LootUnpackContext<'a> {
    /// `level.getServer().reloadableRegistries()` loot tables.
    pub loot: &'a LootRegistry,
    /// Level random sequences used by `LootTable.fill`.
    pub random_state: &'a mut LootRandomState,
    /// `Vec3.atCenterOf(getBlockPos())` (`LootContextParams.ORIGIN`).
    pub origin: [f64; 3],
    /// `System.currentTimeMillis()`, recorded as Paper's `lastFill`.
    pub now_millis: i64,
}

/// Paper `net.minecraft.world.RandomizableContainer` as implemented by
/// `RandomizableContainerBlockEntity`.
pub trait RandomizableContainer: Container + Sized {
    /// Shared `BaseContainerBlockEntity` / loot-table state.
    fn container_state(&self) -> &BaseContainerState;

    /// Mutable shared `BaseContainerBlockEntity` / loot-table state.
    fn container_state_mut(&mut self) -> &mut BaseContainerState;

    /// `RandomizableContainer.unpackLootTable(null)` with Paper's default
    /// `lootables` world configuration (`auto-replenish: false`): the first
    /// unpack always fills, records `lastFill`, and clears the loot table
    /// before filling. The seed is kept, as in Paper.
    fn unpack_loot_table(&mut self, ctx: &mut LootUnpackContext<'_>) {
        let Some(loot_table_key) = self.container_state().loot_table.clone() else {
            return;
        };
        // `PaperLootableInventoryData.shouldClearLootTable(...)`.
        let state = self.container_state_mut();
        state.lootable_data.last_fill = ctx.now_millis;
        state.loot_table = None;
        let seed = state.loot_table_seed;

        let empty = LootTable::empty();
        let loot_table = ctx.loot.get_table(&loot_table_key).unwrap_or(&empty);
        let mut params = LootParams::builder();
        params.with_parameter(
            LootContextParams::origin(),
            ContextValue::Origin(ctx.origin),
        );
        let Ok(params) = params.create(LootContextParamSets::chest()) else {
            return;
        };
        let mut target = LootFillTarget::new(self);
        loot_table.fill(&mut target, params, ctx.loot, ctx.random_state, seed);
    }
}

/// `LootTable.fill` view over a block-entity `Container`: emptiness probes are
/// answered from a parallel `LootItemStack` mirror and every placed stack is
/// written through `Container.setItem`.
struct LootFillTarget<'c, C: Container> {
    container: &'c mut C,
    probe: Vec<LootItemStack>,
}

impl<'c, C: Container> LootFillTarget<'c, C> {
    fn new(container: &'c mut C) -> Self {
        let probe = (0..container.get_container_size())
            .map(|slot| {
                let stack = container.get_item(slot);
                match stack.item().name() {
                    Some(name) if !stack.is_empty() => {
                        LootItemStack::with_count(Identifier::parse(name), stack.get_count())
                    }
                    _ => LootItemStack::empty(),
                }
            })
            .collect();
        Self { container, probe }
    }
}

impl<C: Container> LootContainer for LootFillTarget<'_, C> {
    fn container_size(&self) -> usize {
        self.probe.len()
    }

    fn item(&self, slot: usize) -> &LootItemStack {
        &self.probe[slot]
    }

    fn set_item(&mut self, slot: usize, stack: LootItemStack) {
        self.container
            .set_item(slot, loot_to_inventory_stack(&stack));
        self.probe[slot] = stack;
    }
}

/// Convert a loot-produced stack to an inventory stack. Enchantments land in
/// `minecraft:stored_enchantments` for enchanted books and
/// `minecraft:enchantments` otherwise (`EnchantmentHelper.getComponentType`).
fn loot_to_inventory_stack(stack: &LootItemStack) -> InventoryItemStack {
    if stack.is_empty() {
        return InventoryItemStack::empty();
    }
    let name = stack.item().to_string();
    let Some(item) = ItemId::from_name(&name) else {
        return InventoryItemStack::empty();
    };
    let mut components = ComponentMap::new();
    if !stack.enchantments().is_empty() {
        let mut levels = CompoundTag::new();
        for (enchantment, &level) in stack.enchantments() {
            levels.put_int(&enchantment.to_string(), level);
        }
        let key = if name == "minecraft:enchanted_book" {
            "minecraft:stored_enchantments"
        } else {
            "minecraft:enchantments"
        };
        components.insert(key.to_owned(), encode_component_tag(&Tag::Compound(levels)));
    }
    let max_stack_size = max_stack_size_with_components(item, &components);
    let inventory = InventoryItemStack::with_max_stack_size(item, stack.count(), max_stack_size);
    if components.is_empty() {
        inventory
    } else {
        inventory.with_components(components)
    }
}
