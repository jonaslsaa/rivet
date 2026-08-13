//! Port of `net.minecraft.world.level.levelgen.NoiseGeneratorSettings`
//! (11-field record, 26.2).
//!
//! The record's 11 fields (`noiseSettings`, `defaultBlock`, `defaultFluid`,
//! `noiseRouter`, `surfaceRule`, `spawnTarget`, `seaLevel`,
//! `disableMobGeneration`, `aquifersEnabled`, `oreVeinsEnabled`,
//! `useLegacyRandomSource`), the `DIRECT_CODEC` (the 11-field map codec), the
//! `CODEC` (`RegistryFileCodec` over `Registries.NOISE_SETTINGS`), the seven
//! preset `ResourceKey`s, `isAquifersEnabled`/`oreVeinsEnabled`/`getRandomSource`,
//! `bootstrap`, and `dummy`.
//!
//! Translation notes:
//! - The `DIRECT_CODEC`'s `RecordCodecBuilder` has 11 fields — the
//!   `record_builder` compositor caps at 6, so the codec is built from
//!   explicit `MapEncoder`/`MapDecoder` structs (the `NoiseRouter` 15-field and
//!   `Climate.ParameterPoint` 7-field precedents). `DataResult.apply2` chains
//!   mirror Java's `RecordCodecBuilder` applicative fold.
//! - `spawnTarget` inlines `new OverworldBiomeBuilder().spawnTarget()` (the
//!   two `ParameterPoint`s the biome unit's builder returns — the only leaf
//!   this SCC reads; see the module doc).
//! - `SurfaceRuleData.end` is ported (the end-stone `block` rule);
//!   `SurfaceRuleData.nether/overworld/overworldLike` are AIR STUBs in
//!   `levelgen::surface_rules` (the real builders belong to the
//!   `mc.data.worldgen` unit, still pending; RivetTodo #179) because a faithful
//!   port needs the biome `HolderGetter` threaded through the settings
//!   bootstrap. Each STUB preset carries the same AIR block-state `block` rule
//!   — the real `SurfaceRules.state(Blocks.AIR)` (Java's `makeStateRule`) — so
//!   the `surface_rule` field composes and round-trips through the
//!   `MATERIAL_RULE` codec.
//! - `Blocks.END_STONE/NETHERRACK/LAVA/STONE/WATER/AIR` all have
//!   `default_block_state()` handles.
//! - `WorldgenRandom.Algorithm` — `rivet_util::worldgen_random::Algorithm`.

use crate::biome::{Parameter, ParameterPoint};
use crate::block::BlockState;
use crate::block::blocks::Blocks;
use crate::levelgen::noise::noise_router::NoiseRouter;
use crate::levelgen::noise::noise_settings::NoiseSettings;
use crate::levelgen::noisegen::noise_router_data::DensityFunctionValue;
use crate::levelgen::surface_rules::{
    ArcRuleSource, rule_source_codec, surface_rule_air, surface_rule_end, surface_rule_nether,
    surface_rule_overworld, surface_rule_overworld_like,
};
use crate::levelgen::synth::normal_noise::NoiseParameters;
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::registry_file_codec::RegistryFileCodec;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_registry::{Identifier, ResourceKey};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::{self, MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use rivet_util::worldgen_random::Algorithm;
use std::fmt;
use std::sync::{Arc, LazyLock};

/// `net.minecraft.world.level.levelgen.NoiseGeneratorSettings` — the 11-field
/// worldgen settings record.
#[derive(Debug, Clone)]
pub struct NoiseGeneratorSettings {
    /// `noiseSettings`.
    pub noise_settings: NoiseSettings,
    /// `defaultBlock`.
    pub default_block: BlockState,
    /// `defaultFluid`.
    pub default_fluid: BlockState,
    /// `noiseRouter`.
    pub noise_router: NoiseRouter,
    /// `surfaceRule` — the erased `ArcRuleSource` (STUB carrier).
    pub surface_rule: ArcRuleSource,
    /// `spawnTarget`.
    pub spawn_target: Vec<ParameterPoint>,
    /// `seaLevel`.
    pub sea_level: i32,
    /// `disableMobGeneration` (`@Deprecated`).
    pub disable_mob_generation: bool,
    /// `aquifersEnabled`.
    pub aquifers_enabled: bool,
    /// `oreVeinsEnabled`.
    pub ore_veins_enabled: bool,
    /// `useLegacyRandomSource`.
    pub use_legacy_random_source: bool,
}

/// `ResourceKey<NoiseGeneratorSettings>` — the `LazyLock` keys follow the
/// crate's `Identifier`-owns-`String` convention (`registry_keys` precedent).
pub type NoiseGeneratorSettingsKey = ResourceKey<NoiseGeneratorSettings>;

/// The `NOISE_SETTINGS` registry key — `Registries.NOISE_SETTINGS`.
fn registry_key()
-> &'static rivet_registry::ResourceKey<rivet_registry::Registry<NoiseGeneratorSettings>> {
    &crate::levelgen::noise::registry_keys::NOISE_SETTINGS
}

