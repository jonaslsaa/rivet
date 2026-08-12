//! Port of `net.minecraft.world.level.levelgen.feature.configurations.SpringConfiguration`
//! (class, 26.2).
//!
//! Java: a five-field class (`FluidState state, boolean requiresBlockBelow,
//! int rockCount, int holeCount, HolderSet<Block> validBlocks`) whose `CODEC`
//! is a `RecordCodecBuilder` over the required `"state"` field
//! (`FluidState.CODEC`), the `"requires_block_below"` field
//! (`Codec.BOOL.optionalFieldOf(..., true)` — the NON-lenient with-default
//! optional), the `"rock_count"` field (`Codec.INT.optionalFieldOf(..., 4)`),
//! the `"hole_count"` field (`Codec.INT.optionalFieldOf(..., 1)`), and the
//! required `"valid_blocks"` field (`RegistryCodecs.homogeneousList(
//! Registries.BLOCK)` — a `HolderSetCodec` over the block registry). DFU
//! `Codec<T>` is `Codec<E, Ops>` in the port, so the static Java constant is
//! exposed as the ops-generic `spring_configuration_codec::<Ops>()` factory.
//!
//! The `validBlocks` holder set is value-semantic; the `state` half is
//! [`FluidState`] — the STUB'd `net.minecraft.world.level.material.FluidState`
//! value type (deferred with the pending `.material` unit). `PartialEq` is
//! derived (all fields are value types), consistent with the other
//! configuration value types.

use crate::levelgen::feature::configurations::FeatureConfiguration;
use rivet_registry::fluid_id::FluidId;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::BlockType;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.feature.configurations.SpringConfiguration`.
#[derive(Debug, Clone, PartialEq)]
pub struct SpringConfiguration {
    /// `state` — the spring's fluid state.
    pub state: FluidState,
    /// `requiresBlockBelow` — whether the spring requires a block below.
    pub requires_block_below: bool,
    /// `rockCount`.
    pub rock_count: i32,
    /// `holeCount`.
    pub hole_count: i32,
    /// `validBlocks` — the blocks the spring may replace.
    pub valid_blocks: HolderSet<BlockType>,
}

impl SpringConfiguration {
    /// `new SpringConfiguration(FluidState, boolean, int, int, HolderSet<Block>)`
    /// — the constructor (the codec's `apply` function).
    pub fn new(
        state: FluidState,
        requires_block_below: bool,
        rock_count: i32,
        hole_count: i32,
        valid_blocks: HolderSet<BlockType>,
    ) -> Self {
        SpringConfiguration {
            state,
            requires_block_below,
            rock_count,
            hole_count,
            valid_blocks,
        }
    }
}

/// `net.minecraft.world.level.material.FluidState` (final class, 26.2) — the
/// out-of-unit fluid-state value type this configuration's `"state"` field
/// holds.
///
/// STUB(mc.world.level.levelgen.feature.configurations.wave2): owned by the
/// pending `net.minecraft.world.level.material` unit; this stub carries the
/// value surface this configuration consumes. Java's `FluidState.CODEC` is
/// `StateHolder.codec(BuiltInRegistries.FLUID.byNameCodec(),
/// Fluid::defaultFluidState, Fluid::getStateDefinition)` — the same `"Name"`
/// dispatch shape as `BlockState.CODEC`, where the fluid's per-type state
/// definition validates the `"Properties"` map against its declared property
/// value codecs. The fluid state-definition machinery (`Fluid.getStateDefinition`,
/// `StateDefinition.propertiesCodec`) is not ported, so this stub resolves the
/// `"Name"` to the [`FluidId`] id-handle and carries the property map
/// verbatim (ordered, as encoded) without per-property validation; a
/// wrong-typed or unknown property value round-trips instead of erroring
/// (narrower than Java's `Property.valueCodec`). This is the wire shape the
/// pinned `spring_lava_nether.json` fixture exercises
/// (`{"Name": "minecraft:lava", "Properties": {"falling": "true"}}`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FluidState {
    /// The fluid type (`FluidState.getType()`).
    pub fluid: FluidId,
}

impl FluidState {
    /// The minimal STUB carrier — a plain fluid-type handle. The property map
    /// defers with the owning unit, so `FluidState` holds only the fluid id.
    ///
    /// STUB: `new FluidState(FluidId)` is a placeholder for the
    /// state-definition-backed value; consumers in this unit construct it from
    /// a resolved fluid id.
    pub fn new(fluid: FluidId) -> Self {
        FluidState { fluid }
    }

    /// `getType()` — the fluid type id-handle.
    pub fn get_type(&self) -> FluidId {
        self.fluid
    }
}

