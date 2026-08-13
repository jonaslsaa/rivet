//! Port of `net.minecraft.world.level.biome.BiomeSource` (abstract class,
//! 26.2) — the `mc.world.level.biome.source` unit root.
//!
//! Java `BiomeSource` is the abstract base every biome source implements
//! (`BiomeResolver` + the search/`possibleBiomes` surface), and its `CODEC` is
//! the dispatch root that resolves the concrete source by the registered
//! biome-source type:
//!
//! ```text
//! CODEC = BuiltInRegistries.BIOME_SOURCE.byNameCodec()
//!            .dispatchStable(BiomeSource::codec, Function.identity())
//! ```
//!
//! i.e. a `"type"` field naming the biome-source type via the by-name registry
//! codec, whose per-type `MapCodec` then applies to the whole map — the same
//! dispatch shape as `Feature`/`BlockPredicate`/`FeatureSize`. The Rust port
//! keeps the identity split: [`BiomeSource`] is the object-safe behavior
//! contract, its registry identity is the [`BiomeSourceTypeId`] handle, and the
//! erased carrier `Arc<dyn BiomeSource>` is what the dispatch codec
//! (de)serializes.
//!
//! ## Element typing: `Holder<BiomeId>`
//!
//! Java types every method over `Holder<Biome>` (the behavior-carrying value).
//! The merged `biome.core` model carries the biome reference as the pure
//! [`rivet_registry::biome_id::BiomeId`] handle — the `BiomeResolver` and
//! `BiomeManager::NoiseBiomeSource` traits return `Holder<BiomeId>`, and the
//! value layer's `Biome` is a shell. This unit therefore types its element
//! surface (`collectPossibleBiomes`, `possibleBiomes`, the search results,
//! `getNoiseBiome`) over `Holder<BiomeId>` and its codecs over the id holder
//! codecs in [`biome_id_codec`] (the `Biome.CODEC`/`Biome.LIST_CODEC`
//! analogues). The `.data`-unit `Biomes` `ResourceKey`s and the
//! `OverworldBiomeBuilder` surface are declared as STUBs (see [`biomes`] /
//! [`overworld_biome_builder`]); the concrete preset/`the_end` sources bridge
//! by identifier through the generated `BiomeId` tables.
//!
//! ## The four registered types
//!
//! `BiomeSources.bootstrap` registers `fixed`, `multi_noise`, `checkerboard`,
//! `the_end` in that exact order. The `"type"` by-name codec resolves those
//! four names; the dispatch table is total over them (declaration-order codec
//! dispatch with no fabricated fallback).
//!
//! ## Search semantics
//!
//! `getBiomesWithin` is a trait default (object-safe, no random source), and the
//! two random-consuming searches (`findBiomeHorizontal`/`findClosestBiome3d`)
//! live as free generic functions over `&dyn BiomeSource` — `RandomSource` is
//! `Sized` (not object-safe), so a trait method taking one would break the
//! erased-carrier dispatch. `FixedBiomeSource` overrides both searches with a
//! single `allowed.test(this.biome)` short-circuit; the free functions dispatch
//! the override through the [`BiomeSource::as_any`] downcast (the only dispatch
//! seam for the erased carrier). All search arithmetic wraps like Java `int`
//! (the quart/block geometry, the reservoir `nextInt(found + 1)`, and the
//! `currentRadius += skipSteps` walk), and `Math.abs` is the wrapping
//! `i32::wrapping_abs`.
//! `possibleBiomes` is Java's `Suppliers.memoize` over
//! `distinct().collect(toImmutableSet())` — computed once on first read and
//! cached. The `fixed`/`checkerboard`/`multi_noise` sources port the memo as a
//! per-instance `OnceLock` (see [`BiomeSource::possible_biomes`] and
//! [`dedupe_possible_biomes`]); `the_end` uses the trait default, which
//! recomputes the collect+dedup per call. Either way the sources are immutable
//! value tables, so the recomputed set matches Java's memoized one (Guava's
//! `ImmutableSet` preserves iteration order, so the port's insertion-order
//! `Vec` matches; `Holder` is not `Hash`, so the dedup is a linear scan).
//! `getBiomesWithin` is Java's unordered `Sets.newHashSet` — the port keeps the
//! quart-scan order in a `Vec`, an order Java does not guarantee.
//!
//! Holder equality: the merged model derives value `PartialEq` on `Holder`
//! (`Direct` by `BiomeId`, `Reference` by `(RegistryId, u32)`), so the dedup
//! and the `getBiomesWithin`/`findClosestBiome3d` set-contains compare two
//! distinct reference holders to the same biome as equal. Java
//! `Holder.Reference` does not override `equals` (identity equality), so a
//! source whose parameter list repeats a biome under distinct reference
//! holders would NOT dedupe in Java but does here. This is consistent with
//! OWNERSHIP's pure-id model but is a real cardinality difference for
//! duplicated entries.

