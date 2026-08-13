//! Port of `net.minecraft.world.level.chunk.ChunkGenerator` (abstract class,
//! 26.2) — the abstract/default method surface, as the object-safe
//! `&dyn ChunkGenerator` contract feature placement and the worldgen windows
//! consume.
//!
//! Owned by the `mc.world.level.chunk.generator` manifest unit (RivetTodo
//! #185). This out-of-unit seam ports the trait surface; the owning unit lands
//! the full generator stack (`ChunkGenerator`, `ChunkGeneratorStructureState`,
//! `ChunkGenerators`, and the `CODEC` dispatch), and the
//! `NoiseBasedChunkGenerator` realization, server wiring, status executor, and
//! `WorldGenRegion` remain with that unit / the worldgen pipeline.
//!
//! ## The abstract contract
//!
//! Only `getMinY`/`getGenDepth` are required (Java abstract, no default, and
//! consumed by the ported `WorldGenerationContext`/`CarvingContext` windows).
//! The rest of the Java surface is defaulted: the methods either have real
//! ported bodies, or are explicit capability-unavailable seams that panic with
//! a Paper-grounded message rather than fabricate a result.
//!
//! ## The deferred lifecycle seams
//!
//! The five worldgen lifecycle steps (`create_biomes`/`apply_carvers`/
//! `build_surface`/`spawn_original_mobs`/`fill_from_noise`) are **default
//! panic seams**, not abstract methods. Java's signatures take
//! `WorldGenRegion`, `StructureManager`, and the generic `ChunkAccess` — none
//! of which can be named in an object-safe trait method (the Rust chunk types
//! are generic over their storage strategies, so `&dyn ChunkGenerator` cannot
//! carry them), and `WorldGenRegion`/`StructureManager` defer with their owning
//! units. No production caller exists in this crate, so requiring the steps
//! would force every implementor to supply empty bodies with no fidelity gain
//! — and the owning realization must change the signatures anyway (adding the
//! world-touching parameters) when the status executor lands (RivetTodo #185).
//! `createBiomes` in particular has a Java *default* body (not abstract), so a
//! panic seam is the honest unavailable-capability boundary until
//! `fillBiomesFromNoise` is ported. Each seam documents its exact Java
//! signature; the owning realization provides the faithful parameter surface.
//!
//! ## The default surface
//!
//! The Java defaults existing infrastructure supports are ported faithfully:
//! `getSpawnHeight` (64) and `getFirstFreeHeight`/`getFirstOccupiedHeight`
//! (delegating to `getBaseHeight`). The remaining Java defaults (`createState`,
//! `findNearestMapStructure`, `addVanillaDecorations`, `applyBiomeDecoration`,
//! `getMobsAt`, `validate`, `getTypeNameForDataFixer`) defer with their owning
//! units (RivetTodo #185).
//!
//! `createStructures`/`createReferences` and the biome-membership read
//! (`getBiomeGenerationSettings(biome).hasFeature(feature)`, RivetTodo #178)
//! are declared as explicit capability-unavailable seams: they panic with a
//! Paper-grounded message rather than fabricate a result, so callers fail
//! loudly until the owning unit lands.
//!
//! The world-surface reads that are not yet consumable (`getSeaLevel`,
//! `getBaseHeight`, `getBaseColumn`, `addDebugScreenInfo` — Java abstract — and
//! `getBiomeSource`, whose Java body returns the `biomeSource` field a trait
//! cannot hold) are likewise panicking seams: the noisegen value shell
//! (`NoiseBasedChunkGenerator`) ports the real bodies on a separate type, and
//! the owning realization overrides the seams when it lands.
//! `getFirstFreeHeight`/`getFirstOccupiedHeight` delegate to the
//! `getBaseHeight` seam, so they stay executable once an implementor provides
//! a real `getBaseHeight`.
//!
//! RivetTodo(#185): the owning `.chunk.generator` realization must reconcile the
//! trait's deferred seams with the noisegen value shell's real bodies
//! (`getSeaLevel`/`getBaseHeight`/`getBaseColumn`/`addDebugScreenInfo`, plus
//! `fill_from_noise` and the `*_stub` lifecycle shells) — the shell does not
//! implement this trait, so the two must not become separate sources of truth
//! (either implement the trait by delegating to the shell bodies, or move the
//! bodies onto the realization).

