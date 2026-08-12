//! Port of `net.minecraft.world.level.block.state.predicate.BlockStatePredicate`
//! (MC 26.2).
//!
//! Java:
//! ```java
//! public class BlockStatePredicate implements Predicate<BlockState> {
//!     public static final Predicate<BlockState> ANY = input -> true;
//!     private final StateDefinition<Block, BlockState> definition;
//!     private final Map<Property<?>, Predicate<Object>> properties = Maps.newHashMap();
//!     private BlockStatePredicate(final StateDefinition<Block, BlockState> definition) {
//!         this.definition = definition;
//!     }
//!     public static BlockStatePredicate forBlock(final Block block) {
//!         return new BlockStatePredicate(block.getStateDefinition());
//!     }
//!     @Override public boolean test(final @Nullable BlockState input) {
//!         if (input != null && input.getBlock().equals(this.definition.getOwner())) {
//!             if (this.properties.isEmpty()) return true;
//!             for (Entry<Property<?>, Predicate<Object>> entry : this.properties.entrySet())
//!                 if (!this.applies(input, entry.getKey(), entry.getValue())) return false;
//!             return true;
//!         } else return false;
//!     }
//!     protected <T extends Comparable<T>> boolean applies(
//!             final BlockState input, final Property<T> key, final Predicate<Object> predicate) {
//!         T value = input.getValue(key);
//!         return predicate.test(value);
//!     }
//!     public <V extends Comparable<V>> BlockStatePredicate where(
//!             final Property<V> property, final Predicate<Object> predicate) {
//!         if (!this.definition.getProperties().contains(property))
//!             throw new IllegalArgumentException(this.definition + " cannot support property " + property);
//!         this.properties.put(property, predicate);
//!         return this;
//!     }
//! }
//! ```

use crate::block::Block;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_property::{Property, PropertyValue};
use rivet_registry::state_definition::StateDefinition;
use std::collections::HashMap;

use super::StatePredicate;

/// The anonymous `Predicate<BlockState>` constant `BlockStatePredicate.ANY`
/// (`input -> true`). It is a separate implementation in Java — typed as
/// `Predicate<BlockState>`, not `BlockStatePredicate` — so it is a distinct
/// unit struct here rather than a `BlockStatePredicate`.
///
/// The value is also exposed as [`BlockStatePredicate::ANY`], so a caller
/// porting `BlockStatePredicate.ANY` (e.g. `EndPortalFrameBlock.java:88`)
/// finds the name at the same path as Java.
#[derive(Clone, Copy, Debug)]
pub struct AnyBlockState;

impl StatePredicate for AnyBlockState {
    /// `input -> true` — matches every input, including `null`.
    fn test(&self, _input: Option<&BlockState>) -> bool {
        true
    }
}

/// `Predicate<Object>` on a property's typed value — Java's
/// `Map<Property<?>, Predicate<Object>>` value (the `Object` is the value
/// `input.getValue(property)` yields).
type PropertyPredicate = Box<dyn Fn(&PropertyValue) -> bool>;

/// `net.minecraft.world.level.block.state.predicate.BlockStatePredicate` — the
/// `Predicate<BlockState>` matching states of one block (by its
/// `StateDefinition`), optionally constrained per-`Property` via `where`.
pub struct BlockStatePredicate {
    definition: StateDefinition,
    // Java: `Map<Property<?>, Predicate<Object>> properties = Maps.newHashMap()`.
    // `HashMap` reproduces `put`'s replace-on-re-insert. Iteration order is not
    // observable: every predicate must pass for `test` to return true, so
    // Java's unspecified `HashMap` order cannot change the result.
    properties: HashMap<Property, PropertyPredicate>,
}

impl std::fmt::Debug for BlockStatePredicate {
    /// The boxed predicates are opaque, so the property keys sorted by their
    /// generated id and the definition are shown; `HashMap` iteration order is
    /// not observable here (Java `Map` has no ordering either).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<Property> = self.properties.keys().copied().collect();
        keys.sort_unstable_by_key(|p| p.id() as u16);
        f.debug_struct("BlockStatePredicate")
            .field("definition", &state_definition_to_string(self.definition))
            .field("properties", &keys)
            .finish()
    }
}