use crate::biome::biome_source_type::{BiomeSourceTypeId, BiomeSourceTypes};
use crate::biome::climate::Sampler;
use crate::biome::fixed_biome_source::FixedBiomeSource;
use crate::level::LevelReader;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{BlockPos, Direction, QuartPos};
use rivet_registry::holder::Holder;
use rivet_registry::identifier::Identifier;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_util::mth;
use rivet_util::random::RandomSource;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `BiomeSource.CODEC` — the dispatch codec, as the ops-generic
/// `biome_source_codec::<Ops>()` factory.
///
/// `Ops` must also implement [`RegistryOpsLookup`]: the `fixed`/`multi_noise`/
/// `checkerboard`/`the_end` fields (`"biome"`/`"biomes"`/`"preset"`/the five
/// `retrieveElement` fields) resolve the biome registry through the ops.
pub fn biome_source_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<Arc<dyn BiomeSource>, Ops>> {
    let dispatch = key_dispatch_codec::dispatch_map::<BiomeSourceTypeId, Arc<dyn BiomeSource>, Ops>(
        "type",
        biome_source_type_by_name_codec::<Ops>(),
        // A bare `.type_id()` on `Arc<dyn BiomeSource>` resolves to `Any::type_id`
        // (the supertrait) — disambiguate through the `BiomeSource` trait (the
        // `BlockPredicate` pattern).
        Arc::new(|source: &Arc<dyn BiomeSource>| {
            DataResult::success(BiomeSource::type_id(&**source))
        }),
        Arc::new(codec_for_type),
    );
    map_codec::codec_of(dispatch)
}

/// `BiomeSourceType::codec` — resolve a `BiomeSourceTypeId` to its
/// `MapCodec<Arc<dyn BiomeSource>>` (the dispatch's `codec` function).
fn codec_for_type<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    k: &BiomeSourceTypeId,
) -> DataResult<Arc<dyn MapCodec<Arc<dyn BiomeSource>, Ops>>> {
    if *k == BiomeSourceTypes::FIXED {
        DataResult::success(erase_map_codec::<
            crate::biome::fixed_biome_source::FixedBiomeSource,
            Ops,
        >(
            crate::biome::fixed_biome_source::FixedBiomeSource::map_codec::<Ops>(),
        ))
    } else if *k == BiomeSourceTypes::MULTI_NOISE {
        DataResult::success(erase_map_codec::<
            crate::biome::multi_noise_biome_source::MultiNoiseBiomeSource,
            Ops,
        >(
            crate::biome::multi_noise_biome_source::MultiNoiseBiomeSource::map_codec::<Ops>(),
        ))
    } else if *k == BiomeSourceTypes::CHECKERBOARD {
        DataResult::success(erase_map_codec::<crate::biome::checkerboard_column_biome_source::CheckerboardColumnBiomeSource, Ops>(
            crate::biome::checkerboard_column_biome_source::CheckerboardColumnBiomeSource::map_codec::<Ops>(),
        ))
    } else if *k == BiomeSourceTypes::THE_END {
        DataResult::success(erase_map_codec::<
            crate::biome::the_end_biome_source::TheEndBiomeSource,
            Ops,
        >(
            crate::biome::the_end_biome_source::TheEndBiomeSource::map_codec::<Ops>(),
        ))
    } else {
        DataResult::error(format!("Unknown biome source type '{}'", k.location))
    }
}

