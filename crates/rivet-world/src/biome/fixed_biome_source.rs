//! Port of `net.minecraft.world.level.biome.FixedBiomeSource` (26.2) — the
//! `mc.world.level.biome.source` unit.
//!
//! The single-biome source (`implements BiomeManager.NoiseBiomeSource`): every
//! `getNoiseBiome` query returns the one fixed holder, and the search methods
//! short-circuit on `allowed.test(this.biome)`:
//!
//! ```text
//! CODEC = Biome.CODEC.fieldOf("biome").xmap(FixedBiomeSource::new, s -> s.biome).stable()
//! ```
//!
//! over the id-handle element codec (`biome_id_codec::biome_id_field_codec`,
//! the `Biome.CODEC` analogue — see the module doc of [`crate::biome::biome_id_codec`]).
//!
//! The three search overrides short-circuit on `allowed.test(this.biome)`:
//! `getBiomesWithin` returns the singleton `{this.biome}` set, and the two
//! random-consuming searches (`findBiomeHorizontal`/`findClosestBiome3d`) return
//! Java's exact positions — the clamped origin (Y clamped for 3D), or a random
//! block in the `2r+1` search square for the non-closest horizontal form. The
//! two random-consuming overrides live on this type (Java's methods) and are
//! dispatched from the free functions in [`crate::biome::biome_source`] through
//! the [`BiomeSource::as_any`] downcast (they cannot be trait methods —
//! `RandomSource` is not object-safe; see the `biome_source` module docs).

use crate::biome::biome_id_codec::biome_id_field_codec;
use crate::biome::biome_manager::NoiseBiomeSource;
use crate::biome::biome_resolver::BiomeResolver;
use crate::biome::biome_source::BiomeSource;
use crate::biome::biome_source_type::{BiomeSourceTypeId, BiomeSourceTypes};
use crate::biome::climate::Sampler;
use crate::level::LevelReader;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::BlockPos;
use rivet_registry::holder::Holder;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::mth;
use rivet_util::random::RandomSource;
use std::sync::{Arc, OnceLock};

/// `FixedBiomeSource` — the single-fixed-biome source.
#[derive(Debug)]
pub struct FixedBiomeSource {
    /// `this.biome` — the one biome every query returns.
    pub biome: Holder<BiomeId>,
    /// The `possibleBiomes` memo — Java's `Suppliers.memoize` (computed once on
    /// first read). Not part of equality (the derived cache value).
    possible_biomes: OnceLock<Vec<Holder<BiomeId>>>,
}

impl Clone for FixedBiomeSource {
    fn clone(&self) -> Self {
        FixedBiomeSource {
            biome: self.biome.clone(),
            possible_biomes: OnceLock::new(),
        }
    }
}

impl PartialEq for FixedBiomeSource {
    fn eq(&self, other: &Self) -> bool {
        // Java `Holder` equality on the `biome` field (the memo cache is not
        // part of the value).
        self.biome == other.biome
    }
}

impl FixedBiomeSource {
    /// `new FixedBiomeSource(Holder<Biome>)`.
    pub fn new(biome: Holder<BiomeId>) -> Self {
        FixedBiomeSource {
            biome,
            possible_biomes: OnceLock::new(),
        }
    }

    /// `FixedBiomeSource.CODEC` — the id-element `"biome"` field, xmapped and
    /// `.stable()` (MapCodec).
    pub fn map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn MapCodec<FixedBiomeSource, Ops>> {
        let field = biome_id_field_codec("biome");
        map_codec::stable(map_codec::xmap(
            field,
            Arc::new(|h: &Holder<BiomeId>| FixedBiomeSource::new(h.clone())),
            Arc::new(|s: &FixedBiomeSource| s.biome.clone()),
        ))
    }