fn create_key(path: &str) -> NoiseGeneratorSettingsKey {
    ResourceKey::create(registry_key(), Identifier::with_default_namespace(path))
}

/// The `overworld` preset key — `Registries.NOISE_SETTINGS` / `"overworld"`.
pub static OVERWORLD: LazyLock<NoiseGeneratorSettingsKey> =
    LazyLock::new(|| create_key("overworld"));
/// The `large_biomes` preset key.
pub static LARGE_BIOMES: LazyLock<NoiseGeneratorSettingsKey> =
    LazyLock::new(|| create_key("large_biomes"));
/// The `amplified` preset key.
pub static AMPLIFIED: LazyLock<NoiseGeneratorSettingsKey> =
    LazyLock::new(|| create_key("amplified"));
/// The `nether` preset key.
pub static NETHER: LazyLock<NoiseGeneratorSettingsKey> = LazyLock::new(|| create_key("nether"));
/// The `end` preset key.
pub static END: LazyLock<NoiseGeneratorSettingsKey> = LazyLock::new(|| create_key("end"));
/// The `caves` preset key.
pub static CAVES: LazyLock<NoiseGeneratorSettingsKey> = LazyLock::new(|| create_key("caves"));
/// The `floating_islands` preset key.
pub static FLOATING_ISLANDS: LazyLock<NoiseGeneratorSettingsKey> =
    LazyLock::new(|| create_key("floating_islands"));

impl NoiseGeneratorSettings {
    /// The 11-field record constructor (Java's canonical constructor order).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        noise_settings: NoiseSettings,
        default_block: BlockState,
        default_fluid: BlockState,
        noise_router: NoiseRouter,
        surface_rule: ArcRuleSource,
        spawn_target: Vec<ParameterPoint>,
        sea_level: i32,
        disable_mob_generation: bool,
        aquifers_enabled: bool,
        ore_veins_enabled: bool,
        use_legacy_random_source: bool,
    ) -> Self {
        NoiseGeneratorSettings {
            noise_settings,
            default_block,
            default_fluid,
            noise_router,
            surface_rule,
            spawn_target,
            sea_level,
            disable_mob_generation,
            aquifers_enabled,
            ore_veins_enabled,
            use_legacy_random_source,
        }
    }

    /// `isAquifersEnabled()` — `aquifersEnabled && !DEBUG_DISABLE_AQUIFERS`.
    pub fn is_aquifers_enabled(&self) -> bool {
        self.aquifers_enabled && !rivet_core::shared_constants::DEBUG_DISABLE_AQUIFERS
    }

    /// `oreVeinsEnabled()` — `oreVeinsEnabled && !DEBUG_DISABLE_ORE_VEINS`.
    pub fn ore_veins_enabled(&self) -> bool {
        self.ore_veins_enabled && !rivet_core::shared_constants::DEBUG_DISABLE_ORE_VEINS
    }

    /// `getRandomSource()` — `LEGACY` when `useLegacyRandomSource`, else
    /// `XOROSHIRO`.
    pub fn get_random_source(&self) -> Algorithm {
        if self.use_legacy_random_source {
            Algorithm::Legacy
        } else {
            Algorithm::Xoroshiro
        }
    }

    /// `NoiseGeneratorSettings.CODEC` — the `RegistryFileCodec` over
    /// `Registries.NOISE_SETTINGS`.
    pub fn codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn Codec<Holder<NoiseGeneratorSettings>, Ops>> {
        let registry_key = &crate::levelgen::noise::registry_keys::NOISE_SETTINGS;
        Arc::new(RegistryFileCodec::create(
            registry_key,
            direct_codec::<Ops>(),
        ))
    }

    /// `NoiseGeneratorSettings.DIRECT_CODEC` — the 11-field record codec.
    pub fn direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn Codec<NoiseGeneratorSettings, Ops>> {
        direct_codec::<Ops>()
    }
}

