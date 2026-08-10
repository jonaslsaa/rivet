//! Port of `net.minecraft.util.CubicSpline`.
//!
//! A piecewise-cubic interpolating function over a bounded-float coordinate.
//! Paper uses it for density functions (terrain shaping) and surface rules: a
//! `Multipoint` spline stores `(location, value, derivative)` knots and
//! hermite-interpolates between them, with linear extension past the ends.
//!
//! Translation notes:
//! - Java's `sealed interface CubicSpline<I>` has two cases, `Constant` (a
//!   raw float) and `Multipoint` (knots + a coordinate). A Rust `enum` carries
//!   the sum shape. The coordinate type `I` is a single generic — exactly like
//!   Java's `CubicSpline<I>` — and `BoundedFloatFunction` is split into a
//!   C-independent bounds supertrait (`BoundedFloat`) + the `apply(C)` trait so
//!   a spline tree can compute bounds without pinning the sample coordinate.
//! - The record `Multipoint` canonical constructor validates sizes FIRST
//!   (`validateSizes`), then the delegating 4-arg constructor computes
//!   `minValue`/`maxValue` BEFORE delegating. The order is observable: an
//!   empty locations array fails size validation; a single location with
//!   unbounded coordinate bounds extrapolates to `±Infinity`. The Rust
//!   `Multipoint::new` mirrors the delegating constructor and
//!   `new_with_bounds` the canonical one.
//! - `Math.min`/`Math.max` NaN propagation is `mth::min_f32`/`max_f32`.
//! - The parity string replicates Java's `%.3f` formatting of the locations /
//!   derivatives / values (`fmt_f32_3`), including the half-away-from-zero
//!   tie rounding Rust's `{:.3}` lacks. The coordinate's `toString` in Java is
//!   the anonymous-class identity hash (`BoundedFloatFunction$1@<hash>`), which
//!   is per-JVM; the Rust `parity_string` uses the coordinate's `Debug` instead.
//!
//! The `codec` is a faithful port of `CubicSpline.codec(Codec<I>)`:
//! `Codec.recursive("CubicSpline")` over
//! `either(FLOAT, Multipoint.codec(coordinate, sub))`, xmapped to the sum. It
//! lives here (the value crate) but is built on `rivet-serialization`, with
//! the ops pinned by the generic `Ops` parameter like the rest of the crate's
//! codec surface. `Multipoint.Point` is the internal `(location, value,
//! derivative)` record the packed-point round trip uses.

use crate::bounded_float_function::{BoundedFloat, BoundedFloatFunction};
use crate::mth;
use rivet_serialization::codec as serde_codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::extra_codecs;
use rivet_serialization::record_builder::RecordCodecBuilder;
use std::sync::Arc;

/// `net.minecraft.util.CubicSpline<I>` — a spline over a bounded-float
/// coordinate `I`.
#[derive(Clone, Debug)]
pub enum CubicSpline<I> {
    /// `CubicSpline.Constant(float)` — a constant-valued spline.
    Constant(f32),
    /// `CubicSpline.Multipoint(I, float[], List<CubicSpline<I>>, float[], float, float)`.
    Multipoint(Multipoint<I>),
}

/// `CubicSpline.Multipoint<I>`.
#[derive(Clone, Debug)]
pub struct Multipoint<I> {
    coordinate: I,
    locations: Vec<f32>,
    values: Vec<CubicSpline<I>>,
    derivatives: Vec<f32>,
    min_value: f32,
    max_value: f32,
}

/// `CubicSpline.Multipoint.Point<I>` — the `(location, value, derivative)`
/// knot record used by the codec's packed-point round trip.
#[derive(Clone, Debug)]
pub struct Point<I> {
    location: f32,
    value: CubicSpline<I>,
    derivative: f32,
}

impl<I> Point<I> {
    /// `Point(location, value, derivative)`.
    pub fn new(location: f32, value: CubicSpline<I>, derivative: f32) -> Point<I> {
        Point {
            location,
            value,
            derivative,
        }
    }

    /// `Point.location()` (Java record accessor).
    pub fn location(&self) -> f32 {
        self.location
    }

    /// `Point.value()` (Java record accessor).
    pub fn value(&self) -> &CubicSpline<I> {
        &self.value
    }

