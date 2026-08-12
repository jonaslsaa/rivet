//! Port of `com.mojang.serialization.DataResult`.
//!
//! Java `DataResult<R>` is a `sealed interface` with `Success`/`Error` records,
//! each carrying a monoidal `Lifecycle`. The two records are flattened into one
//! struct; `Error`'s `Supplier<String>` message is stored eagerly (DFU's
//! suppliers are pure string concatenations, so eager evaluation is
//! observationally equivalent — PORTING.md drift checklist: no side-effectful
//! suppliers exist).
//!
//! The Java `Applicative` `Instance` (`ap`/`ap2`/`ap3`) is ported as
//! associated functions. Multi-argument functions are `Arc<dyn Fn + Send + Sync>` so the
//! curried fallback of `ap2`/`ap3` (which applies the function to partial
//! values) can own its captured values (`Box<dyn Fn>` is not cloneable).

use crate::functions::{Fn1, Fn2, Fn3, Fn4, Fn5, Fn6};
use crate::lifecycle::Lifecycle;
use std::fmt;
use std::sync::Arc;

/// `com.mojang.serialization.DataResult<R>`.
#[derive(Debug, Clone)]
pub struct DataResult<T> {
    value: DataResultValue<T>,
    lifecycle: Lifecycle,
}

#[derive(Debug, Clone)]
enum DataResultValue<T> {
    Success(T),
    Error { message: String, partial: Option<T> },
}

/// Borrowed view of the `Error` record (`DataResult.error()`), mirroring
/// `com.mojang.serialization.DataResult.Error`.
#[derive(Debug, Clone, Copy)]
pub struct ErrorRef<'a, T> {
    message: &'a str,
    partial: &'a Option<T>,
    lifecycle: Lifecycle,
}

impl<'a, T> ErrorRef<'a, T> {
    /// `Error.message()`.
    pub fn message(&self) -> &str {
        self.message
    }

    /// `Error.partialValue()`.
    pub fn partial(&self) -> &Option<T> {
        self.partial
    }

    /// `Error.lifecycle()`.
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }
}

impl<T> DataResult<T> {
    /// `DataResult.success(R)` — defaults to `Lifecycle.experimental()`.
    pub fn success(value: T) -> Self {
        Self::success_with_lifecycle(value, Lifecycle::experimental())
    }

    /// `DataResult.success(R, Lifecycle)`.
    pub fn success_with_lifecycle(value: T, lifecycle: Lifecycle) -> Self {
        DataResult {
            value: DataResultValue::Success(value),
            lifecycle,
        }
    }

    /// `DataResult.error(Supplier<String>)` — defaults to
    /// `Lifecycle.experimental()`.
    pub fn error(message: impl Into<String>) -> Self {
        Self::error_with_lifecycle(message.into(), Lifecycle::experimental())
    }

    /// `DataResult.error(Supplier<String>, Lifecycle)`.
    pub fn error_with_lifecycle(message: String, lifecycle: Lifecycle) -> Self {
        DataResult {
            value: DataResultValue::Error {
                message,
                partial: None,
            },
            lifecycle,
        }
    }

    /// `DataResult.error(Supplier<String>, R partialResult)`.
    pub fn error_with_partial(message: impl Into<String>, partial: T) -> Self {
        Self::error_raw(message.into(), Some(partial), Lifecycle::experimental())
    }

    /// `DataResult.error(Supplier<String>, R partialResult, Lifecycle)`.
    pub fn error_with_partial_lifecycle(
        message: String,
        partial: Option<T>,
        lifecycle: Lifecycle,
    ) -> Self {
        DataResult {
            value: DataResultValue::Error { message, partial },
            lifecycle,
        }
    }

    /// Internal constructor spelling out the raw value + lifecycle.
    fn error_raw(message: String, partial: Option<T>, lifecycle: Lifecycle) -> Self {
        DataResult {
            value: DataResultValue::Error { message, partial },
            lifecycle,
        }
    }

    /// `DataResult.result()` — `Some` only for a full success (no partial).
    pub fn result(&self) -> Option<&T> {
        match &self.value {
            DataResultValue::Success(v) => Some(v),
            DataResultValue::Error {
                partial: Some(_), ..
            } => None,
            DataResultValue::Error { partial: None, .. } => None,
        }
    }

