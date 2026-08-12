//! `net.minecraft.world.level.biome.Climate` (issue #178,
//! `mc.world.level.biome.core` unit).
//!
//! Faithful port of the full 26.2 `Climate.java` value layer: the
//! `Parameter`/`ParameterPoint`/`ParameterList`/`TargetPoint`/`Sampler`
//! records with their DFU codecs, the `RTree` build/search, the `SpawnFinder`,
//! and the quantization/wrapping/order semantics.
//!
//! ## Fidelity notes
//!
//! - **Quantization** — `quantizeCoord` is `(long)(coord * 10000.0F)` (f32
//!   multiply, then a saturating f32→i64 cast), `unquantizeCoord` is
//!   `(float)coord / 10000.0F`. Rust's `as` casts saturate exactly like Java's
//!   float→long narrowing (including NaN → 0).
//! - **Wrapping arithmetic** — every long sum/product in `Parameter.distance`,
//!   `ParameterPoint.fitness`, `Node.distance`, `RTree.build`'s magnitude/cost
//!   accumulators, and `SpawnFinder.getSpawnPositionAndFitness` wraps (Java
//!   `long` +/`*`), so the port uses `wrapping_add`/`wrapping_mul`/
//!   `wrapping_sub`/`wrapping_abs`. `Mth.square(long)` is
//!   `x.wrapping_mul(x)`.
//! - **The RTree `lastResult` hint** — Java's `ThreadLocal<Leaf<T>>` becomes a
//!   per-`RTree` `Mutex<Option<Leaf<T>>>` (the `EndIslandDensityFunction`
//!   `NoiseCache` precedent, OWNERSHIP.md's single-tick-thread model). It only
//!   seeds the search's initial min-distance, so it affects tie-breaking, not
//!   the result for distinct targets.
//! - **`Parameter.CODEC`'s `min > max` check** — Java's `min.compareTo(max)`
//!   is `Float.compare` total order (NaN > every value, +0.0 > -0.0), so the
//!   port uses `f32::total_cmp`. `Parameter.span` uses plain `>` (Java's
//!   primitive `>`), matching Java exactly.
//! - **The interval codec encode** — Java's `Objects.equals(getMin(p),
//!   getMax(p))` uses `Float.equals` (total order); the generic
//!   `interval_codec` compares with `PartialEq`. For `Parameter.CODEC` the
//!   two agree on every reachable value (no NaN/−0.0 can come out of
//!   `unquantizeCoord`).

use crate::levelgen::noise::density_function::{DensityFunction, SinglePointContext};
use crate::levelgen::noise::density_functions;
use rivet_registry::core::{BlockPos, QuartPos};
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::extra_codecs;
use rivet_serialization::map_codec::{self, MapCodecDecoderHalf, MapCodecEncoderHalf};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_util::mth;
use std::fmt;
use std::sync::{Arc, Mutex};

/// `Climate.QUANTIZATION_FACTOR` — `10000.0F`.
const QUANTIZATION_FACTOR: f32 = 10000.0f32;

/// `Climate.PARAMETER_COUNT` — `7`.
pub(crate) const PARAMETER_COUNT: usize = 7;

/// `Climate.RTree.CHILDREN_PER_NODE` — `6`.
const CHILDREN_PER_NODE: usize = 6;

/// `net.minecraft.world.level.biome.Climate` — the static climate-surface
/// helpers (all methods are Java statics; the port is a unit struct).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Climate;

impl Climate {
    /// `Climate.target(float ×6)` — quantizes each coord into a `TargetPoint`.
    pub fn target(
        temperature: f32,
        humidity: f32,
        continentalness: f32,
        erosion: f32,
        depth: f32,
        weirdness: f32,
    ) -> TargetPoint {
        TargetPoint {
            temperature: quantize_coord(temperature),
            humidity: quantize_coord(humidity),
            continentalness: quantize_coord(continentalness),
            erosion: quantize_coord(erosion),
            depth: quantize_coord(depth),
            weirdness: quantize_coord(weirdness),
        }
    }

    /// `Climate.parameters(float ×6, float offset)` — quantizes the offset and
    /// builds `Parameter.point` intervals from the six floats.
    pub fn parameters(
        temperature: f32,
        humidity: f32,
        continentalness: f32,
        erosion: f32,
        depth: f32,
        weirdness: f32,
        offset: f32,
    ) -> ParameterPoint {
        ParameterPoint {
            temperature: Parameter::point(temperature),
            humidity: Parameter::point(humidity),
            continentalness: Parameter::point(continentalness),
            erosion: Parameter::point(erosion),
            depth: Parameter::point(depth),
            weirdness: Parameter::point(weirdness),
            offset: quantize_coord(offset),
        }
    }

    /// `Climate.parameters(Parameter ×6, float offset)` — quantizes the offset
    /// and builds a `ParameterPoint` from the six intervals.
    pub fn parameters_from(
        temperature: Parameter,
        humidity: Parameter,
        continentalness: Parameter,
        erosion: Parameter,
        depth: Parameter,
        weirdness: Parameter,
        offset: f32,
    ) -> ParameterPoint {
        ParameterPoint {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
            offset: quantize_coord(offset),
        }
    }

    /// `Climate.empty()` — a `Sampler` of six zero density functions and an
    /// empty spawn target.
    pub fn empty() -> Sampler {
        let zero = density_functions::zero();
        Sampler {
            temperature: zero.clone(),
            humidity: zero.clone(),
            continentalness: zero.clone(),
            erosion: zero.clone(),
            depth: zero.clone(),
            weirdness: zero.clone(),
            spawn_target: Vec::new(),
        }
    }

    /// `Climate.findSpawnPosition(List<ParameterPoint>, Sampler)` — runs the
    /// `SpawnFinder` and returns the best `BlockPos`.
    pub fn find_spawn_position(target_climates: &[ParameterPoint], sampler: &Sampler) -> BlockPos {
        SpawnFinder::new(target_climates, sampler).result.location
    }
}

/// `Climate.quantizeCoord(float)` — `(long)(coord * 10000.0F)`.
pub fn quantize_coord(coord: f32) -> i64 {
    (coord * QUANTIZATION_FACTOR) as i64
}

/// `Climate.unquantizeCoord(long)` — `(float)coord / 10000.0F`.
pub fn unquantize_coord(coord: i64) -> f32 {
    coord as f32 / QUANTIZATION_FACTOR
}

// ---------------------------------------------------------------------------
// Parameter
// ---------------------------------------------------------------------------

/// `Climate.Parameter(long min, long max)` — a quantized coordinate interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parameter {
    pub min: i64,
    pub max: i64,
}

impl Parameter {
    /// `new Climate.Parameter(long min, long max)`.
    pub fn new(min: i64, max: i64) -> Self {
        Parameter { min, max }
    }