    /// `Point.derivative()` (Java record accessor).
    pub fn derivative(&self) -> f32 {
        self.derivative
    }
}

/// `CubicSpline.Builder<I>`.
#[derive(Clone)]
pub struct Builder<I> {
    coordinate: I,
    value_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
    locations: Vec<f32>,
    values: Vec<CubicSpline<I>>,
    derivatives: Vec<f32>,
}

impl<I: std::fmt::Debug> std::fmt::Debug for Builder<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Builder")
            .field("coordinate", &self.coordinate)
            .field("locations", &self.locations)
            .field("derivatives", &self.derivatives)
            .field("values", &self.values)
            .finish()
    }
}

impl<I> CubicSpline<I> {
    /// `CubicSpline.constant(float)`.
    pub fn constant(value: f32) -> CubicSpline<I> {
        CubicSpline::Constant(value)
    }

    /// `CubicSpline.builder(coordinate)`.
    pub fn builder(coordinate: I) -> Builder<I> {
        Builder::new(coordinate)
    }

    /// `CubicSpline.builder(coordinate, valueTransformer)`.
    pub fn builder_with(
        coordinate: I,
        value_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
    ) -> Builder<I> {
        Builder::with_transformer(coordinate, value_transformer)
    }

    /// `CubicSpline.minValue()`.
    pub fn min_value(&self) -> f32 {
        match self {
            CubicSpline::Constant(v) => *v,
            CubicSpline::Multipoint(m) => m.min_value,
        }
    }

    /// `CubicSpline.maxValue()`.
    pub fn max_value(&self) -> f32 {
        match self {
            CubicSpline::Constant(v) => *v,
            CubicSpline::Multipoint(m) => m.max_value,
        }
    }

    /// `CubicSpline.parityString()`.
    pub fn parity_string(&self) -> String
    where
        I: std::fmt::Debug,
    {
        match self {
            CubicSpline::Constant(v) => format!("k={}", fmt_f32_3(*v)),
            CubicSpline::Multipoint(m) => m.parity_string(),
        }
    }

    /// `CubicSpline.mapCoordinates(UnaryOperator<I>)`.
    pub fn map_coordinates(&self, mapper: Arc<dyn Fn(I) -> I + Send + Sync>) -> CubicSpline<I>
    where
        I: Clone + Send + Sync + 'static + BoundedFloat,
    {
        match self {
            CubicSpline::Constant(_) => self.clone(),
            CubicSpline::Multipoint(m) => {
                let coordinate = mapper(m.coordinate.clone());
                let values = m
                    .values
                    .iter()
                    .map(|v| v.map_coordinates(mapper.clone()))
                    .collect();
                // Java `new Multipoint(mapper.apply(coordinate), locations,
                // mappedValues, derivatives)` — the 4-arg delegating
                // constructor, which re-bounds.
                Multipoint::new(
                    coordinate,
                    m.locations.clone(),
                    values,
                    m.derivatives.clone(),
                )
                .into_spline()
            }
        }
    }
}

impl<I> CubicSpline<I> {
    /// `CubicSpline.sample(CubicSpline<I>, C)`.
    ///
    /// Java passes the same coordinate reference `c` to the coordinate
    /// `apply` and to every nested value `sample`; the Rust port clones it
    /// (`C: Clone`), so the coordinate type only needs `Clone` — never the
    /// stronger `Copy`.
    pub fn sample<C: Clone>(&self, c: C) -> f32
    where
        I: BoundedFloatFunction<C>,
    {
        match self {
            CubicSpline::Constant(v) => *v,
            CubicSpline::Multipoint(m) => m.sample(c),
        }
    }

    /// `CubicSpline.asSampler(CubicSpline<I>)` — a `BoundedFloatFunction<C>`
    /// wrapping the spline's `sample`.
    pub fn sampler<C: Clone>(self) -> Sampler<I, C>
    where
        I: BoundedFloatFunction<C>,
    {
        Sampler::new(self)
    }
}

/// `CubicSpline.asSampler(...)` result — a `BoundedFloatFunction<C>` over a
/// spline tree.
#[derive(Clone)]
pub struct Sampler<I, C> {
    spline: CubicSpline<I>,
    _marker: std::marker::PhantomData<fn(C) -> C>,
}