    /// `DataResult.resultOrPartial(Consumer<String>)` — keeps the partial value,
    /// invoking `onError` for an error message (consumes `self` to move the
    /// value out).
    pub fn result_or_partial(self, mut on_error: impl FnMut(&str)) -> Option<T> {
        match self.value {
            DataResultValue::Success(v) => Some(v),
            DataResultValue::Error { message, partial } => {
                on_error(&message);
                partial
            }
        }
    }

    /// `DataResult.resultOrPartial()`.
    pub fn result_or_partial_silent(self) -> Option<T> {
        match self.value {
            DataResultValue::Success(v) => Some(v),
            DataResultValue::Error { partial, .. } => partial,
        }
    }

    /// `DataResult.hasResultOrPartial()`.
    pub fn has_result_or_partial(&self) -> bool {
        match &self.value {
            DataResultValue::Success(_) => true,
            DataResultValue::Error { partial, .. } => partial.is_some(),
        }
    }

    /// `DataResult.error()` — `Some` only for an error (borrowed view).
    pub fn error_ref(&self) -> Option<ErrorRef<'_, T>> {
        match &self.value {
            DataResultValue::Success(_) => None,
            DataResultValue::Error { message, partial } => Some(ErrorRef {
                message,
                partial,
                lifecycle: self.lifecycle,
            }),
        }
    }

    /// `DataResult.isError()`.
    pub fn is_error(&self) -> bool {
        !self.is_success()
    }

    /// `DataResult.isSuccess()`.
    pub fn is_success(&self) -> bool {
        matches!(self.value, DataResultValue::Success(_))
    }

    /// `DataResult.lifecycle()`.
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    /// `DataResult.getOrThrow(Function<String, E>)` — panics on error
    /// (the unchecked `IllegalStateException` path).
    pub fn get_or_throw(&self, message: impl Into<String>) -> &T {
        match &self.value {
            DataResultValue::Success(v) => v,
            DataResultValue::Error { message: e, .. } => panic!("{}: {}", message.into(), e),
        }
    }

    /// `DataResult.getPartialOrThrow(Function<String, E>)` — returns the
    /// partial value when present, otherwise panics on the error message.
    pub fn get_partial_or_throw(&self, message: impl Into<String>) -> &T {
        match &self.value {
            DataResultValue::Success(v) => v,
            DataResultValue::Error {
                partial: Some(p), ..
            } => p,
            DataResultValue::Error { message: e, .. } => panic!("{}: {}", message.into(), e),
        }
    }

    /// `DataResult.getOrThrow()` (no-arg convenience).
    pub fn get_or_throw_unchecked(&self) -> &T {
        self.get_or_throw("")
    }

    /// `DataResult.map(Function)` — maps a success, and maps the partial value
    /// of an error (Java: an error without a partial is passed through).
    pub fn map<U, F: FnOnce(&T) -> U>(self, f: F) -> DataResult<U> {
        match self.value {
            DataResultValue::Success(v) => {
                DataResult::success_with_lifecycle(f(&v), self.lifecycle)
            }
            DataResultValue::Error { message, partial } => {
                DataResult::error_raw(message, partial.map(|p| f(&p)), self.lifecycle)
            }
        }
    }

    /// `DataResult.map(Function)` over the owned value — identical to Java's
    /// `map` (maps the success and the partial, preserves the lifecycle) but
    /// consumes the value so the function can move fields out without a
    /// `Clone` bound. Used where the mapped result is built from the owned
    /// fields (e.g. wrapping a decoded `Pair` into `Either`).
    pub fn map_owned<U, F: FnOnce(T) -> U>(self, f: F) -> DataResult<U> {
        match self.value {
            DataResultValue::Success(v) => DataResult::success_with_lifecycle(f(v), self.lifecycle),
            DataResultValue::Error { message, partial } => {
                DataResult::error_raw(message, partial.map(f), self.lifecycle)
            }
        }
    }

    /// `DataResult.mapOrElse(Function, Function)`.
    pub fn map_or_else<U>(
        &self,
        success_function: impl FnOnce(&T) -> U,
        error_function: impl FnOnce(&ErrorRef<'_, T>) -> U,
    ) -> U {
        match &self.value {
            DataResultValue::Success(v) => success_function(v),
            DataResultValue::Error { .. } => error_function(&self.error_ref().unwrap()),
        }
    }

    /// `DataResult.ifSuccess(Consumer)`.
    pub fn if_success(self, if_success: impl FnOnce(&T)) -> DataResult<T> {
        if let DataResultValue::Success(v) = &self.value {
            if_success(v);
        }
        self
    }

    /// `DataResult.ifError(Consumer)`.
    pub fn if_error(self, if_error: impl FnOnce(&ErrorRef<'_, T>)) -> DataResult<T> {
        if let DataResultValue::Error { .. } = self.value {
            if_error(&self.error_ref().unwrap());
        }
        self
    }

    /// `DataResult.promotePartial(Consumer<String>)` — turns an error with a
    /// partial value into a success, reporting the error message.
    pub fn promote_partial(self, mut on_error: impl FnMut(&str)) -> DataResult<T> {
        match self.value {
            DataResultValue::Success(_) => self,
            DataResultValue::Error { message, partial } => {
                on_error(&message);
                match partial {
                    Some(p) => DataResult::success_with_lifecycle(p, self.lifecycle),
                    None => DataResult::error_with_lifecycle(message, self.lifecycle),
                }
            }
        }
    }

    /// `DataResult.flatMap(Function)` — applies the function to a full result,
    /// and to the partial value of an error. When the outer is an error-with-
    /// partial, the result ALWAYS stays an error: a success continuation keeps
    /// the original error message with the continuation value as the new
    /// partial (`Java Error.flatMap`), and an error continuation concatenates
    /// the two messages and keeps the second partial.
    pub fn flat_map<U, F: FnOnce(T) -> DataResult<U>>(self, f: F) -> DataResult<U> {
        match self.value {
            DataResultValue::Success(v) => f(v).add_lifecycle(self.lifecycle),
            DataResultValue::Error {
                message,
                partial: None,
            } => DataResult::error_raw(message, None, self.lifecycle),
            DataResultValue::Error {
                message,
                partial: Some(p),
            } => {
                let second = f(p);
                let combined = self.lifecycle.add(second.lifecycle());
                match second.value {
                    DataResultValue::Success(v) => {
                        DataResult::error_raw(message, Some(v), combined)
                    }
                    DataResultValue::Error {
                        message: m2,
                        partial: p2,
                    } => DataResult::error_raw(append_messages(&message, &m2), p2, combined),
                }
            }
        }
    }

    /// `DataResult.ap(DataResult<Function<R, R2>>)`.
    pub fn ap<U>(self, function_result: DataResult<Fn1<T, U>>) -> DataResult<U> {
        let combined = self.lifecycle.add(function_result.lifecycle());
        match (self.value, function_result.value) {
            (DataResultValue::Success(v), DataResultValue::Success(func)) => {
                DataResult::success_with_lifecycle(func(&v), combined)
            }
            (DataResultValue::Success(v), DataResultValue::Error { message, partial }) => {
                DataResult::error_raw(message, partial.map(|f| f(&v)), combined)
            }
            (DataResultValue::Error { message, partial }, DataResultValue::Success(func)) => {
                DataResult::error_raw(message, partial.map(|p| func(&p)), combined)
            }
            (
                DataResultValue::Error {
                    message: m1,
                    partial: p1,
                },
                DataResultValue::Error {
                    message: m2,
                    partial: p2,
                },
            ) => DataResult::error_raw(
                append_messages(&m1, &m2),
                p1.and_then(|a| p2.map(|f| f(&a))),
                combined,
            ),
        }
    }

    /// `DataResult.apply2(BiFunction, DataResult<R2>)`.
    pub fn apply2<R2, S>(
        self,
        function: impl Fn(&T, &R2) -> S + Send + Sync + 'static,
        second: DataResult<R2>,
    ) -> DataResult<S>
    where
        T: Clone + Send + Sync + 'static,
        R2: Clone + Send + Sync + 'static,
        S: 'static,
    {
        let fr: DataResult<Fn2<T, R2, S>> =
            DataResult::success_with_lifecycle(Arc::new(function), Lifecycle::experimental());
        ap2(fr, self, second)
    }

    /// `DataResult.apply2stable(BiFunction, DataResult<R2>)`.
    pub fn apply2_stable<R2, S>(
        self,
        function: impl Fn(&T, &R2) -> S + Send + Sync + 'static,
        second: DataResult<R2>,
    ) -> DataResult<S>
    where
        T: Clone + Send + Sync + 'static,
        R2: Clone + Send + Sync + 'static,
        S: 'static,
    {
        let fr: DataResult<Fn2<T, R2, S>> =
            DataResult::success_with_lifecycle(Arc::new(function), Lifecycle::stable());
        ap2(fr, self, second)
    }

    /// `DataResult.apply3(Function3, DataResult<R2>, DataResult<R3>)`.
    pub fn apply3<R2, R3, S>(
        self,
        function: impl Fn(&T, &R2, &R3) -> S + Send + Sync + 'static,
        second: DataResult<R2>,
        third: DataResult<R3>,
    ) -> DataResult<S>
    where
        T: Clone + Send + Sync + 'static,
        R2: Clone + Send + Sync + 'static,
        R3: Clone + Send + Sync + 'static,
        S: 'static,
    {
        let fr: DataResult<Fn3<T, R2, R3, S>> =
            DataResult::success_with_lifecycle(Arc::new(function), Lifecycle::experimental());
        ap3(fr, self, second, third)
    }

    /// `DataResult.setPartial(R)`.
    pub fn set_partial(self, partial: T) -> DataResult<T> {
        match self.value {
            DataResultValue::Success(_) => self,
            DataResultValue::Error { message, .. } => {
                DataResult::error_raw(message, Some(partial), self.lifecycle)
            }
        }
    }

    /// `DataResult.setPartial(Supplier<R>)`.
    pub fn set_partial_with(self, partial: impl FnOnce() -> T) -> DataResult<T> {
        self.set_partial(partial())
    }

    /// `DataResult.setPartial(Optional<R>)` — `partial.map(this::setPartial)
    /// .orElse(this)` (Java's `Optional<R>`-valued partial in
    /// `OptionalFieldCodec`).
    pub fn set_partial_opt(self, partial: Option<T>) -> DataResult<T> {
        match partial {
            Some(p) => self.set_partial(p),
            None => self,
        }
    }

    /// `DataResult.mapError(UnaryOperator<String>)`.
    pub fn map_error<F: FnOnce(String) -> String>(self, f: F) -> DataResult<T> {
        match self.value {
            DataResultValue::Success(v) => DataResult::success_with_lifecycle(v, self.lifecycle),
            DataResultValue::Error { message, partial } => {
                DataResult::error_raw(f(message), partial, self.lifecycle)
            }
        }
    }

    /// `DataResult.setLifecycle(Lifecycle)`.
    pub fn set_lifecycle(self, lifecycle: Lifecycle) -> DataResult<T> {
        if self.lifecycle == lifecycle {
            return self;
        }
        DataResult {
            value: self.value,
            lifecycle,
        }
    }

    /// `DataResult.addLifecycle(Lifecycle)`.
    pub fn add_lifecycle(self, lifecycle: Lifecycle) -> DataResult<T> {
        let combined = self.lifecycle.add(lifecycle);
        self.set_lifecycle(combined)
    }

    /// `DataResult.appendMessages(String, String)` — `first + "; " + second`.
    pub fn append_messages(first: &str, second: &str) -> String {
        append_messages(first, second)
    }

    /// `DataResult.partialGet(Function<K, V>, Supplier<String>)` — Java's
    /// `partialGet`.
    pub fn partial_get<K, V>(
        partial_get: impl Fn(&K) -> Option<V>,
        error_prefix: impl Fn() -> String,
    ) -> impl Fn(&K) -> DataResult<V>
    where
        K: fmt::Display,
    {
        move |name: &K| match partial_get(name) {
            Some(v) => DataResult::success(v),
            None => DataResult::error(format!("{}{}", error_prefix(), name)),
        }
    }
}

/// `DataResult.INSTANCE.ap2` — the `Applicative` fast path with the curried
/// fallback.
pub fn ap2<T: Clone + Send + Sync + 'static, R2: Clone + Send + Sync + 'static, S: 'static>(
    fr: DataResult<Fn2<T, R2, S>>,
    a: DataResult<T>,
    b: DataResult<R2>,
) -> DataResult<S> {
    // for less recursion (Java Instance.ap2 fast path)
    if let (Some(f), Some(av), Some(bv)) = (fr.result(), a.result(), b.result()) {
        let combined = fr.lifecycle().add(a.lifecycle()).add(b.lifecycle());
        return DataResult::success_with_lifecycle(f(av, bv), combined);
    }

    // Applicative.super.ap2: ap(ap(map(curry, func), a), b)
    // Function2.curry: x -> y -> f(x, y)
    let curried: DataResult<Fn1<T, Fn1<R2, S>>> = fr.map(|f| {
        let f = f.clone();
        let curried_fn: Fn1<T, Fn1<R2, S>> = Arc::new(move |x: &T| {
            let f = f.clone();
            let x = x.clone();
            let inner: Arc<dyn Fn(&R2) -> S + Send + Sync> = Arc::new(move |y: &R2| f(&x, y));
            inner
        });
        curried_fn
    });
    let step1 = a.ap(curried);
    b.ap(step1)
}