use crate::biome::biome_source::BiomeSource;
use crate::level::height_accessor::LevelHeightAccessor;
use crate::levelgen::heightmap::Types;
use crate::levelgen::noisegen::random_state::RandomState;
use crate::levelgen::placement::PlacedFeature;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_registry::holder::Holder;

/// `net.minecraft.world.level.chunk.ChunkGenerator` — the chunk generator
/// behind feature placement and the worldgen pipeline.
pub trait ChunkGenerator: Send + Sync + 'static {
    // -- The abstract getters the feature/placement/worldgen-window seams read.

    /// `ChunkGenerator.getMinY()` — abstract in Java (no default).
    fn get_min_y(&self) -> i32;

    /// `ChunkGenerator.getGenDepth()` — abstract in Java (no default).
    fn get_gen_depth(&self) -> i32;

    // -- The deferred worldgen lifecycle seams (see the module doc).

    /// `ChunkGenerator.createBiomes(RandomState, Blender, StructureManager,
    /// ChunkAccess)` — the biomes step of the chunk status ladder.
    ///
    /// Java declares a default body (`protoChunk.fillBiomesFromNoise(
    /// this.biomeSource, randomState.sampler())`); `fillBiomesFromNoise` on the
    /// chunk surface is not ported, so the trait cannot carry that body and the
    /// step is a capability-unavailable seam. The world-touching parameters
    /// (`StructureManager`, the `ChunkAccess`) and the `CompletableFuture` async
    /// wrapper defer with the owning `.chunk.generator` pipeline (RivetTodo
    /// #185); the owning `NoiseBasedChunkGenerator` realization provides the
    /// faithful signature when the status executor lands.
    fn create_biomes(&self) {
        panic!("ChunkGenerator.createBiomes is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.applyCarvers(WorldGenRegion, long, RandomState,
    /// BiomeManager, StructureManager, ChunkAccess)` — the carvers step of the
    /// status ladder.
    ///
    /// The world-touching parameters (`WorldGenRegion`, `StructureManager`, the
    /// `ChunkAccess`) defer with their owning units (RivetTodo #185); the
    /// owning realization provides the faithful signature when the status
    /// executor lands.
    fn apply_carvers(&self) {
        panic!("ChunkGenerator.applyCarvers is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.buildSurface(WorldGenRegion, StructureManager,
    /// RandomState, ChunkAccess)` — the surface step of the status ladder.
    ///
    /// `RandomState` is ported; `WorldGenRegion`, `StructureManager`, and the
    /// `ChunkAccess` defer with their owning units (RivetTodo #185).
    fn build_surface(&self) {
        panic!("ChunkGenerator.buildSurface is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.spawnOriginalMobs(WorldGenRegion)` — the original-mob
    /// spawn step (`NaturalSpawner.spawnMobsForChunkGeneration`).
    ///
    /// The sole `WorldGenRegion` parameter defers with its owning unit
    /// (RivetTodo #185).
    fn spawn_original_mobs(&self) {
        panic!("ChunkGenerator.spawnOriginalMobs is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.fillFromNoise(Blender, RandomState, StructureManager,
    /// ChunkAccess)` — the block-fill step of the status ladder.
    ///
    /// `Blender`/`RandomState` are ported; `StructureManager` and the
    /// `ChunkAccess` defer with their owning units (RivetTodo #185). The
    /// noisegen unit ports the real body on the value shell
    /// (`NoiseBasedChunkGenerator::fill_from_noise`); the owning realization
    /// overrides this seam with the faithful signature when the status executor
    /// lands (see the module-doc `RivetTodo(#185)` reconciliation note).
    fn fill_from_noise(&self) {
        panic!("ChunkGenerator.fillFromNoise is not implemented (RivetTodo #185)")
    }

    // -- The faithful defaults existing infrastructure supports.

    /// `ChunkGenerator.getSpawnHeight(LevelHeightAccessor)` — the default
    /// spawn surface, `64` (Paper: `return 64;`).
    fn get_spawn_height(&self, _height_accessor: &dyn LevelHeightAccessor) -> i32 {
        64
    }

    /// `ChunkGenerator.getFirstFreeHeight(int, int, Heightmap.Types,
    /// LevelHeightAccessor, RandomState)` — `this.getBaseHeight(x, z, type,
    /// heightAccessor, randomState)`.
    fn get_first_free_height(
        &self,
        x: i32,
        z: i32,
        ty: Types,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> i32 {
        self.get_base_height(x, z, ty, height_accessor, random_state)
    }

    /// `ChunkGenerator.getFirstOccupiedHeight(int, int, Heightmap.Types,
    /// LevelHeightAccessor, RandomState)` — `this.getBaseHeight(x, z, type,
    /// heightAccessor, randomState) - 1` (wrapping `int` arithmetic).
    fn get_first_occupied_height(
        &self,
        x: i32,
        z: i32,
        ty: Types,
        height_accessor: &dyn LevelHeightAccessor,
        random_state: &RandomState,
    ) -> i32 {
        self.get_base_height(x, z, ty, height_accessor, random_state)
            .wrapping_sub(1)
    }

    // -- The deferred world-surface reads (the capability is unavailable until
    // -- the owning realization overrides the seam).

    /// `ChunkGenerator.getSeaLevel()` — abstract in Java (no default). The
    /// noisegen value shell ports the real body; the seam fails explicitly
    /// until the owning realization overrides it (RivetTodo #185).
    fn get_sea_level(&self) -> i32 {
        panic!("ChunkGenerator.getSeaLevel is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.getBaseHeight(int, int, Heightmap.Types,
    /// LevelHeightAccessor, RandomState)` — abstract in Java (no default).
    /// `getFirstFreeHeight`/`getFirstOccupiedHeight` delegate here; the seam
    /// fails explicitly until the owning realization overrides it (RivetTodo
    /// #185).
    fn get_base_height(
        &self,
        _x: i32,
        _z: i32,
        _ty: Types,
        _height_accessor: &dyn LevelHeightAccessor,
        _random_state: &RandomState,
    ) -> i32 {
        panic!("ChunkGenerator.getBaseHeight is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.getBaseColumn(int, int, LevelHeightAccessor,
    /// RandomState)` — abstract in Java (no default). The `NoiseColumn` return
    /// value defers with the `mc.world.level` NoiseColumn unit (RivetTodo
    /// #232); the seam carries the same `(clamped_min_y, Vec<BlockState>)`
    /// shape the noisegen value shell's `get_base_column` produces and fails
    /// explicitly until the owning realization overrides it (RivetTodo #185).
    fn get_base_column(
        &self,
        _x: i32,
        _z: i32,
        _height_accessor: &dyn LevelHeightAccessor,
        _random_state: &RandomState,
    ) -> Option<(i32, Vec<BlockState>)> {
        panic!("ChunkGenerator.getBaseColumn is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.addDebugScreenInfo(List<String>, RandomState, BlockPos)`
    /// — abstract in Java (no default). The noisegen value shell ports the real
    /// body (`NoiseBasedChunkGenerator::add_debug_screen_info`); the seam fails
    /// explicitly until the owning realization overrides it (RivetTodo #185).
    fn add_debug_screen_info(
        &self,
        _result: &mut Vec<String>,
        _random_state: &RandomState,
        _feet_pos: &BlockPos,
    ) {
        panic!("ChunkGenerator.addDebugScreenInfo is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.getBiomeSource()` — Java's default returns the
    /// `biomeSource` field; a trait cannot hold the field, so implementors that
    /// own a biome source override this. The seam fails explicitly until one
    /// does (RivetTodo #185).
    fn get_biome_source(&self) -> &dyn BiomeSource {
        panic!("ChunkGenerator.getBiomeSource is not implemented (RivetTodo #185)")
    }

    // -- The deferred structure/codec seams (see the module doc).

    /// `ChunkGenerator.createStructures(RegistryAccess,
    /// ChunkGeneratorStructureState, StructureManager, ChunkAccess,
    /// StructureTemplateManager, ResourceKey<Level>)` — the structure-starts
    /// step of the status ladder.
    ///
    /// The structure surface (`ChunkGeneratorStructureState`, `StructureManager`,
    /// `StructureTemplateManager`) defers with the owning `.chunk.generator`
    /// structure units (RivetTodo #185); the seam fails explicitly rather than
    /// fabricating starts.
    fn create_structures(&self) {
        panic!("ChunkGenerator.createStructures is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.createReferences(WorldGenLevel, StructureManager,
    /// ChunkAccess)` — the structure-references step of the status ladder. The
    /// structure surface defers with the owning units (RivetTodo #185); the
    /// seam fails explicitly rather than fabricating references.
    fn create_references(&self) {
        panic!("ChunkGenerator.createReferences is not implemented (RivetTodo #185)")
    }

    /// `ChunkGenerator.getBiomeGenerationSettings(Holder<Biome>).hasFeature(
    /// PlacedFeature)` — the biome-membership read `BiomeFilter.shouldPlace`
    /// performs (`context.generator().getBiomeGenerationSettings(biome)
    /// .hasFeature(feature)`).
    ///
    /// STUB(mc.world.level.biome.core) — `BiomeGenerationSettings` and its
    /// `featureSet`/`hasFeature` memo are owned by the `#178` biome-core unit,
    /// so the read fails explicitly rather than fabricating a membership
    /// result (the same capability-unavailable seam as `WorldGenLevel::get_biome`).
    fn get_biome_generation_settings_has_feature(
        &self,
        _biome: &Holder<BiomeId>,
        _feature: &PlacedFeature,
    ) -> bool {
        panic!(
            "ChunkGenerator.getBiomeGenerationSettings(...).hasFeature is not implemented (RivetTodo #178)"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::height_accessor::{SimpleLevelHeightAccessor, create as create_accessor};
    use crate::levelgen::noisegen::noise_generator_settings;
    use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
    use crate::levelgen::synth::normal_noise::NoiseParameters;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::registry::Registry;
    use std::sync::Arc;

    /// A mock generator that answers the surfaces the faithful defaults need:
    /// the abstract getters and the real `getBaseHeight` the height-read
    /// defaults delegate to. (It also overrides `get_sea_level` so the seam has
    /// a real value, though no default delegates to it.)
    struct MockGenerator {
        min_y: i32,
        gen_depth: i32,
        sea_level: i32,
        base_height: i32,
    }

    impl ChunkGenerator for MockGenerator {
        fn get_min_y(&self) -> i32 {
            self.min_y
        }

        fn get_gen_depth(&self) -> i32 {
            self.gen_depth
        }

        fn get_sea_level(&self) -> i32 {
            self.sea_level
        }

        fn get_base_height(
            &self,
            _x: i32,
            _z: i32,
            _ty: Types,
            _height_accessor: &dyn LevelHeightAccessor,
            _random_state: &RandomState,
        ) -> i32 {
            self.base_height
        }
    }

    /// A generator implementing only the abstract required surface — every
    /// default seam (which panics) is left at its exact default, so the
    /// deferred-seam test observes the default behavior.
    struct SeamOnlyGenerator;

    impl ChunkGenerator for SeamOnlyGenerator {
        fn get_min_y(&self) -> i32 {
            0
        }

        fn get_gen_depth(&self) -> i32 {
            384
        }
    }

    /// The `noisegen` test pattern — `noise_based_chunk_generator::tests::
    /// populated_registries`: a noise registry populated via `NoiseData.bootstrap`
    /// because `RandomState::create` eagerly constructs the `SurfaceSystem`,
    /// which resolves its nine `Noises.*` keys (including `clay_bands_offset`)
    /// through the registry. The density-function registry stays empty: the
    /// test router carries no `HolderHolder`/`NoiseHolder` nodes. The registries
    /// are returned so the caller keeps them alive in the same scope as the
    /// `RandomState` that borrows them.
    fn populated_registries() -> (Registry<NoiseParameters>, Registry<DensityFunctionValue>) {
        use crate::data::worldgen::bootstrap_context::RecordingContext;
        use crate::data::worldgen::noise_data;
        use rivet_registry::RegistrationInfo;
        use rivet_registry::RegistryAccess;
        use rivet_registry::holder::RegistryId;

        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let mut noise_builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(noise_key);
        let mut noise_ctx = RecordingContext::<NoiseParameters>::new(
            RegistryId(0),
            (*noise_key).clone(),
            RegistryAccess::empty(),
        );
        noise_data::bootstrap(&mut noise_ctx);
        for reg in noise_ctx.registrations() {
            noise_builder.register(
                &reg.key,
                Arc::new(reg.value.clone()),
                RegistrationInfo::BUILT_IN,
            );
        }
        let noise_registry = noise_builder.freeze();
        let df_key = &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION;
        let df_registry: Registry<DensityFunctionValue> = RegistryBuilder::new(df_key).freeze();
        (noise_registry, df_registry)
    }

    /// `RandomState.create` over the [`populated_registries`] fixtures, borrowing
    /// them for the returned state's lifetime.
    fn random_state<'a>(
        noise_registry: &'a Registry<NoiseParameters>,
        df_registry: &'a Registry<DensityFunctionValue>,
    ) -> RandomState<'a> {
        RandomState::create(
            &noise_generator_settings::dummy(),
            noise_registry,
            df_registry,
            1234,
        )
    }

    fn accessor() -> SimpleLevelHeightAccessor {
        create_accessor(-64, 384)
    }

    /// The trait is object-safe: `&dyn ChunkGenerator` can be passed around and
    /// the abstract getters dispatched through the vtable.
    #[test]
    fn trait_is_object_safe() {
        let generator = MockGenerator {
            min_y: -64,
            gen_depth: 384,
            sea_level: 63,
            base_height: 100,
        };
        let dyn_gen: &dyn ChunkGenerator = &generator;
        assert_eq!(dyn_gen.get_min_y(), -64);
        assert_eq!(dyn_gen.get_gen_depth(), 384);
        // The default body routes through the vtable too: `get_first_free_height`
        // is a default delegating to the mock's `get_base_height` override.
        let accessor = accessor();
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        assert_eq!(
            dyn_gen.get_first_free_height(3, 7, Types::WorldSurfaceWg, &accessor, &random_state),
            100
        );
    }

    /// `getSpawnHeight` — Paper's default returns a constant `64` regardless of
    /// the height accessor.
    #[test]
    fn get_spawn_height_defaults_to_64() {
        let generator = MockGenerator {
            min_y: -64,
            gen_depth: 384,
            sea_level: 63,
            base_height: 100,
        };
        let accessor = accessor();
        assert_eq!(ChunkGenerator::get_spawn_height(&generator, &accessor), 64);
    }

    /// `getFirstFreeHeight` — `this.getBaseHeight(x, z, type, heightAccessor,
    /// randomState)`, so a generator answering `getBaseHeight` yields the same
    /// value.
    #[test]
    fn get_first_free_height_delegates_to_base_height() {
        let generator = MockGenerator {
            min_y: -64,
            gen_depth: 384,
            sea_level: 63,
            base_height: 100,
        };
        let accessor = accessor();
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        assert_eq!(
            ChunkGenerator::get_first_free_height(
                &generator,
                3,
                7,
                Types::WorldSurfaceWg,
                &accessor,
                &random_state
            ),
            100
        );
    }

    /// `getFirstOccupiedHeight` — `getBaseHeight(...) - 1` (Java plain `int`
    /// math), so the default wraps at `i32::MIN`.
    #[test]
    fn get_first_occupied_height_is_base_height_minus_one() {
        let generator = MockGenerator {
            min_y: -64,
            gen_depth: 384,
            sea_level: 63,
            base_height: 100,
        };
        let accessor = accessor();
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        assert_eq!(
            ChunkGenerator::get_first_occupied_height(
                &generator,
                3,
                7,
                Types::WorldSurfaceWg,
                &accessor,
                &random_state
            ),
            99
        );

        // Java's `- 1` on an `int` wraps: base height `i32::MIN` → `i32::MAX`.
        let wrapping = MockGenerator {
            min_y: -64,
            gen_depth: 384,
            sea_level: 63,
            base_height: i32::MIN,
        };
        assert_eq!(
            ChunkGenerator::get_first_occupied_height(
                &wrapping,
                3,
                7,
                Types::WorldSurfaceWg,
                &accessor,
                &random_state
            ),
            i32::MAX
        );
    }

    /// The deferred world-surface seams panic with Paper-grounded messages
    /// (the Java method name + the owning issue) rather than fabricating a
    /// result.
    #[test]
    fn deferred_surface_seams_panic_with_paper_grounded_messages() {
        // `SeamOnlyGenerator` provides only the abstract surface, so every
        // default seam is exercised at its exact default.
        let generator = SeamOnlyGenerator;
        let accessor = accessor();
        let (noise_registry, df_registry) = populated_registries();
        let random_state = random_state(&noise_registry, &df_registry);
        let mut result = Vec::new();
        let feet = BlockPos::new(0, 0, 0);
        let biome = Holder::reference(rivet_registry::holder::RegistryId(0), 0);
        let feature = crate::levelgen::placement::PlacedFeature::new(
            rivet_registry::holder::Holder::direct(
                crate::levelgen::feature::ConfiguredFeatureErased {
                    feature: crate::levelgen::feature::FeatureId::new(0),
                    config: std::sync::Arc::new(
                        crate::levelgen::feature::configurations::NoneFeatureConfiguration,
                    ),
                },
            ),
            Vec::new(),
        );

        // Each Java-abstract / deferred seam (the five lifecycle steps, the
        // world-surface reads, and the structure/codec seams) must panic with a
        // Paper-grounded message naming the Java method and the owning issue,
        // rather than fabricate a result.
        for method in [
            "createBiomes",
            "applyCarvers",
            "buildSurface",
            "spawnOriginalMobs",
            "fillFromNoise",
            "getSeaLevel",
            "getBaseHeight",
            "getBaseColumn",
            "addDebugScreenInfo",
            "getBiomeSource",
            "createStructures",
            "createReferences",
        ] {
            let mut call = |g: &SeamOnlyGenerator| {
                let panic_result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match method {
                        "createBiomes" => {
                            ChunkGenerator::create_biomes(g);
                        }
                        "applyCarvers" => {
                            ChunkGenerator::apply_carvers(g);
                        }
                        "buildSurface" => {
                            ChunkGenerator::build_surface(g);
                        }
                        "spawnOriginalMobs" => {
                            ChunkGenerator::spawn_original_mobs(g);
                        }
                        "fillFromNoise" => {
                            ChunkGenerator::fill_from_noise(g);
                        }
                        "getSeaLevel" => {
                            ChunkGenerator::get_sea_level(g);
                        }
                        "getBaseHeight" => {
                            ChunkGenerator::get_base_height(
                                g,
                                0,
                                0,
                                Types::WorldSurfaceWg,
                                &accessor,
                                &random_state,
                            );
                        }
                        "getBaseColumn" => {
                            ChunkGenerator::get_base_column(g, 0, 0, &accessor, &random_state);
                        }
                        "addDebugScreenInfo" => ChunkGenerator::add_debug_screen_info(
                            g,
                            &mut result,
                            &random_state,
                            &feet,
                        ),
                        "getBiomeSource" => {
                            ChunkGenerator::get_biome_source(g);
                        }
                        "createStructures" => {
                            ChunkGenerator::create_structures(g);
                        }
                        "createReferences" => {
                            ChunkGenerator::create_references(g);
                        }
                        _ => unreachable!(),
                    }));
                panic_message(panic_result)
            };
            let message = call(&generator);
            assert!(
                message.contains(method) && message.contains("RivetTodo #185"),
                "seam {method} must fail with a Paper-grounded #185 message, got: {message}"
            );
        }

        // The biome-membership read is the `#178` biome-core seam (a different
        // owner), so it names #178.
        let membership = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ChunkGenerator::get_biome_generation_settings_has_feature(&generator, &biome, &feature)
        }));
        let message = panic_message(membership);
        assert!(
            message.contains("getBiomeGenerationSettings") && message.contains("RivetTodo #178"),
            "biome-membership seam must name #178, got: {message}"
        );
    }

    /// The panic payload, as `&str` if it was a format-string `panic!`.
    fn panic_message<T>(result: std::thread::Result<T>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => payload
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                .unwrap_or("<non-str panic payload>")
                .to_string(),
        }
    }
}