/// `NoiseGeneratorSettings.bootstrap(BootstrapContext<NoiseGeneratorSettings>)`.
///
/// `context.register` mutates the build state, so the Rust port takes a `&mut`
/// context. The presets call the private `overworld`/`nether`/`end`/`caves`/
/// `floatingIslands` builders.
pub fn bootstrap(
    context: &mut impl crate::data::worldgen::bootstrap_context::BootstrapContext<
        NoiseGeneratorSettings,
    >,
) {
    // The `lookup` getters are borrowed views into the build state, so each
    // value is computed inside a block that releases the borrow before the
    // `&mut` register call (Java re-resolves the getters per `register`).
    context.register_default(&OVERWORLD, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in settings bootstrap");
        overworld(functions, noises, false, false)
    });
    context.register_default(&LARGE_BIOMES, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in settings bootstrap");
        overworld(functions, noises, false, true)
    });
    context.register_default(&AMPLIFIED, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in settings bootstrap");
        overworld(functions, noises, true, false)
    });
    context.register_default(&NETHER, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in settings bootstrap");
        nether(functions, noises)
    });
    context.register_default(&END, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        end(functions)
    });
    context.register_default(&CAVES, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        caves(functions)
    });
    context.register_default(&FLOATING_ISLANDS, {
        let functions = context
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("density function registry present in settings bootstrap");
        let noises = context
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry present in settings bootstrap");
        floating_islands(functions, noises)
    });
}

/// `end(BootstrapContext<?>)` — `NoiseSettings.END_NOISE_SETTINGS`, end stone,
/// air fluid, `NoiseRouterData.end`, `SurfaceRuleData.end`, no spawn target,
/// sea level 0, mob generation disabled, no aquifers, no ore veins, legacy
/// random.
fn end(functions: &dyn HolderGetter<DensityFunctionValue>) -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        crate::levelgen::noise::noise_settings::END_NOISE_SETTINGS,
        Blocks::END_STONE.default_block_state(),
        Blocks::AIR.default_block_state(),
        crate::levelgen::noisegen::noise_router_data::end(functions),
        surface_rule_end(),
        Vec::new(),
        0,
        true,
        false,
        false,
        true,
    )
}

/// `nether(BootstrapContext<?>)` — `NoiseSettings.NETHER_NOISE_SETTINGS`,
/// netherrack, lava fluid, `NoiseRouterData.nether`, `SurfaceRuleData.nether`,
/// no spawn target, sea level 32, no mob generation, no aquifers, no ore
/// veins, legacy random.
fn nether(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
) -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        crate::levelgen::noise::noise_settings::NETHER_NOISE_SETTINGS,
        Blocks::NETHERRACK.default_block_state(),
        Blocks::LAVA.default_block_state(),
        crate::levelgen::noisegen::noise_router_data::nether(functions, noises),
        surface_rule_nether(),
        Vec::new(),
        32,
        false,
        false,
        false,
        true,
    )
}

/// `overworld(BootstrapContext<?>, boolean isAmplified, boolean largeBiomes)` —
/// `NoiseSettings.OVERWORLD_NOISE_SETTINGS`, stone, water fluid,
/// `NoiseRouterData.overworld`, `SurfaceRuleData.overworld`, the biome
/// builder's `spawnTarget`, sea level 63, mob generation enabled, aquifers +
/// ore veins enabled, xoroshiro random.
fn overworld(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
    is_amplified: bool,
    large_biomes: bool,
) -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS,
        Blocks::STONE.default_block_state(),
        Blocks::WATER.default_block_state(),
        crate::levelgen::noisegen::noise_router_data::overworld(
            functions,
            noises,
            large_biomes,
            is_amplified,
        ),
        surface_rule_overworld(),
        spawn_target(),
        63,
        false,
        true,
        true,
        false,
    )
}

/// `caves(BootstrapContext<?>)` — `NoiseSettings.CAVES_NOISE_SETTINGS`, stone,
/// water fluid, `NoiseRouterData.caves`, `SurfaceRuleData.overworldLike(false,
/// true, true)`, no spawn target, sea level 32, no mob generation, no
/// aquifers, no ore veins, legacy random.
fn caves(functions: &dyn HolderGetter<DensityFunctionValue>) -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        crate::levelgen::noise::noise_settings::CAVES_NOISE_SETTINGS,
        Blocks::STONE.default_block_state(),
        Blocks::WATER.default_block_state(),
        crate::levelgen::noisegen::noise_router_data::caves(functions),
        surface_rule_overworld_like(false, true, true),
        Vec::new(),
        32,
        false,
        false,
        false,
        true,
    )
}

/// `floatingIslands(BootstrapContext<?>)` —
/// `NoiseSettings.FLOATING_ISLANDS_NOISE_SETTINGS`, stone, water fluid,
/// `NoiseRouterData.floatingIslands`, `SurfaceRuleData.overworldLike(false,
/// false, false)`, no spawn target, sea level -64, no mob generation, no
/// aquifers, no ore veins, legacy random.
fn floating_islands(
    functions: &dyn HolderGetter<DensityFunctionValue>,
    noises: &dyn HolderGetter<NoiseParameters>,
) -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        crate::levelgen::noise::noise_settings::FLOATING_ISLANDS_NOISE_SETTINGS,
        Blocks::STONE.default_block_state(),
        Blocks::WATER.default_block_state(),
        crate::levelgen::noisegen::noise_router_data::floating_islands(functions, noises),
        surface_rule_overworld_like(false, false, false),
        Vec::new(),
        -64,
        false,
        false,
        false,
        true,
    )
}

