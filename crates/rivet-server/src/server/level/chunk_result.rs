//! Port of `net.minecraft.server.level.ChunkResult` (MC 26.2, Paper) — the
//! success/failure result carrier the pipeline uses for chunk operations.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/level/ChunkResult.java`.
//!
//! Owned by the `mc.server.level.pipeline.level` manifest unit (#185).
//!
//! Java models this as an interface with two records (`Success(T value)` and
//! `Fail(Supplier<String> error)`); Rust models it as an enum. The `Fail`
//! error is a lazily-evaluated supplier (`Box<dyn Fn() -> String>`), faithful
//! to Java's `Supplier<String>`: `getError()` evaluates it on each call, and
//! `map` carries the same supplier across (Java shares the reference). Java's
//! `@Nullable` value is modeled with `Option<T>` at the call sites that need
//! it — `Success(T)` cannot hold null, so the static `orElse`'s null-through
//! branch cannot arise.

/// `ChunkResult<T>` — `Success(T)` or `Fail(error supplier)`.
pub enum ChunkResult<T> {
    Success(T),
    Fail { error: Box<dyn Fn() -> String> },
}

impl<T> ChunkResult<T> {
    /// `ChunkResult.of(T value)` — `new ChunkResult.Success<>(value)`.
    pub fn of(value: T) -> Self {
        Self::Success(value)
    }

    /// `ChunkResult.error(String)` — the eager form; the message is captured
    /// and returned lazily by `getError` (Java wraps the eager `String` in
    /// `() -> error`).
    pub fn error(message: impl Into<String>) -> Self {
        let message = message.into();
        Self::Fail {
            error: Box::new(move || message.clone()),
        }
    }

    /// `ChunkResult.error(Supplier<String>)` — the deferred form; the supplier
    /// runs on each `getError` call.
    pub fn error_lazy(error: impl Fn() -> String + 'static) -> Self {
        Self::Fail {
            error: Box::new(error),
        }
    }

