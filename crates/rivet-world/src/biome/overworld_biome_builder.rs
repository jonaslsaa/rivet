//! Port of `net.minecraft.world.level.biome.OverworldBiomeBuilder` (26.2) —
//! the `mc.world.level.biome.source` unit's view of the `.data`-owned builder.
//!
//! The builder's *biome data* (`addBiomes` → the `addPeaks`/`addHighSlice`/
//! `addLowSlice`/`addValleys`/`addOffCoastBiomes` tables that generate the full
//! overworld parameter list) belongs to the `mc.world.level.biome.data` unit
//! (which owns `OverworldBiomeBuilder.java`), so [`OverworldBiomeBuilder::add_biomes`]
//! is a declared STUB — see [`multi_noise_biome_source_parameter_list`], which
//! uses it to build the `Preset::OVERWORLD` parameter list.
//!
//! The header *parameter* fields (`temperatures`/`humidities`/`erosions`/the
//! continentalness spans) and the `@VisibleForDebug` debug-string surface are
//! ported here faithfully — they are pure class-header data (lines 39-74) plus
//! the small `getDebugStringFor*` methods, and `MultiNoiseBiomeSource::addDebugInfo`
//! depends on them.

use crate::biome::climate::{Parameter, ParameterPoint, quantize_coord};
use crate::levelgen::noisegen::noise_router_data::peaks_and_valleys_f32;
use rivet_registry::ResourceKey;
use rivet_registry::biome_id::BiomeId;

/// `OverworldBiomeBuilder` — the `.data`-owned overworld biome builder. The
/// value surface needed by this unit is the debug-string set; the `add_biomes`
/// data table is the STUB.
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

    /// STUB(#178) — `addBiomes(Consumer<Pair<ParameterPoint, ResourceKey<Biome>>>)`.
    ///
    /// The `addPeaks`/`addHighSlice`/`addLowSlice`/`addValleys`/
    /// `addOffCoastBiomes` tables that emit the full overworld parameter list
    /// belong to the `.data` unit (which owns `OverworldBiomeBuilder.java`).
    /// Until it lands, this emits nothing — the `Preset::OVERWORLD` parameter
    /// list is empty (see the STUB marker on this module). The signature mirrors
    /// Java exactly so the `.data` port can fill the body in place;
    /// `pub(crate)` keeps the no-op unobservable outside this crate.
    pub(crate) fn add_biomes(
        &self,
        _biomes: &mut dyn FnMut((ParameterPoint, ResourceKey<BiomeId>)),
    ) {
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