    /// `Climate.Parameter.CODEC` — `ExtraCodecs.intervalCodec(Codec.floatRange(
    /// -2.0F, 2.0F), "min", "max", ...)`, as the ops-generic factory.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Parameter, Ops>> {
        extra_codecs::interval_codec(
            codec::float_range::<Ops>(-2.0, 2.0),
            "min".to_string(),
            "max".to_string(),
            Arc::new(|min: &f32, max: &f32| {
                // Java `min.compareTo(max) > 0` — `Float.compare` total order
                // (NaN > every value; +0.0 > -0.0).
                if min.total_cmp(max) == std::cmp::Ordering::Greater {
                    DataResult::error(format!(
                        "Cannon construct interval, min > max ({} > {})",
                        min, max
                    ))
                } else {
                    DataResult::success(Parameter::new(quantize_coord(*min), quantize_coord(*max)))
                }
            }),
            Arc::new(|p: &Parameter| unquantize_coord(p.min)),
            Arc::new(|p: &Parameter| unquantize_coord(p.max)),
        )
    }

    /// `Climate.Parameter.point(float min)` — `span(min, min)`.
    pub fn point(min: f32) -> Parameter {
        Self::span(min, min)
    }

    /// `Climate.Parameter.span(float min, float max)` — throws when
    /// `min > max` (primitive `>`, so NaN passes through).
    pub fn span(min: f32, max: f32) -> Parameter {
        if min > max {
            panic!("min > max: {} {}", min, max);
        }
        Parameter::new(quantize_coord(min), quantize_coord(max))
    }

    /// `Climate.Parameter.span(Parameter min, Parameter max)` — throws when
    /// `min.min() > max.max()`.
    pub fn span_of(min: &Parameter, max: &Parameter) -> Parameter {
        if min.min > max.max {
            panic!("min > max: {} {}", min, max);
        }
        Parameter::new(min.min, max.max)
    }

    /// `Climate.Parameter.span(Parameter other)` (the nullable-other instance
    /// method; the port's caller resolves the `None` case) — the bounding span
    /// of two intervals.
    pub fn span_with(&self, other: &Parameter) -> Parameter {
        Parameter::new(self.min.min(other.min), self.max.max(other.max))
    }

    /// `Climate.Parameter.distance(long target)`.
    pub fn distance(&self, target: i64) -> i64 {
        let above = target.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(target);
        if above > 0 { above } else { below.max(0) }
    }

    /// `Climate.Parameter.distance(Parameter target)`.
    pub fn distance_to(&self, target: &Parameter) -> i64 {
        let above = target.min.wrapping_sub(self.max);
        let below = self.min.wrapping_sub(target.max);
        if above > 0 { above } else { below.max(0) }
    }
}

impl fmt::Display for Parameter {
    /// `Climate.Parameter.toString()` — `"%d"` when degenerate, else
    /// `"[%d-%d]"` (Java `String.format(Locale.ROOT, ...)`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.min == self.max {
            write!(f, "{}", self.min)
        } else {
            write!(f, "[{}-{}]", self.min, self.max)
        }
    }
}

// ---------------------------------------------------------------------------
// ParameterList
// ---------------------------------------------------------------------------

/// `Climate.ParameterList<T>` — the RTree-indexed parameter-point list.
pub struct ParameterList<T> {
    values: Vec<(ParameterPoint, T)>,
    index: RTree<T>,
}

impl<T> ParameterList<T> {
    /// `Climate.ParameterList.codec(MapCodec<T> valueCodec)` — the ops-generic
    /// factory. `ExtraCodecs.nonEmptyList(RecordCodecBuilder...listOf())`
    /// xmapped to `ParameterList`.
    pub fn codec<Ops: DynamicOps + 'static>(
        value_codec: Arc<dyn rivet_serialization::map_codec::MapCodec<T, Ops>>,
    ) -> Arc<dyn Codec<ParameterList<T>, Ops>>
    where
        T: Clone + Send + Sync + 'static,
    {
        let pair_codec: Arc<dyn Codec<(ParameterPoint, T), Ops>> =
            record_builder::create(|instance| {
                instance
                    .group(RecordCodecBuilder::of(
                        Arc::new(|pair: &(ParameterPoint, T)| pair.0),
                        codec::field_of(ParameterPoint::codec::<Ops>(), "parameters".to_string()),
                    ))
                    .and(RecordCodecBuilder::of(
                        Arc::new(|pair: &(ParameterPoint, T)| pair.1.clone()),
                        value_codec,
                    ))
                    .apply(instance, Arc::new(|p, t| (p, t)))
            });
        let list_codec = extra_codecs::non_empty_list(codec::list(pair_codec));
        codec::xmap(
            list_codec,
            Arc::new(|list: &Vec<(ParameterPoint, T)>| ParameterList::new(list.clone())),
            Arc::new(|pl: &ParameterList<T>| pl.values.clone()),
        )
    }

    /// `new Climate.ParameterList(List<Pair<ParameterPoint, T>>)` — builds the
    /// RTree index over the values.
    pub fn new(values: Vec<(ParameterPoint, T)>) -> Self
    where
        T: Clone,
    {
        let index = RTree::create(&values);
        ParameterList { values, index }
    }

    /// `Climate.ParameterList.values()`.
    pub fn values(&self) -> &[(ParameterPoint, T)] {
        &self.values
    }

    /// `Climate.ParameterList.findValue(TargetPoint)`.
    pub fn find_value(&self, target: &TargetPoint) -> T
    where
        T: Clone,
    {
        self.find_value_index(target)
    }

    /// `Climate.ParameterList.findValueBruteForce(TargetPoint)` — the linear
    /// scan over `values()` (Java `@VisibleForTesting`; used to pin the RTree).
    pub fn find_value_brute_force(&self, target: &TargetPoint) -> T
    where
        T: Clone,
    {
        let mut iter = self.values.iter();
        let (first_point, first_value) = iter
            .next()
            .expect("ParameterList must have at least one value");
        let mut best_fitness = first_point.fitness(target);
        let mut best = first_value.clone();
        for (point, value) in iter {
            let fitness = point.fitness(target);
            if fitness < best_fitness {
                best_fitness = fitness;
                best = value.clone();
            }
        }
        best
    }

    /// `Climate.ParameterList.findValueIndex(TargetPoint)` — the RTree search
    /// with the default `Node::distance` metric.
    pub fn find_value_index(&self, target: &TargetPoint) -> T
    where
        T: Clone,
    {
        self.find_value_index_with_metric(target, &NodeDistanceMetric)
    }

    /// `Climate.ParameterList.findValueIndex(TargetPoint, DistanceMetric)` —
    /// the protected metric-parameterized search (Java `@VisibleForTesting`).
    pub(crate) fn find_value_index_with_metric(
        &self,
        target: &TargetPoint,
        distance_metric: &dyn DistanceMetric<T>,
    ) -> T
    where
        T: Clone,
    {
        self.index.search(target, distance_metric)
    }
}

// ---------------------------------------------------------------------------
// ParameterPoint
// ---------------------------------------------------------------------------

/// `Climate.ParameterPoint` — the seven-parameter target (six intervals plus a
/// quantized offset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterPoint {
    pub temperature: Parameter,
    pub humidity: Parameter,
    pub continentalness: Parameter,
    pub erosion: Parameter,
    pub depth: Parameter,
    pub weirdness: Parameter,
    pub offset: i64,
}

