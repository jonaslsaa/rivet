//! Hand-written `BlockState` value type over the generated global-id tables
//! (issue #228). This is the "pure table ops, no world types" surface the
//! worldgen/heightmap/lighting work consumes: it decodes a `StateId` into the
//! probe-driven behavior word (`generated/block_behaviors.rs`), round-trips
//! through the mixed-radix property tables (`generated/block_states.rs` +
//! `block_properties.rs`), and answers block-tag membership
//! (`generated/tags.rs`). It never reads a world, so it lives in
//! `rivet-registry` and requires only the `blocks` feature.
//!
//! Fidelity notes (Paper 26.2 `net.minecraft.world.level.block.state.BlockBehaviour.BlockStateBase`
//! + `StateHolder`):
//! - `getValue(property)` throws when the property is not on the state; this
//!   module returns `None` (the `getOptionalValue` view, which never throws).
//! - `setValue(property, value)` throws when the property is not on the state
//!   OR the value is not one of the property's allowed values.
//! - `trySetValue` returns `this` unchanged when the property is not on the
//!   state, and throws (like `setValue`) only when the value is invalid for a
//!   property that IS present.
//! - `cycle(property)` = `setValue(property, nextAfter(current))` where
//!   `nextAfter` wraps `indexOf(current) + 1` modulo the value count.
//!
//! The fallible operations return `Result<Self, BlockStateError>` instead of
//! panicking, so worldgen code paths surface the failure as data.
//!
//! RivetTodo(#202): this value type has no serialization codec — the DFU
//! `BlockState` codec (NbtUtils block-state codecs + `ValueOutput` overloads) is
//! tracked by rivet-nbt and deliberately out of scope for the #228 worldgen
//! slice.

use std::fmt;

use crate::block_state_property::{Property, PropertyValue};
use crate::generated::block_behaviors::{
    BEHAVIOR_MASK_LIGHT_DAMPENING, BEHAVIOR_MASK_LIGHT_EMISSION, BEHAVIOR_MASK_MAP_COLOR,
    BEHAVIOR_SHIFT_LIGHT_DAMPENING, BEHAVIOR_SHIFT_LIGHT_EMISSION, BEHAVIOR_SHIFT_MAP_COLOR,
    behavior_of,
};
use crate::generated::block_properties::{
    BLOCK_PROPERTY_VALUES, BlockPropertyId, MAX_BLOCK_STATE_PROPERTY_COUNT,
};
use crate::generated::block_states::{
    StateId, block_of, default_state, shape_of, state_id, values_of,
};
use crate::generated::blocks::BlockId;
use crate::generated::tags::BLOCK_TAG_BY_NAME;

/// A concrete block state: a dense global state id with the owning block's
/// property values already fixed. `Copy`/`Eq`/`Hash` mirror the id it wraps, so
/// it can be used directly in palettes and sets.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct BlockState(StateId);

/// Why a property operation on a `BlockState` failed. Mirrors the
/// `IllegalArgumentException` `StateHolder` throws for the same inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockStateError {
    /// The property is not part of this state's block.
    PropertyNotPresent(BlockPropertyId),
    /// The value index is outside the property's allowed value set.
    ValueOutOfRange {
        /// The property being set.
        prop: BlockPropertyId,
        /// The requested value index.
        value: u16,
        /// The number of allowed values for `prop`.
        count: u16,
    },
    /// A typed value is not one of the property's allowed values (Paper's
    /// `setValue(property, value)` `IllegalArgumentException` for a typed value
    /// outside the property's value set).
    ValueNotAllowed(Property, PropertyValue),
}

impl fmt::Display for BlockStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlockStateError::PropertyNotPresent(prop) => {
                write!(
                    f,
                    "cannot set property {}: it is not on this block state",
                    prop.name()
                )
            }
            BlockStateError::ValueOutOfRange { prop, value, count } => write!(
                f,
                "cannot set property {} to value index {value}: the property has only {count} allowed values",
                prop.name()
            ),
            BlockStateError::ValueNotAllowed(prop, value) => write!(
                f,
                "cannot set property {} to value {value:?}: it is not one of the property's allowed values",
                prop.name()
            ),
        }
    }
}

impl std::error::Error for BlockStateError {}