impl BlockStatePredicate {
    /// `BlockStatePredicate.ANY` — the static `Predicate<BlockState>`
    /// `input -> true` field, exposed here so the Java name resolves at the same
    /// path (`EndPortalFrameBlock.java:88` uses `BlockStatePredicate.ANY`).
    /// The value's type is [`AnyBlockState`], a distinct implementation (as in
    /// Java, where `ANY` is a separate anonymous `Predicate`, not a
    /// `BlockStatePredicate`).
    pub const ANY: AnyBlockState = AnyBlockState;

    /// `BlockStatePredicate.forBlock(Block)` —
    /// `new BlockStatePredicate(block.getStateDefinition())`.
    pub fn for_block(block: Block) -> Self {
        BlockStatePredicate {
            definition: block.state_definition(),
            properties: HashMap::new(),
        }
    }

    /// `applies(BlockState, Property<T>, Predicate<Object>)` — `T value =
    /// input.getValue(key); return predicate.test(value)`. The value is present
    /// because `test` verified the input's block matches the definition and
    /// `where` verified the property is on that definition.
    fn applies(
        &self,
        input: &BlockState,
        key: Property,
        predicate: &dyn Fn(&PropertyValue) -> bool,
    ) -> bool {
        let value = input.get_value(key).expect(
            "block-state predicate property is on the tested state's block (validated by `where`)",
        );
        predicate(&value)
    }

    /// `where(Property<V>, Predicate<Object>)` — Java throws
    /// `IllegalArgumentException` when the property is not on the definition
    /// (`this.definition + " cannot support property " + property`); the port
    /// panics with the same message shape. The `StateDefinition{block=…,
    /// properties=[…]}` prefix matches Java byte-for-byte; the property tail
    /// diverges for `Enum` properties, whose `Display for Property` here omits
    /// the `clazz=class …` field Java's `Property.toString()` renders — Java
    /// prints `EnumProperty{name=axis, clazz=class
    /// net.minecraft.core.Direction$Axis, values=[x, y, z]}` where the port
    /// prints `EnumProperty{name=axis, values=[x, y, z]}` (the id-keyed
    /// registry model does not retain the enum FQN — see
    /// `rivet_registry::block_state_property`).
    ///
    /// Java returns `this` and mutates in place; the port consumes `self` and
    /// returns the updated value. This is a deliberate API-shape choice
    /// consistent with the crate's immutable-value convention
    /// (`BlockState::set_value` consumes and returns): it keeps the dominant
    /// fluent chains of every Java consumer (`forBlock(x).where(a).where(b)`
    /// binds the final value) compiling unchanged. Java's statement-style
    /// accumulation `p.where(a); p.where(b);` on one binding has no silent Rust
    /// equivalent — each `where` moves `self`, so the second call on the moved
    /// `p` is a use-after-move compile error, forcing a porter to rebind
    /// (`p = p.r#where(a);`) or to chain. `#[must_use]` additionally turns a
    /// single discarded `p.r#where(a);` into a compiler warning rather than a
    /// silently dropped constraint.
    #[must_use]
    pub fn r#where(mut self, property: Property, predicate: PropertyPredicate) -> Self {
        if !self.definition.properties().contains(&property) {
            panic!(
                "{} cannot support property {}",
                state_definition_to_string(self.definition),
                property
            );
        }
        self.properties.insert(property, predicate);
        self
    }
}

impl StatePredicate for BlockStatePredicate {
    /// `test(@Nullable BlockState)` — matches states whose block is the
    /// definition's owner, then every `where` predicate in the map.
    fn test(&self, input: Option<&BlockState>) -> bool {
        let Some(input) = input else {
            return false;
        };
        if input.block() != self.definition.block() {
            return false;
        }
        if self.properties.is_empty() {
            return true;
        }
        for (&property, predicate) in &self.properties {
            if !self.applies(input, property, predicate.as_ref()) {
                return false;
            }
        }
        true
    }
}