impl ParameterPoint {
    /// `new Climate.ParameterPoint(Parameter ×6, long offset)`.
    pub fn new(
        temperature: Parameter,
        humidity: Parameter,
        continentalness: Parameter,
        erosion: Parameter,
        depth: Parameter,
        weirdness: Parameter,
        offset: i64,
    ) -> Self {
        ParameterPoint {
            temperature,
            humidity,
            continentalness,
            erosion,
            depth,
            weirdness,
            offset,
        }
    }

    /// `Climate.ParameterPoint.CODEC` — the seven-field record codec, as the
    /// ops-generic factory.
    ///
    /// The `record_builder` compositor caps at 6 fields, so the 7-field codec
    /// is built from explicit `MapEncoder`/`MapDecoder` structs (the
    /// `NoiseRouter` 15-field precedent): encode writes every field in Java's
    /// order; decode accumulates via nested `apply2`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ParameterPoint, Ops>> {
        let parameter_codec = Parameter::codec::<Ops>();
        let temperature = codec::field_of(parameter_codec.clone(), "temperature".to_string());
        let humidity = codec::field_of(parameter_codec.clone(), "humidity".to_string());
        let continentalness =
            codec::field_of(parameter_codec.clone(), "continentalness".to_string());
        let erosion = codec::field_of(parameter_codec.clone(), "erosion".to_string());
        let depth = codec::field_of(parameter_codec.clone(), "depth".to_string());
        let weirdness = codec::field_of(parameter_codec.clone(), "weirdness".to_string());
        let offset = codec::field_of(
            codec::xmap(
                codec::float_range::<Ops>(0.0, 1.0),
                Arc::new(|f: &f32| quantize_coord(*f)),
                Arc::new(|l: &i64| unquantize_coord(*l)),
            ),
            "offset".to_string(),
        );

        let encoder = Arc::new(ParameterPointEncoder {
            temperature: Arc::new(MapCodecEncoderHalf(temperature.clone())),
            humidity: Arc::new(MapCodecEncoderHalf(humidity.clone())),
            continentalness: Arc::new(MapCodecEncoderHalf(continentalness.clone())),
            erosion: Arc::new(MapCodecEncoderHalf(erosion.clone())),
            depth: Arc::new(MapCodecEncoderHalf(depth.clone())),
            weirdness: Arc::new(MapCodecEncoderHalf(weirdness.clone())),
            offset: Arc::new(MapCodecEncoderHalf(offset.clone())),
        });
        let decoder = Arc::new(ParameterPointDecoder {
            temperature: Arc::new(MapCodecDecoderHalf(temperature)),
            humidity: Arc::new(MapCodecDecoderHalf(humidity)),
            continentalness: Arc::new(MapCodecDecoderHalf(continentalness)),
            erosion: Arc::new(MapCodecDecoderHalf(erosion)),
            depth: Arc::new(MapCodecDecoderHalf(depth)),
            weirdness: Arc::new(MapCodecDecoderHalf(weirdness)),
            offset: Arc::new(MapCodecDecoderHalf(offset)),
        });
        map_codec::codec_of(map_codec::of(
            encoder,
            decoder,
            "ParameterPoint".to_string(),
        ))
    }

    /// `Climate.ParameterPoint.fitness(TargetPoint)` — the wrapping sum of the
    /// six squared interval distances plus the squared offset.
    pub(crate) fn fitness(&self, target: &TargetPoint) -> i64 {
        mth::square_i64(self.temperature.distance(target.temperature))
            .wrapping_add(mth::square_i64(self.humidity.distance(target.humidity)))
            .wrapping_add(mth::square_i64(
                self.continentalness.distance(target.continentalness),
            ))
            .wrapping_add(mth::square_i64(self.erosion.distance(target.erosion)))
            .wrapping_add(mth::square_i64(self.depth.distance(target.depth)))
            .wrapping_add(mth::square_i64(self.weirdness.distance(target.weirdness)))
            .wrapping_add(mth::square_i64(self.offset))
    }

    /// `Climate.ParameterPoint.parameterSpace()` — the seven `Parameter`s (the
    /// offset becomes a degenerate interval).
    pub(crate) fn parameter_space(&self) -> [Parameter; PARAMETER_COUNT] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            Parameter::new(self.offset, self.offset),
        ]
    }
}

/// The seven-field `MapEncoder` for `ParameterPoint`.
struct ParameterPointEncoder<Ops: DynamicOps + 'static> {
    temperature: Arc<dyn MapEncoder<Parameter, Ops>>,
    humidity: Arc<dyn MapEncoder<Parameter, Ops>>,
    continentalness: Arc<dyn MapEncoder<Parameter, Ops>>,
    erosion: Arc<dyn MapEncoder<Parameter, Ops>>,
    depth: Arc<dyn MapEncoder<Parameter, Ops>>,
    weirdness: Arc<dyn MapEncoder<Parameter, Ops>>,
    offset: Arc<dyn MapEncoder<i64, Ops>>,
}

impl<Ops: DynamicOps + 'static> fmt::Debug for ParameterPointEncoder<Ops> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParameterPointEncoder")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for ParameterPointEncoder<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.temperature.keys(ops);
        keys.extend(self.humidity.keys(ops));
        keys.extend(self.continentalness.keys(ops));
        keys.extend(self.erosion.keys(ops));
        keys.extend(self.depth.keys(ops));
        keys.extend(self.weirdness.keys(ops));
        keys.extend(self.offset.keys(ops));
        keys
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<ParameterPoint, Ops> for ParameterPointEncoder<Ops> {
    fn encode(
        &self,
        input: &ParameterPoint,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.temperature.encode(&input.temperature, ops, prefix);
        self.humidity.encode(&input.humidity, ops, prefix);
        self.continentalness
            .encode(&input.continentalness, ops, prefix);
        self.erosion.encode(&input.erosion, ops, prefix);
        self.depth.encode(&input.depth, ops, prefix);
        self.weirdness.encode(&input.weirdness, ops, prefix);
        self.offset.encode(&input.offset, ops, prefix);
    }
}

/// The seven-field `MapDecoder` for `ParameterPoint` — every field decoded
/// with error accumulation (Java `RecordCodecBuilder` `ap` chain).
struct ParameterPointDecoder<Ops: DynamicOps + 'static> {
    temperature: Arc<dyn MapDecoder<Parameter, Ops>>,
    humidity: Arc<dyn MapDecoder<Parameter, Ops>>,
    continentalness: Arc<dyn MapDecoder<Parameter, Ops>>,
    erosion: Arc<dyn MapDecoder<Parameter, Ops>>,
    depth: Arc<dyn MapDecoder<Parameter, Ops>>,
    weirdness: Arc<dyn MapDecoder<Parameter, Ops>>,
    offset: Arc<dyn MapDecoder<i64, Ops>>,
}