impl BlockState {
    /// Wrap a raw state id without validation (the id may be out of range,
    /// mirroring `Block.stateById` returning AIR for ids past the registry).
    #[inline]
    pub const fn new(id: StateId) -> Self {
        Self(id)
    }

    /// The block's default state (`block.defaultBlockState()` in Paper). A block
    /// id past the block table (the registry's `idToT.size()` boundary) degrades
    /// to air, mirroring `Block.stateById`, which reads `BLOCK_STATE_REGISTRY`
    /// (an `IdMapper`) and falls back to `Blocks.AIR.defaultBlockState()` when
    /// `byId` returns null for an out-of-range id.
    #[inline]
    pub fn of(block: BlockId) -> Self {
        if (block.0 as usize) < crate::generated::block_states::BLOCK_STATE_BASES.len() {
            Self(default_state(block))
        } else {
            Self(StateId(0)) // air's default state is id 0
        }
    }

    /// The underlying dense global state id.
    #[inline]
    pub const fn id(self) -> StateId {
        self.0
    }

    /// The owning block. Ids outside `0..BLOCK_STATE_COUNT` resolve to air,
    /// mirroring `Block.stateById`.
    #[inline]
    pub fn block(self) -> BlockId {
        block_of(self.0)
    }

    /// Whether the id names a real block state.
    #[inline]
    pub fn is_valid(self) -> bool {
        self.0.0 < crate::generated::block_states::BLOCK_STATE_COUNT
    }

    // --- property access via the mixed-radix tables -------------------------

    /// The property's value index in this state, or `None` if the property is
    /// not on the owning block (Paper `getOptionalValue`; `getValue` throws for
    /// the missing case, which `None` is the Rust-idiomatic equivalent of).
    pub fn get_property(self, prop: BlockPropertyId) -> Option<u16> {
        let block = self.block();
        let shape = shape_of(block);
        let mut buf = [0u16; MAX_BLOCK_STATE_PROPERTY_COUNT];
        values_of(self.0, &mut buf);
        shape.iter().position(|&p| p == prop as u16).map(|i| buf[i])
    }

    /// Set a property value, returning the resulting state. Errors when the
    /// property is not on the owning block, or `value` is not an allowed value
    /// index for it (Paper `setValue` `IllegalArgumentException`).
    pub fn set_property(self, prop: BlockPropertyId, value: u16) -> Result<Self, BlockStateError> {
        let block = self.block();
        let shape = shape_of(block);
        let Some(pos) = shape.iter().position(|&p| p == prop as u16) else {
            return Err(BlockStateError::PropertyNotPresent(prop));
        };
        let count = BLOCK_PROPERTY_VALUES[prop as usize].len() as u16;
        if value >= count {
            return Err(BlockStateError::ValueOutOfRange { prop, value, count });
        }
        let mut buf = [0u16; MAX_BLOCK_STATE_PROPERTY_COUNT];
        values_of(self.0, &mut buf);
        buf[pos] = value;
        Ok(Self(state_id(block, &buf[..shape.len()])))
    }

    /// Set a property value, returning `self` unchanged when the property is
    /// not on the owning block (Paper `trySetValue`). The value is still
    /// validated: an out-of-range value index for a present property errors,
    /// matching Paper's `IllegalArgumentException` for that case.
    pub fn try_set_property(
        self,
        prop: BlockPropertyId,
        value: u16,
    ) -> Result<Self, BlockStateError> {
        let block = self.block();
        let shape = shape_of(block);
        if !shape.contains(&(prop as u16)) {
            return Ok(self);
        }
        self.set_property(prop, value)
    }

    /// Cycle the property to its next allowed value, wrapping past the last
    /// value back to the first (Paper `cycle` = `setValue(property,
    /// nextAfter(current))`). Errors when the property is not on the owning
    /// block.
    pub fn cycle_property(self, prop: BlockPropertyId) -> Result<Self, BlockStateError> {
        let current = self
            .get_property(prop)
            .ok_or(BlockStateError::PropertyNotPresent(prop))?;
        let count = BLOCK_PROPERTY_VALUES[prop as usize].len() as u16;
        self.set_property(prop, (current + 1) % count)
    }

    // --- typed value helpers (block.state.properties, issue #228) -----------
    //
    // Worldgen/lighting sets properties through the *typed* value classes of
    // `block.state.properties` (`state.setValue(SlabBlock.TYPE, SlabType.DOUBLE)`
    // or `state.setValue(BlockStateProperties.FACING, Direction.NORTH)`). These
    // helpers take the typed value and route it through the mixed-radix index
    // surface, so callers never deal in raw value indices.

