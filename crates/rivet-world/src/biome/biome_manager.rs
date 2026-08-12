//! `net.minecraft.world.level.biome.BiomeManager` — the fiddled-distance biome
//! resolver (issue #178, `mc.world.level.biome.core` unit).
//!
//! Faithful port of the 26.2 `BiomeManager.java`: the `NoiseBiomeSource` +
//! `biomeZoomSeed`, the `getBiome` 8-corner fiddled-distance interpolation,
//! `getNoiseBiomeAtPosition`/`getNoiseBiomeAtQuart`, `obfuscateSeed`, and
//! `CHUNK_CENTER_QUART`. The resolved biome is the id-handle `Holder<BiomeId>`
//! (the pure-ID model; Java's `Holder<Biome>`).
//!
//! ## Fidelity notes
//!
//! - `getFiddledDistance` runs six `LinearCongruentialGenerator.next` steps
//!   (wrapping i64), then `getFiddle` for each axis — `((rval >> 24) & 1023) -
//!   512` as a double times `0.9 / 1024.0` (the Paper form: no `floorMod`, no
//!   FP division/subtraction beyond the one multiply).
//! - `getBiome` shifts the block position by `-2` before the `>> 2` quart
//!   split, and the fractional `& 3` is Java's non-negative low two bits (the
//!   position was biased by `-2`, so the corner arithmetic stays exact).
//! - The dead `ZOOM_BITS`/`ZOOM`/`ZOOM_MASK` constants are dropped (unused in
//!   Java 26.2).

use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{BlockPos, QuartPos};
use rivet_registry::holder::Holder;
use rivet_util::java_hash::obfuscate_seed;
use rivet_util::linear_congruential_generator;
use rivet_util::mth;
use std::sync::Arc;

/// `BiomeManager.CHUNK_CENTER_QUART` — `QuartPos.fromBlock(8)` = `2` (the
/// block `8` is the chunk-center offset).
pub const CHUNK_CENTER_QUART: i32 = 2;

/// `net.minecraft.world.level.biome.BiomeManager`.
pub struct BiomeManager {
    /// `BiomeManager.noiseBiomeSource`.
    noise_biome_source: Arc<dyn NoiseBiomeSource>,
    /// `BiomeManager.biomeZoomSeed`.
    biome_zoom_seed: i64,
}

impl BiomeManager {
    /// `new BiomeManager(NoiseBiomeSource, long seed)`.
    pub fn new(noise_biome_source: Arc<dyn NoiseBiomeSource>, seed: i64) -> Self {
        BiomeManager {
            noise_biome_source,
            biome_zoom_seed: seed,
        }
    }

    /// `BiomeManager.obfuscateSeed(long seed)` — `Hashing.sha256().
    /// hashLong(seed).asLong()` (see `rivet_util::java_hash::obfuscate_seed`).
    pub fn obfuscate_seed(seed: i64) -> i64 {
        obfuscate_seed(seed)
    }

    /// `BiomeManager.withDifferentSource(NoiseBiomeSource)`.
    pub fn with_different_source(&self, biome_source: Arc<dyn NoiseBiomeSource>) -> BiomeManager {
        BiomeManager::new(biome_source, self.biome_zoom_seed)
    }

    /// `BiomeManager.getBiome(BlockPos)` — the fiddled-distance 8-corner
    /// interpolation over the four surrounding quart cells.
    pub fn get_biome(&self, pos: &BlockPos) -> Holder<BiomeId> {
        let abs_x = pos.get_x() - 2;
        let abs_y = pos.get_y() - 2;
        let abs_z = pos.get_z() - 2;
        let parent_x = abs_x >> 2;
        let parent_y = abs_y >> 2;
        let parent_z = abs_z >> 2;
        let fract_x = (abs_x & 3) as f64 / 4.0;
        let fract_y = (abs_y & 3) as f64 / 4.0;
        let fract_z = (abs_z & 3) as f64 / 4.0;
        let mut min_i = 0;
        let mut min_fiddled_distance = f64::INFINITY;

        for i in 0..8 {
            let x_even = (i & 4) == 0;
            let y_even = (i & 2) == 0;
            let z_even = (i & 1) == 0;
            let corner_x = if x_even { parent_x } else { parent_x + 1 };
            let corner_y = if y_even { parent_y } else { parent_y + 1 };
            let corner_z = if z_even { parent_z } else { parent_z + 1 };
            let distance_x = if x_even { fract_x } else { fract_x - 1.0 };
            let distance_y = if y_even { fract_y } else { fract_y - 1.0 };
            let distance_z = if z_even { fract_z } else { fract_z - 1.0 };
            let next = get_fiddled_distance(
                self.biome_zoom_seed,
                corner_x,
                corner_y,
                corner_z,
                distance_x,
                distance_y,
                distance_z,
            );
            if min_fiddled_distance > next {
                min_i = i;
                min_fiddled_distance = next;
            }
        }

        let biome_x = if (min_i & 4) == 0 {
            parent_x
        } else {
            parent_x + 1
        };
        let biome_y = if (min_i & 2) == 0 {
            parent_y
        } else {
            parent_y + 1
        };
        let biome_z = if (min_i & 1) == 0 {
            parent_z
        } else {
            parent_z + 1
        };
        self.noise_biome_source
            .get_noise_biome(biome_x, biome_y, biome_z)
    }