/// `DataResult.INSTANCE.ap3` — fast path when all four results are present,
/// otherwise `ap2(ap(map(Function3.curry, func), t1), t2, t3)`.
pub fn ap3<
    T: Clone + Send + Sync + 'static,
    R2: Clone + Send + Sync + 'static,
    R3: Clone + Send + Sync + 'static,
    S: 'static,
>(
    fr: DataResult<Fn3<T, R2, R3, S>>,
    a: DataResult<T>,
    b: DataResult<R2>,
    c: DataResult<R3>,
) -> DataResult<S> {
    // for less recursion (Java Instance.ap3 fast path)
    if let (Some(f), Some(av), Some(bv), Some(cv)) =
        (fr.result(), a.result(), b.result(), c.result())
    {
        let combined = fr
            .lifecycle()
            .add(a.lifecycle())
            .add(b.lifecycle())
            .add(c.lifecycle());
        return DataResult::success_with_lifecycle(f(av, bv, cv), combined);
    }

    // Applicative.super.ap3: ap2(ap(map(Function3::curry, func), t1), t2, t3)
    // Function3.curry: x -> (y, z) -> f(x, y, z)
    let curried: DataResult<Fn1<T, Fn2<R2, R3, S>>> = fr.map(|f| {
        let f = f.clone();
        let curried_fn: Fn1<T, Fn2<R2, R3, S>> = Arc::new(move |x: &T| {
            let f = f.clone();
            let x = x.clone();
            let inner: Fn2<R2, R3, S> = Arc::new(move |y: &R2, z: &R3| f(&x, y, z));
            inner
        });
        curried_fn
    });
    let step1 = a.ap(curried);
    ap2(step1, b, c)
}