impl<I: BoundedFloat, C: 'static> BoundedFloat for Sampler<I, C> {
    fn min_value(&self) -> f32 {
        self.spline.min_value()
    }

    fn max_value(&self) -> f32 {
        self.spline.max_value()
    }
}

impl<I, C: Clone + 'static> BoundedFloatFunction<C> for Sampler<I, C>
where
    I: BoundedFloatFunction<C>,
{
    fn apply(&self, c: C) -> f32 {
        self.spline.sample(c)
    }
}

impl<I, C> Sampler<I, C> {
    /// `CubicSpline.asSampler(spline)`.
    pub fn new(spline: CubicSpline<I>) -> Sampler<I, C> {
        Sampler {
            spline,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<I> Builder<I> {
    /// The 1-arg `Builder(coordinate)` constructor — identity transformer.
    fn new(coordinate: I) -> Builder<I> {
        Builder::with_transformer(coordinate, Arc::new(|v| v))
    }

    fn with_transformer(
        coordinate: I,
        value_transformer: Arc<dyn Fn(f32) -> f32 + Send + Sync>,
    ) -> Builder<I> {
        Builder {
            coordinate,
            value_transformer,
            locations: Vec::new(),
            values: Vec::new(),
            derivatives: Vec::new(),
        }
    }

    /// `addPoint(float location, float value)` — applies the value transformer.
    pub fn add_point(mut self, location: f32, value: f32) -> Builder<I> {
        let transformed = (self.value_transformer)(value);
        self.add_point_impl(location, CubicSpline::Constant(transformed), 0.0);
        self
    }

    /// `addPoint(float location, float value, float derivative)` — applies the
    /// value transformer.
    pub fn add_point_with_derivative(
        mut self,
        location: f32,
        value: f32,
        derivative: f32,
    ) -> Builder<I> {
        let transformed = (self.value_transformer)(value);
        self.add_point_impl(location, CubicSpline::Constant(transformed), derivative);
        self
    }

    /// `addPoint(float location, CubicSpline<I> sampler)`.
    pub fn add_spline(mut self, location: f32, sampler: CubicSpline<I>) -> Builder<I> {
        self.add_point_impl(location, sampler, 0.0);
        self
    }

    fn add_point_impl(&mut self, location: f32, sampler: CubicSpline<I>, derivative: f32) {
        if !self.locations.is_empty() && location <= self.locations[self.locations.len() - 1] {
            panic!("Please register points in ascending order");
        }
        self.locations.push(location);
        self.values.push(sampler);
        self.derivatives.push(derivative);
    }

    /// `build()`.
    pub fn build(self) -> CubicSpline<I>
    where
        I: BoundedFloat,
    {
        if self.locations.is_empty() {
            panic!("No elements added");
        }
        Multipoint::new(
            self.coordinate,
            self.locations,
            self.values,
            self.derivatives,
        )
        .into_spline()
    }
}

impl<I> Multipoint<I> {
    /// The 4-arg delegating constructor — validates sizes, computes
    /// `minValue`/`maxValue` (extrapolation + knot interpolation), then stores.
    /// Panics with the same messages as Java's `IllegalArgumentException` on
    /// inconsistent arrays. On *empty* arrays Java's 4-arg constructor instead
    /// hits `ArrayIndexOutOfBoundsException` before its canonical `validateSizes`
    /// runs; here we validate first and panic with the clean "no points" message
    /// (the deviation is unreachable — the builder and the codec both reject
    /// empty before constructing).
    pub fn new(
        coordinate: I,
        locations: Vec<f32>,
        values: Vec<CubicSpline<I>>,
        derivatives: Vec<f32>,
    ) -> Multipoint<I>
    where
        I: BoundedFloat,
    {
        validate_sizes(&locations, &values, &derivatives);
        let last_index = locations.len() - 1;
        let mut min_value = f32::INFINITY;
        let mut max_value = f32::NEG_INFINITY;
        let min_input = coordinate.min_value();
        let max_input = coordinate.max_value();
        if min_input < locations[0] {
            let edge1 = linear_extend(
                min_input,
                &locations,
                values[0].min_value(),
                &derivatives,
                0,
            );
            let edge2 = linear_extend(
                min_input,
                &locations,
                values[0].max_value(),
                &derivatives,
                0,
            );
            min_value = mth::min_f32(min_value, mth::min_f32(edge1, edge2));
            max_value = mth::max_f32(max_value, mth::max_f32(edge1, edge2));
        }
        if max_input > locations[last_index] {
            let edge1 = linear_extend(
                max_input,
                &locations,
                values[last_index].min_value(),
                &derivatives,
                last_index,
            );
            let edge2 = linear_extend(
                max_input,
                &locations,
                values[last_index].max_value(),
                &derivatives,
                last_index,
            );
            min_value = mth::min_f32(min_value, mth::min_f32(edge1, edge2));
            max_value = mth::max_f32(max_value, mth::max_f32(edge1, edge2));
        }
        for value in &values {
            min_value = mth::min_f32(min_value, value.min_value());
            max_value = mth::max_f32(max_value, value.max_value());
        }
        for i in 0..last_index {
            let x1 = locations[i];
            let x2 = locations[i + 1];
            let x_diff = x2 - x1;
            let v1 = &values[i];
            let v2 = &values[i + 1];
            let min1 = v1.min_value();
            let max1 = v1.max_value();
            let min2 = v2.min_value();
            let max2 = v2.max_value();
            let d1 = derivatives[i];
            let d2 = derivatives[i + 1];
            if d1 != 0.0 || d2 != 0.0 {
                let p1 = d1 * x_diff;
                let p2 = d2 * x_diff;
                let min_lerp1 = mth::min_f32(min1, min2);
                let max_lerp1 = mth::max_f32(max1, max2);
                let min_a = p1 - max2 + min1;
                let max_a = p1 - min2 + max1;
                let min_b = -p2 + min2 - max1;
                let max_b = -p2 + max2 - min1;
                let min_lerp2 = mth::min_f32(min_a, min_b);
                let max_lerp2 = mth::max_f32(max_a, max_b);
                min_value = mth::min_f32(min_value, min_lerp1 + 0.25 * min_lerp2);
                max_value = mth::max_f32(max_value, max_lerp1 + 0.25 * max_lerp2);
            }
        }
        Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
            min_value,
            max_value,
        }
    }

    /// The canonical 6-arg constructor — validates sizes, then stores the
    /// caller-supplied bounds. Mirrors the record's canonical constructor
    /// (used by `new_with_bounds` in tests and future callers that already
    /// know the bounds).
    pub fn new_with_bounds(
        coordinate: I,
        locations: Vec<f32>,
        values: Vec<CubicSpline<I>>,
        derivatives: Vec<f32>,
        min_value: f32,
        max_value: f32,
    ) -> Multipoint<I> {
        validate_sizes(&locations, &values, &derivatives);
        Multipoint {
            coordinate,
            locations,
            values,
            derivatives,
            min_value,
            max_value,
        }
    }

    /// Wrap into a `CubicSpline`.
    pub fn into_spline(self) -> CubicSpline<I> {
        CubicSpline::Multipoint(self)
    }
}

impl<I> Multipoint<I> {
    /// `Multipoint.sample(Multipoint, C)`.
    pub fn sample<C: Clone>(&self, c: C) -> f32
    where
        I: BoundedFloatFunction<C>,
    {
        sample_impl(
            &self.coordinate,
            &self.derivatives,
            &self.locations,
            &self.values,
            c,
        )
    }

    /// `Multipoint.parityString()`.
    pub fn parity_string(&self) -> String
    where
        I: std::fmt::Debug,
    {
        let locations = fmt_array(&self.locations);
        let derivatives = fmt_array(&self.derivatives);
        let values = self
            .values
            .iter()
            .map(CubicSpline::parity_string)
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Spline{{coordinate={:?}, locations={}, derivatives={}, values=[{}]}}",
            self.coordinate, locations, derivatives, values
        )
    }

    /// The location array (Java record accessor `locations()`).
    pub fn locations(&self) -> &[f32] {
        &self.locations
    }

    /// The value splines (Java record accessor `values()`).
    pub fn values(&self) -> &[CubicSpline<I>] {
        &self.values
    }

    /// The derivatives array (Java record accessor `derivatives()`).
    pub fn derivatives(&self) -> &[f32] {
        &self.derivatives
    }

    /// `Multipoint.coordinate()` (Java record accessor).
    pub fn coordinate(&self) -> &I {
        &self.coordinate
    }

    /// `Multipoint.minValue()` (Java record accessor).
    pub fn min_value(&self) -> f32 {
        self.min_value
    }

    /// `Multipoint.maxValue()` (Java record accessor).
    pub fn max_value(&self) -> f32 {
        self.max_value
    }

    /// `Multipoint.packToPoints()` — the `points` list for the codec.
    pub fn pack_to_points(&self) -> Vec<Point<I>>
    where
        I: Clone,
    {
        (0..self.locations.len())
            .map(|i| {
                Point::new(
                    self.locations[i],
                    self.values[i].clone(),
                    self.derivatives[i],
                )
            })
            .collect()
    }

    /// `Multipoint.createFromPoints(coordinate, points)` — the codec's decode
    /// half, which builds via the delegating 4-arg constructor.
    pub fn create_from_points(coordinate: I, points: Vec<Point<I>>) -> Multipoint<I>
    where
        I: BoundedFloat,
    {
        let point_count = points.len();
        let mut locations = Vec::with_capacity(point_count);
        let mut values = Vec::with_capacity(point_count);
        let mut derivatives = Vec::with_capacity(point_count);
        for p in points {
            locations.push(p.location);
            values.push(p.value);
            derivatives.push(p.derivative);
        }
        Multipoint::new(coordinate, locations, values, derivatives)
    }
}

/// `linearExtend(input, locations, value, derivatives, index)`.
fn linear_extend(
    input: f32,
    locations: &[f32],
    value: f32,
    derivatives: &[f32],
    index: usize,
) -> f32 {
    let derivative = derivatives[index];
    if derivative == 0.0 {
        value
    } else {
        value + derivative * (input - locations[index])
    }
}

/// `validateSizes` — Java `IllegalArgumentException`s for mismatched or empty
/// arrays.
fn validate_sizes<I>(locations: &[f32], values: &[CubicSpline<I>], derivatives: &[f32]) {
    if locations.len() != values.len() || locations.len() != derivatives.len() {
        panic!(
            "All lengths must be equal, got: {} {} {}",
            locations.len(),
            values.len(),
            derivatives.len()
        );
    }
    if locations.is_empty() {
        panic!("Cannot create a multipoint spline with no points");
    }
}

/// The private `sample(Coordinate, float[], float[], List<CubicSpline>, C)`.
fn sample_impl<I, C: Clone>(
    coordinate: &I,
    derivatives: &[f32],
    locations: &[f32],
    values: &[CubicSpline<I>],
    c: C,
) -> f32
where
    I: BoundedFloatFunction<C>,
{
    let input = coordinate.apply(c.clone());
    let start = find_interval_start(locations, input);
    let last_index = (locations.len() - 1) as i32;
    if start < 0 {
        return linear_extend(
            input,
            locations,
            values[0].sample(c.clone()),
            derivatives,
            0,
        );
    }
    if start == last_index {
        return linear_extend(
            input,
            locations,
            values[last_index as usize].sample(c.clone()),
            derivatives,
            last_index as usize,
        );
    }
    // `start` is now in `[0, last_index - 1]` — a valid interval start.
    let start = start as usize;
    let x1 = locations[start];
    let x2 = locations[start + 1];
    let t = (input - x1) / (x2 - x1);
    let f1 = &values[start];
    let f2 = &values[start + 1];
    let d1 = derivatives[start];
    let d2 = derivatives[start + 1];
    let y1 = f1.sample(c.clone());
    let y2 = f2.sample(c);
    let a = d1 * (x2 - x1) - (y2 - y1);
    let b = -d2 * (x2 - x1) + (y2 - y1);
    mth::lerp_f32(t, y1, y2) + t * (1.0 - t) * mth::lerp_f32(t, a, b)
}

/// `findIntervalStart` — `Mth.binarySearch(0, locations.length, i -> input <
/// locations[i]) - 1`.
fn find_interval_start(locations: &[f32], input: f32) -> i32 {
    mth::binary_search(0, locations.len() as i32, |i| input < locations[i as usize]) - 1
}

/// Java `String.format(Locale.ROOT, "%.3f", float)` — the parity-string number
/// format.
///
/// Java's `Formatter` widens the float to a double, takes that double's
/// shortest round-trip decimal, and rounds it to 3 fractional digits with
/// `RoundingMode.HALF_UP` (half away from zero). Rust's `{:.3}` instead rounds
/// the *binary* value ties-to-even, so the two diverge at exact decimal ties
/// (e.g. `0.0625f32` → Java `"0.063"`, Rust `"0.062"`). Scaling to thousandths
/// does not fix it either: at |v| ≳ 1e17 the binary double is a different
/// decimal than the shortest round-trip one, so scaling diverges from Java too.
///
/// This replicates the JDK 25 algorithm exactly: widen to `f64`, format the
/// shortest round-trip decimal (Rust `{}` Display, which is the same digits
/// Java's `Double.toString` prints), scale by 1000, round half away from zero,
/// and re-emit with exactly 3 fraction digits. `NaN`/`±Infinity` are spelled as
/// Java prints them. Validated bit-exact against `javac`/`java` 25 on a 4.6M
/// sweep (random bit patterns, dense ties, subnormals, ±0.0, huge magnitudes).
fn fmt_f32_3(f: f32) -> String {
    if f.is_nan() {
        return "NaN".to_string();
    }
    if f == f32::INFINITY {
        return "Infinity".to_string();
    }
    if f == f32::NEG_INFINITY {
        return "-Infinity".to_string();
    }

    let s = format!("{}", f as f64);
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, s.as_str()),
    };

    // `body` is the shortest round-trip decimal of the widened double: a
    // mantissa possibly with a '.' and possibly an exponent suffix. Rust's
    // `{}` Display only uses exponent notation for extreme magnitudes, and the
    // mantissa is the full significant digit string in either case.
    let (mantissa, exp10) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], body[i + 1..].parse::<i64>().unwrap_or(0)),
        None => (body, 0),
    };
    let frac_digits = match mantissa.find('.') {
        Some(dot) => (mantissa.len() - dot - 1) as i64,
        None => 0,
    };
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    // Strip leading zeros but keep one digit (so `0.0004` → `"4"`).
    let trimmed = digits.trim_start_matches('0').to_string();
    let c = if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed
    };

    // value * 1000 = C * 10^k, where C = digits, k = exp10 - frac_digits + 3.
    let k = exp10 - frac_digits + 3;
    let (int_part, frac_part) = if c == "0" {
        ("0".to_string(), 0u32)
    } else if k >= 0 {
        // C * 10^k is an integer; no rounding needed.
        let mut r = c;
        for _ in 0..k {
            r.push('0');
        }
        split_last3(r)
    } else {
        // k < 0: C * 10^k is a fraction. Round half away from zero.
        let shift = (-k) as usize;
        if shift > c.len() {
            // C < 10^shift, so value*1000 rounds to 0 or 1. 2C has at most
            // len+1 digits while 10^shift has shift+1 > len+1, so 2C < 10^shift
            // always: rounds to zero. (Avoids `10u128::pow` overflow for the
            // huge shifts of subnormal magnitudes.)
            ("0".to_string(), 0u32)
        } else {
            // shift <= len <= 17 significant digits: C and 10^shift both fit u128.
            let c_u: u128 = c.parse().unwrap_or(0);
            let ten: u128 = 10u128.pow(shift as u32);
            let (q, rem) = (c_u / ten, c_u % ten);
            split_last3(if rem * 2 >= ten {
                (q + 1).to_string()
            } else {
                q.to_string()
            })
        }
    };

    format!(
        "{}{}.{:03}",
        if neg { "-" } else { "" },
        int_part,
        frac_part
    )
}

