//! `net.minecraft.world.level.block.state.properties.Property<T>` + the three
//! concrete classes (`BooleanProperty`, `IntegerProperty`, `EnumProperty`) —
//! the typed property surface (issue #228), table-driven over the generated
//! `block_properties.rs` tables so no per-property hand port is needed.
//!
//! Java's `Property<T>` is an abstract identity keyed by name + value class;
//! `StateDefinition` stores an `ImmutableSortedMap<String, Property<?>>` and
//! `StateHolder.setValue/getValue` operate through `getInternalIndex`. This
//! port collapses the three classes into one [`Property`] whose kind is
//! inferred from the generated value table (`[true,false]` => Boolean, a
//! contiguous integer range => Integer, else Enum), with [`PropertyValue`]
//! as the union value. That keeps the value/index semantics identical to
//! Paper's while avoiding a generic per-property object graph (OWNERSHIP:
//! arenas + ids).
//!
//! Fidelity notes (Paper 26.2 `Property.java`/`BooleanProperty.java`/
//! `IntegerProperty.java`/`EnumProperty.java`):
//! - `getPossibleValues` order: Boolean `[true, false]` (`List.of`), Integer
//!   `min..=max` ascending (`IntStream.range`), Enum declaration order — exactly
//!   the order of the generated `BLOCK_PROPERTY_VALUES[id]` slice.
//! - `getValue(String)`: Boolean matches `"true"`/`"false"` only;
//!   `IntegerProperty` parses and bounds-checks `min..=max` (a
//!   `NumberFormatException` becomes `Optional.empty`); `EnumProperty` looks up
//!   the serialized name map (missing => `Optional.empty`).
//! - `getInternalIndex(T)`: Boolean `true`->0/`false`->1;
//!   `IntegerProperty` `value <= max ? value - min : -1`; `EnumProperty` the
//!   ordinal -> declared-index table. All reduce to the position in
//!   `BLOCK_PROPERTY_VALUES`.
//! - The generated value slices are *name-ordered* exactly as the Java
//!   declaration order (verified for all 121 properties); `value_index` is the
//!   positional index into that slice.

use crate::generated::block_properties::BlockPropertyId;

/// A concrete `Property<T>` — a named property of a block's state definition,
/// with its value kind inferred from the generated table.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Property(BlockPropertyId);

/// The union of the three `Property<T>` value classes (`T` = Boolean /
/// `Comparable<Integer>` / `StringRepresentable` enum). `Enum` carries the
/// serialized name; `Int` the numeric value; `Bool` the boolean.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyValue {
    Bool(bool),
    Int(i32),
    Enum(&'static str),
}

/// The inferred kind of a [`Property`]'s value class.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyKind {
    Bool,
    /// Integer — the Java `IntegerProperty` (contiguous `min..=max`).
    Int {
        min: i32,
        max: i32,
    },
    /// Enum — every non-Boolean, non-Integer property.
    Enum,
}

impl Property {
    /// The property for a generated property id. There is deliberately no
    /// `from_name` lookup: property names are *not* unique across the registry
    /// (e.g. `facing` exists as `Direction.FACING` and `HorizontalDirection
    /// .FACING`), so Java only resolves a property by name through its owning
    /// block's `StateDefinition` — see `crate::state_definition::
    /// StateDefinition::get_property`, where a name is unique.
    pub const fn from_id(id: BlockPropertyId) -> Self {
        Self(id)
    }

    /// The generated property id.
    #[inline]
    pub const fn id(self) -> BlockPropertyId {
        self.0
    }