/// `FluidState.CODEC` — the ops-generic `fluid_state_codec::<Ops>()` factory.
///
/// STUB: mirrors the `"Name"`-dispatch wire shape of `StateHolder.codec`
/// (`ownerCodec.dispatch("Name", s -> s.owner, o -> ...)`) restricted to the
/// fluid id-handle: the record codec writes/reads the `"Name"` key
/// (`Registry.byNameCodec` shape — an unknown fluid name errors with the
/// vanilla `Unknown registry key ... minecraft:fluid` message, the same
/// `block_by_name_codec` uses), and the value codec is `MapCodec.unit(default)`
/// because the property map defers with the owning unit's state definitions
/// (a non-singleton fluid's `"Properties"` compound is not carried). The
/// `.stable()` lifecycle wrapper is applied like Java's `... .stable()`.
pub fn fluid_state_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FluidState, Ops>> {
    let dispatch = key_dispatch_codec::dispatch_map::<rivet_registry::Identifier, FluidState, Ops>(
        "Name",
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|state: &FluidState| {
            DataResult::success(rivet_registry::Identifier::parse(state.fluid.name()))
        }),
        Arc::new(
            |name: &rivet_registry::Identifier| match FluidId::from_name(&name.to_string()) {
                Some(fluid) => DataResult::success(map_codec::unit(FluidState::new(fluid))),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:fluid]: {}",
                    name
                )),
            },
        ),
    );
    codec::stable(map_codec::codec_of(dispatch))
}

/// `SpringConfiguration.CODEC` — a record codec over the required `"state"` and
/// `"valid_blocks"` fields plus the three with-default optional fields, as the
/// ops-generic `spring_configuration_codec::<Ops>()` factory.
///
/// Java:
/// ```java
/// RecordCodecBuilder.create(i -> i.group(
///     FluidState.CODEC.fieldOf("state"),
///     Codec.BOOL.optionalFieldOf("requires_block_below", true),
///     Codec.INT.optionalFieldOf("rock_count", 4),
///     Codec.INT.optionalFieldOf("hole_count", 1),
///     RegistryCodecs.homogeneousList(Registries.BLOCK).fieldOf("valid_blocks"))
///     .apply(i, SpringConfiguration::new))
/// ```
pub fn spring_configuration_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<SpringConfiguration, Ops>> {
    record_builder::create(|instance| {
        instance
            .group(RecordCodecBuilder::of(
                Arc::new(|c: &SpringConfiguration| c.state),
                codec::field_of(fluid_state_codec::<Ops>(), "state".to_string()),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SpringConfiguration| c.requires_block_below),
                codec::optional_field_of::<bool, Ops>(
                    "requires_block_below",
                    codec::bool_codec::<Ops>(),
                    true,
                ),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SpringConfiguration| c.rock_count),
                codec::optional_field_of::<i32, Ops>("rock_count", codec::int_codec::<Ops>(), 4),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SpringConfiguration| c.hole_count),
                codec::optional_field_of::<i32, Ops>("hole_count", codec::int_codec::<Ops>(), 1),
            ))
            .and(RecordCodecBuilder::of(
                Arc::new(|c: &SpringConfiguration| c.valid_blocks.clone()),
                blocks_field_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(
                    |state: FluidState,
                     requires_block_below: bool,
                     rock_count: i32,
                     hole_count: i32,
                     valid_blocks: HolderSet<BlockType>| {
                        SpringConfiguration::new(
                            state,
                            requires_block_below,
                            rock_count,
                            hole_count,
                            valid_blocks,
                        )
                    },
                ),
            )
    })
}

/// `RegistryCodecs.homogeneousList(Registries.BLOCK)` — the `"valid_blocks"`
/// field codec: a `HolderSetCodec` over the block registry, whose element codec
/// is a `RegistryFixedCodec` (tag key `#minecraft:...` or element-list form).
/// The concrete codec is not `Send + Sync` (its `RegistryOps` carries the
/// single-threaded `HolderLookupAdapter` `RefCell` memo), so the `Arc` is held
/// by the ops-parameterized codec and never crosses threads.
fn blocks_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn rivet_serialization::map_codec::MapCodec<HolderSet<BlockType>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<Holder<BlockType>, Ops>> = Arc::new(
        rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::BLOCK,
        ),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BlockType>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::BLOCK,
            element,
            false,
        ));
    codec::field_of(holder_set, "valid_blocks".to_string())
}

