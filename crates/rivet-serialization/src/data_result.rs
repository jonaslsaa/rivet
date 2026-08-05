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
//! associated functions. Multi-argument functions are `Arc<dyn Fn>` so the
//! curried fallback of `ap2`/`ap3` (which applies the function to partial
//! values) can own its captured values (`Box<dyn Fn>` is not cloneable).

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
                message: e,
                partial: Some(p),
            } => p,
            DataResultValue::Error {
                message: e,
                partial: None,
            } => panic!("{}: {}", message.into(), e),
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
    /// and to the partial value of an error, concatenating messages when both
    /// are errors (the accumulate-and-retry semantics).
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
                    DataResultValue::Success(v) => DataResult::success_with_lifecycle(v, combined),
                    DataResultValue::Error {
                        message: m2,
                        partial: p2,
                    } => DataResult::error_raw(append_messages(&message, &m2), p2, combined),
                }
            }
        }
    }

    /// `DataResult.ap(DataResult<Function<R, R2>>)`.
    pub fn ap<U>(self, function_result: DataResult<Arc<dyn Fn(&T) -> U>>) -> DataResult<U> {
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
        function: impl Fn(&T, &R2) -> S + 'static,
        second: DataResult<R2>,
    ) -> DataResult<S>
    where
        T: Clone + 'static,
        R2: Clone + 'static,
        S: 'static,
    {
        let fr: DataResult<Arc<dyn Fn(&T, &R2) -> S>> =
            DataResult::success_with_lifecycle(Arc::new(function), Lifecycle::experimental());
        ap2(fr, self, second)
    }

    /// `DataResult.apply2stable(BiFunction, DataResult<R2>)`.
    pub fn apply2_stable<R2, S>(
        self,
        function: impl Fn(&T, &R2) -> S + 'static,
        second: DataResult<R2>,
    ) -> DataResult<S>
    where
        T: Clone + 'static,
        R2: Clone + 'static,
        S: 'static,
    {
        let fr: DataResult<Arc<dyn Fn(&T, &R2) -> S>> =
            DataResult::success_with_lifecycle(Arc::new(function), Lifecycle::stable());
        ap2(fr, self, second)
    }

    /// `DataResult.apply3(Function3, DataResult<R2>, DataResult<R3>)`.
    pub fn apply3<R2, R3, S>(
        self,
        function: impl Fn(&T, &R2, &R3) -> S + 'static,
        second: DataResult<R2>,
        third: DataResult<R3>,
    ) -> DataResult<S>
    where
        T: Clone + 'static,
        R2: Clone + 'static,
        R3: Clone + 'static,
        S: 'static,
    {
        let fr: DataResult<Arc<dyn Fn(&T, &R2, &R3) -> S>> =
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
pub fn ap2<T: Clone + 'static, R2: Clone + 'static, S: 'static>(
    fr: DataResult<Arc<dyn Fn(&T, &R2) -> S>>,
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
    let curried: DataResult<Arc<dyn Fn(&T) -> Arc<dyn Fn(&R2) -> S>>> = fr.map(|f| {
        let f = f.clone();
        let curried_fn: Arc<dyn Fn(&T) -> Arc<dyn Fn(&R2) -> S>> = Arc::new(move |x: &T| {
            let f = f.clone();
            let x = x.clone();
            let inner: Arc<dyn Fn(&R2) -> S> = Arc::new(move |y: &R2| f(&x, y));
            inner
        });
        curried_fn
    });
    let step1 = a.ap(curried);
    b.ap(step1)
}

/// `DataResult.INSTANCE.ap3` — fast path when all four results are present,
/// otherwise `ap2(ap(map(Function3.curry, func), t1), t2, t3)`.
pub fn ap3<T: Clone + 'static, R2: Clone + 'static, R3: Clone + 'static, S: 'static>(
    fr: DataResult<Arc<dyn Fn(&T, &R2, &R3) -> S>>,
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
    let curried: DataResult<Arc<dyn Fn(&T) -> Arc<dyn Fn(&R2, &R3) -> S>>> = fr.map(|f| {
        let f = f.clone();
        let curried_fn: Arc<dyn Fn(&T) -> Arc<dyn Fn(&R2, &R3) -> S>> = Arc::new(move |x: &T| {
            let f = f.clone();
            let x = x.clone();
            let inner: Arc<dyn Fn(&R2, &R3) -> S> = Arc::new(move |y: &R2, z: &R3| f(&x, y, z));
            inner
        });
        curried_fn
    });
    let step1 = a.ap(curried);
    ap2(step1, b, c)
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
        match &self.value {
            DataResultValue::Success(v) => write!(f, "Success[{}]", v),
            DataResultValue::Error {
                message,
                partial: Some(p),
            } => write!(f, "Error['{}': {}]", message, p),
            DataResultValue::Error {
                message,
                partial: None,
            } => write!(f, "Error['{}']", message),
        }
    }
}
