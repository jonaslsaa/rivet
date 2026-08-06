//! Port of `java.lang.Number` — the smallest DFU-relevant typed numeric box.
//!
//! Java `Number` is abstract; the subclasses the DFU codec surface actually
//! carries are the six primitive wrappers (`Byte`, `Short`, `Integer`, `Long`,
//! `Float`, `Double`). NBT's `NumericTag.box()` returns exactly those six;
//! JsonOps' `getNumberValue` returns Gson numbers that collapse to the same set
//! (i32 range → `Integer`, i64 range → `Long`, otherwise `Double`; the
//! compressed string form → `Integer.parseInt`). `BigInteger`/`BigDecimal` are
//! NOT needed for this surface — no ops produces them from `getNumberValue`.
//!
//! The six `*_value` methods mirror `Number`'s `intValue`/`longValue`/... with
//! Java's JLS 5.1.3 narrowing semantics, which Rust's `as` casts reproduce
//! exactly:
//! - integral variants (`Byte`/`Short`/`Int`/`Long`): narrowing truncates the
//!   high bits (wrap) for `byteValue`/`shortValue`/`intValue` (e.g.
//!   `Long(300).byteValue()` == 44), sign-extends when widening.
//! - `Float`/`Double`: `intValue`/`longValue` truncate toward zero and
//!   saturate to the target's min/max when out of range (`NaN` → 0), while
//!   `byteValue`/`shortValue` go through `(byte)(int)`/`(short)(int)` — truncate
//!   to `int` first, *then* wrap (`Double(300).byteValue()` == 44, NOT the
//!   saturated `f64 as i8`).
//!
//! Equality follows Java `Number.equals`: same-variant value equality with
//! `Float.compare`/`Double.compare` semantics (`NaN == NaN`, `0.0 != -0.0`),
//! and mixed variants are unequal (`Integer(5) != Long(5)`).

/// A typed numeric value — `java.lang.Number`.
#[derive(Debug, Clone, Copy)]
pub enum Number {
    /// `java.lang.Byte`.
    Byte(i8),
    /// `java.lang.Short`.
    Short(i16),
    /// `java.lang.Integer`.
    Int(i32),
    /// `java.lang.Long`.
    Long(i64),
    /// `java.lang.Float`.
    Float(f32),
    /// `java.lang.Double`.
    Double(f64),
}

impl Number {
    /// `Number.byteValue()` — `(byte)` narrowing.
    pub fn byte_value(&self) -> i8 {
        match self {
            Number::Byte(v) => *v,
            Number::Short(v) => *v as i8,
            Number::Int(v) => *v as i8,
            Number::Long(v) => *v as i8,
            // Java `Double.byteValue()` = `(byte)(int)value`: truncate toward
            // zero + saturate to int, then wrap the low byte. A direct `f64 as
            // i8` would saturate to i8 range instead of wrapping.
            Number::Float(v) => (*v as i32) as i8,
            Number::Double(v) => (*v as i32) as i8,
        }
    }

    /// `Number.shortValue()` — `(short)` narrowing.
    pub fn short_value(&self) -> i16 {
        match self {
            Number::Byte(v) => *v as i16,
            Number::Short(v) => *v,
            Number::Int(v) => *v as i16,
            Number::Long(v) => *v as i16,
            Number::Float(v) => (*v as i32) as i16,
            Number::Double(v) => (*v as i32) as i16,
        }
    }

    /// `Number.intValue()` — `(int)` narrowing.
    pub fn int_value(&self) -> i32 {
        match self {
            Number::Byte(v) => *v as i32,
            Number::Short(v) => *v as i32,
            Number::Int(v) => *v,
            Number::Long(v) => *v as i32,
            // JLS 5.1.3: NaN → 0, out-of-range saturates, else truncate toward
            // zero — identical to Rust `f64 as i32` / `f32 as i32`.
            Number::Float(v) => *v as i32,
            Number::Double(v) => *v as i32,
        }
    }

    /// `Number.longValue()` — `(long)` narrowing.
    pub fn long_value(&self) -> i64 {
        match self {
            Number::Byte(v) => *v as i64,
            Number::Short(v) => *v as i64,
            Number::Int(v) => *v as i64,
            Number::Long(v) => *v,
            Number::Float(v) => *v as i64,
            Number::Double(v) => *v as i64,
        }
    }

    /// `Number.floatValue()` — `(float)` narrowing.
    pub fn float_value(&self) -> f32 {
        match self {
            Number::Byte(v) => *v as f32,
            Number::Short(v) => *v as f32,
            Number::Int(v) => *v as f32,
            Number::Long(v) => *v as f32,
            Number::Float(v) => *v,
            Number::Double(v) => *v as f32,
        }
    }

    /// `Number.doubleValue()` — `(double)` widening.
    pub fn double_value(&self) -> f64 {
        match self {
            Number::Byte(v) => *v as f64,
            Number::Short(v) => *v as f64,
            Number::Int(v) => *v as f64,
            Number::Long(v) => *v as f64,
            Number::Float(v) => *v as f64,
            Number::Double(v) => *v,
        }
    }
}

impl From<i8> for Number {
    fn from(value: i8) -> Self {
        Number::Byte(value)
    }
}

impl From<i16> for Number {
    fn from(value: i16) -> Self {
        Number::Short(value)
    }
}

impl From<i32> for Number {
    fn from(value: i32) -> Self {
        Number::Int(value)
    }
}

impl From<i64> for Number {
    fn from(value: i64) -> Self {
        Number::Long(value)
    }
}

impl From<f32> for Number {
    fn from(value: f32) -> Self {
        Number::Float(value)
    }
}

impl From<f64> for Number {
    fn from(value: f64) -> Self {
        Number::Double(value)
    }
}

impl PartialEq for Number {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Number::Byte(a), Number::Byte(b)) => a == b,
            (Number::Short(a), Number::Short(b)) => a == b,
            (Number::Int(a), Number::Int(b)) => a == b,
            (Number::Long(a), Number::Long(b)) => a == b,
            (Number::Float(a), Number::Float(b)) => float_eq(*a, *b),
            (Number::Double(a), Number::Double(b)) => double_eq(*a, *b),
            _ => false,
        }
    }
}

impl Eq for Number {}

/// `Float.compare(value, that) == 0` — `NaN == NaN`, `0.0 != -0.0`, equal by
/// magnitude otherwise (the tag structs' `PartialEq` convention).
fn float_eq(a: f32, b: f32) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == 0.0 && b == 0.0 {
        return a.is_sign_negative() == b.is_sign_negative();
    }
    a == b
}

/// `Double.compare(value, that) == 0` — `NaN == NaN`, `0.0 != -0.0`.
fn double_eq(a: f64, b: f64) -> bool {
    if a.is_nan() && b.is_nan() {
        return true;
    }
    if a == 0.0 && b == 0.0 {
        return a.is_sign_negative() == b.is_sign_negative();
    }
    a == b
}