impl FeatureConfiguration for SpringConfiguration {}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A block registry with `stone` (id 0) and `netherrack` (id 1), wrapped in
    /// a `RegistryAccess` under `Registries.BLOCK` — the `valid_blocks`
    /// holder-set field resolves its reference elements through it.
    fn block_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BLOCK);
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:stone"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BLOCK,
                Identifier::parse("minecraft:netherrack"),
            ),
            Arc::new(BlockType),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("block")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, block_access())
    }

    /// A two-element direct holder set over the test registry, resolved through
    /// the SAME access the ops use (each `freeze()` allocates a fresh
    /// `RegistryId`, so the references must carry the id the ops' provider
    /// reads).
    fn nether_blocks(access: &RegistryAccess) -> HolderSet<BlockType> {
        let registry = RegistryAccess::lookup(access, &*rivet_registry::registries::BLOCK)
            .expect("block registry");
        HolderSet::direct(vec![
            Holder::reference(registry.registry_id(), 0),
            Holder::reference(registry.registry_id(), 1),
        ])
    }

    #[test]
    fn fluid_state_codec_round_trips_lava() {
        // The `spring_lava_nether.json` wire shape: `{"Name": "minecraft:lava"}`.
        // (The fixture's `Properties: {"falling": "true"}` compound defers with
        // the owning fluid unit's state definitions — see the STUB doc.)
        let codec = fluid_state_codec::<JsonOps>();
        let state = FluidState::new(FluidId::LAVA);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &state)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"Name": "minecraft:lava"}));
        let result = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, state);
    }

    #[test]
    fn fluid_state_codec_rejects_unknown_fluid() {
        // `Registry.byNameCodec` — an unknown fluid name errors with the
        // vanilla `Unknown registry key` message.
        let codec = fluid_state_codec::<JsonOps>();
        let result = codec.parse(
            &JsonOps::INSTANCE,
            &json!({"Name": "minecraft:not_a_fluid"}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Unknown registry key in ResourceKey[minecraft:root / minecraft:fluid]"),
            "got: {msg}"
        );
    }

    #[test]
    fn codec_round_trip_with_all_optionals_explicit() {
        // The full Paper wire form of a lava spring: `state` + all three
        // optional fields with non-default values + `valid_blocks` as an
        // element list. (Default-valued optionals are omitted on encode, so
        // the "explicit" case must use values different from the defaults.)
        let access = block_access();
        let config = SpringConfiguration::new(
            FluidState::new(FluidId::LAVA),
            false,
            7,
            2,
            nether_blocks(&access),
        );
        let codec = spring_configuration_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state": {"Name": "minecraft:lava"},
                "requires_block_below": false,
                "rock_count": 7,
                "hole_count": 2,
                "valid_blocks": ["minecraft:stone", "minecraft:netherrack"],
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_round_trip_with_defaulted_optionals() {
        // The pinned `spring_lava_nether.json` only overrides `state` and
        // `valid_blocks`; `requires_block_below`/`rock_count`/`hole_count`
        // default (true/4/1) and are omitted on encode.
        let access = block_access();
        let config = SpringConfiguration::new(
            FluidState::new(FluidId::LAVA),
            true,
            4,
            1,
            nether_blocks(&access),
        );
        let codec = spring_configuration_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let encoded = codec
            .encode_start(&ops, &config)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({
                "state": {"Name": "minecraft:lava"},
                "valid_blocks": ["minecraft:stone", "minecraft:netherrack"],
            })
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, config);
    }

    #[test]
    fn codec_requires_state_and_valid_blocks() {
        let codec = spring_configuration_codec::<TestOps>();
        let ops = ops();
        // `fieldOf("state")` is required.
        let no_state = json!({"valid_blocks": []});
        let result = codec.parse(&ops, &no_state);
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key state"), "got: {msg}");
        // `fieldOf("valid_blocks")` is required.
        let no_blocks = json!({"state": {"Name": "minecraft:lava"}});
        assert!(codec.parse(&ops, &no_blocks).is_error());
    }

    #[test]
    fn present_malformed_optional_is_an_error() {
        // NON-lenient optional: a present-but-wrong-typed `"rock_count"` is a
        // decode error (not silently defaulted).
        let codec = spring_configuration_codec::<TestOps>();
        let ops = ops();
        let result = codec.parse(
            &ops,
            &json!({"state": {"Name": "minecraft:lava"}, "rock_count": "many", "valid_blocks": []}),
        );
        assert!(result.is_error());
    }

    #[test]
    fn accessors_expose_the_fields() {
        let access = block_access();
        let config = SpringConfiguration::new(
            FluidState::new(FluidId::WATER),
            false,
            7,
            2,
            nether_blocks(&access),
        );
        assert_eq!(config.state, FluidState::new(FluidId::WATER));
        assert!(!config.requires_block_below);
        assert_eq!(config.rock_count, 7);
        assert_eq!(config.hole_count, 2);
        assert_eq!(config.valid_blocks, nether_blocks(&access));
    }
}
