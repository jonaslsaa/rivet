//! Port of `com.mojang.brigadier.context.ParsedArgument` (upstream brigadier-1.3.10).

use std::any::Any;
use std::sync::Arc;

use crate::context::string_range::StringRange;

/// Java `ParsedArgument<S, T>`.
///
/// Java keeps `T` as a type-only parameter and erases it at runtime
/// (`Map<String, ParsedArgument<S, ?>>`); here the result is stored as
/// `Arc<dyn Any + Send + Sync>` and recovered by downcast in
/// `CommandContext::get_argument`.
#[derive(Debug, Clone)]
pub struct ParsedArgument {
    range: StringRange,
    result: Arc<dyn Any + Send + Sync>,
    /// The Rust `type_name` of `T`, for the type-mismatch panic message. Java uses
    /// `result.getClass().getSimpleName()`; the exact text differs, only the
    /// panic is behavior-relevant.
    type_name: &'static str,
}

impl ParsedArgument {
    /// Java `ParsedArgument(int start, int end, T result)`.
    pub fn new<T: Any + Send + Sync>(start: i32, end: i32, result: T) -> Self {
        ParsedArgument {
            range: StringRange::between(start, end),
            result: Arc::new(result),
            type_name: std::any::type_name::<T>(),
        }
    }

    /// Java `getRange()`.
    pub fn get_range(&self) -> StringRange {
        self.range
    }

    /// The erased result as a `&dyn Any`, for `CommandContext::get_argument`'s
    /// downcast.
    pub fn result_as_any(&self) -> &dyn Any {
        &*self.result
    }

    /// The stored `T` as a concrete reference, recovered by downcast (Java's
    /// unchecked `getResult()`).
    pub fn get_result<T: Any>(&self) -> &T {
        self.result
            .downcast_ref::<T>()
            .expect("ParsedArgument result type mismatch")
    }

    /// The stored type name (see the struct doc).
    pub fn type_name(&self) -> &'static str {
        self.type_name
    }

    /// Java `hashCode()` — `Objects.hash(range, result)`.
    pub fn hash_code(&self) -> i32 {
        crate::java_hash::objects_hash(&[self.range.hash_code(), any_hash_code(&*self.result)])
    }
}

/// Java `equals`: `Objects.equals(range, that.range) && Objects.equals(result,
/// that.result)`. The concrete argument results this crate produces are `String`,
/// the numeric wrappers and `Boolean`; Java compares them with their own `equals`,
/// which the downcast `any_eq` reproduces.
impl PartialEq for ParsedArgument {
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range && any_eq(&*self.result, &*other.result)
    }
}

impl Eq for ParsedArgument {}

/// Downcast equality over the result types argument parsers produce. See
/// `ParsedArgument::eq`.
pub(crate) fn any_eq(a: &dyn Any, b: &dyn Any) -> bool {
    macro_rules! cmp {
        ($t:ty) => {
            if let (Some(x), Some(y)) = (a.downcast_ref::<$t>(), b.downcast_ref::<$t>()) {
                return x == y;
            }
        };
    }
    cmp!(String);
    cmp!(i8);
    cmp!(i16);
    cmp!(i32);
    cmp!(i64);
    cmp!(u8);
    cmp!(u16);
    cmp!(u32);
    cmp!(u64);
    cmp!(f32);
    cmp!(f64);
    cmp!(bool);
    cmp!(char);
    false
}

/// A hash consistent with `any_eq` (equal results hash equal).
pub(crate) fn any_hash_code(a: &dyn Any) -> i32 {
    macro_rules! hash {
        ($t:ty) => {
            if let Some(x) = a.downcast_ref::<$t>() {
                return hash_value(x);
            }
        };
    }
    hash!(String);
    hash!(i8);
    hash!(i16);
    hash!(i32);
    hash!(i64);
    hash!(u8);
    hash!(u16);
    hash!(u32);
    hash!(u64);
    hash!(bool);
    hash!(char);
    if let Some(x) = a.downcast_ref::<f32>() {
        return crate::java_hash::float_hash(*x);
    }
    if let Some(x) = a.downcast_ref::<f64>() {
        return crate::java_hash::double_hash(*x);
    }
    // Unknown type: Java would use the object's own hashCode (identity for objects
    // without an override). Rust has no such value; fall back to the TypeId so
    // equal-by-identity results (the only equality `any_eq` admits for them) hash
    // equal.
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    a.type_id().hash(&mut hasher);
    hasher.finish() as i32
}

fn hash_value<T: std::hash::Hash>(value: &T) -> i32 {
    use std::hash::Hasher;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    // Java's hashCode is not Rust's SipHash; only the equal-implies-equal-hash
    // invariant is behavior-relevant (matches how `HashSet`/`HashMap` use it).
    hasher.finish() as i32
}
