//! Port of `net.minecraft.util.BoundedFloatFunction`.
//!
//! A `Float`-valued function with known min/max bounds. In Paper it is the
//! abstract basis for the `CubicSpline` coordinate and value types (the spline
//! builder's "values" are `CubicSpline<I>`; the "coordinate" `I extends
//! BoundedFloatFunction<?>` provides `apply` + the input bounds used to bound
//! the extrapolation in the `Multipoint` constructor).
//!
//! Java's `BoundedFloatFunction<C>` has `minValue()`/`maxValue()` that do not
//! depend on the type argument `C` (they are callable on
//! `I extends BoundedFloatFunction<?>` regardless of the wildcard). The Rust
//! port encodes that with a C-independent supertrait `BoundedFloat` (bounds
//! only), so `CubicSpline<I>` can stay a single-generic type — exactly like
//! Java's `CubicSpline<I>` — and still compute spline bounds from the
//! coordinate without pinning the sample coordinate type `C`.

use std::sync::Arc;

/// The bounds half of `BoundedFloatFunction` — `minValue()`/`maxValue()`,
/// independent of the input type `C`.
pub trait BoundedFloat: Send + Sync + 'static {
    /// `minValue()`.
    fn min_value(&self) -> f32;

    /// `maxValue()`.
    fn max_value(&self) -> f32;
}

/// `net.minecraft.util.BoundedFloatFunction<C>`.
pub trait BoundedFloatFunction<C>: BoundedFloat {
    /// `apply(C)`.
    fn apply(&self, c: C) -> f32;
}

/// `BoundedFloatFunction.IDENTITY` — `apply` is the identity over `Float`,
/// bounds unbounded.
#[derive(Debug, Clone, Copy)]
pub struct Identity;
impl BoundedFloat for Identity {
    fn min_value(&self) -> f32 {
        f32::NEG_INFINITY
    }

    fn max_value(&self) -> f32 {
        f32::INFINITY
    }
}
impl BoundedFloatFunction<f32> for Identity {
    fn apply(&self, c: f32) -> f32 {
        c
    }
}

/// `BoundedFloatFunction.constant(float)`.
pub struct Constant {
    value: f32,
}

impl BoundedFloat for Constant {
    fn min_value(&self) -> f32 {
        self.value
    }

    fn max_value(&self) -> f32 {
        self.value
    }
}

impl<C> BoundedFloatFunction<C> for Constant {
    fn apply(&self, _c: C) -> f32 {
        self.value
    }
}

impl Constant {
    /// `BoundedFloatFunction.constant(value)`.
    pub fn new(value: f32) -> Self {
        Constant { value }
    }
}

/// `BoundedFloatFunction.comap(Function<C2, C>)` result.
pub struct Comapped<C, C2, F: BoundedFloatFunction<C>> {
    outer: Arc<F>,
    function: Arc<dyn Fn(C2) -> C + Send + Sync>,
}

impl<C: 'static, C2: 'static, F: BoundedFloatFunction<C>> BoundedFloat for Comapped<C, C2, F> {
    fn min_value(&self) -> f32 {
        self.outer.min_value()
    }

    fn max_value(&self) -> f32 {
        self.outer.max_value()
    }
}

impl<C: 'static, C2: 'static, F: BoundedFloatFunction<C>> BoundedFloatFunction<C2>
    for Comapped<C, C2, F>
{
    fn apply(&self, c2: C2) -> f32 {
        self.outer.apply((self.function)(c2))
    }
}

impl<C: 'static, C2: 'static, F: BoundedFloatFunction<C>> Comapped<C, C2, F> {
    /// `BoundedFloatFunction.comap(function)`.
    pub fn new(outer: Arc<F>, function: Arc<dyn Fn(C2) -> C + Send + Sync>) -> Self {
        Comapped { outer, function }
    }
}
