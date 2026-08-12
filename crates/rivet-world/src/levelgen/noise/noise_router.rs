//! Port of `net.minecraft.world.level.levelgen.NoiseRouter` (record, 26.2).
//!
//! The 15-field density-function record and its `CODEC` (each field is
//! `DensityFunction.CODEC.fieldOf(name)`), plus `mapAll` — the visitor that
//! maps every field through `DensityFunction.mapAll`.
//!
//! The `record_builder` compositor caps at Group5, so the 15-field `CODEC` is
//! built from explicit `MapEncoder`/`MapDecoder` structs that run each field
//! half in Java's `RecordCodecBuilder` order (encode: all fields; decode:
//! every field decoded with error accumulation via `DataResult.apply2` chains,
//! matching `DataResult.instance().apN`).

use crate::levelgen::noise::density_function::{DensityFunction, Visitor, map_all};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::{self, MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::fmt::Debug;
use std::sync::Arc;

/// `net.minecraft.world.level.levelgen.NoiseRouter` — the 15 density functions
/// driving noise-based worldgen.
#[derive(Debug, Clone)]
pub struct NoiseRouter {
    barrier_noise: Arc<dyn DensityFunction>,
    fluid_level_floodedness_noise: Arc<dyn DensityFunction>,
    fluid_level_spread_noise: Arc<dyn DensityFunction>,
    lava_noise: Arc<dyn DensityFunction>,
    temperature: Arc<dyn DensityFunction>,
    vegetation: Arc<dyn DensityFunction>,
    continents: Arc<dyn DensityFunction>,
    erosion: Arc<dyn DensityFunction>,
    depth: Arc<dyn DensityFunction>,
    ridges: Arc<dyn DensityFunction>,
    preliminary_surface_level: Arc<dyn DensityFunction>,
    final_density: Arc<dyn DensityFunction>,
    vein_toggle: Arc<dyn DensityFunction>,
    vein_ridged: Arc<dyn DensityFunction>,
    vein_gap: Arc<dyn DensityFunction>,
}

impl NoiseRouter {
    /// `NoiseRouter(...)` — the record constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        barrier_noise: Arc<dyn DensityFunction>,
        fluid_level_floodedness_noise: Arc<dyn DensityFunction>,
        fluid_level_spread_noise: Arc<dyn DensityFunction>,
        lava_noise: Arc<dyn DensityFunction>,
        temperature: Arc<dyn DensityFunction>,
        vegetation: Arc<dyn DensityFunction>,
        continents: Arc<dyn DensityFunction>,
        erosion: Arc<dyn DensityFunction>,
        depth: Arc<dyn DensityFunction>,
        ridges: Arc<dyn DensityFunction>,
        preliminary_surface_level: Arc<dyn DensityFunction>,
        final_density: Arc<dyn DensityFunction>,
        vein_toggle: Arc<dyn DensityFunction>,
        vein_ridged: Arc<dyn DensityFunction>,
        vein_gap: Arc<dyn DensityFunction>,
    ) -> Self {
        NoiseRouter {
            barrier_noise,
            fluid_level_floodedness_noise,
            fluid_level_spread_noise,
            lava_noise,
            temperature,
            vegetation,
            continents,
            erosion,
            depth,
            ridges,
            preliminary_surface_level,
            final_density,
            vein_toggle,
            vein_ridged,
            vein_gap,
        }
    }

    /// `barrierNoise()`.
    pub fn barrier_noise(&self) -> &Arc<dyn DensityFunction> {
        &self.barrier_noise
    }
    /// `fluidLevelFloodednessNoise()`.
    pub fn fluid_level_floodedness_noise(&self) -> &Arc<dyn DensityFunction> {
        &self.fluid_level_floodedness_noise
    }
    /// `fluidLevelSpreadNoise()`.
    pub fn fluid_level_spread_noise(&self) -> &Arc<dyn DensityFunction> {
        &self.fluid_level_spread_noise
    }
    /// `lavaNoise()`.
    pub fn lava_noise(&self) -> &Arc<dyn DensityFunction> {
        &self.lava_noise
    }
    /// `temperature()`.
    pub fn temperature(&self) -> &Arc<dyn DensityFunction> {
        &self.temperature
    }
    /// `vegetation()`.
    pub fn vegetation(&self) -> &Arc<dyn DensityFunction> {
        &self.vegetation
    }
    /// `continents()`.
    pub fn continents(&self) -> &Arc<dyn DensityFunction> {
        &self.continents
    }
    /// `erosion()`.
    pub fn erosion(&self) -> &Arc<dyn DensityFunction> {
        &self.erosion
    }
    /// `depth()`.
    pub fn depth(&self) -> &Arc<dyn DensityFunction> {
        &self.depth
    }
    /// `ridges()`.
    pub fn ridges(&self) -> &Arc<dyn DensityFunction> {
        &self.ridges
    }
    /// `preliminarySurfaceLevel()`.
    pub fn preliminary_surface_level(&self) -> &Arc<dyn DensityFunction> {
        &self.preliminary_surface_level
    }
    /// `finalDensity()`.
    pub fn final_density(&self) -> &Arc<dyn DensityFunction> {
        &self.final_density
    }
    /// `veinToggle()`.
    pub fn vein_toggle(&self) -> &Arc<dyn DensityFunction> {
        &self.vein_toggle
    }
    /// `veinRidged()`.
    pub fn vein_ridged(&self) -> &Arc<dyn DensityFunction> {
        &self.vein_ridged
    }
    /// `veinGap()`.
    pub fn vein_gap(&self) -> &Arc<dyn DensityFunction> {
        &self.vein_gap
    }

    /// `NoiseRouter.mapAll(Visitor)` — maps every field through
    /// `DensityFunction.mapAll`.
    pub fn map_all(&self, visitor: &dyn Visitor) -> NoiseRouter {
        NoiseRouter::new(
            map_all(&*self.barrier_noise, visitor),
            map_all(&*self.fluid_level_floodedness_noise, visitor),
            map_all(&*self.fluid_level_spread_noise, visitor),
            map_all(&*self.lava_noise, visitor),
            map_all(&*self.temperature, visitor),
            map_all(&*self.vegetation, visitor),
            map_all(&*self.continents, visitor),
            map_all(&*self.erosion, visitor),
            map_all(&*self.depth, visitor),
            map_all(&*self.ridges, visitor),
            map_all(&*self.preliminary_surface_level, visitor),
            map_all(&*self.final_density, visitor),
            map_all(&*self.vein_toggle, visitor),
            map_all(&*self.vein_ridged, visitor),
            map_all(&*self.vein_gap, visitor),
        )
    }
}