/// `Applicative.super.ap4` — Java's default:
/// `ap2(ap2(map(Function4::curry2, func), t1, t2), t3, t4)`.
/// `Function4.curry2`: `(t1, t2) -> (t3, t4) -> f(t1, t2, t3, t4)`.
pub fn ap4<
    T1: Clone + Send + Sync + 'static,
    T2: Clone + Send + Sync + 'static,
    T3: Clone + Send + Sync + 'static,
    T4: Clone + Send + Sync + 'static,
    R: 'static,
>(
    fr: DataResult<Fn4<T1, T2, T3, T4, R>>,
    a: DataResult<T1>,
    b: DataResult<T2>,
    c: DataResult<T3>,
    d: DataResult<T4>,
) -> DataResult<R> {
    let curried: DataResult<Fn2<T1, T2, Fn2<T3, T4, R>>> = fr.map(|f| {
        let f = f.clone();
        let curried_fn: Fn2<T1, T2, Fn2<T3, T4, R>> = Arc::new(move |x1: &T1, x2: &T2| {
            let f = f.clone();
            let x1 = x1.clone();
            let x2 = x2.clone();
            let inner: Fn2<T3, T4, R> = Arc::new(move |y1: &T3, y2: &T4| f(&x1, &x2, y1, y2));
            inner
        });
        curried_fn
    });
    let step1 = ap2(curried, a, b);
    ap2(step1, c, d)
}