/// `dummy()` — `NoiseSettings.OVERWORLD_NOISE_SETTINGS`, stone, air fluid,
/// `NoiseRouterData.none`, `SurfaceRuleData.air`, no spawn target, sea level
/// 63, mob generation disabled, no aquifers, no ore veins, xoroshiro.
pub fn dummy() -> NoiseGeneratorSettings {
    NoiseGeneratorSettings::new(
        crate::levelgen::noise::noise_settings::OVERWORLD_NOISE_SETTINGS,
        Blocks::STONE.default_block_state(),
        Blocks::AIR.default_block_state(),
        crate::levelgen::noisegen::noise_router_data::none(),
        surface_rule_air(),
        Vec::new(),
        63,
        true,
        false,
        false,
        false,
    )
}

/// `new OverworldBiomeBuilder().spawnTarget()` — the two `ParameterPoint`s the
/// biome unit's builder returns (inlined here; see the module doc).
fn spawn_target() -> Vec<ParameterPoint> {
    let full_range = Parameter::span(-1.0, 1.0);
    let inland_continentalness = Parameter::span(-0.11, 0.55);
    let surface_depth = Parameter::point(0.0);
    Vec::from([
        ParameterPoint::new(
            full_range,
            full_range,
            Parameter::span_of(&inland_continentalness, &full_range),
            full_range,
            surface_depth,
            Parameter::span(-1.0, -0.16),
            0,
        ),
        ParameterPoint::new(
            full_range,
            full_range,
            Parameter::span_of(&inland_continentalness, &full_range),
            full_range,
            surface_depth,
            Parameter::span(0.16, 1.0),
            0,
        ),
    ])
}

/// `NoiseGeneratorSettings.DIRECT_CODEC` — the 11-field record codec, as the
/// ops-generic factory (the `NoiseRouter` 15-field / `ParameterPoint` 7-field
/// precedent: explicit `MapEncoder`/`MapDecoder` structs).
fn direct_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<NoiseGeneratorSettings, Ops>> {
    let noise_settings = codec::field_of(
        crate::levelgen::noise::noise_settings::noise_settings_codec::<Ops>(),
        "noise".to_string(),
    );
    let block_state = || rivet_registry::block_state_codec::block_state_codec::<Ops>();
    let default_block = codec::field_of(block_state(), "default_block".to_string());
    let default_fluid = codec::field_of(block_state(), "default_fluid".to_string());
    let noise_router = codec::field_of(
        crate::levelgen::noise::noise_router::noise_router_codec::<Ops>(),
        "noise_router".to_string(),
    );
    let surface_rule = codec::field_of(rule_source_codec::<Ops>(), "surface_rule".to_string());
    let spawn_target = codec::field_of(
        codec::list(ParameterPoint::codec::<Ops>()),
        "spawn_target".to_string(),
    );
    let sea_level = codec::field_of(codec::int_codec::<Ops>(), "sea_level".to_string());
    let disable_mob_generation = codec::field_of(
        codec::bool_codec::<Ops>(),
        "disable_mob_generation".to_string(),
    );
    let aquifers_enabled =
        codec::field_of(codec::bool_codec::<Ops>(), "aquifers_enabled".to_string());
    let ore_veins_enabled =
        codec::field_of(codec::bool_codec::<Ops>(), "ore_veins_enabled".to_string());
    let legacy_random_source = codec::field_of(
        codec::bool_codec::<Ops>(),
        "legacy_random_source".to_string(),
    );

    let encoder = Arc::new(NoiseGeneratorSettingsEncoder {
        noise_settings: Arc::new(MapCodecEncoderHalf(noise_settings.clone())),
        default_block: Arc::new(MapCodecEncoderHalf(default_block.clone())),
        default_fluid: Arc::new(MapCodecEncoderHalf(default_fluid.clone())),
        noise_router: Arc::new(MapCodecEncoderHalf(noise_router.clone())),
        surface_rule: Arc::new(MapCodecEncoderHalf(surface_rule.clone())),
        spawn_target: Arc::new(MapCodecEncoderHalf(spawn_target.clone())),
        sea_level: Arc::new(MapCodecEncoderHalf(sea_level.clone())),
        disable_mob_generation: Arc::new(MapCodecEncoderHalf(disable_mob_generation.clone())),
        aquifers_enabled: Arc::new(MapCodecEncoderHalf(aquifers_enabled.clone())),
        ore_veins_enabled: Arc::new(MapCodecEncoderHalf(ore_veins_enabled.clone())),
        legacy_random_source: Arc::new(MapCodecEncoderHalf(legacy_random_source.clone())),
    });
    let decoder = Arc::new(NoiseGeneratorSettingsDecoder {
        noise_settings: Arc::new(MapCodecDecoderHalf(noise_settings)),
        default_block: Arc::new(MapCodecDecoderHalf(default_block)),
        default_fluid: Arc::new(MapCodecDecoderHalf(default_fluid)),
        noise_router: Arc::new(MapCodecDecoderHalf(noise_router)),
        surface_rule: Arc::new(MapCodecDecoderHalf(surface_rule)),
        spawn_target: Arc::new(MapCodecDecoderHalf(spawn_target)),
        sea_level: Arc::new(MapCodecDecoderHalf(sea_level)),
        disable_mob_generation: Arc::new(MapCodecDecoderHalf(disable_mob_generation)),
        aquifers_enabled: Arc::new(MapCodecDecoderHalf(aquifers_enabled)),
        ore_veins_enabled: Arc::new(MapCodecDecoderHalf(ore_veins_enabled)),
        legacy_random_source: Arc::new(MapCodecDecoderHalf(legacy_random_source)),
    });
    map_codec::codec_of(map_codec::of(
        encoder,
        decoder,
        "NoiseGeneratorSettings".to_string(),
    ))
}

