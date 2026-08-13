//! Port of `net.minecraft.world.level.biome.OverworldBiomeBuilder` (26.2) —
//! the `mc.world.level.biome.data`-owned builder.
//!
//! [`OverworldBiomeBuilder::add_biomes`] emits the full overworld biome
//! parameter list (Java `addBiomes` → `addOffCoastBiomes`/`addInlandBiomes`/
//! `addUndergroundBiomes` → the `addPeaks`/`addHighSlice`/`addLowSlice`/
//! `addValleys` tables). The 7594 parameter points are not re-derived from the
//! Java tables in Rust: they are extracted once from a live Paper 26.2 load
//! (`MultiNoiseBiomeSourceParameterList.knownPresets()` — the builder's exact
//! output in value order) into the generated
//! [`rivet_registry::generated::worldgen::OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS`],
//! and this method projects that table into the runtime
//! [`ParameterPoint`] + `ResourceKey<BiomeId>` pairs, bit-exact (the generated
//! spans are the already-quantized longs) and in the generated (== Paper) order.
//!
//! The header *parameter* fields (`temperatures`/`humidities`/`erosions`/the
//! continentalness spans) and the `@VisibleForDebug` debug-string surface are
//! ported here faithfully — they are pure class-header data (lines 39-74) plus
//! the small `getDebugStringFor*` methods, and `MultiNoiseBiomeSource::addDebugInfo`
//! depends on them.

use crate::biome::biomes::register_from_full_name;
use crate::biome::climate::{Parameter, ParameterPoint, quantize_coord};
use crate::levelgen::noisegen::noise_router_data::peaks_and_valleys_f32;
use rivet_registry::ResourceKey;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::generated::worldgen::OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS;

/// `OverworldBiomeBuilder` — the `.data`-owned overworld biome builder. The
/// header parameter spans and the debug-string surface live here; `add_biomes`
/// emits the generated overworld parameter table.
pub struct OverworldBiomeBuilder {
    /// `temperatures` — the five temperature spans.
    temperatures: Vec<Parameter>,
    /// `humidities` — the five humidity spans.
    humidities: Vec<Parameter>,
    /// `erosions` — the seven erosion spans.
    erosions: Vec<Parameter>,
    /// `mushroomFieldsContinentalness` — `span(-1.2F, -1.05F)`.
    mushroom_fields_continentalness: Parameter,
    /// `deepOceanContinentalness` — `span(-1.05F, -0.455F)`.
    deep_ocean_continentalness: Parameter,
    /// `oceanContinentalness` — `span(-0.455F, -0.19F)`.
    ocean_continentalness: Parameter,
    /// `coastContinentalness` — `span(-0.19F, -0.11F)`.
    coast_continentalness: Parameter,
    /// `nearInlandContinentalness` — `span(-0.11F, 0.03F)`.
    near_inland_continentalness: Parameter,
    /// `midInlandContinentalness` — `span(0.03F, 0.3F)`.
    mid_inland_continentalness: Parameter,
}

impl Default for OverworldBiomeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl OverworldBiomeBuilder {
    /// `new OverworldBiomeBuilder()` — the header parameter spans.
    pub fn new() -> Self {
        OverworldBiomeBuilder {
            temperatures: vec![
                Parameter::span(-1.0, -0.45),
                Parameter::span(-0.45, -0.15),
                Parameter::span(-0.15, 0.2),
                Parameter::span(0.2, 0.55),
                Parameter::span(0.55, 1.0),
            ],
            humidities: vec![
                Parameter::span(-1.0, -0.35),
                Parameter::span(-0.35, -0.1),
                Parameter::span(-0.1, 0.1),
                Parameter::span(0.1, 0.3),
                Parameter::span(0.3, 1.0),
            ],
            erosions: vec![
                Parameter::span(-1.0, -0.78),
                Parameter::span(-0.78, -0.375),
                Parameter::span(-0.375, -0.2225),
                Parameter::span(-0.2225, 0.05),
                Parameter::span(0.05, 0.45),
                Parameter::span(0.45, 0.55),
                Parameter::span(0.55, 1.0),
            ],
            mushroom_fields_continentalness: Parameter::span(-1.2, -1.05),
            deep_ocean_continentalness: Parameter::span(-1.05, -0.455),
            ocean_continentalness: Parameter::span(-0.455, -0.19),
            coast_continentalness: Parameter::span(-0.19, -0.11),
            near_inland_continentalness: Parameter::span(-0.11, 0.03),
            mid_inland_continentalness: Parameter::span(0.03, 0.3),
        }
    }

