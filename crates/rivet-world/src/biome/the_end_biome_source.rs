//! Port of `net.minecraft.world.level.biome.TheEndBiomeSource` (26.2) — the
//! `mc.world.level.biome.source` unit.
//!
//! The End's biome source: five `RegistryOps.retrieveElement(Biomes.X)` holder
//! fields resolved from the biome registry at decode, and a `getNoiseBiome`
//! that buckets the sampled erosion at the weird-block centers of the section:
//!
//! ```text
//! CODEC = RecordCodecBuilder.mapCodec(i -> i.group(
//!     RegistryOps.retrieveElement(Biomes.THE_END),
//!     ... five fields ...
//! ).apply(i, i.stable(TheEndBiomeSource::new)))
//! ```
//!
//! `i.stable(TheEndBiomeSource::new)` is Java's `Instance.stable` — a
//! `Lifecycle.stable()` point field, not an `.stable()` lifecycle wrapper on a
//! field codec. The port's `record_builder` applicative takes the constructor
//! as a bare function, so the stable point is recovered with
//! [`map_codec::stable`] over the built codec (see [`TheEndBiomeSource::map_codec`]).

use crate::biome::biome_source::BiomeSource;
use crate::biome::biome_source_type::{BiomeSourceTypeId, BiomeSourceTypes};
use crate::biome::biomes;
use crate::biome::climate::Sampler;
use crate::levelgen::noise::density_function::SinglePointContext;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::QuartPos;
use rivet_registry::core::SectionPos;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::registry_ops::{RegistryOpsLookup, retrieve_element};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::sync::{Arc, OnceLock};

/// `TheEndBiomeSource` — the End's five-biome source.
#[derive(Debug)]
pub struct TheEndBiomeSource {
    /// `this.end` — `Biomes.THE_END`.
    pub end: Holder<BiomeId>,
    /// `this.highlands` — `Biomes.END_HIGHLANDS`.
    pub highlands: Holder<BiomeId>,
    /// `this.midlands` — `Biomes.END_MIDLANDS`.
    pub midlands: Holder<BiomeId>,
    /// `this.islands` — `Biomes.SMALL_END_ISLANDS`.
    pub islands: Holder<BiomeId>,
    /// `this.barrens` — `Biomes.END_BARRENS`.
    pub barrens: Holder<BiomeId>,
    /// The `possibleBiomes` memo — Java's `Suppliers.memoize` on the abstract
    /// `BiomeSource` base (computed once on first read; the other sources also
    /// port it as a per-instance `OnceLock`). Not part of equality (the derived
    /// cache value).
    possible_biomes: OnceLock<Vec<Holder<BiomeId>>>,
}

impl Clone for TheEndBiomeSource {
    fn clone(&self) -> Self {
        TheEndBiomeSource {
            end: self.end.clone(),
            highlands: self.highlands.clone(),
            midlands: self.midlands.clone(),
            islands: self.islands.clone(),
            barrens: self.barrens.clone(),
            possible_biomes: OnceLock::new(),
        }
    }
}

impl PartialEq for TheEndBiomeSource {
    fn eq(&self, other: &Self) -> bool {
        self.end == other.end
            && self.highlands == other.highlands
            && self.midlands == other.midlands
            && self.islands == other.islands
            && self.barrens == other.barrens
    }
}

impl TheEndBiomeSource {
    /// `new TheEndBiomeSource(Holder<Biome> ×5)` (private in Java).
    pub fn new(
        end: Holder<BiomeId>,
        highlands: Holder<BiomeId>,
        midlands: Holder<BiomeId>,
        islands: Holder<BiomeId>,
        barrens: Holder<BiomeId>,
    ) -> Self {
        TheEndBiomeSource {
            end,
            highlands,
            midlands,
            islands,
            barrens,
            possible_biomes: OnceLock::new(),
        }
    }

    /// `TheEndBiomeSource.create(HolderGetter<Biome> biomes)` — the five
    /// `getOrThrow(Biomes.X)` lookups.
    pub fn create(biomes: &dyn HolderGetter<BiomeId>) -> Self {
        TheEndBiomeSource::new(
            biomes.get_or_throw(&biomes::THE_END),
            biomes.get_or_throw(&biomes::END_HIGHLANDS),
            biomes.get_or_throw(&biomes::END_MIDLANDS),
            biomes.get_or_throw(&biomes::SMALL_END_ISLANDS),
            biomes.get_or_throw(&biomes::END_BARRENS),
        )
    }

    /// `TheEndBiomeSource.CODEC`.
    pub fn map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn MapCodec<TheEndBiomeSource, Ops>> {
        // Java's `.apply(i, i.stable(TheEndBiomeSource::new))` makes the
        // constructor a `Lifecycle.stable()` point field. The port's
        // `Group5::apply` takes the constructor as a bare function (no lifecycle
        // point — `compose5` stamps the applied result experimental), so the
        // stable stamp is recovered by wrapping the built codec
        // (`set_lifecycle` overrides the compose result on decode/encode).
        map_codec::stable(record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|s: &TheEndBiomeSource| s.end.clone()),
                    retrieve_element(&biomes::THE_END),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &TheEndBiomeSource| s.highlands.clone()),
                    retrieve_element(&biomes::END_HIGHLANDS),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &TheEndBiomeSource| s.midlands.clone()),
                    retrieve_element(&biomes::END_MIDLANDS),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &TheEndBiomeSource| s.islands.clone()),
                    retrieve_element(&biomes::SMALL_END_ISLANDS),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|s: &TheEndBiomeSource| s.barrens.clone()),
                    retrieve_element(&biomes::END_BARRENS),
                ))
                .apply(instance, Arc::new(TheEndBiomeSource::new))
        }))
    }
}

