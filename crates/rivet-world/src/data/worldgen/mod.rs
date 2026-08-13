//! `net.minecraft.data.worldgen` — the `mc.data.worldgen.prereq` unit's
//! `rivet-world::data::worldgen` slice.
//!
//! PROVENANCE: the 3-file prerequisite slice split out of the 29-file
//! `net.minecraft.data.worldgen` package (MANIFEST `mc.data.worldgen` keeps
//! the 26-file residual; `mc.data.worldgen.prereq` owns these three). The
//! crate is `rivet-world` (not the package default `rivet-registry`) because
//! these build on the `rivet-world` noise/synth/`CubicSpline` layers they
//! register and shape.
//!
//! - `BootstrapContext` — the registry-bootstrap contract (`register` +
//!   `lookup`) that every `net.minecraft.data.worldgen` bootstrap method takes.
//!   The production implementation is `RegistrySetBuilder.BuildState`'s
//!   anonymous `BootstrapContext` (deferred — the registry-builder unit); this
//!   module ships the trait plus a test-only recording context until then.
//! - `TerrainProvider` — the overworld offset/factor/jaggedness `CubicSpline`
//!   builders and the `peaksAndValleys` ridge function (the `#178` biome and
//!   `#177` density-function consumers need these values before the
//!   data-driven registries exist).
//! - `NoiseData` — the noise registry bootstrap: `DEFAULT_SHIFT` plus the
//!   63 declaration-ordered `register` calls and the `registerBiomeNoises`
//!   /`register` helpers.
//! - `WorldgenBootstraps` — the production bootstrap consumer: drives the
//!   NOISE / DENSITY_FUNCTION / NOISE_SETTINGS bootstraps through
//!   `RecordingContext` into frozen `Registry<T>`s, bundled in a `RegistryAccess`
//!   (the real overworld composition for probes/offline samplers).
//!
//! The `data::worldgen` module stays otherwise minimal: the test-only
//! `RecordingContext` is the seam until `RegistrySetBuilder` (`#126`) lands, and
//! `WorldgenBootstraps` is its production consumer.

pub mod bootstrap_context;
pub mod noise_data;
pub mod terrain_provider;
pub mod worldgen_bootstraps;