    /// `FixedBiomeSource.findBiomeHorizontal(...)` — the 9-arg override: a
    /// single `allowed.test(this.biome)` short-circuit (dispatched from
    /// [`crate::biome::biome_source::find_biome_horizontal_full`]).
    ///
    /// `findClosest` returns the exact origin (Java `new BlockPos(originX,
    /// originY, originZ)`); otherwise a random block in the `2r+1` search square
    /// (`originX - r + random.nextInt(r * 2 + 1)` per axis — two draws, x then
    /// z, with Java `int` wrapping). Java's `skipStep` and `Sampler` params are
    /// ignored by the fixed override and dropped here.
    #[allow(clippy::too_many_arguments)] // Java's 9-arg override (7 + `&self`; skipStep/sampler dropped).
    pub(crate) fn find_biome_horizontal_short_circuit<R: RandomSource>(
        &self,
        origin_x: i32,
        origin_y: i32,
        origin_z: i32,
        r: i32,
        allowed: &dyn Fn(Holder<BiomeId>) -> bool,
        random: &mut R,
        find_closest: bool,
    ) -> Option<(BlockPos, Holder<BiomeId>)> {
        if !allowed(self.biome.clone()) {
            return None;
        }
        if find_closest {
            Some((
                BlockPos::new(origin_x, origin_y, origin_z),
                self.biome.clone(),
            ))
        } else {
            let bound = r.wrapping_mul(2).wrapping_add(1);
            let block_x = origin_x
                .wrapping_sub(r)
                .wrapping_add(random.next_int_bound(bound));
            let block_z = origin_z
                .wrapping_sub(r)
                .wrapping_add(random.next_int_bound(bound));
            Some((
                BlockPos::new(block_x, origin_y, block_z),
                self.biome.clone(),
            ))
        }
    }

    /// `FixedBiomeSource.findClosestBiome3d(...)` — the override: a single
    /// `allowed.test(this.biome)` short-circuit returning the origin with its Y
    /// clamped to `[level.getMinY() + 1, level.getMaxY() + 1]` (Java `origin
    /// .atY(Mth.clamp(...))`; dispatched from
    /// [`crate::biome::biome_source::find_closest_biome_3d`]).
    pub(crate) fn find_closest_biome_3d_short_circuit(
        &self,
        origin: &BlockPos,
        level: &dyn LevelReader,
        allowed: &dyn Fn(Holder<BiomeId>) -> bool,
    ) -> Option<(BlockPos, Holder<BiomeId>)> {
        if !allowed(self.biome.clone()) {
            return None;
        }
        let clamped_y = mth::clamp(
            origin.get_y(),
            level.get_min_y().wrapping_add(1),
            level.get_max_y().wrapping_add(1),
        );
        Some((
            BlockPos::new(origin.get_x(), clamped_y, origin.get_z()),
            self.biome.clone(),
        ))
    }
}

impl BiomeSource for FixedBiomeSource {
    fn type_id(&self) -> BiomeSourceTypeId {
        BiomeSourceTypes::FIXED
    }

