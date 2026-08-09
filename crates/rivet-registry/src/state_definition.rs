//! `net.minecraft.world.level.block.state.StateDefinition<O, S>` — the
//! property map of a block's states (issue #228), table-driven over the
//! generated `BLOCK_STATE_SHAPES` (name-sorted property ids per block).
//!
//! Java's `StateDefinition` holds an `ImmutableSortedMap<String, Property<?>>`
//! (name-sorted) plus the concrete state table; `NbtUtils.readBlockState` needs
//! `getProperty(name)` and `getValues()`, and `StateHolder.setValue` needs the
//! property->value-index mapping. This port collapses the map into the block's
//! shape: [`StateDefinition::for_block`] derives the name-sorted [`Property`]
//! list straight from the generated tables (verified name-sorted for all 797
//! multi-property blocks), and the value ops reuse the existing
//! `block_state::BlockState` mixed-radix surface.
//!
//! Fidelity notes (Paper 26.2 `StateDefinition.java` + `StateHolder.java`):
//! - `getProperty(String)` returns `null` for a missing name; the Rust view is
//!   `Option`.
//! - `getPossibleStates()`/`any()`/`isSingletonState()` are served by the
//!   `block_state::BlockState` value type (`BlockState::of` = `any()`, and the
//!   singleton check is `shape.is_empty()`).
//! - The builder validation (`NAME_PATTERN`, `<= 1` values, duplicate names) is
//!   not reachable here: the generated tables are already the validated Java
//!   `Block` state definitions.

use crate::block_state::BlockState;
use crate::block_state_property::{Property, PropertyValue};
use crate::generated::block_states::shape_of;
use crate::generated::blocks::BlockId;

/// `StateDefinition<Block, BlockState>` for a single block — the name-sorted
/// property map plus the operations `NbtUtils.readBlockState` needs.
#[derive(Clone, Copy, Debug)]
pub struct StateDefinition {
    block: BlockId,
    /// The block's property ids, name-sorted (the generated shape).
    props: &'static [u16],
}

impl StateDefinition {
    /// The block's state definition. A block id with no property entries yields
    /// the empty (singleton) definition — Java `Block` always builds one.
    pub fn for_block(block: BlockId) -> Self {
        Self {
            block,
            props: shape_of(block),
        }
    }

    /// The owning block (`StateDefinition.getOwner()`).
    #[inline]
    pub fn block(self) -> BlockId {
        self.block
    }

    /// `StateDefinition.isSingletonState()` — `propertiesByName.isEmpty()`.
    #[inline]
    pub fn is_singleton_state(self) -> bool {
        self.props.is_empty()
    }

    /// `StateDefinition.getProperties()` — the name-sorted `Property` list
    /// (Java `ImmutableSortedMap.values()`).
    pub fn properties(self) -> Vec<Property> {
        self.props
            .iter()
            .map(|&pid| {
                let id = crate::generated::block_properties::BlockPropertyId::from_id(pid)
                    .expect("shape property id in table");
                Property::from_id(id)
            })
            .collect()
    }

    /// `StateDefinition.getProperty(String)` — the `Property` named `name`, or
    /// `None` (Java `null`) if the block has no such property.
    pub fn get_property(self, name: &str) -> Option<Property> {
        // The shape is name-sorted (verified), so a binary search by name maps
        // directly onto the property-id slice.
        let pid = self
            .props
            .binary_search_by_key(&name, |&pid| {
                crate::generated::block_properties::BLOCK_PROPERTY_NAMES[pid as usize]
            })
            .ok()?;
        let id = crate::generated::block_properties::BlockPropertyId::from_id(self.props[pid])?;
        Some(Property::from_id(id))
    }

    /// `StateHolder.setValue(Property, T)` bridge — the value index of `value`
    /// for `prop` on a state of this block, used by the NBT codec to set a
    /// property by its typed value. `None` when the property is not on this
    /// block's definition or the value is not allowed (Java
    /// `IllegalArgumentException`).
    pub fn value_index(self, prop: Property, value: PropertyValue) -> Option<u16> {
        if self.props.contains(&(prop.id() as u16)) {
            prop.value_index(value)
        } else {
            None
        }
    }

    /// `StateDefinition.any()` — the block's default state.
    pub fn any(self) -> BlockState {
        BlockState::of(self.block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_state_property::PropertyValue;

    #[test]
    fn singleton_block_has_no_properties() {
        let air = StateDefinition::for_block(BlockId::from_name("minecraft:air").unwrap());
        assert!(air.is_singleton_state());
        assert_eq!(air.properties(), vec![]);
        assert_eq!(air.get_property("powered"), None);
        assert_eq!(
            air.any(),
            BlockState::of(BlockId::from_name("minecraft:air").unwrap())
        );
    }

    #[test]
    fn multi_property_block_lists_name_sorted_properties() {
        // acacia_button's shape is name-sorted by construction.
        let def =
            StateDefinition::for_block(BlockId::from_name("minecraft:acacia_button").unwrap());
        let names: Vec<&str> = def.properties().iter().map(|p| p.name()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        assert!(names.contains(&"powered"));
        assert!(names.contains(&"face"));
    }

    #[test]
    fn get_property_resolves_by_name_and_absent_is_none() {
        let def = StateDefinition::for_block(BlockId::from_name("minecraft:oak_log").unwrap());
        let axis = def.get_property("axis").expect("axis on oak_log");
        assert_eq!(axis.name(), "axis");
        assert_eq!(def.get_property("powered"), None);
        assert_eq!(def.get_property(""), None);
        // A property on a different block is absent here.
        let stone = StateDefinition::for_block(BlockId::from_name("minecraft:stone").unwrap());
        assert_eq!(stone.get_property("axis"), None);
    }

    #[test]
    fn value_index_requires_property_on_this_block() {
        let def = StateDefinition::for_block(BlockId::from_name("minecraft:oak_log").unwrap());
        let axis = def.get_property("axis").unwrap();
        // A property not on the block has no index.
        let powered = crate::block_state_property::Property::from_id(
            crate::generated::block_properties::BlockPropertyId::Powered,
        );
        assert_eq!(def.value_index(powered, PropertyValue::Bool(true)), None);
        // A present property maps its typed value.
        assert_eq!(def.value_index(axis, PropertyValue::Enum("x")), Some(0));
        assert_eq!(
            def.value_index(axis, PropertyValue::Enum("not_an_axis")),
            None
        );
    }

    #[test]
    fn any_is_the_default_state() {
        let def = StateDefinition::for_block(BlockId::from_name("minecraft:stone").unwrap());
        let default = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        assert_eq!(def.any(), default);
    }
}