    /// `state.hasProperty(property)` — whether the property is part of this
    /// state's block definition.
    #[inline]
    pub fn has_property(self, prop: Property) -> bool {
        shape_of(self.block()).contains(&(prop.id() as u16))
    }

    /// `state.getValue(property)` — the typed value, `None` when the property
    /// is not on the owning block (the `getOptionalValue` view; Java's
    /// `getValue` throws for that case).
    pub fn get_value(self, prop: Property) -> Option<PropertyValue> {
        let idx = self.get_property(prop.id())? as usize;
        let name = *prop.values().get(idx)?;
        prop.get_value(name)
    }

    /// `state.setValue(property, value)` — set a typed value, erroring when
    /// the property is not on the block (Paper's optimised-table `setValue`
    /// returns null for an absent property, throwing "Cannot set property … on
    /// …") or, for a present property, when the value is not one of its allowed
    /// values (`setValueInternal`'s "not an allowed value").
    ///
    /// `value` accepts any `Into<PropertyValue>` — the raw `PropertyValue`
    /// union or the typed leaf enums (`state.set_value(SlabBlock.TYPE,
    /// SlabType::Double)`).
    pub fn set_value(
        self,
        prop: Property,
        value: impl Into<PropertyValue>,
    ) -> Result<Self, BlockStateError> {
        let value = value.into();
        if !self.has_property(prop) {
            return Err(BlockStateError::PropertyNotPresent(prop.id()));
        }
        let idx = prop
            .value_index(value)
            .ok_or(BlockStateError::ValueNotAllowed(prop, value))?;
        self.set_property(prop.id(), idx)
    }

    /// `state.trySetValue(property, value)` — set a typed value, returning
    /// `self` unchanged when the property is not on the owning block. The value
    /// is still validated for a property that IS present (Paper `trySetValue`).
    pub fn try_set_value(
        self,
        prop: Property,
        value: impl Into<PropertyValue>,
    ) -> Result<Self, BlockStateError> {
        let value = value.into();
        if !self.has_property(prop) {
            return Ok(self);
        }
        self.set_value(prop, value)
    }

    // --- behavior queries (probe-driven, no world types) --------------------

    /// The raw 32-bit behavior word for this state.
    #[inline]
    pub fn behavior(self) -> u32 {
        behavior_of(self.0)
    }

    /// Whether the state is air (Paper `isAir`).
    #[inline]
    pub fn is_air(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_IS_AIR != 0
    }

    /// Whether the state blocks motion (the Heightmap `OCEAN_FLOOR`/
    /// `MOTION_BLOCKING` predicate).
    #[inline]
    pub fn blocks_motion(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_BLOCKS_MOTION != 0
    }

    /// Whether the state's collision/occlusion shape is a full block (Paper
    /// `isSolidRender`).
    #[inline]
    pub fn solid_render(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_SOLID_RENDER != 0
    }

    /// Whether the state can occlude light (Paper `canOcclude`).
    #[inline]
    pub fn can_occlude(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_CAN_OCCLUDE != 0
    }

    /// Whether light occlusion follows the non-full occlusion shape (Paper
    /// `useShapeForLightOcclusion`; Starlight's `isConditionallyFullOpaque` is
    /// `canOcclude & useShapeForLightOcclusion`).
    #[inline]
    pub fn use_shape_for_light_occlusion(self) -> bool {
        self.behavior()
            & crate::generated::block_behaviors::BEHAVIOR_FLAG_USE_SHAPE_FOR_LIGHT_OCCLUSION
            != 0
    }

    /// Whether sky light passes straight through (Paper `propagatesSkylightDown`).
    #[inline]
    pub fn propagates_skylight_down(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_PROPAGATES_SKYLIGHT_DOWN
            != 0
    }

    /// Whether the state is random-ticked (Paper `isRandomlyTicking`).
    #[inline]
    pub fn random_ticking(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_RANDOM_TICKING != 0
    }

    /// Whether the state carries no fluid (Paper `getFluidState().isEmpty()`).
    #[inline]
    pub fn fluid_empty(self) -> bool {
        self.behavior() & crate::generated::block_behaviors::BEHAVIOR_FLAG_FLUID_EMPTY != 0
    }