    /// `addBiomes(Consumer<Pair<Climate.ParameterPoint, ResourceKey<Biome>>>)`.
    ///
    /// Emits the full overworld parameter list — the 7594 points of
    /// `OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS` (extracted from Paper's
    /// `knownPresets()` in the builder's value order), each projected to a
    /// runtime [`ParameterPoint`] and the matching `ResourceKey<BiomeId>`.
    /// Java's package-private scope maps to `pub(crate)`: the preset builder in
    /// `multi_noise_biome_source_parameter_list` is the only caller.
    pub(crate) fn add_biomes(
        &self,
        biomes: &mut dyn FnMut((ParameterPoint, ResourceKey<BiomeId>)),
    ) {
        for generated in OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS {
            let point = ParameterPoint::new(
                Parameter::new(generated.temperature.0, generated.temperature.1),
                Parameter::new(generated.humidity.0, generated.humidity.1),
                Parameter::new(generated.continentalness.0, generated.continentalness.1),
                Parameter::new(generated.erosion.0, generated.erosion.1),
                Parameter::new(generated.depth.0, generated.depth.1),
                Parameter::new(generated.weirdness.0, generated.weirdness.1),
                generated.offset,
            );
            biomes((point, register_from_full_name(generated.biome)));
        }
    }

    /// `OverworldBiomeBuilder.getDebugStringForPeaksAndValleys(double)` (static).
    ///
    /// The `@VisibleForDebug` peaks-and-valleys label for the given
    /// `NoiseRouterData.peaksAndValleys` value.
    pub fn get_debug_string_for_peaks_and_valleys(peaks_and_valleys: f64) -> &'static str {
        if peaks_and_valleys < peaks_and_valleys_f32(0.05) as f64 {
            "Valley"
        } else if peaks_and_valleys < peaks_and_valleys_f32(0.26666668) as f64 {
            "Low"
        } else if peaks_and_valleys < peaks_and_valleys_f32(0.4) as f64 {
            "Mid"
        } else {
            // `peaksAndValleys < NoiseRouterData.peaksAndValleys(0.56666666F)
            // ? "High" : "Peak"`.
            if peaks_and_valleys < peaks_and_valleys_f32(0.56666666) as f64 {
                "High"
            } else {
                "Peak"
            }
        }
    }

    /// `getDebugStringForContinentalness(double)` — the continentalness band
    /// label over the quantized value.
    pub fn get_debug_string_for_continentalness(&self, continentalness: f64) -> &'static str {
        // `double continentalnessQuantized = Climate.quantizeCoord((float)
        // continentalness)`.
        let continentalness_quantized = quantize_coord(continentalness as f32);
        if continentalness_quantized < self.mushroom_fields_continentalness.max {
            "Mushroom fields"
        } else if continentalness_quantized < self.deep_ocean_continentalness.max {
            "Deep ocean"
        } else if continentalness_quantized < self.ocean_continentalness.max {
            "Ocean"
        } else if continentalness_quantized < self.coast_continentalness.max {
            "Coast"
        } else if continentalness_quantized < self.near_inland_continentalness.max {
            "Near inland"
        } else {
            // `continentalnessQuantized < this.midInlandContinentalness.max()
            // ? "Mid inland" : "Far inland"`.
            if continentalness_quantized < self.mid_inland_continentalness.max {
                "Mid inland"
            } else {
                "Far inland"
            }
        }
    }

    /// `getDebugStringForErosion(double)`.
    pub fn get_debug_string_for_erosion(&self, erosion: f64) -> String {
        get_debug_string_for_noise_value(erosion, &self.erosions)
    }

    /// `getDebugStringForTemperature(double)`.
    pub fn get_debug_string_for_temperature(&self, temperature: f64) -> String {
        get_debug_string_for_noise_value(temperature, &self.temperatures)
    }

    /// `getDebugStringForHumidity(double)`.
    pub fn get_debug_string_for_humidity(&self, humidity: f64) -> String {
        get_debug_string_for_noise_value(humidity, &self.humidities)
    }
}