/// The 11-field `MapEncoder` — encodes every field in Java's order.
struct NoiseGeneratorSettingsEncoder<Ops: DynamicOps + 'static> {
    noise_settings: Arc<dyn MapEncoder<NoiseSettings, Ops>>,
    default_block: Arc<dyn MapEncoder<BlockState, Ops>>,
    default_fluid: Arc<dyn MapEncoder<BlockState, Ops>>,
    noise_router: Arc<dyn MapEncoder<NoiseRouter, Ops>>,
    surface_rule: Arc<dyn MapEncoder<ArcRuleSource, Ops>>,
    spawn_target: Arc<dyn MapEncoder<Vec<ParameterPoint>, Ops>>,
    sea_level: Arc<dyn MapEncoder<i32, Ops>>,
    disable_mob_generation: Arc<dyn MapEncoder<bool, Ops>>,
    aquifers_enabled: Arc<dyn MapEncoder<bool, Ops>>,
    ore_veins_enabled: Arc<dyn MapEncoder<bool, Ops>>,
    legacy_random_source: Arc<dyn MapEncoder<bool, Ops>>,
}
impl<Ops: DynamicOps + 'static> fmt::Debug for NoiseGeneratorSettingsEncoder<Ops> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NoiseGeneratorSettingsEncoder")
    }
}
impl<Ops: DynamicOps + 'static> Keyable<Ops> for NoiseGeneratorSettingsEncoder<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.noise_settings.keys(ops);
        keys.extend(self.default_block.keys(ops));
        keys.extend(self.default_fluid.keys(ops));
        keys.extend(self.noise_router.keys(ops));
        keys.extend(self.surface_rule.keys(ops));
        keys.extend(self.spawn_target.keys(ops));
        keys.extend(self.sea_level.keys(ops));
        keys.extend(self.disable_mob_generation.keys(ops));
        keys.extend(self.aquifers_enabled.keys(ops));
        keys.extend(self.ore_veins_enabled.keys(ops));
        keys.extend(self.legacy_random_source.keys(ops));
        keys
    }
}
impl<Ops: DynamicOps + 'static> MapEncoder<NoiseGeneratorSettings, Ops>
    for NoiseGeneratorSettingsEncoder<Ops>
{
    fn encode(
        &self,
        input: &NoiseGeneratorSettings,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.noise_settings
            .encode(&input.noise_settings, ops, prefix);
        self.default_block.encode(&input.default_block, ops, prefix);
        self.default_fluid.encode(&input.default_fluid, ops, prefix);
        self.noise_router.encode(&input.noise_router, ops, prefix);
        self.surface_rule.encode(&input.surface_rule, ops, prefix);
        self.spawn_target.encode(&input.spawn_target, ops, prefix);
        self.sea_level.encode(&input.sea_level, ops, prefix);
        self.disable_mob_generation
            .encode(&input.disable_mob_generation, ops, prefix);
        // Java's `DIRECT_CODEC` `forGetter` is the DEBUG-gated accessor
        // (`isAquifersEnabled` / `oreVeinsEnabled`), so encode through it.
        self.aquifers_enabled
            .encode(&input.is_aquifers_enabled(), ops, prefix);
        self.ore_veins_enabled
            .encode(&input.ore_veins_enabled(), ops, prefix);
        self.legacy_random_source
            .encode(&input.use_legacy_random_source, ops, prefix);
    }
}