/// Split an integer digit string `R = value * 1000` into `(R/1000, R%1000)`.
fn split_last3(r: String) -> (String, u32) {
    if r.len() <= 3 {
        ("0".to_string(), r.parse().unwrap_or(0))
    } else {
        let (head, tail) = r.split_at(r.len() - 3);
        (head.to_string(), tail.parse().unwrap_or(0))
    }
}

/// `toString(float[])` — the `%.3f` list format of the parity string.
fn fmt_array(arr: &[f32]) -> String {
    let inner = arr
        .iter()
        .map(|f| fmt_f32_3(*f))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{}]", inner)
}

// ---------------------------------------------------------------------------
// Codec
// ---------------------------------------------------------------------------

/// `CubicSpline.codec(Codec<I>)` — `Codec.recursive("CubicSpline")` over
/// `either(FLOAT, Multipoint.codec(coordinate, sub))`, xmapped to the sum.
pub fn codec<I, Ops>(
    coordinate_codec: Arc<dyn rivet_serialization::Codec<I, Ops>>,
) -> Arc<dyn rivet_serialization::Codec<CubicSpline<I>, Ops>>
where
    I: BoundedFloat + Clone + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    serde_codec::recursive(
        "CubicSpline".to_string(),
        Arc::new(move |sub_spline_codec| {
            let float_codec = serde_codec::float_codec::<Ops>();
            let multipoint_codec =
                multipoint_codec(coordinate_codec.clone(), sub_spline_codec.clone());
            let either_codec = serde_codec::either(float_codec, multipoint_codec);
            serde_codec::xmap(
                either_codec,
                Arc::new(|e: &Either<f32, Multipoint<I>>| {
                    e.map_ref(
                        |v| CubicSpline::Constant(*v),
                        |m| CubicSpline::Multipoint(m.clone()),
                    )
                }),
                Arc::new(|s: &CubicSpline<I>| match s {
                    CubicSpline::Constant(v) => Either::left(*v),
                    CubicSpline::Multipoint(m) => Either::right(m.clone()),
                }),
            )
        }),
    )
}