    /// The state's light dampening in `0..=15` (Paper `getLightDampening`).
    #[inline]
    pub fn light_dampening(self) -> u8 {
        ((self.behavior() >> BEHAVIOR_SHIFT_LIGHT_DAMPENING) & BEHAVIOR_MASK_LIGHT_DAMPENING) as u8
    }

    /// The state's emitted light level in `0..=15` (Paper `getLightEmission`).
    #[inline]
    pub fn light_emission(self) -> u8 {
        ((self.behavior() >> BEHAVIOR_SHIFT_LIGHT_EMISSION) & BEHAVIOR_MASK_LIGHT_EMISSION) as u8
    }

    /// The state's map color id in `0..=63` (Paper `getMapColor(...).id`).
    #[inline]
    pub fn map_color_id(self) -> u8 {
        ((self.behavior() >> BEHAVIOR_SHIFT_MAP_COLOR) & BEHAVIOR_MASK_MAP_COLOR) as u8
    }

    // --- tag query ----------------------------------------------------------

    /// Whether the owning block is a member of the block tag `tag` (e.g.
    /// `"minecraft:planks"`). Unknown tags read as `false`, matching Paper's
    /// `is(TagKey)` on a registry that has not bound the tag.
    pub fn is_in_tag(self, tag: &str) -> bool {
        let Some(elements) = BLOCK_TAG_BY_NAME.get(tag) else {
            return false;
        };
        let block_name = self.block().name();
        // `elements` is a tag-file-ordered slice of block names; a linear scan
        // is fine (tags are at most a few hundred entries, and tag checks are
        // not on the per-block hot path the probe tables serve).
        elements.contains(&block_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `digits` helper mirroring `block_state_tests.rs`: map `(prop, value)`
    /// name pairs to digit indices in declaration order.
    fn digits(block: &str, pairs: &[(&str, &str)]) -> (BlockId, Vec<u16>) {
        let block = BlockId::from_name(block).expect("block in generated table");
        let shape = shape_of(block);
        let mut values = vec![0u16; shape.len()];
        for (prop_name, value_name) in pairs {
            let pos = shape
                .iter()
                .position(|&p| {
                    crate::generated::block_properties::BLOCK_PROPERTY_NAMES[p as usize]
                        == *prop_name
                })
                .unwrap_or_else(|| panic!("{block:?} has no property `{prop_name}`"));
            let prop_values = BLOCK_PROPERTY_VALUES[shape[pos] as usize];
            let value_pos = prop_values
                .iter()
                .position(|&v| v == *value_name)
                .unwrap_or_else(|| panic!("`{prop_name}` has no value `{value_name}`"));
            values[pos] = value_pos as u16;
        }
        (block, values)
    }

    /// A `BlockPropertyId` by name (mirrors the value-position logic above).
    fn prop(name: &str) -> BlockPropertyId {
        let id = crate::generated::block_properties::BLOCK_PROPERTY_NAMES
            .iter()
            .position(|&n| n == name)
            .expect("property in generated table");
        // Discriminants are contiguous 0..n (codegen order), so the position IS
        // the discriminant; `from_id` resolves it safely.
        BlockPropertyId::from_id(id as u16).expect("property id in range")
    }

    #[test]
    fn golden_defaults_and_behavior_word() {
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        assert_eq!(air.id(), StateId(0));
        assert!(air.is_air());
        assert!(!air.blocks_motion());
        assert!(air.fluid_empty());
        assert_eq!(air.light_dampening(), 0);
        assert_eq!(air.light_emission(), 0);
        // air map color id 0.
        assert_eq!(air.map_color_id(), 0);

        let stone = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        assert!(stone.blocks_motion());
        assert!(stone.solid_render());
        assert!(stone.can_occlude());
        // Paper's BlockBehaviour default for useShapeForLightOcclusion is false
        // (only specific blocks override it); stone does not, so it reads false.
        assert!(!stone.use_shape_for_light_occlusion());
        assert!(!stone.is_air());
        // Default dampening for a full-opaque, non-skylight-propagating block
        // is 15 (isSolidRender => 15).
        assert_eq!(stone.light_dampening(), 15);

        // Water: carries a fluid, so fluid_empty is false and skylight does not
        // propagate through it (the state-level propagatesSkylightDown requires
        // an empty fluid state).
        let water = BlockState::of(BlockId::from_name("minecraft:water").unwrap());
        assert!(!water.fluid_empty());
        assert!(!water.propagates_skylight_down());
        assert!(!water.solid_render());
    }

    #[test]
    fn set_get_cycle_property_round_trips() {
        // acacia_button default {face: wall, facing: north, powered: false}
        // (golden id 10780 from block_state_tests.rs).
        let (block, _vals) = digits(
            "minecraft:acacia_button",
            &[("face", "wall"), ("facing", "north"), ("powered", "false")],
        );
        let default = BlockState::of(block);
        assert_eq!(default.id(), StateId(10780));

        let powered = prop("powered");
        let face = prop("face");

        // get_property on a present property reads the value index. `powered`'s
        // value set is ["true", "false"] (BooleanProperty declaration order), so
        // the default `powered: false` reads index 1.
        assert_eq!(default.get_property(powered), Some(1));
        // set_property to index 0 flips it to `powered: true`.
        let on = default.set_property(powered, 0).unwrap();
        assert_eq!(on.get_property(powered), Some(0));
        assert_ne!(on.id(), default.id());
        // The other properties are preserved by the mixed-radix recomposition
        // (AttachFace orders ["floor", "wall", "ceiling"], so the default
        // `face: wall` reads index 1 — compare against the default, not 0).
        assert_eq!(on.get_property(face), default.get_property(face));
        assert_eq!(on.block(), block);

        // cycle wraps: powered has 2 values, so cycling 0 goes back to 1, i.e.
        // the original default state.
        assert_eq!(
            on.cycle_property(powered).unwrap().get_property(powered),
            Some(1)
        );
        assert_eq!(on.cycle_property(powered).unwrap(), default);

        // try_set_property with a property not on the block returns self.
        let waterlogged = prop("waterlogged");
        assert!(!shape_of(block).contains(&(waterlogged as u16)));
        assert_eq!(default.try_set_property(waterlogged, 0).unwrap(), default);

        // set_property with a property not on the block errors.
        assert_eq!(
            default.set_property(waterlogged, 0),
            Err(BlockStateError::PropertyNotPresent(waterlogged))
        );
        // set_property with an out-of-range value errors.
        assert!(matches!(
            default.set_property(powered, 2),
            Err(BlockStateError::ValueOutOfRange { .. })
        ));
        // cycle_property on a property not on the block errors.
        assert_eq!(
            default.cycle_property(waterlogged),
            Err(BlockStateError::PropertyNotPresent(waterlogged))
        );
    }

    #[test]
    fn behavior_queries_match_probe_anchors() {
        // The exact default-state words BlockBehaviourProbe emitted for the
        // representative blocks (the anchors it prints), decoded through the
        // newtype surface. These are Paper's live accessor values, not
        // hand-derived, so any probe/fixture change that shifts a behavior
        // fails here loudly.
        let word = |name: &str| BlockState::of(BlockId::from_name(name).unwrap()).behavior();
        assert_eq!(word("minecraft:air"), 0xA1);
        assert_eq!(word("minecraft:stone"), 0xB0F8E);
        assert_eq!(word("minecraft:water"), 0xC0100);
        assert_eq!(word("minecraft:lava"), 0x4F140);
        assert_eq!(word("minecraft:oak_leaves"), 0x701C2);
        assert_eq!(word("minecraft:glass"), 0xA2);
        assert_eq!(word("minecraft:torch"), 0xE0A0);
    }

    #[test]
    fn behavior_word_fields_match_paper_semantics() {
        // The whole-word anchor test above pins the probe's raw words; this
        // test independently decodes every field of those same words through the
        // documented bit layout and asserts Paper's semantic values. It guards
        // against a systematic probe bit-packing/accessor bug: if the probe
        // packed a flag into the wrong bit, `behavior_of` would read the
        // mis-packed bit and one of these fields would come back wrong for an
        // anchor where the two swapped flags differ (e.g. oak_leaves and glass
        // have blocks_motion != solid_render, so a motion/render swap fails).
        // Note the anchors do not distinguish the correlated solid_render and
        // can_occlude bits, which are equal on every anchor.
        let state = |name: &str| BlockState::of(BlockId::from_name(name).unwrap());
        let fields = |s: BlockState| {
            (
                s.is_air(),
                s.blocks_motion(),
                s.solid_render(),
                s.can_occlude(),
                s.use_shape_for_light_occlusion(),
                s.propagates_skylight_down(),
                s.random_ticking(),
                s.fluid_empty(),
                s.light_dampening(),
                s.light_emission(),
                s.map_color_id(),
            )
        };
        // Paper 26.2 semantics for the probe anchors. Map colors are the
        // `MapColor` enum ids (STONE=11, WATER=12, FIRE/lava=4, PLANT/leaves=7).
        // Air, glass, and torch all propagate skylight down (bit 5): a
        // non-occluding block lets sky light pass straight through.
        assert_eq!(
            fields(state("minecraft:air")),
            (true, false, false, false, false, true, false, true, 0, 0, 0)
        );
        assert_eq!(
            fields(state("minecraft:stone")),
            (
                false, true, true, true, false, false, false, true, 15, 0, 11
            )
        );
        assert_eq!(
            fields(state("minecraft:water")),
            (
                false, false, false, false, false, false, false, false, 1, 0, 12
            )
        );
        assert_eq!(
            fields(state("minecraft:lava")),
            (
                false, false, false, false, false, false, true, false, 1, 15, 4
            )
        );
        assert_eq!(
            fields(state("minecraft:oak_leaves")),
            (false, true, false, false, false, false, true, true, 1, 0, 7)
        );
        assert_eq!(
            fields(state("minecraft:glass")),
            (false, true, false, false, false, true, false, true, 0, 0, 0)
        );
        assert_eq!(
            fields(state("minecraft:torch")),
            (
                false, false, false, false, false, true, false, true, 0, 14, 0
            )
        );
    }

    #[test]
    fn tag_membership_reads_owning_block() {
        let planks = BlockState::of(BlockId::from_name("minecraft:oak_planks").unwrap());
        assert!(planks.is_in_tag("minecraft:planks"));
        let log = BlockState::of(BlockId::from_name("minecraft:oak_log").unwrap());
        assert!(log.is_in_tag("minecraft:logs"));
        assert!(!log.is_in_tag("minecraft:planks"));
        // Unknown tags read as false, not an error.
        assert!(!log.is_in_tag("minecraft:does_not_exist"));
    }

    #[test]
    fn out_of_range_state_is_valid_false() {
        let bad = BlockState::new(StateId(crate::generated::block_states::BLOCK_STATE_COUNT));
        assert!(!bad.is_valid());
        // Mirror Block.stateById's AIR fallback.
        assert_eq!(bad.block(), BlockId::from_name("minecraft:air").unwrap());
        assert!(bad.is_air());
    }

    #[test]
    fn out_of_range_block_defaults_to_air() {
        // A block id past the table falls back to air's default state, matching
        // Paper's `stateById` (IdMapper.byId returns null out of range and
        // stateById substitutes Blocks.AIR.defaultBlockState()). `of` must not
        // index past BLOCK_STATE_BASES.
        let air = BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        let bad = BlockState::of(BlockId::from_id(u16::MAX));
        assert!(bad.is_valid());
        assert_eq!(bad, air);
        assert_eq!(bad.block(), BlockId::from_name("minecraft:air").unwrap());
        assert!(bad.is_air());
    }

    #[test]
    fn block_state_is_copy_and_hashable() {
        let a = BlockState::of(BlockId::from_name("minecraft:stone").unwrap());
        let b = a; // Copy
        assert_eq!(a, b);
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    /// Sanity: every block's default state decodes to a valid state and its own
    /// block (the id round-trip the worldgen consumers rely on).
    #[test]
    fn every_default_state_round_trips() {
        for (id, name) in crate::generated::blocks::BLOCK_BY_ID.iter().enumerate() {
            let block = BlockId::from_id(id as u16);
            let state = BlockState::of(block);
            assert!(state.is_valid());
            assert_eq!(
                state.block(),
                block,
                "default state of {name} does not round-trip"
            );
        }
    }

    #[test]
    fn display_error_is_actionable() {
        let err = BlockStateError::ValueOutOfRange {
            prop: prop("powered"),
            value: 2,
            count: 2,
        };
        let msg = err.to_string();
        assert!(msg.contains("powered"), "got: {msg}");
        assert!(msg.contains("only 2"), "got: {msg}");
    }
}