/// The 11-field `MapDecoder` — accumulates via nested `apply2` in Java's
/// field order (the `ParameterPointDecoder` precedent).
struct NoiseGeneratorSettingsDecoder<Ops: DynamicOps + 'static> {
    noise_settings: Arc<dyn MapDecoder<NoiseSettings, Ops>>,
    default_block: Arc<dyn MapDecoder<BlockState, Ops>>,
    default_fluid: Arc<dyn MapDecoder<BlockState, Ops>>,
    noise_router: Arc<dyn MapDecoder<NoiseRouter, Ops>>,
    surface_rule: Arc<dyn MapDecoder<ArcRuleSource, Ops>>,
    spawn_target: Arc<dyn MapDecoder<Vec<ParameterPoint>, Ops>>,
    sea_level: Arc<dyn MapDecoder<i32, Ops>>,
    disable_mob_generation: Arc<dyn MapDecoder<bool, Ops>>,
    aquifers_enabled: Arc<dyn MapDecoder<bool, Ops>>,
    ore_veins_enabled: Arc<dyn MapDecoder<bool, Ops>>,
    legacy_random_source: Arc<dyn MapDecoder<bool, Ops>>,
}
impl<Ops: DynamicOps + 'static> fmt::Debug for NoiseGeneratorSettingsDecoder<Ops> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NoiseGeneratorSettingsDecoder")
    }
}
impl<Ops: DynamicOps + 'static> Keyable<Ops> for NoiseGeneratorSettingsDecoder<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.noise_settings.keys(ops);
        keys.extend(self.default_block.keys(ops));
        keys.extend(self.default_fluid.keys(ops));
        keys.extend(self.noise_router.keys(ops));
        keys.extend(self.surface_rule.keys(ops));
        keys.extend(self.spawn_target.keys(ops));
        keys.extend(self.sea_level.keys(ops));
        keys.extend(self.disable_mob_generation.keys(ops));
        keys.extend(self.aquifers_enabled.keys(ops));
        keys.extend(self.ore_veins_enabled.keys(ops));
        keys.extend(self.legacy_random_source.keys(ops));
        keys
    }
}
impl<Ops: DynamicOps + 'static> MapDecoder<NoiseGeneratorSettings, Ops>
    for NoiseGeneratorSettingsDecoder<Ops>
{
    #[allow(clippy::type_complexity)]
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<NoiseGeneratorSettings> {
        let noise_settings = self.noise_settings.decode(ops, input);
        let default_block = self.default_block.decode(ops, input);
        let default_fluid = self.default_fluid.decode(ops, input);
        let noise_router = self.noise_router.decode(ops, input);
        let surface_rule = self.surface_rule.decode(ops, input);
        let spawn_target = self.spawn_target.decode(ops, input);
        let sea_level = self.sea_level.decode(ops, input);
        let disable_mob_generation = self.disable_mob_generation.decode(ops, input);
        let aquifers_enabled = self.aquifers_enabled.decode(ops, input);
        let ore_veins_enabled = self.ore_veins_enabled.decode(ops, input);
        let legacy_random_source = self.legacy_random_source.decode(ops, input);
        noise_settings
            .apply2(|a: &NoiseSettings, b: &BlockState| (*a, *b), default_block)
            .apply2(
                |(a, b): &(NoiseSettings, BlockState), c: &BlockState| (*a, *b, *c),
                default_fluid,
            )
            .apply2(
                |(a, b, c): &(NoiseSettings, BlockState, BlockState), d: &NoiseRouter| {
                    (*a, *b, *c, d.clone())
                },
                noise_router,
            )
            .apply2(
                |(a, b, c, d): &(NoiseSettings, BlockState, BlockState, NoiseRouter),
                 e: &ArcRuleSource| { (*a, *b, *c, d.clone(), e.clone()) },
                surface_rule,
            )
            .apply2(
                |(a, b, c, d, e): &(
                    NoiseSettings,
                    BlockState,
                    BlockState,
                    NoiseRouter,
                    ArcRuleSource,
                ),
                 f: &Vec<ParameterPoint>| {
                    (*a, *b, *c, d.clone(), e.clone(), f.clone())
                },
                spawn_target,
            )
            .apply2(
                |(a, b, c, d, e, f): &(
                    NoiseSettings,
                    BlockState,
                    BlockState,
                    NoiseRouter,
                    ArcRuleSource,
                    Vec<ParameterPoint>,
                ),
                 g: &i32| (*a, *b, *c, d.clone(), e.clone(), f.clone(), *g),
                sea_level,
            )
            .apply2(
                |(a, b, c, d, e, f, g): &(
                    NoiseSettings,
                    BlockState,
                    BlockState,
                    NoiseRouter,
                    ArcRuleSource,
                    Vec<ParameterPoint>,
                    i32,
                ),
                 h: &bool| (*a, *b, *c, d.clone(), e.clone(), f.clone(), *g, *h),
                disable_mob_generation,
            )
            .apply2(
                |(a, b, c, d, e, f, g, h): &(
                    NoiseSettings,
                    BlockState,
                    BlockState,
                    NoiseRouter,
                    ArcRuleSource,
                    Vec<ParameterPoint>,
                    i32,
                    bool,
                ),
                 i: &bool| {
                    (*a, *b, *c, d.clone(), e.clone(), f.clone(), *g, *h, *i)
                },
                aquifers_enabled,
            )
            .apply2(
                |(a, b, c, d, e, f, g, h, i): &(
                    NoiseSettings,
                    BlockState,
                    BlockState,
                    NoiseRouter,
                    ArcRuleSource,
                    Vec<ParameterPoint>,
                    i32,
                    bool,
                    bool,
                ),
                 j: &bool| {
                    (*a, *b, *c, d.clone(), e.clone(), f.clone(), *g, *h, *i, *j)
                },
                ore_veins_enabled,
            )
            .apply2(
                |(a, b, c, d, e, f, g, h, i, j): &(
                    NoiseSettings,
                    BlockState,
                    BlockState,
                    NoiseRouter,
                    ArcRuleSource,
                    Vec<ParameterPoint>,
                    i32,
                    bool,
                    bool,
                    bool,
                ),
                 k: &bool| {
                    NoiseGeneratorSettings::new(
                        *a,
                        *b,
                        *c,
                        d.clone(),
                        e.clone(),
                        f.clone(),
                        *g,
                        *h,
                        *i,
                        *j,
                        *k,
                    )
                },
                legacy_random_source,
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::worldgen::bootstrap_context::RecordingContext;
    use crate::data::worldgen::noise_data;
    use crate::levelgen::noisegen::noise_router_data::bootstrap as density_function_bootstrap;
    use rivet_registry::RegistrationInfo;
    use rivet_registry::RegistryAccess;
    use rivet_registry::RegistryBuilder;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::registry::Registry;
    use rivet_registry::root::AnyBox;

    /// A freshly-frozen noise registry (via `NoiseData.bootstrap`). Built
    /// per-call: the `RegistryAccess` value model shares registries by moving
    /// the unique `Box<dyn AnyRegistry>` (OWNERSHIP forbids `Arc<dyn
    /// AnyRegistry>`), so a test needing the noise registry in two accesses
    /// freezes two identical instances (same `RegistryId`, same elements).
    fn build_noise_registry() -> Registry<NoiseParameters> {
        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let mut noise_builder: RegistryBuilder<NoiseParameters> = RegistryBuilder::new(noise_key);
        let mut noise_ctx = RecordingContext::<NoiseParameters>::new(
            RegistryId(0),
            (*crate::levelgen::noise::registry_keys::NOISE).clone(),
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
        noise_builder.freeze()
    }

    /// A `RegistryAccess` with the noise + density-function registries
    /// populated (the `NoiseGeneratorSettings.bootstrap` lookups need them).
    fn make_access() -> RegistryAccess {
        let noise_key = &crate::levelgen::noise::registry_keys::NOISE;
        let df_key = &crate::levelgen::noise::registry_keys::DENSITY_FUNCTION;
        let df_access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(noise_key.identifier().clone()),
            Box::new(build_noise_registry()) as AnyBox,
        )]);
        let mut df_builder: RegistryBuilder<DensityFunctionValue> = RegistryBuilder::new(df_key);
        let mut df_ctx = RecordingContext::<DensityFunctionValue>::new(
            RegistryId(1),
            (*crate::levelgen::noise::registry_keys::DENSITY_FUNCTION).clone(),
            df_access,
        );
        density_function_bootstrap(&mut df_ctx);
        for reg in df_ctx.registrations() {
            df_builder.register(
                &reg.key,
                Arc::new(reg.value.clone()),
                RegistrationInfo::BUILT_IN,
            );
        }
        let df_registry = df_builder.freeze();

        RegistryAccess::from_pairs(vec![
            (
                ResourceKey::create_registry_key(noise_key.identifier().clone()),
                Box::new(build_noise_registry()) as AnyBox,
            ),
            (
                ResourceKey::create_registry_key(df_key.identifier().clone()),
                Box::new(df_registry) as AnyBox,
            ),
        ])
    }

    #[test]
    fn preset_keys_match_java_identifiers() {
        assert_eq!(OVERWORLD.identifier().to_string(), "minecraft:overworld");
        assert_eq!(
            LARGE_BIOMES.identifier().to_string(),
            "minecraft:large_biomes"
        );
        assert_eq!(AMPLIFIED.identifier().to_string(), "minecraft:amplified");
        assert_eq!(NETHER.identifier().to_string(), "minecraft:nether");
        assert_eq!(END.identifier().to_string(), "minecraft:end");
        assert_eq!(CAVES.identifier().to_string(), "minecraft:caves");
        assert_eq!(
            FLOATING_ISLANDS.identifier().to_string(),
            "minecraft:floating_islands"
        );
    }

    #[test]
    fn dummy_matches_java_field_values() {
        let settings = dummy();
        assert_eq!(settings.noise_settings.min_y(), -64);
        assert_eq!(settings.noise_settings.height(), 384);
        assert_eq!(settings.sea_level, 63);
        assert!(settings.disable_mob_generation);
        assert!(!settings.aquifers_enabled);
        assert!(!settings.ore_veins_enabled);
        assert!(!settings.use_legacy_random_source);
        assert_eq!(settings.get_random_source(), Algorithm::Xoroshiro);
        assert!(settings.spawn_target.is_empty());
    }

    #[test]
    fn spawn_target_matches_biome_builder() {
        let target = spawn_target();
        assert_eq!(target.len(), 2);
        // First point: inland(-0.11..0.55)..FULL_RANGE(-1..1) continentalness,
        // weirdness span(-1.0, -0.16); the Parameter stores quantized coords
        // (Java `quantizeCoord` — `(long)(coord * 10000.0F)`).
        assert_eq!(target[0].weirdness.min, -10000);
        assert_eq!(target[0].weirdness.max, -1600);
        assert_eq!(target[0].continentalness.min, -1100);
        assert_eq!(target[0].continentalness.max, 10000);
        // Second point: weirdness span(0.16, 1.0).
        assert_eq!(target[1].weirdness.min, 1600);
        assert_eq!(target[1].weirdness.max, 10000);
        assert_eq!(target[0].temperature.min, -10000);
        assert_eq!(target[0].temperature.max, 10000);
        assert_eq!(target[0].offset, 0);
    }

    #[test]
    fn overworld_preset_field_values() {
        let access = make_access();
        let functions = access
            .lookup(&crate::levelgen::noise::registry_keys::DENSITY_FUNCTION)
            .expect("df registry");
        let noises = access
            .lookup(&crate::levelgen::noise::registry_keys::NOISE)
            .expect("noise registry");
        let settings = overworld(functions, noises, false, false);
        assert_eq!(settings.sea_level, 63);
        assert!(settings.aquifers_enabled);
        assert!(settings.ore_veins_enabled);
        assert!(!settings.disable_mob_generation);
        assert_eq!(settings.get_random_source(), Algorithm::Xoroshiro);
        assert_eq!(settings.spawn_target.len(), 2);
    }

    #[test]
    fn bootstrap_registers_all_seven_presets() {
        let access = make_access();
        let mut context = RecordingContext::<NoiseGeneratorSettings>::new(
            RegistryId(7),
            (*crate::levelgen::noise::registry_keys::NOISE_SETTINGS).clone(),
            access,
        );
        bootstrap(&mut context);
        let regs: Vec<_> = context.registrations().iter().cloned().collect();
        let ids: Vec<String> = regs
            .iter()
            .map(|r| r.key.identifier().to_string())
            .collect();
        assert_eq!(ids.len(), 7);
        assert_eq!(ids[0], "minecraft:overworld");
        assert_eq!(ids[1], "minecraft:large_biomes");
        assert_eq!(ids[2], "minecraft:amplified");
        assert_eq!(ids[3], "minecraft:nether");
        assert_eq!(ids[4], "minecraft:end");
        assert_eq!(ids[5], "minecraft:caves");
        assert_eq!(ids[6], "minecraft:floating_islands");
        assert_eq!(ids[3], NETHER.identifier().to_string());
    }

    /// The `DIRECT_CODEC` must round-trip the full settings record — in
    /// particular the `surface_rule` field, which carries
    /// `surface_rule_air()` = `SurfaceRules.state(Blocks.AIR)` (the real
    /// `block` rule, Java's `makeStateRule`). Before the audit this field was a
    /// fabricated `Air` with an unregistered `"air"` type id, which made the
    /// settings record unencodable. This pins encodability end-to-end.
    #[test]
    fn direct_codec_round_trips_settings_including_surface_rule() {
        use crate::levelgen::surface_rules::BlockRuleSource;
        use rivet_registry::registry_ops::RegistryOps;
        use rivet_serialization::json_ops::JsonOps;

        type TestOps = RegistryOps<serde_json::Value, JsonOps>;
        let access = make_access();
        let ops = TestOps::create_from_access(&JsonOps::INSTANCE, access);
        let settings = dummy();
        let codec = NoiseGeneratorSettings::direct_codec::<TestOps>();

        let encoded = codec
            .encode_start(&ops, &settings)
            .get_or_throw("encode settings")
            .clone();
        // The `surface_rule` field round-trips as the Java `block` rule.
        assert_eq!(
            encoded.get("surface_rule"),
            Some(&serde_json::json!({
                "type": "minecraft:block",
                "result_state": {"Name": "minecraft:air"}
            }))
        );
        let (decoded, _rest) = codec
            .decode(&ops, &encoded)
            .result()
            .expect("decode settings")
            .clone();
        assert!(decoded.surface_rule.as_any().is::<BlockRuleSource>());
    }
}