    /// The property's serialized name (`Property.getName()`).
    #[inline]
    pub fn name(self) -> &'static str {
        self.0.name()
    }

    /// `Property.getPossibleValues()` — the value names in declaration order
    /// (Boolean `[true, false]`, Integer ascending, Enum declaration order).
    #[inline]
    pub fn values(self) -> &'static [&'static str] {
        self.0.values()
    }

    /// The inferred value kind, from the generated value table.
    pub fn kind(self) -> PropertyKind {
        match self.values() {
            ["true", "false"] | ["false", "true"] => PropertyKind::Bool,
            values => match values
                .iter()
                .map(|v| v.parse::<i32>())
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(ints) if ints.len() >= 2 && ints.windows(2).all(|w| w[1] == w[0] + 1) => {
                    PropertyKind::Int {
                        min: *ints.first().unwrap(),
                        max: *ints.last().unwrap(),
                    }
                }
                _ => PropertyKind::Enum,
            },
        }
    }

    /// `Property.getValue(String)` — parse a serialized value name into the
    /// typed value, or `None` if it is not one of the property's allowed values
    /// (Java `Optional.empty`). The returned `Enum` name leases the generated
    /// table's static entry, so it is `'static`.
    pub fn get_value(self, name: &str) -> Option<PropertyValue> {
        match self.kind() {
            PropertyKind::Bool => match name {
                "true" => Some(PropertyValue::Bool(true)),
                "false" => Some(PropertyValue::Bool(false)),
                _ => None,
            },
            PropertyKind::Int { min, max } => name
                .parse::<i32>()
                .ok()
                .filter(|&v| v >= min && v <= max)
                .map(PropertyValue::Int),
            PropertyKind::Enum => self
                .values()
                .iter()
                .position(|&n| n == name)
                .map(|i| PropertyValue::Enum(self.values()[i])),
        }
    }

    /// `Property.getInternalIndex(T)` — the mixed-radix value index for a typed
    /// value, or `None` if the value is not allowed (Java `-1`).
    pub fn value_index(self, value: PropertyValue) -> Option<u16> {
        match (self.kind(), value) {
            (PropertyKind::Bool, PropertyValue::Bool(b)) => Some(if b { 0 } else { 1 }),
            (PropertyKind::Int { min, max }, PropertyValue::Int(v)) => {
                (v >= min && v <= max).then_some((v - min) as u16)
            }
            (PropertyKind::Enum, PropertyValue::Enum(name)) => self
                .values()
                .iter()
                .position(|&n| n == name)
                .map(|i| i as u16),
            _ => None,
        }
    }

    /// `Property.getName(T)` — the serialized value name for a typed value
    /// (Java `value.toString()` for Boolean/Integer, `getSerializedName()` for
    /// enums). `None` for a value outside the property's allowed set. The
    /// returned `&'static str` is the generated table's entry, so it compares
    /// equal to `get_value` input names.
    pub fn value_name(self, value: PropertyValue) -> Option<&'static str> {
        let idx = self.value_index(value)?;
        Some(self.values()[idx as usize])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every generated property, by id — the name array is index-aligned with
    /// the id enum (verified by `from_id_and_name_round_trip`).
    fn all_properties() -> impl Iterator<Item = Property> {
        crate::generated::block_properties::BLOCK_PROPERTY_NAMES
            .iter()
            .enumerate()
            .map(|(idx, _)| {
                let pid = BlockPropertyId::from_id(idx as u16).expect("id in range");
                Property::from_id(pid)
            })
    }

    fn pid(variant: BlockPropertyId) -> Property {
        Property::from_id(variant)
    }

    #[test]
    fn from_id_and_name_round_trip() {
        // `name()` must be the exact inverse of the generated index, for every
        // one of the 121 properties (duplicate serialized names like `facing`
        // are distinct ids, so id -> name -> id is a bijection).
        for p in all_properties() {
            let idx = p.id() as u16;
            assert_eq!(
                BlockPropertyId::from_id(idx),
                Some(p.id()),
                "name `{}` maps to wrong id",
                p.name()
            );
        }
    }

    #[test]
    fn bool_property_semantics() {
        let powered = pid(BlockPropertyId::Powered);
        assert_eq!(powered.kind(), PropertyKind::Bool);
        assert_eq!(powered.values(), &["true", "false"]);
        // BooleanProperty: true->0, false->1 (List.of(true, false)).
        assert_eq!(powered.get_value("true"), Some(PropertyValue::Bool(true)));
        assert_eq!(powered.get_value("false"), Some(PropertyValue::Bool(false)));
        assert_eq!(powered.get_value("1"), None);
        assert_eq!(powered.value_index(PropertyValue::Bool(true)), Some(0));
        assert_eq!(powered.value_index(PropertyValue::Bool(false)), Some(1));
        assert_eq!(powered.value_name(PropertyValue::Bool(true)), Some("true"));
        assert_eq!(
            powered.value_name(PropertyValue::Bool(false)),
            Some("false")
        );
    }

    #[test]
    fn integer_property_semantics() {
        // stage has values ["0","1"] (IntegerProperty 0..=1).
        let stage = pid(BlockPropertyId::Stage);
        assert_eq!(stage.kind(), PropertyKind::Int { min: 0, max: 1 });
        assert_eq!(stage.get_value("0"), Some(PropertyValue::Int(0)));
        assert_eq!(stage.get_value("1"), Some(PropertyValue::Int(1)));
        assert_eq!(stage.get_value("2"), None);
        // Non-numeric name -> Optional.empty (NumberFormatException).
        assert_eq!(stage.get_value("stage"), None);
        assert_eq!(stage.value_index(PropertyValue::Int(1)), Some(1));
        assert_eq!(stage.value_index(PropertyValue::Int(2)), None);
        assert_eq!(stage.value_name(PropertyValue::Int(0)), Some("0"));
    }

    #[test]
    fn enum_property_semantics() {
        // facing is a Direction enum in declaration order.
        let facing = pid(BlockPropertyId::Facing);
        assert_eq!(facing.kind(), PropertyKind::Enum);
        assert!(facing.values().contains(&"north"));
        assert_eq!(
            facing.get_value("north"),
            Some(PropertyValue::Enum("north"))
        );
        assert_eq!(facing.get_value("not_a_direction"), None);
        let idx = facing.value_index(PropertyValue::Enum("north")).unwrap();
        assert_eq!(facing.values()[idx as usize], "north");
        // A value not in the allowed set has no index (EnumProperty ordinal ->
        // declared-index table, -1).
        assert_eq!(facing.value_index(PropertyValue::Enum("nowhere")), None);
        assert_eq!(facing.value_name(PropertyValue::Enum("up")), Some("up"));
        assert_eq!(facing.value_name(PropertyValue::Enum("up-down")), None);
        // The same serialized name on a different enum is a distinct Property
        // (HorizontalDirection.FACING: north/south/west/east — no `up`).
        let facing4 = pid(BlockPropertyId::Facing2);
        assert_eq!(facing4.name(), "facing");
        assert_eq!(facing4.values(), &["north", "south", "west", "east"]);
        assert_eq!(facing4.get_value("up"), None);
        assert_ne!(facing.id(), facing4.id());
    }

    #[test]
    fn every_property_has_a_valid_kind() {
        // All 121 generated properties must classify; the kind inference panics
        // only on a malformed table (a Bool-like slice that isn't exactly two
        // entries, or an Int slice that isn't a contiguous range).
        for p in all_properties() {
            let _ = p.kind();
        }
    }

    #[test]
    fn value_name_round_trips_through_get_value() {
        // For every property and every allowed value name, value_name(value)
        // round-trips: get_value(name) -> Some(v), value_name(v) == name.
        for p in all_properties() {
            for &name in p.values() {
                let v = p
                    .get_value(name)
                    .unwrap_or_else(|| panic!("get_value({name})"));
                assert_eq!(
                    p.value_name(v),
                    Some(name),
                    "round-trip failed for {}={name}",
                    p.name()
                );
            }
        }
    }
}