/// `StateDefinition.toString()` — `MoreObjects.toStringHelper(this)
/// .add("block", owner).add("properties", getProperties() names)`, where the
/// owner (`Block`) renders as `Block{<registered name>}`.
fn state_definition_to_string(def: StateDefinition) -> String {
    let names: Vec<&str> = def.properties().iter().map(|p| p.name()).collect();
    format!(
        "StateDefinition{{block=Block{{{}}}, properties=[{}]}}",
        def.block().name(),
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::block::state::predicate::BlockPredicate;
    use rivet_registry::block_state_properties::BlockStateProperties;
    use rivet_registry::core::Axis;

    fn oak_log_axis(value: Axis) -> BlockState {
        Blocks::OAK_LOG
            .default_block_state()
            .set_value(BlockStateProperties::AXIS, value)
            .expect("axis is on oak_log")
    }

    /// An oak_leaves state with the given `distance` (Int 1..=7) and
    /// `waterlogged` (Bool) — defaults are distance=7, waterlogged=false,
    /// persistent=false.
    fn oak_leaves_state(distance: i32, waterlogged: bool) -> BlockState {
        Blocks::OAK_LEAVES
            .default_block_state()
            .set_value(BlockStateProperties::DISTANCE, distance)
            .expect("distance is on oak_leaves")
            .set_value(BlockStateProperties::WATERLOGGED, waterlogged)
            .expect("waterlogged is on oak_leaves")
    }

    #[test]
    fn any_matches_everything() {
        assert!(AnyBlockState.test(Some(&Blocks::AIR.default_block_state())));
        assert!(AnyBlockState.test(Some(&Blocks::STONE.default_block_state())));
        assert!(AnyBlockState.test(None));
    }

    #[test]
    fn any_constant_is_exposed_at_the_java_name() {
        // `BlockStatePredicate.ANY` (EndPortalFrameBlock.java:88) resolves here.
        assert!(BlockStatePredicate::ANY.test(Some(&Blocks::AIR.default_block_state())));
        assert!(BlockStatePredicate::ANY.test(Some(&Blocks::STONE.default_block_state())));
        assert!(BlockStatePredicate::ANY.test(None));
    }

    #[test]
    fn matches_only_its_block_and_no_properties_is_all_states() {
        let p = BlockStatePredicate::for_block(Blocks::OAK_LOG);
        // No `where` constraints: every state of oak_log matches.
        assert!(p.test(Some(&oak_log_axis(Axis::X))));
        assert!(p.test(Some(&oak_log_axis(Axis::Y))));
        // A state of a different block fails, as does Java's `null` input.
        assert!(!p.test(Some(&Blocks::STONE.default_block_state())));
        assert!(!p.test(None));
    }

    #[test]
    fn where_constrains_by_property_value() {
        let p = BlockStatePredicate::for_block(Blocks::OAK_LOG).r#where(
            BlockStateProperties::AXIS,
            Box::new(|v| matches!(*v, PropertyValue::Enum("x"))),
        );
        assert!(p.test(Some(&oak_log_axis(Axis::X))));
        assert!(!p.test(Some(&oak_log_axis(Axis::Y))));
        assert!(!p.test(Some(&oak_log_axis(Axis::Z))));
    }

    #[test]
    fn where_replaces_a_previous_predicate_for_the_same_property() {
        // Java `Map.put` replaces on the same key: the second `where(AXIS, …)`
        // wins, so every state matches.
        let p = BlockStatePredicate::for_block(Blocks::OAK_LOG)
            .r#where(BlockStateProperties::AXIS, Box::new(|_| false))
            .r#where(BlockStateProperties::AXIS, Box::new(|_| true));
        assert!(p.test(Some(&oak_log_axis(Axis::X))));
        assert!(p.test(Some(&oak_log_axis(Axis::Y))));
    }

    #[test]
    fn where_conjoins_two_distinct_properties() {
        // Java `where` puts both (property, predicate) pairs in the map; `test`
        // requires every entry to pass, so the constraints combine by
        // conjunction. DISTANCE (Int) and WATERLOGGED (Bool) are distinct
        // properties on oak_leaves.
        let p = BlockStatePredicate::for_block(Blocks::OAK_LEAVES)
            .r#where(
                BlockStateProperties::DISTANCE,
                Box::new(|v| *v == PropertyValue::Int(1)),
            )
            .r#where(
                BlockStateProperties::WATERLOGGED,
                Box::new(|v| *v == PropertyValue::Bool(true)),
            );
        assert!(p.test(Some(&oak_leaves_state(1, true))));
        assert!(!p.test(Some(&oak_leaves_state(7, true))));
        assert!(!p.test(Some(&oak_leaves_state(1, false))));
        assert!(!p.test(Some(&oak_leaves_state(7, false))));
    }

    #[test]
    fn where_on_a_bool_property() {
        // WATERLOGGED is a BooleanProperty; the predicate receives the Bool
        // value (`input.getValue(key)` yields a Boolean).
        let p = BlockStatePredicate::for_block(Blocks::OAK_LEAVES).r#where(
            BlockStateProperties::WATERLOGGED,
            Box::new(|v| *v == PropertyValue::Bool(true)),
        );
        assert!(p.test(Some(&oak_leaves_state(7, true))));
        assert!(!p.test(Some(&oak_leaves_state(7, false))));
    }

    #[test]
    fn where_on_an_int_property() {
        // DISTANCE is an IntegerProperty (1..=7); the predicate receives the
        // Int value. The default oak_leaves distance is 7, so a `== 1`
        // predicate rejects the default state.
        let p = BlockStatePredicate::for_block(Blocks::OAK_LEAVES).r#where(
            BlockStateProperties::DISTANCE,
            Box::new(|v| *v == PropertyValue::Int(1)),
        );
        assert!(p.test(Some(&oak_leaves_state(1, false))));
        assert!(!p.test(Some(&oak_leaves_state(7, false))));
    }

    #[test]
    fn any_combines_through_the_state_predicate_combinators() {
        // ANY is a full StatePredicate (its own anonymous implementation in
        // Java), so it composes with `and`/`or`/`negate` like any other
        // predicate. `and` short-circuits on the right side; `or` short-circuits
        // on ANY's constant true.
        let stone = BlockPredicate::for_block(Blocks::STONE);
        let and = BlockStatePredicate::ANY.and(stone);
        assert!(and.test(Some(&Blocks::STONE.default_block_state())));
        assert!(!and.test(Some(&Blocks::DIRT.default_block_state())));
        assert!(!and.test(None));

        let or = BlockStatePredicate::ANY.or(stone);
        assert!(or.test(Some(&Blocks::STONE.default_block_state())));
        assert!(or.test(Some(&Blocks::DIRT.default_block_state())));
        assert!(or.test(None));

        let negated = BlockStatePredicate::ANY.negate();
        assert!(!negated.test(Some(&Blocks::STONE.default_block_state())));
        assert!(!negated.test(None));
    }

    #[test]
    fn where_on_a_missing_property_panics() {
        // stone has no properties; `where(AXIS, …)` panics where Java throws
        // `IllegalArgumentException` with the `StateDefinition`-based message.
        let result = std::panic::catch_unwind(|| {
            // `#[must_use]` would warn on the discarded value — deliberately
            // silenced: the panic is the point of this test.
            let _ = BlockStatePredicate::for_block(Blocks::STONE)
                .r#where(BlockStateProperties::AXIS, Box::new(|_| true));
        });
        let err = result.expect_err("where with an unsupported property must panic");
        let msg = err
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .expect("panic payload is a string");
        // The `StateDefinition{block=…, properties=[…]}` prefix matches Java
        // byte-for-byte. The `EnumProperty` tail omits the `clazz=class …`
        // field Java's `Property.toString()` renders — Java would print
        // `EnumProperty{name=axis, clazz=class net.minecraft.core.Direction
        // $Axis, values=[x, y, z]}` (documented divergence in
        // `rivet_registry::block_state_property`).
        assert_eq!(
            msg,
            "StateDefinition{block=Block{minecraft:stone}, properties=[]} \
             cannot support property EnumProperty{name=axis, values=[x, y, z]}"
        );
    }
}
