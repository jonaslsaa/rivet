//! Port of `net.minecraft.world.level.levelgen.feature.stateproviders.
//! RandomizedIntStateProvider` (class, 26.2).
//!
//! Java: a provider that takes a `source` state provider, a named integer
//! property, and an `IntProvider` of values. `getState` samples the source,
//! resolves the property on the resulting state's block (lazily cached in
//! `this.property`), and `setValue` the sampled value onto it. `type()` is
//! `BlockStateProviderType.RANDOMIZED_INT_STATE_PROVIDER`.
//!
//! `CODEC` is the 3-field record over `"source"` (`BlockStateProvider.CODEC`,
//! the recursive self threaded from the dispatch graph), `"property"`
//! (`Codec.STRING`), and `"values"` (`IntProviders.CODEC`); the codec
//! constructor is the *string*-property form, which (unlike the
//! `IntegerProperty` form) performs no range validation.
//!
//! `findProperty(BlockState, String)` filters the block's state-definition
//! properties by name and integer kind (`p instanceof IntegerProperty`) — the
//! `PropertyKind::Int` case of the generated property table.

use crate::level::WorldGenLevel;
use crate::levelgen::feature::stateproviders::block_state_provider::{
    BlockStateProvider, ErasedBlockStateProvider, block_state_provider_get_state,
};
use crate::levelgen::feature::stateproviders::block_state_provider_type::{
    BlockStateProviderTypeId, BlockStateProviderTypes,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_property::{Property, PropertyKind};
use rivet_registry::core::BlockPos;
use rivet_registry::state_definition::StateDefinition;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::RandomSource;
use rivet_util::valueproviders::int_provider::{IntProvider, int_provider_codec};
use std::fmt;
use std::sync::{Arc, Mutex};

/// `net.minecraft.world.level.levelgen.feature.stateproviders.
/// RandomizedIntStateProvider`.
pub struct RandomizedIntStateProvider {
    /// `this.source`.
    source: Arc<dyn ErasedBlockStateProvider>,
    /// `this.propertyName`.
    property_name: String,
    /// `this.property` — the lazily-resolved `IntegerProperty`, cached across
    /// calls (Java's mutable `@Nullable` field; `Mutex` gives the interior
    /// mutability `getState`'s `&self` needs while keeping the provider `Sync`).
    property: Mutex<Option<Property>>,
    /// `this.values`.
    values: IntProvider,
}

impl RandomizedIntStateProvider {
    /// `RandomizedIntStateProvider(BlockStateProvider, IntegerProperty,
    /// IntProvider)` — the property form. Validates that every integer in
    /// `values.minInclusive()..=values.maxInclusive()` is a possible value of
    /// the property, throwing `IllegalArgumentException("Property value out of
    /// range: " + property.getName() + ": " + i)` otherwise (the Rust analog
    /// panics with Paper's exact message).
    pub fn new(
        source: Arc<dyn ErasedBlockStateProvider>,
        property: Property,
        values: IntProvider,
    ) -> RandomizedIntStateProvider {
        let property_name = property.name().to_string();
        let possible_values: Vec<i32> = match property.kind() {
            PropertyKind::Int { min, max } => (min..=max).collect(),
            _ => Vec::new(),
        };
        for i in values.min_inclusive()..=values.max_inclusive() {
            if !possible_values.contains(&i) {
                panic!("Property value out of range: {}: {}", property.name(), i);
            }
        }
        RandomizedIntStateProvider {
            source,
            property_name,
            property: Mutex::new(Some(property)),
            values,
        }
    }

    /// `RandomizedIntStateProvider(BlockStateProvider, String, IntProvider)` —
    /// the string-property form the codec constructor uses; no validation (the
    /// property is resolved lazily by `getState`).
    pub fn from_codec(
        source: Arc<dyn ErasedBlockStateProvider>,
        property_name: String,
        values: IntProvider,
    ) -> RandomizedIntStateProvider {
        RandomizedIntStateProvider {
            source,
            property_name,
            property: Mutex::new(None),
            values,
        }
    }

    /// `this.values`.
    pub fn values(&self) -> &IntProvider {
        &self.values
    }
}

impl fmt::Debug for RandomizedIntStateProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RandomizedIntStateProvider")
            .field("source", &self.source)
            .field("property_name", &self.property_name)
            .field("values", &self.values)
            .finish()
    }
}