/// `NoiseRouter.CODEC` — the 15-field record codec, as the ops-generic
/// `noise_router_codec::<Ops>()` factory. Each field is
/// `DensityFunction.CODEC.fieldOf(name)`.
///
/// `type_complexity`: the applicative fold grows a 15-tuple of
/// `Arc<dyn DensityFunction>` one `apply2` at a time, mirroring Java's
/// `RecordCodecBuilder.apply` chain.
#[allow(clippy::type_complexity)]
pub fn noise_router_codec<Ops>() -> Arc<dyn Codec<NoiseRouter, Ops>>
where
    Ops: DynamicOps + 'static + rivet_registry::registry_ops::RegistryOpsLookup,
{
    #[allow(clippy::type_complexity)]
    {
        let top = crate::levelgen::noise::density_function::density_function_codec::<Ops>();
        let f = |name: &str| codec::field_of(top.clone(), name.to_string());

        let barrier = f("barrier");
        let floodedness = f("fluid_level_floodedness");
        let spread = f("fluid_level_spread");
        let lava = f("lava");
        let temperature = f("temperature");
        let vegetation = f("vegetation");
        let continents = f("continents");
        let erosion = f("erosion");
        let depth = f("depth");
        let ridges = f("ridges");
        let preliminary = f("preliminary_surface_level");
        let final_density = f("final_density");
        let vein_toggle = f("vein_toggle");
        let vein_ridged = f("vein_ridged");
        let vein_gap = f("vein_gap");

        let encoder = Arc::new(NoiseRouterEncoder {
            barrier: Arc::new(MapCodecEncoderHalf(barrier.clone())),
            floodedness: Arc::new(MapCodecEncoderHalf(floodedness.clone())),
            spread: Arc::new(MapCodecEncoderHalf(spread.clone())),
            lava: Arc::new(MapCodecEncoderHalf(lava.clone())),
            temperature: Arc::new(MapCodecEncoderHalf(temperature.clone())),
            vegetation: Arc::new(MapCodecEncoderHalf(vegetation.clone())),
            continents: Arc::new(MapCodecEncoderHalf(continents.clone())),
            erosion: Arc::new(MapCodecEncoderHalf(erosion.clone())),
            depth: Arc::new(MapCodecEncoderHalf(depth.clone())),
            ridges: Arc::new(MapCodecEncoderHalf(ridges.clone())),
            preliminary: Arc::new(MapCodecEncoderHalf(preliminary.clone())),
            final_density: Arc::new(MapCodecEncoderHalf(final_density.clone())),
            vein_toggle: Arc::new(MapCodecEncoderHalf(vein_toggle.clone())),
            vein_ridged: Arc::new(MapCodecEncoderHalf(vein_ridged.clone())),
            vein_gap: Arc::new(MapCodecEncoderHalf(vein_gap.clone())),
        });
        let decoder = Arc::new(NoiseRouterDecoder {
            barrier: Arc::new(MapCodecDecoderHalf(barrier)),
            floodedness: Arc::new(MapCodecDecoderHalf(floodedness)),
            spread: Arc::new(MapCodecDecoderHalf(spread)),
            lava: Arc::new(MapCodecDecoderHalf(lava)),
            temperature: Arc::new(MapCodecDecoderHalf(temperature)),
            vegetation: Arc::new(MapCodecDecoderHalf(vegetation)),
            continents: Arc::new(MapCodecDecoderHalf(continents)),
            erosion: Arc::new(MapCodecDecoderHalf(erosion)),
            depth: Arc::new(MapCodecDecoderHalf(depth)),
            ridges: Arc::new(MapCodecDecoderHalf(ridges)),
            preliminary: Arc::new(MapCodecDecoderHalf(preliminary)),
            final_density: Arc::new(MapCodecDecoderHalf(final_density)),
            vein_toggle: Arc::new(MapCodecDecoderHalf(vein_toggle)),
            vein_ridged: Arc::new(MapCodecDecoderHalf(vein_ridged)),
            vein_gap: Arc::new(MapCodecDecoderHalf(vein_gap)),
        });
        map_codec::codec_of(map_codec::of(encoder, decoder, "NoiseRouter".to_string()))
    }
}