/// `Applicative.super.ap5` — Java's default, chaining `ap4`:
/// `ap2(ap2(map(Function5::curry2, func), t1, t2), t3, ap2(t4, t5))` — in the
/// Rust port the inner `ap4` yields a two-element `Fn2` for `(t1..t4)` whose
/// remaining pair `(t4, t5)` still has the last two arguments to apply.
#[allow(clippy::type_complexity)] // nested `Fn2<_, _, Fn3<...>>` mirror Java's `Function` curry
pub fn ap5<
    T1: Clone + Send + Sync + 'static,
    T2: Clone + Send + Sync + 'static,
    T3: Clone + Send + Sync + 'static,
    T4: Clone + Send + Sync + 'static,
    T5: Clone + Send + Sync + 'static,
    R: 'static,
>(
    fr: DataResult<Fn5<T1, T2, T3, T4, T5, R>>,
    a: DataResult<T1>,
    b: DataResult<T2>,
    c: DataResult<T3>,
    d: DataResult<T4>,
    e: DataResult<T5>,
) -> DataResult<R> {
    // `curry2` on the 5-arg function: `(t1, t2) -> (t3, t4, t5) -> f(t1..t5)`.
    let curried: DataResult<Fn2<T1, T2, Fn3<T3, T4, T5, R>>> = fr.map(|f| {
        let f = f.clone();
        let curried_fn: Fn2<T1, T2, Fn3<T3, T4, T5, R>> = Arc::new(move |x1: &T1, x2: &T2| {
            let f = f.clone();
            let x1 = x1.clone();
            let x2 = x2.clone();
            let inner: Fn3<T3, T4, T5, R> =
                Arc::new(move |y1: &T3, y2: &T4, y3: &T5| f(&x1, &x2, y1, y2, y3));
            inner
        });
        curried_fn
    });
    let step1 = ap2(curried, a, b);
    ap3(step1, c, d, e)
}