/// `BuiltInRegistries.BIOME_SOURCE.byNameCodec()` over the erased id —
/// `Identifier.CODEC.comapFlatMap(...)`, with Paper's exact
/// `"Unknown registry key in <registry>: <name>"` error.
pub fn biome_source_type_by_name_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<BiomeSourceTypeId, Ops>> {
    codec::comap_flat_map::<Identifier, BiomeSourceTypeId, Ops>(
        rivet_registry::identifier::identifier_codec::<Ops>(),
        Arc::new(|name: &Identifier| {
            match crate::biome::biome_source_type::biome_source_type_by_name(&name.to_string()) {
                Some(id) => DataResult::success(id),
                None => DataResult::error(format!(
                    "Unknown registry key in ResourceKey[minecraft:root / minecraft:worldgen/biome_source]: {}",
                    name
                )),
            }
        }),
        Arc::new(|id: &BiomeSourceTypeId| Identifier::parse(id.location)),
    )
}

/// Lift a concrete source's `MapCodec<C>` to `MapCodec<Arc<dyn BiomeSource>>`
/// — Java's `MapCodec<? extends BiomeSource>` variance, via xmap.
fn erase_map_codec<C, Ops>(
    inner: Arc<dyn MapCodec<C, Ops>>,
) -> Arc<dyn MapCodec<Arc<dyn BiomeSource>, Ops>>
where
    C: BiomeSource + Clone + 'static,
    Ops: DynamicOps + 'static,
{
    map_codec::xmap(
        inner,
        Arc::new(|c: &C| -> Arc<dyn BiomeSource> { Arc::new(c.clone()) }),
        Arc::new(downcast_erased::<C>),
    )
}

/// The encode-side `from` of the erase lift: downcast the erased value to its
/// concrete source (safe — the dispatch guarantees the value's type).
fn downcast_erased<C: BiomeSource + Clone + 'static>(source: &Arc<dyn BiomeSource>) -> C {
    source
        .as_any()
        .downcast_ref::<C>()
        .expect("biome source codec applied to a value of a different type")
        .clone()
}