/// The 15-field `MapEncoder` — encodes every field in Java's order.
struct NoiseRouterEncoder<Ops: DynamicOps + 'static> {
    barrier: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    floodedness: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    spread: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    lava: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    temperature: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    vegetation: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    continents: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    erosion: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    depth: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    ridges: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    preliminary: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    final_density: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    vein_toggle: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    vein_ridged: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
    vein_gap: Arc<dyn MapEncoder<Arc<dyn DensityFunction>, Ops>>,
}
impl<Ops: DynamicOps + 'static> Debug for NoiseRouterEncoder<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NoiseRouterEncoder")
    }
}
impl<Ops: DynamicOps + 'static> Keyable<Ops> for NoiseRouterEncoder<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.barrier.keys(ops);
        keys.extend(self.floodedness.keys(ops));
        keys.extend(self.spread.keys(ops));
        keys.extend(self.lava.keys(ops));
        keys.extend(self.temperature.keys(ops));
        keys.extend(self.vegetation.keys(ops));
        keys.extend(self.continents.keys(ops));
        keys.extend(self.erosion.keys(ops));
        keys.extend(self.depth.keys(ops));
        keys.extend(self.ridges.keys(ops));
        keys.extend(self.preliminary.keys(ops));
        keys.extend(self.final_density.keys(ops));
        keys.extend(self.vein_toggle.keys(ops));
        keys.extend(self.vein_ridged.keys(ops));
        keys.extend(self.vein_gap.keys(ops));
        keys
    }
}
impl<Ops: DynamicOps + 'static> MapEncoder<NoiseRouter, Ops> for NoiseRouterEncoder<Ops> {
    fn encode(
        &self,
        input: &NoiseRouter,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.barrier.encode(&input.barrier_noise, ops, prefix);
        self.floodedness
            .encode(&input.fluid_level_floodedness_noise, ops, prefix);
        self.spread
            .encode(&input.fluid_level_spread_noise, ops, prefix);
        self.lava.encode(&input.lava_noise, ops, prefix);
        self.temperature.encode(&input.temperature, ops, prefix);
        self.vegetation.encode(&input.vegetation, ops, prefix);
        self.continents.encode(&input.continents, ops, prefix);
        self.erosion.encode(&input.erosion, ops, prefix);
        self.depth.encode(&input.depth, ops, prefix);
        self.ridges.encode(&input.ridges, ops, prefix);
        self.preliminary
            .encode(&input.preliminary_surface_level, ops, prefix);
        self.final_density.encode(&input.final_density, ops, prefix);
        self.vein_toggle.encode(&input.vein_toggle, ops, prefix);
        self.vein_ridged.encode(&input.vein_ridged, ops, prefix);
        self.vein_gap.encode(&input.vein_gap, ops, prefix);
    }
}