impl<Ops: DynamicOps + 'static> fmt::Debug for ParameterPointDecoder<Ops> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ParameterPointDecoder")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for ParameterPointDecoder<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.temperature.keys(ops);
        keys.extend(self.humidity.keys(ops));
        keys.extend(self.continentalness.keys(ops));
        keys.extend(self.erosion.keys(ops));
        keys.extend(self.depth.keys(ops));
        keys.extend(self.weirdness.keys(ops));
        keys.extend(self.offset.keys(ops));
        keys
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<ParameterPoint, Ops> for ParameterPointDecoder<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ParameterPoint> {
        // The applicative fold grows a 6-tuple of `Parameter` one `apply2` at a
        // time, then combines with the offset (mirroring Java's `RecordCodecBuilder`
        // `ap7` chain); each intermediate tuple trips clippy's `type_complexity`.
        #[allow(clippy::type_complexity)]
        {
            let t = self.temperature.decode(ops, input);
            let h = self.humidity.decode(ops, input);
            let c = self.continentalness.decode(ops, input);
            let e = self.erosion.decode(ops, input);
            let d = self.depth.decode(ops, input);
            let w = self.weirdness.decode(ops, input);
            let o = self.offset.decode(ops, input);
            t.apply2(|t: &Parameter, h: &Parameter| (*t, *h), h)
                .apply2(
                    |(t, h): &(Parameter, Parameter), c: &Parameter| (*t, *h, *c),
                    c,
                )
                .apply2(
                    |(t, h, c): &(Parameter, Parameter, Parameter), e: &Parameter| (*t, *h, *c, *e),
                    e,
                )
                .apply2(
                    |(t, h, c, e): &(Parameter, Parameter, Parameter, Parameter), d: &Parameter| {
                        (*t, *h, *c, *e, *d)
                    },
                    d,
                )
                .apply2(
                    |(t, h, c, e, d): &(Parameter, Parameter, Parameter, Parameter, Parameter),
                     w: &Parameter| { (*t, *h, *c, *e, *d, *w) },
                    w,
                )
                .apply2(
                    |(t, h, c, e, d, w): &(
                        Parameter,
                        Parameter,
                        Parameter,
                        Parameter,
                        Parameter,
                        Parameter,
                    ),
                     o: &i64| ParameterPoint::new(*t, *h, *c, *e, *d, *w, *o),
                    o,
                )
        }
    }
}

// ---------------------------------------------------------------------------
// TargetPoint
// ---------------------------------------------------------------------------

/// `Climate.TargetPoint` — the quantized sample target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetPoint {
    pub temperature: i64,
    pub humidity: i64,
    pub continentalness: i64,
    pub erosion: i64,
    pub depth: i64,
    pub weirdness: i64,
}

impl TargetPoint {
    /// `Climate.TargetPoint.toParameterArray()` — the seven-long array with a
    /// zero offset (`@VisibleForTesting`).
    pub(crate) fn to_parameter_array(self) -> [i64; 7] {
        [
            self.temperature,
            self.humidity,
            self.continentalness,
            self.erosion,
            self.depth,
            self.weirdness,
            0,
        ]
    }
}

// ---------------------------------------------------------------------------
// Sampler
// ---------------------------------------------------------------------------

/// `Climate.Sampler` — the six density functions plus the spawn target.
#[derive(Debug, Clone)]
pub struct Sampler {
    pub temperature: Arc<dyn DensityFunction>,
    pub humidity: Arc<dyn DensityFunction>,
    pub continentalness: Arc<dyn DensityFunction>,
    pub erosion: Arc<dyn DensityFunction>,
    pub depth: Arc<dyn DensityFunction>,
    pub weirdness: Arc<dyn DensityFunction>,
    pub spawn_target: Vec<ParameterPoint>,
}

impl Sampler {
    /// `Climate.Sampler.sample(int quartX, int quartY, int quartZ)` — samples
    /// each density function at the quart-center block and quantizes to a
    /// `TargetPoint`.
    pub fn sample(&self, quart_x: i32, quart_y: i32, quart_z: i32) -> TargetPoint {
        let block_x = QuartPos::to_block(quart_x);
        let block_y = QuartPos::to_block(quart_y);
        let block_z = QuartPos::to_block(quart_z);
        let context = SinglePointContext::new(block_x, block_y, block_z);
        Climate::target(
            self.temperature.compute(&context) as f32,
            self.humidity.compute(&context) as f32,
            self.continentalness.compute(&context) as f32,
            self.erosion.compute(&context) as f32,
            self.depth.compute(&context) as f32,
            self.weirdness.compute(&context) as f32,
        )
    }

    /// `Climate.Sampler.findSpawnPosition()` — `BlockPos.ZERO` when the spawn
    /// target is empty, else the `SpawnFinder` result.
    pub fn find_spawn_position(&self) -> BlockPos {
        if self.spawn_target.is_empty() {
            BlockPos::ZERO
        } else {
            Climate::find_spawn_position(&self.spawn_target, self)
        }
    }
}

// ---------------------------------------------------------------------------
// RTree
// ---------------------------------------------------------------------------

/// `Climate.DistanceMetric<T>` — the `@VisibleForTesting` metric interface.
pub(crate) trait DistanceMetric<T> {
    /// `distance(Node<T>, long[])`.
    fn distance(&self, node: &Node<T>, target: &[i64; PARAMETER_COUNT]) -> i64;
}

/// The default metric — `Climate.RTree.Node::distance`.
pub(crate) struct NodeDistanceMetric;

impl<T> DistanceMetric<T> for NodeDistanceMetric {
    fn distance(&self, node: &Node<T>, target: &[i64; PARAMETER_COUNT]) -> i64 {
        node.distance(target)
    }
}

/// `Climate.RTree.Node<T>` — a tree node (leaf or subtree).
#[derive(Clone)]
pub(crate) enum Node<T> {
    Leaf(Leaf<T>),
    SubTree(SubTree<T>),
}

impl<T> Node<T> {
    /// `Climate.RTree.Node.parameterSpace[i]` — the node's bound for dimension
    /// `i`.
    fn parameter_space(&self, dim: usize) -> Parameter {
        match self {
            Node::Leaf(leaf) => leaf.parameter_space[dim],
            Node::SubTree(sub) => sub.parameter_space[dim],
        }
    }

    /// `Climate.RTree.Node.distance(long[])` — the wrapping sum of squared
    /// interval distances over all seven dimensions.
    fn distance(&self, target: &[i64; PARAMETER_COUNT]) -> i64 {
        let mut distance = 0i64;
        for (i, t) in target.iter().enumerate() {
            let d = self.parameter_space(i).distance(*t);
            distance = distance.wrapping_add(mth::square_i64(d));
        }
        distance
    }