    fn collect_possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        vec![self.biome.clone()]
    }

    fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        self.possible_biomes
            .get_or_init(|| vec![self.biome.clone()])
            .clone()
    }

    fn get_biomes_within(
        &self,
        _x: i32,
        _y: i32,
        _z: i32,
        _r: i32,
        _sampler: &Sampler,
    ) -> Vec<Holder<BiomeId>> {
        // `Sets.newHashSet(Set.of(this.biome))` — the singleton set (every
        // quart in the box resolves to `this.biome`).
        vec![self.biome.clone()]
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// The source is its own resolver (Java `BiomeSource implements
/// BiomeResolver`): every quart resolves to the fixed biome.
impl BiomeResolver for FixedBiomeSource {
    fn get_noise_biome(
        &self,
        _quart_x: i32,
        _quart_y: i32,
        _quart_z: i32,
        _sampler: &Sampler,
    ) -> Holder<BiomeId> {
        self.biome.clone()
    }
}

impl NoiseBiomeSource for FixedBiomeSource {
    /// The no-sampler overload — `getNoiseBiome(int, int, int)`.
    fn get_noise_biome(&self, _quart_x: i32, _quart_y: i32, _quart_z: i32) -> Holder<BiomeId> {
        self.biome.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::biome_source::biome_source_codec;
    use crate::biome::biome_source::find_biome_horizontal_full;
    use crate::level::{BlockGetter, LevelHeightAccessor, LevelReader};
    use rivet_registry::RegistryAccess;
    use rivet_registry::core::BlockPos;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_registry::{Identifier, RegistrationInfo, RegistryBuilder, ResourceKey};
    use rivet_serialization::json_ops::JsonOps;
    use rivet_util::random::LegacyRandomSource;
    use serde_json::{Value, json};
    use std::sync::Arc;

    /// The registry-backed ops the `BiomeSource` codecs require (the `"biome"`
    /// field resolves the biome registry through the ops).
    type TestOps = RegistryOps<Value, JsonOps>;

    fn empty_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    fn holder() -> Holder<BiomeId> {
        Holder::direct(BiomeId::from_id(40))
    }

    /// A minimal `LevelReader` over a `[minY, minY+height)` extent for the
    /// `findClosestBiome3d` Y-clamp.
    struct FakeLevel {
        min_y: i32,
        height: i32,
    }

    impl LevelHeightAccessor for FakeLevel {
        fn get_height(&self) -> i32 {
            self.height
        }
        fn get_min_y(&self) -> i32 {
            self.min_y
        }
    }

    impl BlockGetter for FakeLevel {}

    impl LevelReader for FakeLevel {
        fn has_chunk(&self, _chunk_x: i32, _chunk_z: i32) -> bool {
            true
        }
        fn get_sky_darken(&self) -> i32 {
            0
        }
        fn is_client_side(&self) -> bool {
            false
        }
        fn get_sea_level(&self) -> i32 {
            63
        }
    }

    /// A `RegistryOps` whose access carries a biome registry with `plains`
    /// registered at element id 0 — the decode path of the `"biome"` field
    /// resolves the identifier to a `Reference` through the getter.
    fn biome_ops() -> (TestOps, RegistryId) {
        let key = rivet_registry::registries::BIOME.clone();
        let mut builder = RegistryBuilder::<BiomeId>::new(&key);
        builder.register(
            &ResourceKey::create(&key, Identifier::with_default_namespace("plains")),
            Arc::new(BiomeId::from_id(40)),
            RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let owner = registry.registry_id();
        let access = RegistryAccess::from_single_registry(key, registry);
        (
            RegistryOps::create_from_access(&JsonOps::INSTANCE, access),
            owner,
        )
    }

    #[test]
    fn codec_round_trip() {
        let codec = map_codec::codec_of(FixedBiomeSource::map_codec::<TestOps>());
        // A decoded holder is a `Reference` (the getter resolves the
        // identifier), so the source under test carries the same reference.
        let (ops, owner) = biome_ops();
        let src = FixedBiomeSource::new(Holder::reference(owner, 0));
        let encoded = codec
            .encode_start(&ops, &src)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(encoded, json!({"biome": "minecraft:plains"}));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, src);
    }

    #[test]
    fn get_noise_biome_is_fixed() {
        let src = FixedBiomeSource::new(holder());
        // `FixedBiomeSource` implements `BiomeResolver` (4-arg, inherited by
        // `BiomeSource`) and `NoiseBiomeSource` (3-arg) `get_noise_biome` —
        // two traits define the same method name, so qualify the resolver
        // overload.
        assert_eq!(
            src.biome,
            BiomeResolver::get_noise_biome(
                &src,
                -5,
                64,
                9,
                &crate::biome::climate::Climate::empty()
            )
        );
    }

    #[test]
    fn dispatch_encodes_type() {
        let codec = biome_source_codec::<TestOps>();
        let src: Arc<dyn BiomeSource> = Arc::new(FixedBiomeSource::new(holder()));
        let ops = empty_ops();
        let encoded = codec
            .encode_start(&ops, &src)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            encoded,
            json!({"biome": "minecraft:plains", "type": "minecraft:fixed"})
        );
    }

    #[test]
    fn get_biomes_within_is_the_singleton_biome() {
        let src = FixedBiomeSource::new(holder());
        let sampler = crate::biome::climate::Climate::empty();
        // Java `Sets.newHashSet(Set.of(this.biome))` — the singleton set,
        // regardless of the box.
        assert_eq!(
            src.get_biomes_within(-100, 0, 50, 30, &sampler),
            vec![holder()]
        );
    }

    #[test]
    fn find_biome_horizontal_short_circuits() {
        let src = FixedBiomeSource::new(holder());
        let allowed = |h: Holder<BiomeId>| h == holder();
        let disallowed = |h: Holder<BiomeId>| h != holder();
        let mut random = LegacyRandomSource::new(1234);

        // findClosest → the exact origin (not a quart-snapped spiral position).
        let closest =
            src.find_biome_horizontal_short_circuit(10, 5, -3, 4, &allowed, &mut random, true);
        assert_eq!(closest, Some((BlockPos::new(10, 5, -3), holder())));
        // Non-closest → a random block inside the 2r+1 search square (two
        // draws, x then z), Y unchanged.
        let (pos, biome) = src
            .find_biome_horizontal_short_circuit(10, 5, -3, 4, &allowed, &mut random, false)
            .expect("allowed");
        assert_eq!(biome, holder());
        assert_eq!(pos.get_y(), 5);
        assert!(
            pos.get_x() >= 10 - 4 && pos.get_x() <= 10 + 4,
            "x={}",
            pos.get_x()
        );
        assert!(
            pos.get_z() >= -3 - 4 && pos.get_z() <= -3 + 4,
            "z={}",
            pos.get_z()
        );
        // Not allowed → null.
        assert_eq!(
            src.find_biome_horizontal_short_circuit(10, 5, -3, 4, &disallowed, &mut random, false),
            None
        );
    }

    #[test]
    fn find_closest_biome_3d_short_circuits() {
        let src = FixedBiomeSource::new(holder());
        let allowed = |h: Holder<BiomeId>| h == holder();
        let disallowed = |h: Holder<BiomeId>| h != holder();
        // `[minY+1, maxY+1]` = `[-63, 320]` for the overworld extent.
        let level = FakeLevel {
            min_y: -64,
            height: 384,
        };

        let in_range = src
            .find_closest_biome_3d_short_circuit(&BlockPos::new(7, 200, -9), &level, &allowed)
            .expect("allowed");
        assert_eq!(in_range, (BlockPos::new(7, 200, -9), holder()));
        let below = src
            .find_closest_biome_3d_short_circuit(&BlockPos::new(7, -100, -9), &level, &allowed)
            .expect("allowed");
        assert_eq!(below.0.get_y(), -63);
        assert_eq!(below.0.get_x(), 7);
        assert_eq!(below.0.get_z(), -9);
        let above = src
            .find_closest_biome_3d_short_circuit(&BlockPos::new(7, 500, -9), &level, &allowed)
            .expect("allowed");
        assert_eq!(above.0.get_y(), 320);
        assert_eq!(
            src.find_closest_biome_3d_short_circuit(
                &BlockPos::new(7, 200, -9),
                &level,
                &disallowed
            ),
            None
        );
    }

    #[test]
    fn search_free_functions_dispatch_to_the_fixed_short_circuit() {
        // The `&dyn BiomeSource` free functions downcast to `FixedBiomeSource`
        // and take its override (Java's virtual dispatch), not the base spiral.
        let src: Arc<dyn BiomeSource> = Arc::new(FixedBiomeSource::new(holder()));
        let sampler = crate::biome::climate::Climate::empty();
        let allowed = |h: Holder<BiomeId>| h == holder();
        let mut random = LegacyRandomSource::new(99);

        // findClosest → the exact origin (the base spiral would quart-snap to a
        // multiple-of-4 block).
        let r = find_biome_horizontal_full(
            &*src,
            10,
            5,
            -3,
            4,
            1,
            &allowed,
            &mut random,
            true,
            &sampler,
        );
        assert_eq!(r, Some((BlockPos::new(10, 5, -3), holder())));
        // The trait override fires for getBiomesWithin too.
        assert_eq!(src.get_biomes_within(0, 0, 0, 4, &sampler), vec![holder()]);
    }
}