/// `net.minecraft.world.level.biome.BiomeSource` — the behavior contract of a
/// biome source (Java's abstract `codec()`/`collectPossibleBiomes()`/
/// `getNoiseBiome()` + the concrete search defaults).
///
/// The erased carrier `Arc<dyn BiomeSource>` is what the dispatch codec
/// (de)serializes — the Rust analogue of Java's `Codec<BiomeSource>` value.
/// `Any` (supertrait) enables the dispatch codec's downcast of an erased value
/// back to its concrete type on encode, via the explicit [`BiomeSource::as_any`]
/// seam (the same pattern `BlockPredicate` uses).
pub trait BiomeSource: Any + Debug + Send + Sync + 'static {
    /// `BiomeSource.codec()` — the registered `MapCodec` identity this source
    /// dispatches on (the key `BiomeSource.CODEC` uses).
    fn type_id(&self) -> BiomeSourceTypeId;

    /// `BiomeSource.collectPossibleBiomes()` — the (not yet deduplicated)
    /// stream of biomes this source can return.
    fn collect_possible_biomes(&self) -> Vec<Holder<BiomeId>>;

    /// `BiomeSource.possibleBiomes()` — the deduplicated set
    /// (`distinct().collect(toImmutableSet())`; Java memoizes it with
    /// `Suppliers.memoize`).
    ///
    /// `fixed`/`checkerboard`/`multi_noise` override this with a per-instance
    /// `OnceLock` memo (computed once on first read, like Java's supplier); the
    /// default recomputes the collect+dedup on every call
    /// ([`dedupe_possible_biomes`]) and is used by `the_end`. The sources are
    /// immutable value tables, so the recomputed set matches Java's memoized
    /// one.
    fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        dedupe_possible_biomes(self.collect_possible_biomes())
    }

    /// `BiomeSource.getBiomesWithin(int x, int y, int z, int r, Sampler)` —
    /// the distinct `Holder<BiomeId>` set over the quart box `[x±r, y±r, z±r]`
    /// (Java's `Sets.newHashSet` — an unordered `HashSet`; the port keeps the
    /// quart-scan order in a `Vec` with a linear-scan dedup, an order Java does
    /// not guarantee). The box arithmetic wraps like Java `int`.
    fn get_biomes_within(
        &self,
        x: i32,
        y: i32,
        z: i32,
        r: i32,
        sampler: &Sampler,
    ) -> Vec<Holder<BiomeId>> {
        let x0 = QuartPos::from_block(x.wrapping_sub(r));
        let y0 = QuartPos::from_block(y.wrapping_sub(r));
        let z0 = QuartPos::from_block(z.wrapping_sub(r));
        let x1 = QuartPos::from_block(x.wrapping_add(r));
        let y1 = QuartPos::from_block(y.wrapping_add(r));
        let z1 = QuartPos::from_block(z.wrapping_add(r));
        let w = x1.wrapping_sub(x0).wrapping_add(1);
        let d = y1.wrapping_sub(y0).wrapping_add(1);
        let h = z1.wrapping_sub(z0).wrapping_add(1);
        let mut biome_set = Vec::new();

        for row in 0..h {
            for column in 0..w {
                for depth in 0..d {
                    let noise_x = x0.wrapping_add(column);
                    let noise_y = y0.wrapping_add(depth);
                    let noise_z = z0.wrapping_add(row);
                    let biome = self.get_noise_biome(noise_x, noise_y, noise_z, sampler);
                    if !biome_set.contains(&biome) {
                        biome_set.push(biome);
                    }
                }
            }
        }

        biome_set
    }

    /// `BiomeSource.getNoiseBiome(int quartX, int quartY, int quartZ, Sampler)`
    /// — the quart-position resolver (Java's abstract, over `BiomeResolver`).
    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId>;

    /// `BiomeSource.addDebugInfo(List<String>, BlockPos, Sampler)` — the empty
    /// default; `MultiNoiseBiomeSource` overrides it.
    fn add_debug_info(&self, _result: &mut Vec<String>, _feet_pos: &BlockPos, _sampler: &Sampler) {}

    /// `as_any` — the downcast seam (Java's erased `BiomeSource` cast) the
    /// dispatch codec uses on encode to recover the concrete variant type.
    fn as_any(&self) -> &dyn Any;
}

/// `distinct().collect(toImmutableSet())` over `collectPossibleBiomes()` — the
/// dedup half of Java's `possibleBiomes` supplier. Insertion-ordered linear-scan
/// dedup (`Holder` is not `Hash`).
pub(crate) fn dedupe_possible_biomes(collected: Vec<Holder<BiomeId>>) -> Vec<Holder<BiomeId>> {
    let mut seen = Vec::new();
    collected
        .into_iter()
        .filter(|h| {
            if seen.contains(h) {
                false
            } else {
                seen.push(h.clone());
                true
            }
        })
        .collect()
}

/// `BiomeSource.findBiomeHorizontal(int x, int y, int z, int searchRadius,
/// Predicate<Holder<Biome>>, RandomSource, Sampler)` — the `skipSteps = 1` /
/// `findClosest = false` convenience overload of [`find_biome_horizontal_full`].
///
/// `RandomSource` is `Sized` (not object-safe), so the search methods live as
/// free generic functions over the object-safe `dyn BiomeSource` (the
/// `carver_is_start_chunk` pattern) rather than trait methods.
#[allow(clippy::too_many_arguments)] // Java's 7-param overload + the `source` receiver.
pub fn find_biome_horizontal<R: RandomSource>(
    source: &dyn BiomeSource,
    x: i32,
    y: i32,
    z: i32,
    search_radius: i32,
    allowed: &dyn Fn(Holder<BiomeId>) -> bool,
    random: &mut R,
    sampler: &Sampler,
) -> Option<(BlockPos, Holder<BiomeId>)> {
    find_biome_horizontal_full(
        source,
        x,
        y,
        z,
        search_radius,
        1,
        allowed,
        random,
        false,
        sampler,
    )
}