    /// `Climate.RTree.Node.search(long[], @Nullable Leaf<T>, DistanceMetric)`.
    fn search(
        &self,
        target: &[i64; PARAMETER_COUNT],
        candidate: Option<&Leaf<T>>,
        distance_metric: &dyn DistanceMetric<T>,
    ) -> Leaf<T>
    where
        T: Clone,
    {
        match self {
            Node::Leaf(leaf) => leaf.clone(),
            Node::SubTree(sub) => sub.search(target, candidate, distance_metric),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Node<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Climate.RTree.Node.toString()` — `Arrays.toString(parameterSpace)`.
        match self {
            Node::Leaf(leaf) => write!(f, "{:?}", leaf.parameter_space),
            Node::SubTree(sub) => write!(f, "{:?}", sub.parameter_space),
        }
    }
}

/// `Climate.RTree.Leaf<T>`.
#[derive(Clone)]
pub(crate) struct Leaf<T> {
    parameter_space: [Parameter; PARAMETER_COUNT],
    value: T,
}

impl<T> Leaf<T> {
    /// `new Leaf(ParameterPoint, T)` — `super(parameterPoint.parameterSpace())`.
    fn new(parameter_point: &ParameterPoint, value: T) -> Self {
        Leaf {
            parameter_space: parameter_point.parameter_space(),
            value,
        }
    }
}

/// `Climate.RTree.SubTree<T>`.
#[derive(Clone)]
pub(crate) struct SubTree<T> {
    parameter_space: [Parameter; PARAMETER_COUNT],
    children: Vec<Node<T>>,
}

impl<T> SubTree<T> {
    /// `new SubTree(List<Node<T>>)` — `this(buildParameterSpace(children),
    /// children)`.
    fn new(children: Vec<Node<T>>) -> Self {
        SubTree {
            parameter_space: build_parameter_space(&children),
            children,
        }
    }

    /// `Climate.RTree.SubTree.search(...)` — prunes children whose bound
    /// distance already exceeds the running minimum.
    fn search(
        &self,
        target: &[i64; PARAMETER_COUNT],
        candidate: Option<&Leaf<T>>,
        distance_metric: &dyn DistanceMetric<T>,
    ) -> Leaf<T>
    where
        T: Clone,
    {
        let mut min_distance = match candidate {
            Some(c) => distance_metric.distance(&Node::Leaf(c.clone()), target),
            None => i64::MAX,
        };
        let mut closest: Option<Leaf<T>> = candidate.cloned();
        for child in &self.children {
            let child_distance = distance_metric.distance(child, target);
            if min_distance > child_distance {
                let leaf = child.search(target, closest.as_ref(), distance_metric);
                let leaf_distance = if matches!(child, Node::Leaf(_)) {
                    child_distance
                } else {
                    distance_metric.distance(&Node::Leaf(leaf.clone()), target)
                };
                if min_distance > leaf_distance {
                    min_distance = leaf_distance;
                    closest = Some(leaf);
                }
            }
        }
        closest.expect("RTree search must find a leaf")
    }
}

/// `Climate.RTree` — the parameter-space spatial index.
pub(crate) struct RTree<T> {
    root: Node<T>,
    last_result: Mutex<Option<Leaf<T>>>,
}

impl<T> RTree<T> {
    /// `Climate.RTree.create(List<Pair<ParameterPoint, T>>)` — validates the
    /// 7-parameter space and builds the tree.
    pub(crate) fn create(values: &[(ParameterPoint, T)]) -> RTree<T>
    where
        T: Clone,
    {
        if values.is_empty() {
            panic!("Need at least one value to build the search tree.");
        }
        let dimensions = values[0].0.parameter_space().len();
        if dimensions != PARAMETER_COUNT {
            panic!("Expecting parameter space to be 7, got {}", dimensions);
        }
        let leaves: Vec<Node<T>> = values
            .iter()
            .map(|(point, value)| Node::Leaf(Leaf::new(point, value.clone())))
            .collect();
        RTree {
            root: build(dimensions, leaves),
            last_result: Mutex::new(None),
        }
    }

    /// `Climate.RTree.search(TargetPoint, DistanceMetric)` — searches with the
    /// previous result as the initial candidate, then caches the new leaf.
    pub(crate) fn search(&self, target: &TargetPoint, distance_metric: &dyn DistanceMetric<T>) -> T
    where
        T: Clone,
    {
        let target_array = target.to_parameter_array();
        let mut last = self.last_result.lock().unwrap();
        let candidate = last.clone();
        let leaf = self
            .root
            .search(&target_array, candidate.as_ref(), distance_metric);
        *last = Some(leaf.clone());
        leaf.value
    }
}

/// `Climate.RTree.build(int, List<? extends Node<T>>)` — the recursive
/// bucketized construction.
fn build<T>(dimensions: usize, mut children: Vec<Node<T>>) -> Node<T>
where
    T: Clone,
{
    if children.is_empty() {
        panic!("Need at least one child to build a node");
    }
    if children.len() == 1 {
        return children.remove(0);
    }
    if children.len() <= CHILDREN_PER_NODE {
        children.sort_by_key(|n| total_magnitude(n, dimensions));
        return Node::SubTree(SubTree::new(children));
    }

    let mut min_cost = i64::MAX;
    let mut min_dimension = 0usize;
    let mut min_buckets: Vec<Vec<Node<T>>> = Vec::new();
    for d in 0..dimensions {
        let mut sorted = children.clone();
        sort(&mut sorted, dimensions, d, false);
        let buckets = bucketize(&sorted);
        let mut total_cost = 0i64;
        for bucket in &buckets {
            let space = bucket_parameter_space(bucket);
            total_cost = total_cost.wrapping_add(cost(&space));
        }
        if min_cost > total_cost {
            min_cost = total_cost;
            min_dimension = d;
            min_buckets = buckets;
        }
    }

    sort_buckets(&mut min_buckets, dimensions, min_dimension, true);
    let children_nodes: Vec<Node<T>> = min_buckets
        .into_iter()
        .map(|bucket| build(dimensions, bucket))
        .collect();
    Node::SubTree(SubTree::new(children_nodes))
}

/// `Climate.RTree.sort(List<Node>, int dimensions, int dimension, boolean
/// absolute)` — the lexicographic center sort starting at `dimension`.
fn sort<T>(children: &mut [Node<T>], dimensions: usize, dimension: usize, absolute: bool) {
    children.sort_by(|a, b| {
        for d in 0..dimensions {
            let dim = (dimension + d) % dimensions;
            let ka = center(a, dim, absolute);
            let kb = center(b, dim, absolute);
            let ord = ka.cmp(&kb);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

/// `Climate.RTree.sort(List<SubTree<T>>, ...)` — the bucket-list variant used
/// after bucketing (the buckets are sorted by their first dimension only, then
/// re-sorted with `absolute`).
fn sort_buckets<T>(
    buckets: &mut [Vec<Node<T>>],
    dimensions: usize,
    dimension: usize,
    absolute: bool,
) {
    buckets.sort_by(|a, b| {
        for d in 0..dimensions {
            let dim = (dimension + d) % dimensions;
            let ka = bucket_center(a, dim, absolute);
            let kb = bucket_center(b, dim, absolute);
            let ord = ka.cmp(&kb);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        std::cmp::Ordering::Equal
    });
}

/// The comparator key — `(parameter.min() + parameter.max()) / 2L` (wrapping),
/// `Math.abs`'d when `absolute`.
fn center<T>(node: &Node<T>, dim: usize, absolute: bool) -> i64 {
    let parameter = node.parameter_space(dim);
    let center = parameter.min.wrapping_add(parameter.max) / 2;
    if absolute {
        center.wrapping_abs()
    } else {
        center
    }
}

/// A bucket's bound center — the center of the bucket's accumulated parameter
/// space.
fn bucket_center<T>(bucket: &[Node<T>], dim: usize, absolute: bool) -> i64 {
    let space = bucket_parameter_space(bucket);
    let center = space[dim].min.wrapping_add(space[dim].max) / 2;
    if absolute {
        center.wrapping_abs()
    } else {
        center
    }
}

/// `Climate.RTree.bucketize(List<Node>)` — splits into `expectedChildrenCount`
/// (a power of six) sized buckets.
fn bucketize<T>(nodes: &[Node<T>]) -> Vec<Vec<Node<T>>>
where
    T: Clone,
{
    let expected_children_count =
        (6.0f64).powf(((nodes.len() as f64 - 0.01).ln() / (6.0f64).ln()).floor()) as i32;
    let mut buckets: Vec<Vec<Node<T>>> = Vec::new();
    let mut children: Vec<Node<T>> = Vec::new();
    for child in nodes {
        children.push(child.clone());
        if children.len() as i32 >= expected_children_count {
            buckets.push(std::mem::take(&mut children));
        }
    }
    if !children.is_empty() {
        buckets.push(children);
    }
    buckets
}

/// `Climate.RTree.cost(Parameter[])` — the wrapping sum of the absolute span
/// widths.
fn cost(parameter_space: &[Parameter; PARAMETER_COUNT]) -> i64 {
    let mut result = 0i64;
    for parameter in parameter_space {
        let width = parameter.max.wrapping_sub(parameter.min);
        result = result.wrapping_add(width.wrapping_abs());
    }
    result
}

/// `Climate.RTree.buildParameterSpace(List<? extends Node<T>>)` — the bounding
/// spans over every child.
fn build_parameter_space<T>(children: &[Node<T>]) -> [Parameter; PARAMETER_COUNT] {
    if children.is_empty() {
        panic!("SubTree needs at least one child");
    }
    let mut bounds: [Option<Parameter>; PARAMETER_COUNT] = [None; PARAMETER_COUNT];
    for child in children {
        for (d, slot) in bounds.iter_mut().enumerate() {
            let parameter = child.parameter_space(d);
            *slot = Some(match *slot {
                Some(existing) => parameter.span_with(&existing),
                None => parameter,
            });
        }
    }
    // Every slot is `Some` once the children are non-empty.
    let mut out = [Parameter::new(0, 0); PARAMETER_COUNT];
    for (d, slot) in out.iter_mut().enumerate() {
        *slot = bounds[d].unwrap();
    }
    out
}

/// The parameter space of a bucket (all children present by construction).
fn bucket_parameter_space<T>(bucket: &[Node<T>]) -> [Parameter; PARAMETER_COUNT] {
    build_parameter_space(bucket)
}

/// `Climate.RTree`'s `<= 6` sort key — `Climate.RTree.sort`'s inner lambda:
/// the wrapping sum of the absolute dimension centers.
fn total_magnitude<T>(node: &Node<T>, dimensions: usize) -> i64 {
    let mut total = 0i64;
    for dx in 0..dimensions {
        let parameter = node.parameter_space(dx);
        let center = parameter.min.wrapping_add(parameter.max) / 2;
        total = total.wrapping_add(center.wrapping_abs());
    }
    total
}

// ---------------------------------------------------------------------------
// SpawnFinder
// ---------------------------------------------------------------------------

/// `Climate.SpawnFinder` — the radial spawn search.
struct SpawnFinder {
    result: SpawnFinderResult,
}

impl SpawnFinder {
    /// `new SpawnFinder(List<ParameterPoint>, Sampler)` — the origin probe plus
    /// two radial searches.
    fn new(target_climates: &[ParameterPoint], sampler: &Sampler) -> Self {
        let mut finder = SpawnFinder {
            result: get_spawn_position_and_fitness(target_climates, sampler, 0, 0),
        };
        finder.radial_search(target_climates, sampler, 2048.0f32, 512.0f32);
        finder.radial_search(target_climates, sampler, 512.0f32, 32.0f32);
        finder
    }

    /// `Climate.SpawnFinder.radialSearch(...)`.
    fn radial_search(
        &mut self,
        target_climates: &[ParameterPoint],
        sampler: &Sampler,
        max_radius: f32,
        radius_increment: f32,
    ) {
        let mut angle = 0.0f32;
        let mut radius = radius_increment;
        let search_origin = self.result.location;
        while radius <= max_radius {
            let x = search_origin
                .get_x()
                .wrapping_add(((angle as f64).sin() * radius as f64) as i32);
            let z = search_origin
                .get_z()
                .wrapping_add(((angle as f64).cos() * radius as f64) as i32);
            let candidate = get_spawn_position_and_fitness(target_climates, sampler, x, z);
            if candidate.fitness < self.result.fitness {
                self.result = candidate;
            }
            angle += radius_increment / radius;
            if angle as f64 > std::f64::consts::PI * 2.0 {
                angle = 0.0f32;
                radius += radius_increment;
            }
        }
    }
}

/// `Climate.SpawnFinder.Result(BlockPos, long)`.
struct SpawnFinderResult {
    location: BlockPos,
    fitness: i64,
}

/// `Climate.SpawnFinder.getSpawnPositionAndFitness(...)`.
fn get_spawn_position_and_fitness(
    target_climates: &[ParameterPoint],
    sampler: &Sampler,
    block_x: i32,
    block_z: i32,
) -> SpawnFinderResult {
    let target_point = sampler.sample(
        QuartPos::from_block(block_x),
        0,
        QuartPos::from_block(block_z),
    );
    let zero_depth_target_point = TargetPoint {
        temperature: target_point.temperature,
        humidity: target_point.humidity,
        continentalness: target_point.continentalness,
        erosion: target_point.erosion,
        depth: 0,
        weirdness: target_point.weirdness,
    };
    let mut min_fitness = i64::MAX;
    for point in target_climates {
        min_fitness = min_fitness.min(point.fitness(&zero_depth_target_point));
    }
    let distance_bias_to_world_origin =
        mth::square_i64(block_x as i64).wrapping_add(mth::square_i64(block_z as i64));
    let fitness_with_distance = min_fitness
        .wrapping_mul(mth::square_i64(2048))
        .wrapping_add(distance_bias_to_world_origin);
    SpawnFinderResult {
        location: BlockPos::new(block_x, 0, block_z),
        fitness: fitness_with_distance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::core::BlockPos;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    fn params(t: f32, h: f32, c: f32, e: f32, d: f32, w: f32, offset: f32) -> ParameterPoint {
        Climate::parameters(t, h, c, e, d, w, offset)
    }

    // ------------------------------------------------------------------
    // Quantization / unquantization
    // ------------------------------------------------------------------

    #[test]
    fn quantize_coord_is_truncating_f32_multiply() {
        // Java `(long)(coord * 10000.0F)`.
        assert_eq!(quantize_coord(0.0), 0);
        assert_eq!(quantize_coord(0.15), 1500);
        assert_eq!(quantize_coord(-0.15), -1500);
        assert_eq!(quantize_coord(2.0), 20000);
        // A value just below a whole unit truncates.
        assert_eq!(quantize_coord(1.9999), 19999);
    }

    #[test]
    fn quantize_coord_saturates_on_overflow() {
        // Java float→long narrowing saturates; Rust `as i64` matches.
        assert_eq!(quantize_coord(f32::MAX), i64::MAX);
        assert_eq!(quantize_coord(-f32::MAX), i64::MIN);
    }

    #[test]
    fn quantize_coord_nan_is_zero() {
        // Java `(long)NaN` → 0; Rust `f32::NAN as i64` → 0.
        assert_eq!(quantize_coord(f32::NAN), 0);
    }

    #[test]
    fn quantize_unquantize_round_trips() {
        for coord in [-2.0, -0.5, 0.0, 0.5, 1.5, 2.0] {
            assert_eq!(unquantize_coord(quantize_coord(coord)), coord);
        }
    }

    #[test]
    fn unquantize_is_float_division() {
        assert_eq!(unquantize_coord(1500), 0.15);
        assert_eq!(unquantize_coord(-1500), -0.15);
    }

    // ------------------------------------------------------------------
    // Parameter
    // ------------------------------------------------------------------

    #[test]
    fn parameter_distance_point() {
        // distance(long target) = max(above, 0) or below, with wrapping.
        let p = Parameter::new(100, 200);
        assert_eq!(p.distance(150), 0);
        assert_eq!(p.distance(300), 100);
        assert_eq!(p.distance(50), 50);
        // Exact edge.
        assert_eq!(p.distance(100), 0);
        assert_eq!(p.distance(200), 0);
    }

    #[test]
    fn parameter_distance_interval() {
        let p = Parameter::new(100, 200);
        assert_eq!(p.distance_to(&Parameter::new(150, 180)), 0);
        assert_eq!(p.distance_to(&Parameter::new(180, 300)), 0);
        assert_eq!(p.distance_to(&Parameter::new(300, 400)), 100);
        assert_eq!(p.distance_to(&Parameter::new(0, 50)), 50);
    }

    #[test]
    fn parameter_distance_wraps_like_java_long() {
        // Java `above = target - max` and `below = min - target` wrap.
        let p = Parameter::new(i64::MIN, 0);
        assert_eq!(p.distance(1), 1);
        // target - max with target = i64::MIN - 1 wraps.
        let q = Parameter::new(0, i64::MAX);
        assert_eq!(q.distance(i64::MIN), i64::MIN.wrapping_sub(i64::MAX));
    }

    #[test]
    fn parameter_span_constructs_and_panics() {
        assert_eq!(Parameter::span(0.0, 1.0), Parameter::new(0, 10000));
        assert_eq!(Parameter::point(0.5), Parameter::new(5000, 5000));
        assert_eq!(
            Parameter::span_of(&Parameter::new(0, 100), &Parameter::new(50, 200)),
            Parameter::new(0, 200)
        );
        assert!(std::panic::catch_unwind(|| Parameter::span(1.0, 0.0)).is_err());
        assert!(
            std::panic::catch_unwind(|| {
                Parameter::span_of(&Parameter::new(100, 200), &Parameter::new(0, 50))
            })
            .is_err()
        );
    }

    #[test]
    fn parameter_span_nan_passes_through_plain_gt() {
        // `Parameter.span` uses primitive `>` (NaN > NaN is false), so NaN does
        // not panic — matching Java.
        assert_eq!(Parameter::span(f32::NAN, f32::NAN), Parameter::new(0, 0));
    }

    #[test]
    fn parameter_to_string_matches_java_format() {
        assert_eq!(Parameter::new(0, 0).to_string(), "0");
        assert_eq!(Parameter::new(100, 200).to_string(), "[100-200]");
        assert_eq!(Parameter::new(-5000, -1000).to_string(), "[-5000--1000]");
    }

    #[test]
    fn parameter_span_with_bounds_union() {
        let a = Parameter::new(0, 100);
        let b = Parameter::new(50, 200);
        assert_eq!(a.span_with(&b), Parameter::new(0, 200));
        assert_eq!(b.span_with(&a), Parameter::new(0, 200));
    }

    // ------------------------------------------------------------------
    // Parameter codec (interval codec wired up)
    // ------------------------------------------------------------------

    #[test]
    fn parameter_codec_round_trips_degenerate() {
        let codec = Parameter::codec::<JsonOps>();
        // A degenerate interval encodes as the bare point form.
        let value = Parameter::point(0.5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &value)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!(0.5));
        let parsed = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = parsed.result().expect("decode");
        assert_eq!(*decoded, value);
    }

    #[test]
    fn parameter_codec_round_trips_wide() {
        let codec = Parameter::codec::<JsonOps>();
        let value = Parameter::span(-0.5, 1.5);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &value)
            .result()
            .expect("encode")
            .clone();
        assert_eq!(encoded, json!([-0.5, 1.5]));
        let parsed = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = parsed.result().expect("decode");
        assert_eq!(*decoded, value);
    }

    #[test]
    fn parameter_codec_decodes_object_form() {
        let codec = Parameter::codec::<JsonOps>();
        let parsed = codec.parse(&JsonOps::INSTANCE, &json!({"min": -0.5, "max": 1.5}));
        let decoded = parsed.result().expect("decode");
        assert_eq!(*decoded, Parameter::span(-0.5, 1.5));
    }

    #[test]
    fn parameter_codec_rejects_min_greater_than_max() {
        let codec = Parameter::codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!([1.0, 0.5]));
        assert!(result.is_error());
        let error_ref = result.error_ref().expect("error");
        let message = error_ref.message();
        assert!(
            message.contains("Cannon construct interval, min > max"),
            "unexpected message: {message}"
        );
    }

    #[test]
    fn parameter_codec_uses_float_total_order_for_zero_sign() {
        // Java `min.compareTo(max)` is `Float.compare` (total order): +0.0 >
        // -0.0, so `{"min": 0.0, "max": -0.0}` is an error even though plain
        // `0.0 > -0.0` is false. serde_json preserves the -0.0 sign bit.
        let codec = Parameter::codec::<JsonOps>();
        let result = codec.parse(&JsonOps::INSTANCE, &json!({"min": 0.0, "max": -0.0}));
        assert!(result.is_error());
    }

    #[test]
    fn parameter_codec_rejects_out_of_range_point() {
        let codec = Parameter::codec::<JsonOps>();
        assert!(codec.parse(&JsonOps::INSTANCE, &json!(2.5)).is_error());
        assert!(codec.parse(&JsonOps::INSTANCE, &json!(-2.5)).is_error());
    }

    // ------------------------------------------------------------------
    // ParameterPoint codec
    // ------------------------------------------------------------------

    #[test]
    fn parameter_point_codec_round_trips() {
        let codec = ParameterPoint::codec::<JsonOps>();
        let value = params(-0.5, 0.25, 0.0, 1.0, -0.5, 0.75, 0.2);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &value)
            .result()
            .expect("encode")
            .clone();
        let parsed = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = parsed.result().expect("decode");
        assert_eq!(*decoded, value);
    }

    #[test]
    fn parameter_point_codec_rejects_out_of_range_offset() {
        let codec = ParameterPoint::codec::<JsonOps>();
        let bad = json!({
            "temperature": 0.0,
            "humidity": 0.0,
            "continentalness": 0.0,
            "erosion": 0.0,
            "depth": 0.0,
            "weirdness": 0.0,
            "offset": 1.5,
        });
        assert!(codec.parse(&JsonOps::INSTANCE, &bad).is_error());
    }

    // ------------------------------------------------------------------
    // ParameterList + RTree vs brute force
    // ------------------------------------------------------------------

    #[test]
    fn rtree_matches_brute_force_on_synthetic_space() {
        // A spread of parameter points; every target must resolve to the same
        // value via the RTree and the linear scan.
        let values = vec![
            (
                params(-0.5, -0.5, -0.5, -0.5, -0.5, -0.5, 0.0),
                "A".to_string(),
            ),
            (
                params(0.5, -0.5, 0.5, -0.5, 0.5, -0.5, 0.1),
                "B".to_string(),
            ),
            (
                params(-0.5, 0.5, -0.5, 0.5, -0.5, 0.5, 0.2),
                "C".to_string(),
            ),
            (params(0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.3), "D".to_string()),
            (params(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.4), "E".to_string()),
            (
                params(-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, 0.5),
                "F".to_string(),
            ),
            (params(1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.6), "G".to_string()),
        ];
        let list = ParameterList::new(values);
        let targets = [
            Climate::target(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            Climate::target(-0.6, -0.6, -0.6, -0.6, -0.6, -0.6),
            Climate::target(0.6, 0.6, 0.6, 0.6, 0.6, 0.6),
            Climate::target(-0.2, 0.8, -0.2, 0.8, -0.2, 0.8),
            Climate::target(0.9, -0.9, 0.9, -0.9, 0.9, -0.9),
            Climate::target(0.1, 0.2, 0.3, 0.4, 0.5, 0.6),
            Climate::target(-1.0, 1.0, -1.0, 1.0, -1.0, 1.0),
        ];
        for target in &targets {
            let via_rtree = list.find_value(target);
            let via_brute = list.find_value_brute_force(target);
            assert_eq!(
                via_rtree, via_brute,
                "RTree and brute force disagree for target {target:?}"
            );
        }
    }

    #[test]
    fn rtree_returns_minimum_fitness_leaf() {
        // For a target whose nearest point is unique, the returned value must
        // be that point, and the brute-force minimum must match.
        let values = vec![
            (
                params(-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                "left".to_string(),
            ),
            (
                params(1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
                "right".to_string(),
            ),
            (
                params(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5),
                "center".to_string(),
            ),
        ];
        let list = ParameterList::new(values);
        // Target nearest the left point.
        let target = Climate::target(-0.9, 0.1, -0.1, 0.05, 0.0, -0.05);
        assert_eq!(list.find_value(&target), "left");
        let target = Climate::target(0.9, 0.1, -0.1, 0.05, 0.0, -0.05);
        assert_eq!(list.find_value(&target), "right");
    }

    #[test]
    #[should_panic(expected = "Need at least one value to build the search tree.")]
    fn parameter_list_rejects_empty_values() {
        let _ = ParameterList::new(Vec::<(ParameterPoint, String)>::new());
    }

    #[test]
    fn parameter_list_codec_round_trips() {
        // A simple single-field record value codec, flattening "v" into the
        // pair record (matching Java's `valueCodec.forGetter` flattening).
        let value_codec: Arc<dyn rivet_serialization::map_codec::MapCodec<String, JsonOps>> = {
            let field = codec::field_of(codec::string_codec::<JsonOps>(), "v".to_string());
            map_codec::of(
                Arc::new(MapCodecEncoderHalf(field.clone())),
                Arc::new(MapCodecDecoderHalf(field)),
                "StringValue".to_string(),
            )
        };
        let codec = ParameterList::codec(value_codec);
        let list = ParameterList::new(vec![
            (params(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0), "a".to_string()),
            (params(0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.25), "b".to_string()),
        ]);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &list)
            .result()
            .expect("encode")
            .clone();
        let parsed = codec.parse(&JsonOps::INSTANCE, &encoded);
        let decoded = parsed.result().expect("decode");
        assert_eq!(decoded.values(), list.values());
    }

    #[test]
    fn parameter_list_codec_rejects_empty_list() {
        let value_codec: Arc<dyn rivet_serialization::map_codec::MapCodec<String, JsonOps>> = {
            let field = codec::field_of(codec::string_codec::<JsonOps>(), "v".to_string());
            map_codec::of(
                Arc::new(MapCodecEncoderHalf(field.clone())),
                Arc::new(MapCodecDecoderHalf(field)),
                "StringValue".to_string(),
            )
        };
        let codec = ParameterList::codec(value_codec);
        assert!(codec.parse(&JsonOps::INSTANCE, &json!([])).is_error());
    }

    // ------------------------------------------------------------------
    // Sampler
    // ------------------------------------------------------------------

    #[test]
    fn empty_sampler_samples_zero() {
        let sampler = Climate::empty();
        let target = sampler.sample(0, 0, 0);
        assert_eq!(
            target,
            TargetPoint {
                temperature: 0,
                humidity: 0,
                continentalness: 0,
                erosion: 0,
                depth: 0,
                weirdness: 0,
            }
        );
    }

    #[test]
    fn sampler_find_spawn_position_empty_returns_zero() {
        assert_eq!(Climate::empty().find_spawn_position(), BlockPos::ZERO);
    }

    #[test]
    fn find_spawn_position_targeting_origin_with_zero_sampler() {
        // The zero sampler always samples (0,0,0,0,0,0). A single target point
        // at the origin minimizes fitness there; the origin probe wins.
        let sampler = Climate::empty();
        let target = vec![params(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)];
        let pos = Climate::find_spawn_position(&target, &sampler);
        assert_eq!(pos, BlockPos::ZERO);
        assert_eq!(pos.get_y(), 0);
    }

    #[test]
    fn find_spawn_position_is_deterministic() {
        let sampler = Climate::empty();
        let targets = vec![
            params(0.1, -0.2, 0.3, -0.4, 0.5, -0.6, 0.1),
            params(-0.1, 0.2, -0.3, 0.4, -0.5, 0.6, 0.2),
            params(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.3),
        ];
        let a = Climate::find_spawn_position(&targets, &sampler);
        let b = Climate::find_spawn_position(&targets, &sampler);
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------
    // RTree node semantics (hostile/edge)
    // ------------------------------------------------------------------

    #[test]
    fn parameter_point_parameter_space_has_seven_entries() {
        let p = params(-1.0, 0.0, 1.0, -0.5, 0.5, 0.25, 0.75);
        let space = p.parameter_space();
        assert_eq!(space.len(), 7);
        assert_eq!(space[0], Parameter::new(-10000, -10000));
        assert_eq!(space[6], Parameter::new(7500, 7500));
    }

    #[test]
    fn target_point_array_has_zero_offset() {
        let t = Climate::target(0.1, 0.2, 0.3, 0.4, 0.5, 0.6);
        assert_eq!(
            t.to_parameter_array(),
            [1000, 2000, 3000, 4000, 5000, 6000, 0]
        );
    }
}