/// `Applicative.super.ap6` — Java's default, chaining `ap2`/`ap3`:
/// `ap3(ap3(map(Function6::curry2, func), t1, t2, t3), t4, t5, t6)` — in the
/// Rust port the inner `ap3` yields a three-element `Fn3` for `(t1..t3)` whose
/// remaining triple `(t4, t5, t6)` still has the last three arguments to apply.
#[allow(clippy::type_complexity)] // nested `Fn3<_, _, _, Fn3<...>>` mirror Java's `Function` curry
pub fn ap6<
    T1: Clone + Send + Sync + 'static,
    T2: Clone + Send + Sync + 'static,
    T3: Clone + Send + Sync + 'static,
    T4: Clone + Send + Sync + 'static,
    T5: Clone + Send + Sync + 'static,
    T6: Clone + Send + Sync + 'static,
    R: 'static,
>(
    fr: DataResult<Fn6<T1, T2, T3, T4, T5, T6, R>>,
    a: DataResult<T1>,
    b: DataResult<T2>,
    c: DataResult<T3>,
    d: DataResult<T4>,
    e: DataResult<T5>,
    f: DataResult<T6>,
) -> DataResult<R> {
    // `curry2` on the 6-arg function: `(t1, t2, t3) -> (t4, t5, t6) -> f(t1..t6)`.
    let curried: DataResult<Fn3<T1, T2, T3, Fn3<T4, T5, T6, R>>> = fr.map(|func| {
        let func = func.clone();
        let curried_fn: Fn3<T1, T2, T3, Fn3<T4, T5, T6, R>> =
            Arc::new(move |x1: &T1, x2: &T2, x3: &T3| {
                let func = func.clone();
                let x1 = x1.clone();
                let x2 = x2.clone();
                let x3 = x3.clone();
                let inner: Fn3<T4, T5, T6, R> =
                    Arc::new(move |y1: &T4, y2: &T5, y3: &T6| func(&x1, &x2, &x3, y1, y2, y3));
                inner
            });
        curried_fn
    });
    let step1 = ap3(curried, a, b, c);
    ap3(step1, d, e, f)
}

/// `DataResult.appendMessages(String, String)`.
pub fn append_messages(first: &str, second: &str) -> String {
    format!("{}; {}", first, second)
}

impl<T> From<T> for DataResult<T> {
    fn from(value: T) -> Self {
        DataResult::success(value)
    }
}

impl<T: fmt::Display> fmt::Display for DataResult<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Java `Success.toString()` = "DataResult.Success[" + value + "]";
        // `Error.toString()` = "DataResult.Error['" + message + "'" + (partial
        // present ? ": " + value : "") + "]".
        match &self.value {
            DataResultValue::Success(v) => write!(f, "DataResult.Success[{}]", v),
            DataResultValue::Error {
                message,
                partial: Some(p),
            } => write!(f, "DataResult.Error['{}': {}]", message, p),
            DataResultValue::Error {
                message,
                partial: None,
            } => write!(f, "DataResult.Error['{}']", message),
        }
    }
}
