//! `net.minecraft.world.level.biome.BiomeGenerationSettings` — identity shell
//! (issue #178, `mc.world.level.biome.core` unit).
//!
//! Java's class holds the `HolderSet<ConfiguredWorldCarver<?>>` carvers and
//! `List<HolderSet<PlacedFeature>>` features, the memoized
//! `boneMealFeatures`/`featureSet`, the `CODEC`, the `Builder`/`PlainBuilder`,
//! and the `EMPTY` constant. All of it defers to the owning unit (it is
//! entangled with the `levelgen.carver`/`levelgen.feature`/`levelgen.placement`
//! holder codecs). This slice only carries the type identity.
//!
//! RivetTodo(#178): full value/codec/behavior port.

/// `net.minecraft.world.level.biome.BiomeGenerationSettings` — the biome
/// generation-settings identity. A unit shell (see the module doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct BiomeGenerationSettings;

impl BiomeGenerationSettings {
    /// `BiomeGenerationSettings.EMPTY` — the identity shell's single value.
    pub const EMPTY: BiomeGenerationSettings = BiomeGenerationSettings;
}