    /// `ChunkResult.isSuccess()`.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Success(_))
    }

    /// `ChunkResult.orElse(@Nullable T orElse)` — the value on `Success`, the
    /// fallback on `Fail`. The `@Nullable` fallback is modeled as
    /// `Option<T>`: `None` plays Java's `null` fallback, `Some(t)` the non-null
    /// fallback.
    pub fn or_else(self, or_else: Option<T>) -> Option<T> {
        match self {
            Self::Success(value) => Some(value),
            Self::Fail { .. } => or_else,
        }
    }

    /// Static `ChunkResult.orElse(result, orElse)` — the value on `Success`,
    /// the fallback on `Fail`. Java's null-through case (`Success` holding a
    /// null value → fallback) cannot arise: Rust `Success(T)` cannot hold
    /// null, so the `result != null` test always passes for a `Success`.
    pub fn or_else_value(result: Self, or_else: T) -> T {
        match result {
            Self::Success(value) => value,
            Self::Fail { .. } => or_else,
        }
    }

    /// `ChunkResult.getError()` — `None` on `Success`, the evaluated supplier
    /// on `Fail` (Java returns `@Nullable String`, allocating a fresh string
    /// from the supplier each call).
    pub fn get_error(&self) -> Option<String> {
        match self {
            Self::Success(_) => None,
            Self::Fail { error } => Some(error()),
        }
    }

    /// `ChunkResult.ifSuccess(Consumer<T>)` — runs the consumer on the value
    /// and returns `this` unchanged (Java's `consumer.accept(this.value);
    /// return this;`).
    pub fn if_success(&self, consumer: impl FnOnce(&T)) -> &Self {
        if let Self::Success(value) = self {
            consumer(value);
        }
        self
    }

    /// `ChunkResult.map(Function<T, R>)` — `new Success<>(map(value))` on
    /// `Success`; `new Fail(this.error)` on `Fail`, carrying the same error
    /// supplier across (Java shares the reference).
    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> ChunkResult<U> {
        match self {
            Self::Success(value) => ChunkResult::Success(map(value)),
            Self::Fail { error } => ChunkResult::Fail { error },
        }
    }

    /// `ChunkResult.orElseThrow(Supplier<E>)` — the value on `Success`, the
    /// supplied error on `Fail`. Java's `throws E` is modeled as `Result<T, E>`.
    pub fn or_else_throw<E>(self, exception_supplier: impl FnOnce() -> E) -> Result<T, E> {
        match self {
            Self::Success(value) => Ok(value),
            Self::Fail { .. } => Err(exception_supplier()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn of_is_success_and_carries_the_value() {
        let ok = ChunkResult::of(42);
        assert!(ok.is_success());
        assert_eq!(ok.get_error(), None);
        // `orElse` consumes; inspect before, or consume last.
        assert_eq!(ok.or_else(None), Some(42));
    }

    #[test]
    fn error_eager_message_is_returned_lazily() {
        let err = ChunkResult::<i32>::error("boom");
        assert!(!err.is_success());
        // `getError` evaluates the supplier each call, like Java.
        assert_eq!(err.get_error(), Some("boom".to_string()));
        assert_eq!(err.get_error(), Some("boom".to_string()));
        // `orElse` consumes; the fallback is returned on Fail.
        assert_eq!(err.or_else(None), None);
        let err2 = ChunkResult::<i32>::error("boom");
        assert_eq!(err2.or_else(Some(7)), Some(7));
    }

    #[test]
    fn error_lazy_defers_the_supplier() {
        use std::cell::Cell;
        use std::rc::Rc;
        // Java's `Supplier.get()` may have side effects; the `Fn` supplier
        // models that with interior mutability (`Cell`), and each `getError`
        // re-runs it, so the counter advances.
        let calls = Rc::new(Cell::new(0));
        let err = {
            let calls = Rc::clone(&calls);
            ChunkResult::<i32>::error_lazy(move || {
                calls.set(calls.get() + 1);
                format!("call {}", calls.get())
            })
        };
        assert!(!err.is_success());
        assert_eq!(err.get_error(), Some("call 1".to_string()));
        assert_eq!(err.get_error(), Some("call 2".to_string()));
    }

    #[test]
    fn or_else_value_returns_value_or_fallback() {
        assert_eq!(ChunkResult::or_else_value(ChunkResult::of(5), 99), 5);
        assert_eq!(
            ChunkResult::or_else_value(ChunkResult::<i32>::error("x"), 99),
            99
        );
    }

    #[test]
    fn if_success_runs_only_on_success_and_returns_this() {
        let mut seen = Vec::new();
        let ok = ChunkResult::of(3);
        let returned = ok.if_success(|v| seen.push(*v));
        assert_eq!(seen, vec![3]);
        // `if_success` returns `this` — the original is still a success.
        assert!(returned.is_success());

        let mut not_called = true;
        let err = ChunkResult::<i32>::error("x");
        err.if_success(|_| not_called = false);
        assert!(not_called, "if_success must not run on Fail");
    }

    #[test]
    fn map_transforms_success_and_carries_the_error() {
        assert_eq!(ChunkResult::of(2).map(|v| v * 10).or_else(None), Some(20));
        let err = ChunkResult::<i32>::error("still failing");
        let mapped: ChunkResult<String> = err.map(|v| format!("{v}"));
        assert!(!mapped.is_success());
        assert_eq!(mapped.get_error(), Some("still failing".to_string()));
    }

    #[test]
    fn or_else_throw_is_ok_on_success_and_err_on_fail() {
        assert_eq!(ChunkResult::of(1).or_else_throw(|| "never"), Ok(1));
        assert_eq!(
            ChunkResult::<i32>::error("bad").or_else_throw(|| "thrown"),
            Err("thrown")
        );
    }
}