    /// `BiomeManager.getNoiseBiomeAtPosition(double x, double y, double z)`.
    pub fn get_noise_biome_at_position(&self, x: f64, y: f64, z: f64) -> Holder<BiomeId> {
        let quart_x = QuartPos::from_block(mth::floor_d(x));
        let quart_y = QuartPos::from_block(mth::floor_d(y));
        let quart_z = QuartPos::from_block(mth::floor_d(z));
        self.get_noise_biome_at_quart(quart_x, quart_y, quart_z)
    }

    /// `BiomeManager.getNoiseBiomeAtPosition(BlockPos)`.
    pub fn get_noise_biome_at_position_pos(&self, block_pos: &BlockPos) -> Holder<BiomeId> {
        let quart_x = QuartPos::from_block(block_pos.get_x());
        let quart_y = QuartPos::from_block(block_pos.get_y());
        let quart_z = QuartPos::from_block(block_pos.get_z());
        self.get_noise_biome_at_quart(quart_x, quart_y, quart_z)
    }

    /// `BiomeManager.getNoiseBiomeAtQuart(int quartX, int quartY, int quartZ)`.
    pub fn get_noise_biome_at_quart(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
    ) -> Holder<BiomeId> {
        self.noise_biome_source
            .get_noise_biome(quart_x, quart_y, quart_z)
    }
}

/// `BiomeManager.getFiddledDistance(...)` — the six-step LCG scramble then the
/// squared-distance sum over the fiddled offsets.
fn get_fiddled_distance(
    seed: i64,
    x_random: i32,
    y_random: i32,
    z_random: i32,
    distance_x: f64,
    distance_y: f64,
    distance_z: f64,
) -> f64 {
    let mut rval = seed;
    rval = linear_congruential_generator::next(rval, x_random as i64);
    rval = linear_congruential_generator::next(rval, y_random as i64);
    rval = linear_congruential_generator::next(rval, z_random as i64);
    rval = linear_congruential_generator::next(rval, x_random as i64);
    rval = linear_congruential_generator::next(rval, y_random as i64);
    rval = linear_congruential_generator::next(rval, z_random as i64);
    let fiddle_x = get_fiddle(rval);
    rval = linear_congruential_generator::next(rval, seed);
    let fiddle_y = get_fiddle(rval);
    rval = linear_congruential_generator::next(rval, seed);
    let fiddle_z = get_fiddle(rval);
    mth::square_f64(distance_z + fiddle_z)
        + mth::square_f64(distance_y + fiddle_y)
        + mth::square_f64(distance_x + fiddle_x)
}

/// `BiomeManager.getFiddle(long rval)` —
/// `(double)(((rval >> 24) & 1023) - 512) * (0.9 / 1024.0)`.
fn get_fiddle(rval: i64) -> f64 {
    (((rval >> 24) & 1023) - 512) as f64 * (0.9 / 1024.0)
}

/// `BiomeManager.NoiseBiomeSource` — resolves a quart position to a biome
/// holder. The port returns the id-handle `Holder<BiomeId>` (Java's
/// `Holder<Biome>`).
pub trait NoiseBiomeSource: Send + Sync {
    /// `getNoiseBiome(int quartX, int quartY, int quartZ)`.
    fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Holder<BiomeId>;
}

