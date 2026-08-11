//! Port of `net.minecraft.world.level.levelgen.blockpredicates.MatchingBiomesPredicate`
//! (class, 26.2).
//!
//! Java: a `BlockPredicate` (not state-testing) whose `test` is
//! `this.biomes.contains(worldGenLevel.getBiome(blockPos))` and whose `type()`
//! is `BlockPredicateType.MATCHING_BIOMES`. Its `CODEC` is a single required
//! `"biomes"` field — `RegistryCodecs.homogeneousList(Registries.BIOME)`, a
//! `HolderSetCodec` whose element codec is `RegistryFixedCodec(Registries.BIOME)`.
//!
//! `Biome` is the id-handle placeholder [`BiomeId`]; the biome read is the
//! [`WorldGenLevel::get_biome`] seam (RivetTodo #399 — unavailable until the
//! world-access lands, then failing explicitly rather than fabricating).

use crate::level::WorldGenLevel;
use crate::levelgen::blockpredicates::block_predicate::BlockPredicate;
use crate::levelgen::blockpredicates::block_predicate_type::{
    BlockPredicateTypeId, BlockPredicateTypes,
};
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::BlockPos;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.blockpredicates.MatchingBiomesPredicate`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchingBiomesPredicate {
    /// `this.biomes` — the matching biome holder set.
    biomes: HolderSet<BiomeId>,
}

impl MatchingBiomesPredicate {
    /// `new MatchingBiomesPredicate(HolderSet<Biome>)`.
    pub fn new(biomes: HolderSet<BiomeId>) -> Self {
        MatchingBiomesPredicate { biomes }
    }

    /// `this.biomes`.
    pub fn biomes(&self) -> &HolderSet<BiomeId> {
        &self.biomes
    }
}

impl BlockPredicate for MatchingBiomesPredicate {
    fn test(&self, level: &dyn WorldGenLevel, origin: &BlockPos) -> bool {
        // `this.biomes.contains(worldGenLevel.getBiome(blockPos))`. The biome
        // read is the `#399` world-access seam — no production world provides
        // it yet, so this panics (never fabricating a biome) until the world
        // unit lands.
        let biome = level.get_biome(origin);
        self.biomes.contains(&biome)
    }

    fn type_id(&self) -> BlockPredicateTypeId {
        BlockPredicateTypes::MATCHING_BIOMES
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// `RegistryCodecs.homogeneousList(Registries.BIOME)` — the `"biomes"` field
/// codec: a `HolderSetCodec` over the biome registry, whose element codec is a
/// `RegistryFixedCodec` (tag key `#minecraft:...` or element-list form).
///
/// The concrete codec is not `Send + Sync` (its `RegistryOps` carries the
/// single-threaded `HolderLookupAdapter`, `RefCell` memo — OWNERSHIP's single
/// sync tick); the `Arc` is held by the ops-parameterized predicate codec and
/// never crosses threads.
fn biomes_field_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<HolderSet<BiomeId>, Ops>> {
    #[allow(clippy::arc_with_non_send_sync)]
    let element: Arc<dyn Codec<rivet_registry::holder::Holder<BiomeId>, Ops>> = Arc::new(
        rivet_registry::registry_file_codec::RegistryFixedCodec::create(
            &rivet_registry::registries::BIOME,
        ),
    );
    #[allow(clippy::arc_with_non_send_sync)]
    let holder_set: Arc<dyn Codec<HolderSet<BiomeId>, Ops>> =
        Arc::new(rivet_registry::registry_file_codec::HolderSetCodec::create(
            &rivet_registry::registries::BIOME,
            element,
            false,
        ));
    codec::field_of(holder_set, "biomes".to_string())
}

/// `MatchingBiomesPredicate.CODEC` — the required `"biomes"` holder-set field,
/// as the ops-generic `matching_biomes_predicate_map_codec::<Ops>()` factory.
pub fn matching_biomes_predicate_map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<MatchingBiomesPredicate, Ops>> {
    record_builder::map_codec(|instance| {
        instance
            .group(record_builder::RecordCodecBuilder::of(
                Arc::new(|p: &MatchingBiomesPredicate| p.biomes.clone()),
                biomes_field_codec::<Ops>(),
            ))
            .apply(
                instance,
                Arc::new(|biomes: HolderSet<BiomeId>| MatchingBiomesPredicate::new(biomes)),
            )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::LevelHeightAccessor;
    use crate::levelgen::blockpredicates::block_predicate::block_predicate_codec;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::holder::Holder;
    use rivet_registry::registration_info::RegistrationInfo;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::root::AnyBox;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A biome registry with `plains` (id 0 — the first registered entry, so
    /// the holder id matches the registry) wrapped under `Registries.BIOME`.
    fn biome_access() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&*rivet_registry::registries::BIOME);
        builder.register(
            &ResourceKey::create(
                &*rivet_registry::registries::BIOME,
                Identifier::parse("minecraft:plains"),
            ),
            Arc::new(BiomeId::from_id(0)),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("worldgen/biome")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, biome_access())
    }

    /// A `WorldGenLevel` double whose `get_biome` returns a plains reference
    /// in the test biome registry — the only capability the predicate reads.
    /// The registry id is captured so the constructed holder matches the set's
    /// member (same (registry, id) pair).
    #[derive(Clone, Copy)]
    struct PlainsLevel {
        registry_id: u32,
    }

    impl LevelHeightAccessor for PlainsLevel {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for PlainsLevel {
        fn get_seed(&self) -> i64 {
            0
        }
        fn get_block_state(&self, _pos: &BlockPos) -> rivet_registry::block_state::BlockState {
            panic!("WorldGenLevel.getBlockState is not implemented (RivetTodo #399)")
        }
        fn get_biome(&self, _pos: &BlockPos) -> Holder<BiomeId> {
            Holder::reference(rivet_registry::holder::RegistryId(self.registry_id), 0)
        }
    }

    #[test]
    fn test_checks_biome_membership() {
        let access = biome_access();
        let registry = rivet_registry::access::RegistryAccess::lookup(
            &access,
            &*rivet_registry::registries::BIOME,
        )
        .expect("biome registry");
        let registry_id = registry.registry_id().0;
        let level = PlainsLevel { registry_id };
        let in_set = MatchingBiomesPredicate::new(HolderSet::direct(vec![Holder::reference(
            rivet_registry::holder::RegistryId(registry_id),
            0,
        )]));
        assert!(in_set.test(&level, &BlockPos::new(0, 0, 0)));
        // A set without plains (empty) does not match.
        let empty = MatchingBiomesPredicate::new(HolderSet::empty());
        assert!(!empty.test(&level, &BlockPos::new(0, 0, 0)));
    }

    #[test]
    fn codec_round_trips_and_encodes_biomes() {
        // One access builds BOTH the set's reference holders and the ops: each
        // `freeze()` allocates a fresh `RegistryId`, so the holder must carry
        // the same registry id the ops' access reads.
        let access = biome_access();
        let registry = rivet_registry::access::RegistryAccess::lookup(
            &access,
            &*rivet_registry::registries::BIOME,
        )
        .expect("biome registry");
        let p: Arc<dyn BlockPredicate> =
            Arc::new(MatchingBiomesPredicate::new(HolderSet::direct(vec![
                Holder::reference(registry.registry_id(), 0),
            ])));
        let codec = block_predicate_codec::<TestOps>();
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, access);
        let encoded = codec
            .encode_start(&ops, &p)
            .result()
            .expect("encode should succeed")
            .clone();
        // A single member compacts to the bare element.
        assert_eq!(
            encoded,
            json!({"type": "minecraft:matching_biomes", "biomes": "minecraft:plains"})
        );
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            BlockPredicate::type_id(&*decoded),
            BlockPredicateTypes::MATCHING_BIOMES
        );
    }

    #[test]
    fn missing_biomes_field_errors() {
        let ops = ops();
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(&ops, &json!({"type": "minecraft:matching_biomes"}));
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(msg.starts_with("No key biomes"), "got: {msg}");
    }

    #[test]
    fn unknown_biome_name_errors() {
        let ops = ops();
        let codec = block_predicate_codec::<TestOps>();
        let result = codec.parse(
            &ops,
            &json!({"type": "minecraft:matching_biomes", "biomes": "minecraft:not_a_biome"}),
        );
        assert!(result.is_error());
        let msg = result.error_ref().map(|e| e.message().to_string()).unwrap();
        assert!(
            msg.contains("Failed to get element minecraft:not_a_biome"),
            "got: {msg}"
        );
    }
}