/// `BiomeSource.findClosestBiome3d(...)` — the 3D search over the
/// possible-biome candidates, spiral-ring by spiral-ring over the
/// `sampleResolution*`-spaced block columns. `FixedBiomeSource` overrides the
/// method with a single `allowed.test(this.biome)` short-circuit (see
/// [`FixedBiomeSource::find_closest_biome_3d_short_circuit`]), dispatched
/// through the `as_any` downcast.
#[allow(clippy::too_many_arguments)] // Java's 7-param method + the `source` receiver.
pub fn find_closest_biome_3d(
    source: &dyn BiomeSource,
    origin: &BlockPos,
    search_radius: i32,
    sample_resolution_horizontal: i32,
    sample_resolution_vertical: i32,
    allowed: &dyn Fn(Holder<BiomeId>) -> bool,
    sampler: &Sampler,
    level: &dyn LevelReader,
) -> Option<(BlockPos, Holder<BiomeId>)> {
    if let Some(fixed) = source.as_any().downcast_ref::<FixedBiomeSource>() {
        return fixed.find_closest_biome_3d_short_circuit(origin, level, allowed);
    }

    let candidate_biomes: Vec<Holder<BiomeId>> = source
        .possible_biomes()
        .into_iter()
        .filter(|h| allowed(h.clone()))
        .collect();
    if candidate_biomes.is_empty() {
        return None;
    }

    let sample_radius = mth::floor_div(search_radius, sample_resolution_horizontal);
    let sample_ys: Vec<i32> = mth::out_from_origin_with_step(
        origin.get_y(),
        level.get_min_y().wrapping_add(1),
        level.get_max_y().wrapping_add(1),
        sample_resolution_vertical,
    )
    .collect();

    for sample_column in BlockPos::spiral_around(
        &BlockPos::ZERO,
        sample_radius,
        &Direction::East,
        &Direction::South,
    ) {
        let block_x = origin.get_x().wrapping_add(
            sample_column
                .get_x()
                .wrapping_mul(sample_resolution_horizontal),
        );
        let block_z = origin.get_z().wrapping_add(
            sample_column
                .get_z()
                .wrapping_mul(sample_resolution_horizontal),
        );
        let noise_x = QuartPos::from_block(block_x);
        let noise_z = QuartPos::from_block(block_z);

        for &block_y in &sample_ys {
            let noise_y = QuartPos::from_block(block_y);
            let biome = source.get_noise_biome(noise_x, noise_y, noise_z, sampler);
            if candidate_biomes.contains(&biome) {
                return Some((BlockPos::new(block_x, block_y, block_z), biome));
            }
        }
    }

    None
}