/// The 15-field `MapDecoder` — every field decoded with error accumulation
/// (Java `DataResult.instance().ap15`), combining via nested `apply2`.
struct NoiseRouterDecoder<Ops: DynamicOps + 'static> {
    barrier: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    floodedness: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    spread: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    lava: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    temperature: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    vegetation: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    continents: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    erosion: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    depth: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    ridges: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    preliminary: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    final_density: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    vein_toggle: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    vein_ridged: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
    vein_gap: Arc<dyn MapDecoder<Arc<dyn DensityFunction>, Ops>>,
}
impl<Ops: DynamicOps + 'static> Debug for NoiseRouterDecoder<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NoiseRouterDecoder")
    }
}
impl<Ops: DynamicOps + 'static> Keyable<Ops> for NoiseRouterDecoder<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.barrier.keys(ops);
        keys.extend(self.floodedness.keys(ops));
        keys.extend(self.spread.keys(ops));
        keys.extend(self.lava.keys(ops));
        keys.extend(self.temperature.keys(ops));
        keys.extend(self.vegetation.keys(ops));
        keys.extend(self.continents.keys(ops));
        keys.extend(self.erosion.keys(ops));
        keys.extend(self.depth.keys(ops));
        keys.extend(self.ridges.keys(ops));
        keys.extend(self.preliminary.keys(ops));
        keys.extend(self.final_density.keys(ops));
        keys.extend(self.vein_toggle.keys(ops));
        keys.extend(self.vein_ridged.keys(ops));
        keys.extend(self.vein_gap.keys(ops));
        keys
    }
}
impl<Ops: DynamicOps + 'static> MapDecoder<NoiseRouter, Ops> for NoiseRouterDecoder<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<NoiseRouter> {
        // The applicative fold below grows a 15-tuple of
        // `Arc<dyn DensityFunction>` one `apply2` at a time (mirroring Java's
        // `RecordCodecBuilder.apply` chain); each intermediate closure
        // parameter type trips clippy's `type_complexity`.
        #[allow(clippy::type_complexity)]
        {
            let b = self.barrier.decode(ops, input);
            let fl = self.floodedness.decode(ops, input);
            let sp = self.spread.decode(ops, input);
            let la = self.lava.decode(ops, input);
            let te = self.temperature.decode(ops, input);
            let ve = self.vegetation.decode(ops, input);
            let co = self.continents.decode(ops, input);
            let er = self.erosion.decode(ops, input);
            let de = self.depth.decode(ops, input);
            let ri = self.ridges.decode(ops, input);
            let pr = self.preliminary.decode(ops, input);
            let fd = self.final_density.decode(ops, input);
            let vt = self.vein_toggle.decode(ops, input);
            let vr = self.vein_ridged.decode(ops, input);
            let vg = self.vein_gap.decode(ops, input);
            b.apply2(
                move |b: &Arc<dyn DensityFunction>, fl: &Arc<dyn DensityFunction>| {
                    (b.clone(), fl.clone())
                },
                fl,
            )
            .apply2(
                move |(b, fl): &(Arc<dyn DensityFunction>, Arc<dyn DensityFunction>),
                      sp: &Arc<dyn DensityFunction>| {
                    (b.clone(), fl.clone(), sp.clone())
                },
                sp,
            )
            .apply2(
                move |(b, fl, sp): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      la: &Arc<dyn DensityFunction>| {
                    (b.clone(), fl.clone(), sp.clone(), la.clone())
                },
                la,
            )
            .apply2(
                move |(b, fl, sp, la): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      te: &Arc<dyn DensityFunction>| {
                    (b.clone(), fl.clone(), sp.clone(), la.clone(), te.clone())
                },
                te,
            )
            .apply2(
                move |(b, fl, sp, la, te): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      ve: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                    )
                },
                ve,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      co: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                    )
                },
                co,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      er: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                    )
                },
                er,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      de: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                    )
                },
                de,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er, de): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      ri: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                        ri.clone(),
                    )
                },
                ri,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er, de, ri): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      pr: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                        ri.clone(),
                        pr.clone(),
                    )
                },
                pr,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er, de, ri, pr): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      fd: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                        ri.clone(),
                        pr.clone(),
                        fd.clone(),
                    )
                },
                fd,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er, de, ri, pr, fd): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      vt: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                        ri.clone(),
                        pr.clone(),
                        fd.clone(),
                        vt.clone(),
                    )
                },
                vt,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er, de, ri, pr, fd, vt): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      vr: &Arc<dyn DensityFunction>| {
                    (
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                        ri.clone(),
                        pr.clone(),
                        fd.clone(),
                        vt.clone(),
                        vr.clone(),
                    )
                },
                vr,
            )
            .apply2(
                move |(b, fl, sp, la, te, ve, co, er, de, ri, pr, fd, vt, vr): &(
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                    Arc<dyn DensityFunction>,
                ),
                      vg: &Arc<dyn DensityFunction>| {
                    NoiseRouter::new(
                        b.clone(),
                        fl.clone(),
                        sp.clone(),
                        la.clone(),
                        te.clone(),
                        ve.clone(),
                        co.clone(),
                        er.clone(),
                        de.clone(),
                        ri.clone(),
                        pr.clone(),
                        fd.clone(),
                        vt.clone(),
                        vr.clone(),
                        vg.clone(),
                    )
                },
                vg,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::noise::density_function::SinglePointContext;
    use crate::levelgen::noise::density_functions::constant;
    use crate::levelgen::noise::registry_keys;
    use rivet_registry::Identifier;
    use rivet_registry::ResourceKey;
    use rivet_registry::access::RegistryAccess;
    use rivet_registry::builder::RegistryBuilder;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// The `RegistryAccess` providing the `DENSITY_FUNCTION` key (empty frozen
    /// registry) that `DensityFunction.CODEC`'s `RegistryFileCodec` needs to
    /// encode/decode a `Direct` holder.
    fn test_ops() -> TestOps {
        let builder = RegistryBuilder::new(&*registry_keys::DENSITY_FUNCTION);
        let registry = builder.freeze();
        let access = RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace(
                "worldgen/density_function",
            )),
            Box::new(registry) as rivet_registry::root::AnyBox,
        )]);
        TestOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    /// A router whose 15 fields are the constants `0.0 .. 15.0` (the field's
    /// 1-based slot). Every field is a `Direct` constant, so the codec round-trips
    /// each as a bare double.
    fn router_of_constants() -> NoiseRouter {
        NoiseRouter::new(
            constant(1.0),
            constant(2.0),
            constant(3.0),
            constant(4.0),
            constant(5.0),
            constant(6.0),
            constant(7.0),
            constant(8.0),
            constant(9.0),
            constant(10.0),
            constant(11.0),
            constant(12.0),
            constant(13.0),
            constant(14.0),
            constant(15.0),
        )
    }

    #[test]
    fn codec_round_trips_all_fifteen_fields_in_java_order() {
        let codec = noise_router_codec::<TestOps>();
        let ops = test_ops();
        let router = router_of_constants();
        let encoded = codec
            .encode_start(&ops, &router)
            .result()
            .expect("encode should succeed")
            .clone();
        // Java `NoiseRouter.CODEC` field order (record declaration order). Each
        // field is `DensityFunction.CODEC`; a constant encodes as a bare double.
        assert_eq!(
            encoded,
            serde_json::json!({
                "barrier": 1.0,
                "fluid_level_floodedness": 2.0,
                "fluid_level_spread": 3.0,
                "lava": 4.0,
                "temperature": 5.0,
                "vegetation": 6.0,
                "continents": 7.0,
                "erosion": 8.0,
                "depth": 9.0,
                "ridges": 10.0,
                "preliminary_surface_level": 11.0,
                "final_density": 12.0,
                "vein_toggle": 13.0,
                "vein_ridged": 14.0,
                "vein_gap": 15.0,
            })
        );
        let (decoded, _rest) = codec
            .decode(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(
            decoded
                .barrier_noise()
                .compute(&SinglePointContext::new(0, 0, 0)),
            1.0
        );
        assert_eq!(
            decoded
                .fluid_level_floodedness_noise()
                .compute(&SinglePointContext::new(0, 0, 0)),
            2.0
        );
        assert_eq!(
            decoded
                .fluid_level_spread_noise()
                .compute(&SinglePointContext::new(0, 0, 0)),
            3.0
        );
        assert_eq!(
            decoded
                .lava_noise()
                .compute(&SinglePointContext::new(0, 0, 0)),
            4.0
        );
        assert_eq!(
            decoded
                .temperature()
                .compute(&SinglePointContext::new(0, 0, 0)),
            5.0
        );
        assert_eq!(
            decoded
                .vegetation()
                .compute(&SinglePointContext::new(0, 0, 0)),
            6.0
        );
        assert_eq!(
            decoded
                .continents()
                .compute(&SinglePointContext::new(0, 0, 0)),
            7.0
        );
        assert_eq!(
            decoded.erosion().compute(&SinglePointContext::new(0, 0, 0)),
            8.0
        );
        assert_eq!(
            decoded.depth().compute(&SinglePointContext::new(0, 0, 0)),
            9.0
        );
        assert_eq!(
            decoded.ridges().compute(&SinglePointContext::new(0, 0, 0)),
            10.0
        );
        assert_eq!(
            decoded
                .preliminary_surface_level()
                .compute(&SinglePointContext::new(0, 0, 0)),
            11.0
        );
        assert_eq!(
            decoded
                .final_density()
                .compute(&SinglePointContext::new(0, 0, 0)),
            12.0
        );
        assert_eq!(
            decoded
                .vein_toggle()
                .compute(&SinglePointContext::new(0, 0, 0)),
            13.0
        );
        assert_eq!(
            decoded
                .vein_ridged()
                .compute(&SinglePointContext::new(0, 0, 0)),
            14.0
        );
        assert_eq!(
            decoded
                .vein_gap()
                .compute(&SinglePointContext::new(0, 0, 0)),
            15.0
        );
    }

    #[test]
    fn map_all_maps_every_field() {
        // A visitor that squares every constant (Java `mapChildren` then
        // `apply`). All 15 fields are constants, so every field is rewritten.
        struct Square;
        impl Visitor for Square {
            fn apply(&self, input: &dyn DensityFunction) -> Arc<dyn DensityFunction> {
                if let Some(c) = input
                    .as_any()
                    .downcast_ref::<crate::levelgen::noise::density_functions::Constant>()
                {
                    constant(c.value() * c.value())
                } else {
                    input.clone_arc()
                }
            }
        }
        let router = router_of_constants();
        let mapped = router.map_all(&Square);
        let ctx = SinglePointContext::new(0, 0, 0);
        assert_eq!(mapped.barrier_noise().compute(&ctx), 1.0);
        assert_eq!(mapped.fluid_level_floodedness_noise().compute(&ctx), 4.0);
        assert_eq!(mapped.fluid_level_spread_noise().compute(&ctx), 9.0);
        assert_eq!(mapped.lava_noise().compute(&ctx), 16.0);
        assert_eq!(mapped.temperature().compute(&ctx), 25.0);
        assert_eq!(mapped.vegetation().compute(&ctx), 36.0);
        assert_eq!(mapped.continents().compute(&ctx), 49.0);
        assert_eq!(mapped.erosion().compute(&ctx), 64.0);
        assert_eq!(mapped.depth().compute(&ctx), 81.0);
        assert_eq!(mapped.ridges().compute(&ctx), 100.0);
        assert_eq!(mapped.preliminary_surface_level().compute(&ctx), 121.0);
        assert_eq!(mapped.final_density().compute(&ctx), 144.0);
        assert_eq!(mapped.vein_toggle().compute(&ctx), 169.0);
        assert_eq!(mapped.vein_ridged().compute(&ctx), 196.0);
        assert_eq!(mapped.vein_gap().compute(&ctx), 225.0);
    }
}
