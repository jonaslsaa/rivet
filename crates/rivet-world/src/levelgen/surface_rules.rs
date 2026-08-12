//! STUB(mc.world.level.levelgen.surface) — `net.minecraft.world.level.levelgen.SurfaceRules`.
//!
//! The translate-wave absorbs `SurfaceRules` (and `SurfaceSystem`) as stubs:
//! the full `SurfaceRules.RuleSource` tree — the `CODEC` dispatch (the ~15
//! `Rule` variants, `SequenceRuleSource`, `ConditionRuleSource`, the material
//! conditions, `NoiseThresholdConditionSource`, `VerticalGradientConditionSource`,
//! `BlockStateRuleSource`/`BlockRuleSource`), the `Context` runtime, and
//! `SurfaceRuleData`'s static builders (`end`/`nether`/`overworld`/
//! `overworldLike`/`air`) — belongs to the owning `mc.world.level.levelgen.surface`
//! manifest unit. `NoiseGeneratorSettings` carries a `SurfaceRules.RuleSource`
//! field (and its `CODEC` a `surface_rule` field), so the noisegen unit ports
//! the type *identity* and defers the value surface with a
//! `RivetTodo(#177)`-style marker to the owning unit.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/levelgen/SurfaceRules.java`.

use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `SurfaceRules.Context` — the runtime context a `RuleSource.apply` receives.
/// Only the block coordinates are needed by the noisegen unit's seam; the
/// `blockStateRule`/biome/random/heightmap fields defer with the owning
/// surface unit.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// `Context.blockX()`.
    pub block_x: i32,
    /// `Context.blockY()`.
    pub block_y: i32,
    /// `Context.blockZ()`.
    pub block_z: i32,
    /// `Context.isSurfaceBlock()` — the surface-y predicate the
    /// `VerticalGradient`/`NoiseThreshold` conditions consult; the full
    /// definition defers with the owning unit.
    pub is_surface_block: bool,
}

/// `SurfaceRules.RuleSource` — the interface every surface rule implements.
/// The owning unit replaces this with the real `Function<Context, SurfaceRule>`
/// hierarchy; the noisegen unit only needs the type to exist so
/// `NoiseGeneratorSettings` can carry it.
pub trait RuleSource: Any + Debug + Send + Sync + 'static {
    /// `RuleSource.apply(Context)` — deferred (RivetTodo to the surface unit).
    fn apply(&self, context: &Context) -> Box<dyn SurfaceRule + '_>;

    /// `type_id`-style identity for the erased carrier.
    fn as_any(&self) -> &dyn Any;
}

/// `SurfaceRules.SurfaceRule` — the applied rule; the full `SurfaceRule`/
/// `SequenceRule`/`BlockRule` matrix defers with the owning unit.
pub trait SurfaceRule: Debug + 'static {
    /// `apply(BlockPos)` — the `BlockState` or `null` (no change) decision.
    fn apply(&self, context: &Context) -> Option<crate::block::BlockState>;
}

/// The erased `Arc<dyn RuleSource>` carrier `NoiseGeneratorSettings` stores.
pub type ArcRuleSource = Arc<dyn RuleSource>;

/// `SurfaceRules.RuleSource.CODEC` — deferred. The owning surface unit ports
/// the full key-dispatch codec. The noisegen unit's placeholder is
/// `Codec.EMPTY.xmap(...)` — encodes any rule to `{}` and always decodes to
/// the `Air` stand-in — so `NoiseGeneratorSettings.DIRECT_CODEC` can carry the
/// field until then.
pub fn rule_source_codec<Ops: rivet_serialization::dynamic_ops::DynamicOps + 'static>()
-> Arc<dyn rivet_serialization::codec::Codec<ArcRuleSource, Ops>> {
    use rivet_serialization::codec;
    use rivet_serialization::unit::Unit;
    // `Codec.EMPTY` (`MapCodec.unit(Unit.INSTANCE).codec()`) xmapped onto the
    // erased rule source: decode always yields `Air`, encode always `{}`.
    codec::xmap(
        codec::empty::<Ops>(),
        Arc::new(|_unit: &Unit| Arc::new(Air) as ArcRuleSource),
        Arc::new(|_rule: &ArcRuleSource| Unit),
    )
}