/// `CubicSpline.Multipoint.codec(Codec<I>, Codec<CubicSpline<I>>)`.
pub fn multipoint_codec<I, Ops>(
    coordinate_codec: Arc<dyn rivet_serialization::Codec<I, Ops>>,
    sub_spline_codec: Arc<dyn rivet_serialization::Codec<CubicSpline<I>, Ops>>,
) -> Arc<dyn rivet_serialization::Codec<Multipoint<I>, Ops>>
where
    I: BoundedFloat + Clone + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    let coordinate_builder = RecordCodecBuilder::of_named(
        Arc::new(|m: &Multipoint<I>| m.coordinate.clone()),
        "coordinate".to_string(),
        coordinate_codec,
    );
    let points_field = {
        let point_codec = point_codec(sub_spline_codec);
        let list_codec = serde_codec::list(point_codec);
        let nonempty = extra_codecs::non_empty_list(list_codec);
        serde_codec::field_of(nonempty, "points".to_string())
    };
    let points_builder = RecordCodecBuilder::of(
        Arc::new(|m: &Multipoint<I>| m.pack_to_points()),
        points_field,
    );
    rivet_serialization::record_builder::create(|instance| {
        instance
            .group(coordinate_builder)
            .and(points_builder)
            .apply(instance, Arc::new(Multipoint::create_from_points))
    })
}

/// `CubicSpline.Multipoint.Point.codec(Codec<CubicSpline<I>>)`.
fn point_codec<I, Ops>(
    sub_spline_codec: Arc<dyn rivet_serialization::Codec<CubicSpline<I>, Ops>>,
) -> Arc<dyn rivet_serialization::Codec<Point<I>, Ops>>
where
    I: BoundedFloat + Clone + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    let location_builder = RecordCodecBuilder::of_named(
        Arc::new(|p: &Point<I>| p.location),
        "location".to_string(),
        serde_codec::float_codec::<Ops>(),
    );
    let value_builder = RecordCodecBuilder::of_named(
        Arc::new(|p: &Point<I>| p.value.clone()),
        "value".to_string(),
        sub_spline_codec,
    );
    let derivative_builder = RecordCodecBuilder::of_named(
        Arc::new(|p: &Point<I>| p.derivative),
        "derivative".to_string(),
        serde_codec::float_codec::<Ops>(),
    );
    rivet_serialization::record_builder::create(|instance| {
        instance
            .group(location_builder)
            .and(value_builder)
            .and(derivative_builder)
            .apply(instance, Arc::new(Point::new))
    })
}