/// `getDebugStringForNoiseValue(double, Climate.Parameter[])` (static) — the
/// index of the first span whose `max` exceeds the quantized value, or `"?"`.
fn get_debug_string_for_noise_value(noise_value: f64, array: &[Parameter]) -> String {
    let noise_value_quantized = quantize_coord(noise_value as f32);
    for (i, parameter) in array.iter().enumerate() {
        if noise_value_quantized < parameter.max {
            return i.to_string();
        }
    }
    "?".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::worldgen::OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS;

    /// Runs `add_biomes` and returns the emitted `(point, key)` pairs.
    fn emitted_pairs() -> Vec<(ParameterPoint, rivet_registry::ResourceKey<BiomeId>)> {
        let builder = OverworldBiomeBuilder::new();
        let mut out = Vec::new();
        builder.add_biomes(&mut |pair| out.push(pair));
        out
    }

    #[test]
    fn add_biomes_emits_the_full_paper_parameter_list_in_order() {
        let emitted = emitted_pairs();
        // Cardinality: the live Paper 26.2 overworld preset has exactly 7594
        // points (`worldgen.rs` anchor + the fixture probe pin this).
        assert_eq!(emitted.len(), OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS.len());
        assert_eq!(emitted.len(), 7594);
        // The emitted pairs are a 1:1 projection of the generated table in the
        // same order: the point spans are bit-identical (quantized longs) and
        // the key resolves to the same biome name at every index.
        for (i, (point, key)) in emitted.iter().enumerate() {
            let generated = &OVERWORLD_BIOME_SOURCE_PARAMETER_POINTS[i];
            assert_eq!(
                point,
                &ParameterPoint::new(
                    Parameter::new(generated.temperature.0, generated.temperature.1),
                    Parameter::new(generated.humidity.0, generated.humidity.1),
                    Parameter::new(generated.continentalness.0, generated.continentalness.1),
                    Parameter::new(generated.erosion.0, generated.erosion.1),
                    Parameter::new(generated.depth.0, generated.depth.1),
                    Parameter::new(generated.weirdness.0, generated.weirdness.1),
                    generated.offset,
                ),
                "point {i} must match the generated table bit-for-bit"
            );
            assert_eq!(
                key.identifier().to_string(),
                generated.biome,
                "key {i} must resolve the generated biome name"
            );
        }
    }

    #[test]
    fn add_biomes_covers_fifty_five_distinct_biomes() {
        let emitted = emitted_pairs();
        let mut names: Vec<String> = emitted
            .iter()
            .map(|(_, k)| k.identifier().to_string())
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 55);
        // Spot-check a few representative members of the surface + underground set.
        for expected in [
            "minecraft:mushroom_fields",
            "minecraft:plains",
            "minecraft:deep_dark",
            "minecraft:dripstone_caves",
            "minecraft:lush_caves",
            "minecraft:sulfur_caves",
            "minecraft:windswept_savanna",
            "minecraft:eroded_badlands",
        ] {
            assert!(
                names.binary_search(&expected.to_string()).is_ok(),
                "{expected} must be present"
            );
        }
    }

    #[test]
    fn first_entry_is_the_mushroom_fields_off_coast_pair() {
        let emitted = emitted_pairs();
        let (point, key) = &emitted[0];
        // `addOffCoastBiomes` — mushroom fields: FULL_RANGE temp/humidity,
        // mushroom-fields continentalness span, FULL_RANGE erosion, surface
        // depth 0.0F, FULL_RANGE weirdness, offset 0.0F.
        assert_eq!(key.identifier().to_string(), "minecraft:mushroom_fields");
        assert_eq!(
            *point,
            ParameterPoint::new(
                Parameter::new(-10000, 10000),
                Parameter::new(-10000, 10000),
                Parameter::new(-12000, -10500),
                Parameter::new(-10000, 10000),
                Parameter::new(0, 0),
                Parameter::new(-10000, 10000),
                0,
            )
        );
        // The paired depth=1.0F entry follows immediately.
        assert_eq!(
            emitted[1].0.depth,
            Parameter::new(10000, 10000),
            "each addSurfaceBiome emits the depth 0.0F then 1.0F pair"
        );
    }

    #[test]
    fn underground_biomes_carry_the_depth_spans() {
        let emitted = emitted_pairs();
        // The underground section is the final four entries: dripstone, lush,
        // sulfur at depth span (0.2F, 0.9F), then deep_dark at depth point
        // 1.1F (the `addBottomBiome`).
        let (dripstone, key0) = &emitted[7590];
        assert_eq!(key0.identifier().to_string(), "minecraft:dripstone_caves");
        assert_eq!(dripstone.depth, Parameter::new(2000, 9000));
        assert_eq!(dripstone.continentalness, Parameter::new(8000, 10000));
        let (lush, key1) = &emitted[7591];
        assert_eq!(key1.identifier().to_string(), "minecraft:lush_caves");
        assert_eq!(lush.depth, Parameter::new(2000, 9000));
        assert_eq!(lush.humidity, Parameter::new(7000, 10000));
        let (sulfur, key2) = &emitted[7592];
        assert_eq!(key2.identifier().to_string(), "minecraft:sulfur_caves");
        assert_eq!(sulfur.depth, Parameter::new(2000, 9000));
        assert_eq!(sulfur.weirdness, Parameter::new(-11000, -8500));
        let (deep_dark, key3) = &emitted[7593];
        assert_eq!(key3.identifier().to_string(), "minecraft:deep_dark");
        assert_eq!(deep_dark.depth, Parameter::new(11000, 11000));
        assert_eq!(deep_dark.erosion, Parameter::new(-10000, -3750));
    }

    #[test]
    fn depth_span_distribution_matches_the_builder_surface_underground_split() {
        let emitted = emitted_pairs();
        // Surface biomes pair depth 0.0F/1.0F (3795 each — every addSurfaceBiome
        // emits two points); the three underground biomes use (0.2F, 0.9F); the
        // bottom deep_dark uses the single 1.1F point.
        let mut counts = std::collections::HashMap::new();
        for (point, _) in &emitted {
            *counts
                .entry((point.depth.min, point.depth.max))
                .or_insert(0usize) += 1;
        }
        assert_eq!(counts.get(&(0, 0)), Some(&3795));
        assert_eq!(counts.get(&(10000, 10000)), Some(&3795));
        assert_eq!(counts.get(&(2000, 9000)), Some(&3));
        assert_eq!(counts.get(&(11000, 11000)), Some(&1));
        assert_eq!(counts.len(), 4);
    }

    #[test]
    fn peaks_and_valleys_labels_match_paper_buckets() {
        // Buckets are `< peaksAndValleys(0.05F)` Valley, then Low/Mid, then
        // `>= peaksAndValleys(0.56666666F)` Peak. `peaksAndValleys(x)` is
        // monotonic increasing on [0, 0.7], so the boundaries order as Java.
        let pv = |x: f32| peaks_and_valleys_f32(x) as f64;
        assert_eq!(
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(pv(0.0)),
            "Valley"
        );
        assert_eq!(
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(pv(0.05)),
            "Low"
        );
        assert_eq!(
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(pv(0.26666668)),
            "Mid"
        );
        assert_eq!(
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(pv(0.4)),
            "High"
        );
        assert_eq!(
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(pv(0.56666666)),
            "Peak"
        );
        assert_eq!(
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(pv(0.7)),
            "Peak"
        );
    }

    #[test]
    fn noise_value_bucket_is_first_max_exceeding() {
        let builder = OverworldBiomeBuilder::new();
        // Temperature spans' maxes quantize to [-4500, -1500, 2000, 5500, 10000].
        assert_eq!(builder.get_debug_string_for_temperature(-0.5), "0");
        assert_eq!(builder.get_debug_string_for_temperature(-0.2), "1");
        assert_eq!(builder.get_debug_string_for_temperature(0.0), "2");
        assert_eq!(builder.get_debug_string_for_temperature(0.4), "3");
        assert_eq!(builder.get_debug_string_for_temperature(0.9), "4");
        // Above the top max → "?".
        assert_eq!(builder.get_debug_string_for_temperature(1.5), "?");
    }

    #[test]
    fn continentalness_bands_in_declaration_order() {
        let builder = OverworldBiomeBuilder::new();
        assert_eq!(
            builder.get_debug_string_for_continentalness(-1.1),
            "Mushroom fields"
        );
        assert_eq!(
            builder.get_debug_string_for_continentalness(-0.8),
            "Deep ocean"
        );
        assert_eq!(builder.get_debug_string_for_continentalness(-0.3), "Ocean");
        assert_eq!(builder.get_debug_string_for_continentalness(-0.15), "Coast");
        assert_eq!(
            builder.get_debug_string_for_continentalness(-0.05),
            "Near inland"
        );
        assert_eq!(
            builder.get_debug_string_for_continentalness(0.1),
            "Mid inland"
        );
        assert_eq!(
            builder.get_debug_string_for_continentalness(0.8),
            "Far inland"
        );
    }
}