/// `BiomeSource.findBiomeHorizontal(int originX, int originY, int originZ,
/// int searchRadius, int skipSteps, Predicate<Holder<Biome>>, RandomSource,
/// boolean findClosest, Sampler)` — the full horizontal spiral search with the
/// debug-world start-radius gate.
///
/// Java's `startRadius = findClosest ? 0 : noiseRadius` and the
/// `z = !DEBUG_ONLY_GENERATE_HALF_THE_WORLD &&
/// !debugGenerateSquareTerrainWithoutNoise ? -currentRadius : 0` loop start
/// (both constants false in the pinned defaults, so `startRadius =
/// noiseRadius` and the z scan begins at `-currentRadius`). The reservoir
/// `random.nextInt(found + 1) == 0` picks uniformly among the matches;
/// `findClosest` returns the first allowed biome on the edge ring. `FixedBiomeSource`
/// overrides the 9-arg method with a single `allowed.test(this.biome)`
/// short-circuit (see [`FixedBiomeSource::find_biome_horizontal_short_circuit`]),
/// dispatched through the `as_any` downcast.
#[allow(clippy::too_many_arguments)]
pub fn find_biome_horizontal_full<R: RandomSource>(
    source: &dyn BiomeSource,
    origin_x: i32,
    origin_y: i32,
    origin_z: i32,
    search_radius: i32,
    skip_steps: i32,
    allowed: &dyn Fn(Holder<BiomeId>) -> bool,
    random: &mut R,
    find_closest: bool,
    sampler: &Sampler,
) -> Option<(BlockPos, Holder<BiomeId>)> {
    if let Some(fixed) = source.as_any().downcast_ref::<FixedBiomeSource>() {
        return fixed.find_biome_horizontal_short_circuit(
            origin_x,
            origin_y,
            origin_z,
            search_radius,
            allowed,
            random,
            find_closest,
        );
    }

    let noise_center_x = QuartPos::from_block(origin_x);
    let noise_center_z = QuartPos::from_block(origin_z);
    let noise_radius = QuartPos::from_block(search_radius);
    let noise_y = QuartPos::from_block(origin_y);
    let mut result: Option<(BlockPos, Holder<BiomeId>)> = None;
    let mut found: i32 = 0;
    let start_radius = if find_closest { 0 } else { noise_radius };
    let mut current_radius = start_radius;

    while current_radius <= noise_radius {
        let z_start = if !rivet_core::shared_constants::DEBUG_ONLY_GENERATE_HALF_THE_WORLD
            && !rivet_core::shared_constants::DEBUG_GENERATE_SQUARE_TERRAIN_WITHOUT_NOISE
        {
            current_radius.wrapping_neg()
        } else {
            0
        };

        let mut z = z_start;
        while z <= current_radius {
            let z_edge = z.wrapping_abs() == current_radius;

            let mut x = current_radius.wrapping_neg();
            while x <= current_radius {
                if find_closest {
                    let x_edge = x.wrapping_abs() == current_radius;
                    if !x_edge && !z_edge {
                        x = x.wrapping_add(skip_steps);
                        continue;
                    }
                }

                let noise_x = noise_center_x.wrapping_add(x);
                let noise_z = noise_center_z.wrapping_add(z);
                let biome = source.get_noise_biome(noise_x, noise_y, noise_z, sampler);
                if allowed(biome.clone()) {
                    if result.is_none() || random.next_int_bound(found.wrapping_add(1)) == 0 {
                        let result_pos = BlockPos::new(
                            QuartPos::to_block(noise_x),
                            origin_y,
                            QuartPos::to_block(noise_z),
                        );
                        if find_closest {
                            return Some((result_pos, biome));
                        }
                        result = Some((result_pos, biome));
                    }
                    found = found.wrapping_add(1);
                }

                x = x.wrapping_add(skip_steps);
            }

            z = z.wrapping_add(skip_steps);
        }

        current_radius = current_radius.wrapping_add(skip_steps);
    }

    result
}

/// The shared registry-key helper this unit's sources and the bootstrap use
/// (`Registries.BIOME` lives in `rivet_registry::registries` — the canonical
/// key the id codecs already resolve through).
pub(crate) mod keys {
    use crate::biome::multi_noise_biome_source_parameter_list::MultiNoiseBiomeSourceParameterList;
    use rivet_registry::Identifier;
    use rivet_registry::registry::RegistryKey;
    use std::sync::LazyLock;

    /// `Registries.MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST` —
    /// `createRegistryKey("worldgen/multi_noise_biome_source_parameter_list")`.
    pub static MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST: LazyLock<
        RegistryKey<MultiNoiseBiomeSourceParameterList>,
    > = LazyLock::new(|| {
        rivet_registry::ResourceKey::create_registry_key(Identifier::with_default_namespace(
            "worldgen/multi_noise_biome_source_parameter_list",
        ))
    });
}