impl std::fmt::Debug for BiomeManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BiomeManager")
            .field("biome_zoom_seed", &self.biome_zoom_seed)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A `NoiseBiomeSource` recording the quart positions it is asked for in a
    /// shared `Mutex` (the trait is not `Any`-downcastable, so the recorder is
    /// shared by `Arc` instead).
    struct RecordingSource {
        calls: Arc<Mutex<Vec<(i32, i32, i32)>>>,
    }

    impl RecordingSource {
        fn new() -> (Arc<Mutex<Vec<(i32, i32, i32)>>>, Self) {
            let calls = Arc::new(Mutex::new(Vec::new()));
            (calls.clone(), RecordingSource { calls })
        }
    }

    impl NoiseBiomeSource for RecordingSource {
        fn get_noise_biome(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> Holder<BiomeId> {
            self.calls.lock().unwrap().push((quart_x, quart_y, quart_z));
            Holder::reference(rivet_registry::holder::RegistryId(0), 0)
        }
    }

    #[test]
    fn chunk_center_quart_is_two() {
        assert_eq!(CHUNK_CENTER_QUART, 2);
    }

    #[test]
    fn obfuscate_seed_matches_util() {
        // The manager delegates to the shared `java_hash::obfuscate_seed`.
        assert_eq!(BiomeManager::obfuscate_seed(42), obfuscate_seed(42));
        assert_eq!(
            BiomeManager::obfuscate_seed(0),
            0x7A0B81A1F57055AFu64 as i64
        );
    }

    #[test]
    fn get_noise_biome_at_quart_delegates() {
        let (calls, source) = RecordingSource::new();
        let manager = BiomeManager::new(Arc::new(source), 42);
        let holder = manager.get_noise_biome_at_quart(1, 2, 3);
        assert_eq!(
            holder,
            Holder::reference(rivet_registry::holder::RegistryId(0), 0)
        );
        assert_eq!(calls.lock().unwrap()[0], (1, 2, 3));
    }

    #[test]
    fn get_biome_picks_a_corner_within_parent_quart() {
        let (calls, source) = RecordingSource::new();
        let manager = BiomeManager::new(Arc::new(source), 1234);
        // A block at (4, 4, 4) -> abs (-2+4)=2 -> parent quart 0; the resolved
        // corner must be 0 or 1 in each axis.
        let holder = manager.get_biome(&BlockPos::new(4, 4, 4));
        assert_eq!(
            holder,
            Holder::reference(rivet_registry::holder::RegistryId(0), 0)
        );
        let call = calls.lock().unwrap()[0];
        assert!((0..=1).contains(&call.0));
        assert!((0..=1).contains(&call.1));
        assert!((0..=1).contains(&call.2));
    }

    #[test]
    fn get_biome_is_deterministic() {
        let (calls_a, source_a) = RecordingSource::new();
        let (calls_b, source_b) = RecordingSource::new();
        let manager_a = BiomeManager::new(Arc::new(source_a), 999);
        let manager_b = BiomeManager::new(Arc::new(source_b), 999);
        let pos = BlockPos::new(-7, 60, 19);
        let _ = manager_a.get_biome(&pos);
        let _ = manager_b.get_biome(&pos);
        assert_eq!(
            calls_a.lock().unwrap().clone(),
            calls_b.lock().unwrap().clone()
        );
    }

    #[test]
    fn get_noise_biome_at_position_floors_then_quarts() {
        let (calls, source) = RecordingSource::new();
        let manager = BiomeManager::new(Arc::new(source), 42);
        // x = 8.5 -> floor 8 -> quart 2; y = -1.2 -> floor -2 -> quart -1.
        let _ = manager.get_noise_biome_at_position(8.5, -1.2, 15.9);
        assert_eq!(calls.lock().unwrap()[0], (2, -1, 3));
    }

    #[test]
    fn get_fiddle_matches_paper_form() {
        // `((rval >> 24) & 1023) - 512` times `0.9 / 1024.0`.
        assert_eq!(get_fiddle(0), -512.0 * 0.9 / 1024.0);
        // `rval >> 24 == 512` needs bit 33 set (bits 24..32 clear).
        assert_eq!(get_fiddle(1i64 << 33), 0.0 * 0.9 / 1024.0);
        assert_eq!(get_fiddle(0x7F000000), (127 - 512) as f64 * 0.9 / 1024.0);
    }
}