/// A placeholder `RuleSource` used by the noisegen unit's `NoiseGeneratorSettings::dummy()`
/// — the `SurfaceRuleData.air()` builder's stand-in. The owning unit replaces
/// `dummy` with the real `SurfaceRuleData` builders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Air;

impl RuleSource for Air {
    fn apply(&self, _context: &Context) -> Box<dyn SurfaceRule + '_> {
        Box::new(AirRule)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The `SurfaceRuleData.air()` applied rule — every position yields
/// `Blocks.AIR`.
#[derive(Debug, Clone, Copy)]
pub struct AirRule;

impl SurfaceRule for AirRule {
    fn apply(&self, _context: &Context) -> Option<crate::block::BlockState> {
        Some(crate::block::blocks::Blocks::AIR.default_block_state())
    }
}

// ---------------------------------------------------------------------------
// SurfaceRuleData (STUB) — `net.minecraft.data.worldgen.SurfaceRuleData`
// ---------------------------------------------------------------------------
//
// STUB(mc.world.level.levelgen.surface) — the static `RuleSource` builders the
// noisegen unit's `NoiseGeneratorSettings.bootstrap` calls
// (`end`/`nether`/`overworld`/`overworldLike`/`air`). The owning surface unit
// ports the real builders (they build the full `Rule` trees over the biome
// lookup); until then every builder yields the `Air` stand-in so the
// `NoiseGeneratorSettings` presets compose. The biome-getter parameter Java
// passes (`context.lookup(Registries.BIOME)`) is dropped: this tree's
// `Registries.BIOME` is typed over the unported `BiomeId` handle, and the
// placeholder rule never reads it.

/// STUB: `SurfaceRuleData.air()` — the `Air` rule source.
pub fn surface_rule_air() -> ArcRuleSource {
    Arc::new(Air)
}

/// STUB: `SurfaceRuleData.end()` — the `Air` stand-in.
pub fn surface_rule_end() -> ArcRuleSource {
    Arc::new(Air)
}

/// STUB: `SurfaceRuleData.nether(HolderGetter<Biome>)` — the `Air` stand-in.
pub fn surface_rule_nether() -> ArcRuleSource {
    Arc::new(Air)
}

/// STUB: `SurfaceRuleData.overworld(HolderGetter<Biome>)` — the `Air` stand-in.
pub fn surface_rule_overworld() -> ArcRuleSource {
    Arc::new(Air)
}

/// STUB: `SurfaceRuleData.overworldLike(HolderGetter<Biome>, boolean
/// hasCeiling, boolean hasFloor, boolean isFrozen)` — the `Air` stand-in.
pub fn surface_rule_overworld_like(
    _has_ceiling: bool,
    _has_floor: bool,
    _is_frozen: bool,
) -> ArcRuleSource {
    Arc::new(Air)
}

// ---------------------------------------------------------------------------
// SurfaceSystem (STUB) — `net.minecraft.world.level.levelgen.SurfaceSystem`
// ---------------------------------------------------------------------------
//
// STUB(mc.world.level.levelgen.surface) — the `SurfaceSystem` value type
// `RandomState` carries (`new SurfaceSystem(this, defaultBlock, seaLevel,
// random)`). The owning surface unit ports the constructor + `buildSurface`;
// the noisegen unit only needs the type to exist so `RandomState.surfaceSystem`
// compiles. `RandomState` holds it behind an `Option` (the constructor is
// deferred) — see `random_state.rs`.

/// STUB: `net.minecraft.world.level.levelgen.SurfaceSystem` — the type
/// identity only (the owning `mc.world.level.levelgen.surface` unit ports the
/// value/behavior).
#[derive(Debug, Clone, Copy, Default)]
pub struct SurfaceSystem;
