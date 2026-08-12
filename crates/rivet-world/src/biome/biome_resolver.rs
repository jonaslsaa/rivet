//! `net.minecraft.world.level.biome.BiomeResolver` — the noise-biome
//! resolver (issue #178, `mc.world.level.biome.core` unit).
//!
//! Java's interface:
//!
//! ```java
//! public interface BiomeResolver {
//!     Holder<Biome> getNoiseBiome(int quartX, int quartY, int quartZ, Climate.Sampler sampler);
//! }
//! ```
//!
//! The port returns `Holder<BiomeId>` — the registry-held biome reference the
//! pure-ID model carries (see `rivet-registry::biome_id`); the `Biome` element
//! value is a shell (this unit), so the resolved handle is the id, not the
//! value. The quart coordinates and `Climate::Sampler` match Java exactly. A
//! trivial object-safe trait: implementors resolve a quart-position to a biome
//! holder given the climate sampler.

use crate::biome::climate::Sampler;
use rivet_registry::Holder;
use rivet_registry::biome_id::BiomeId;

/// `net.minecraft.world.level.biome.BiomeResolver`.
pub trait BiomeResolver {
    /// `getNoiseBiome(int quartX, int quartY, int quartZ, Climate.Sampler)`.
    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId>;
}