impl BiomeSource for TheEndBiomeSource {
    fn type_id(&self) -> BiomeSourceTypeId {
        BiomeSourceTypes::THE_END
    }

    fn collect_possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        vec![
            self.end.clone(),
            self.highlands.clone(),
            self.midlands.clone(),
            self.islands.clone(),
            self.barrens.clone(),
        ]
    }

    fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        // Java's `Suppliers.memoize` lives on the abstract `BiomeSource` base
        // (`collectPossibleBiomes().distinct().collect(toImmutableSet())`), so
        // the End source memoizes the collect+dedup too (the five holders are
        // fixed at construction).
        self.possible_biomes
            .get_or_init(|| {
                crate::biome::biome_source::dedupe_possible_biomes(self.collect_possible_biomes())
            })
            .clone()
    }

    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId> {
        let block_x = QuartPos::to_block(quart_x);
        let block_y = QuartPos::to_block(quart_y);
        let block_z = QuartPos::to_block(quart_z);
        let chunk_x = SectionPos::block_to_section_coord(block_x);
        let chunk_z = SectionPos::block_to_section_coord(block_z);
        // `(long)chunkX * chunkX + (long)chunkZ * chunkZ <= 4096L` — the center
        // `±64`-chunk (radius 8 chunks) circle is always `end`.
        let distance_sq = (chunk_x as i64)
            .wrapping_mul(chunk_x as i64)
            .wrapping_add((chunk_z as i64).wrapping_mul(chunk_z as i64));
        if distance_sq <= 4096 {
            return self.end.clone();
        }
        // `weirdBlockX = (SectionPos.blockToSectionCoord(blockX) * 2 + 1) * 8`.
        let weird_block_x = chunk_x.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
        let weird_block_z = chunk_z.wrapping_mul(2).wrapping_add(1).wrapping_mul(8);
        let height_value = sampler.erosion.compute(&SinglePointContext::new(
            weird_block_x,
            block_y,
            weird_block_z,
        ));
        if height_value > 0.25 {
            self.highlands.clone()
        } else if height_value >= -0.0625 {
            self.midlands.clone()
        } else if height_value < -0.21875 {
            self.islands.clone()
        } else {
            self.barrens.clone()
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn end_source() -> TheEndBiomeSource {
        let h = |id: u16| Holder::direct(BiomeId::from_id(id));
        TheEndBiomeSource::new(h(9), h(40), h(41), h(42), h(43))
    }

    #[test]
    fn possible_biomes_dedupes_value_equal_duplicates() {
        // Java's abstract `possibleBiomes` = `collectPossibleBiomes().distinct()
        // .collect(toImmutableSet())`; a source whose fields alias the same
        // biome reports the deduped set, not the five raw fields.
        let h = |id: u16| Holder::direct(BiomeId::from_id(id));
        let src = TheEndBiomeSource::new(h(9), h(9), h(40), h(40), h(43));
        assert_eq!(src.possible_biomes(), vec![h(9), h(40), h(43)]);
    }

    #[test]
    fn center_chunks_are_always_the_end() {
        // `chunkX² + chunkZ² <= 4096` (|chunkX|,|chunkZ| <= 64): the whole
        // 2048×2048 block center square returns `end`.
        let src = end_source();
        let sampler = crate::biome::climate::Climate::empty();
        // block 0,0 → chunk 0,0.
        assert_eq!(
            src.get_noise_biome(0, 0, 0, &sampler),
            src.end,
            "origin quart is inside the 4096 circle"
        );
        // block 1024,1024 → chunk 64,64 → 64² + 64² = 8192 > 4096: outside.
        let outside = src.get_noise_biome(
            QuartPos::from_block(1024),
            0,
            QuartPos::from_block(1024),
            &sampler,
        );
        assert_ne!(outside, src.end);
        // block 512,512 → chunk 32,32 → 2048 <= 4096: inside.
        assert_eq!(
            src.get_noise_biome(
                QuartPos::from_block(512),
                0,
                QuartPos::from_block(512),
                &sampler
            ),
            src.end
        );
    }

    #[test]
    fn erosion_buckets_follow_paper_thresholds() {
        use crate::levelgen::noise::density_functions;

        let zero = density_functions::zero();
        let sampler = |v: f64| Sampler {
            temperature: zero.clone(),
            humidity: zero.clone(),
            continentalness: zero.clone(),
            erosion: density_functions::constant(v),
            depth: zero.clone(),
            weirdness: zero.clone(),
            spawn_target: Vec::new(),
        };

        let src = end_source();
        // A far-out quart (outside the 4096 circle) so the erosion branch runs.
        let qx = QuartPos::from_block(4096);
        let qz = QuartPos::from_block(4096);
        let y = 64;
        assert_eq!(src.get_noise_biome(qx, y, qz, &sampler(0.3)), src.highlands);
        assert_eq!(src.get_noise_biome(qx, y, qz, &sampler(0.0)), src.midlands);
        assert_eq!(src.get_noise_biome(qx, y, qz, &sampler(-0.3)), src.islands);
        assert_eq!(src.get_noise_biome(qx, y, qz, &sampler(-0.1)), src.barrens);
    }
}