impl Clone for RandomizedIntStateProvider {
    fn clone(&self) -> Self {
        RandomizedIntStateProvider {
            source: self.source.clone(),
            property_name: self.property_name.clone(),
            property: Mutex::new(*self.property.lock().unwrap()),
            values: self.values.clone(),
        }
    }
}

impl BlockStateProvider for RandomizedIntStateProvider {
    fn get_state<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        random: &mut R,
        pos: &BlockPos,
    ) -> BlockState {
        // `BlockState unmodifiedState = this.source.getState(level, random, pos);`
        let unmodified_state =
            block_state_provider_get_state(self.source.as_ref(), level, random, pos);

        // `if (this.property == null || !unmodifiedState.hasProperty(this.property))`
        let mut property_slot = self.property.lock().unwrap();
        let property = match *property_slot {
            Some(property) if unmodified_state.has_property(property) => property,
            _ => {
                // `IntegerProperty property = findProperty(unmodifiedState,
                // this.propertyName); if (property == null) return
                // unmodifiedState; this.property = property;`
                let property = match find_property(unmodified_state, &self.property_name) {
                    Some(property) => property,
                    None => return unmodified_state,
                };
                *property_slot = Some(property);
                property
            }
        };
        drop(property_slot);

        // `return unmodifiedState.setValue(this.property, this.values.sample(random));`
        let sampled = self.values.sample(random);
        unmodified_state
            .set_value(property, sampled)
            .expect("RandomizedIntStateProvider set a value the property allows")
    }

    fn type_id(&self) -> BlockStateProviderTypeId {
        BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `RandomizedIntStateProvider.findProperty(BlockState, String)` — the first
/// property on the state's block definition named `propertyName` that is an
/// `IntegerProperty` (the `PropertyKind::Int` case), or `None` (Java `null`).
fn find_property(state: BlockState, property_name: &str) -> Option<Property> {
    let definition = StateDefinition::for_block(state.block());
    definition.properties().into_iter().find(|property| {
        property.name() == property_name && matches!(property.kind(), PropertyKind::Int { .. })
    })
}

/// `RandomizedIntStateProvider.CODEC` — the 3-field record
/// (`"source"`/`"property"`/`"values"`), as the ops-generic
/// `randomized_int_state_provider_map_codec::<Ops>(top)` factory. `top` is the
/// `BlockStateProvider.CODEC` `RecursiveSelf` from the dispatch graph, so a
/// nested `source` round-trips through the single recursive codec.
pub fn randomized_int_state_provider_map_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Arc<dyn ErasedBlockStateProvider>, Ops>>,
) -> Arc<dyn MapCodec<RandomizedIntStateProvider, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|r: &RandomizedIntStateProvider| r.source.clone()),
                // `BlockStateProvider.CODEC.fieldOf("source")`.
                codec::field_of::<Arc<dyn ErasedBlockStateProvider>, Ops>(
                    top,
                    "source".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &RandomizedIntStateProvider| r.property_name.clone()),
                // `Codec.STRING.fieldOf("property")`.
                codec::field_of::<String, Ops>(
                    codec::string_codec::<Ops>(),
                    "property".to_string(),
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|r: &RandomizedIntStateProvider| r.values.clone()),
                // `IntProviders.CODEC.fieldOf("values")`.
                codec::field_of::<IntProvider, Ops>(
                    int_provider_codec::<Ops>(),
                    "values".to_string(),
                ),
            ))
            .apply(
                instance,
                Arc::new(
                    |source: Arc<dyn ErasedBlockStateProvider>,
                     property_name: String,
                     values: IntProvider| {
                        RandomizedIntStateProvider::from_codec(source, property_name, values)
                    },
                ),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::generated::blocks::BlockId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use serde_json::json;

    /// The test ops: a `RegistryOps` over JSON (the only ops implementing
    /// `RegistryOpsLookup`, required by the recursive `BlockStateProvider.CODEC`).
    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    fn test_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn air() -> BlockState {
        BlockState::of(BlockId::from_id(0))
    }

    fn simple_source(state: BlockState) -> Arc<dyn ErasedBlockStateProvider> {
        Arc::new(crate::levelgen::feature::stateproviders::simple_state_provider::SimpleStateProvider::new(
            state,
        )) as Arc<dyn ErasedBlockStateProvider>
    }

    #[test]
    fn codec_round_trips_the_record() {
        // `source` recurses through `BlockStateProvider.CODEC`, so the map codec
        // is threaded the real recursive codec and parsed with `RegistryOps`
        // (the only ops implementing `RegistryOpsLookup`, which the dispatch
        // graph requires for the embedded `BlockPredicate.CODEC`).
        let codec =
            rivet_serialization::map_codec::codec_of(randomized_int_state_provider_map_codec::<
                TestOps,
            >(
                super::super::block_state_provider_codec::<TestOps>(),
            ));
        let input = json!({
            "source": {"type": "minecraft:simple_state_provider", "state": {"Properties": {"axis": "y"}, "Name": "minecraft:oak_log"}},
            "property": "axis",
            "values": 1
        });
        let decoded_result = codec.parse(&test_ops(), &input);
        let decoded = decoded_result.result().expect("decode should succeed");
        assert_eq!(
            BlockStateProvider::type_id(decoded),
            BlockStateProviderTypes::RANDOMIZED_INT_STATE_PROVIDER
        );
        assert_eq!(decoded.property_name, "axis");
        assert_eq!(decoded.values, IntProvider::Constant(ConstantInt::of(1)));
        let encoded = codec
            .encode_start(&test_ops(), decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, input);
    }

    #[test]
    fn get_state_sets_the_sampled_value_on_the_source_state() {
        // Source is a constant `oak_log` (whose `axis` property is an integer
        // — no, it's an enum; use `age`-style: `oak_leaves` has `distance`).
        // `oak_leaves`'s `distance` property is `IntegerProperty(1, 7)`.
        let leaves = BlockId::from_name("minecraft:oak_leaves").expect("oak_leaves block exists");
        let p = RandomizedIntStateProvider::from_codec(
            simple_source(BlockState::of(leaves)),
            "distance".to_string(),
            IntProvider::Constant(ConstantInt::of(3)),
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let state = p.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert_eq!(state.block(), leaves);
        let distance = state
            .get_value(rivet_registry::block_state_properties::BlockStateProperties::DISTANCE)
            .expect("distance property set");
        assert_eq!(
            distance,
            rivet_registry::block_state_property::PropertyValue::Int(3)
        );
    }

    #[test]
    fn get_state_returns_source_unchanged_when_property_is_absent() {
        // `air` has no `distance` property → `findProperty` returns None →
        // the unmodified source state is returned.
        let p = RandomizedIntStateProvider::from_codec(
            simple_source(air()),
            "distance".to_string(),
            IntProvider::Constant(ConstantInt::of(3)),
        );
        let mut random = rivet_util::random::LegacyRandomSource::new(1);
        let state = p.get_state(&TestLevel, &mut random, &BlockPos::new(0, 0, 0));
        assert_eq!(state, air());
    }

    #[test]
    fn property_form_validates_the_value_range() {
        let leaves = BlockId::from_name("minecraft:oak_leaves").expect("oak_leaves block exists");
        let distance = StateDefinition::for_block(leaves)
            .get_property("distance")
            .expect("distance property on oak_leaves");
        // A valid range constructs fine.
        let _valid = RandomizedIntStateProvider::new(
            simple_source(BlockState::of(leaves)),
            distance,
            IntProvider::Constant(ConstantInt::of(3)),
        );
        // Out-of-range throws with Paper's exact message.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            RandomizedIntStateProvider::new(
                simple_source(BlockState::of(leaves)),
                distance,
                IntProvider::Constant(ConstantInt::of(99)),
            )
        }));
        let panic = result.expect_err("construction should panic");
        let msg = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        assert_eq!(msg, "Property value out of range: distance: 99");
    }

    struct TestLevel;

    impl LevelHeightAccessor for TestLevel {
        fn get_height(&self) -> i32 {
            384
        }

        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for TestLevel {
        fn get_seed(&self) -> i64 {
            0
        }

        fn get_block_state(&self, _pos: &BlockPos) -> BlockState {
            // RivetTodo(#399): never read here.
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
    }
}
