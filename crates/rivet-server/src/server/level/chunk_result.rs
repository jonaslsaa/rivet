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
//! error is a lazily-evaluated supplier, faithful to Java's `Supplier<String>`:
//! `getError()` evaluates it on each call, and `map` carries the same supplier
//! across (Java shares the reference). The supplier is `dyn Fn() -> String +
//! Send + Sync` because the pipeline passes `ChunkResult` through async
//! scheduler seams (CompletableFuture chains, chunk-task dispatchers) and holds
//! static unloaded-chunk constants (Paper's `ChunkMap.UNLOADED_CHUNK_LIST_RESULT`
//! / `GenerationChunkHolder.UNLOADED_CHUNK`) — the boxed closure must cross
//! thread boundaries.
//!
//! **`Success(null)`.** Java's `ChunkResult.of(T)` accepts a `@Nullable T`, so
//! `Success(null)` is legal, and the static `orElse` falls through to the
//! fallback when a `Success` holds null. Rust models this with the value type
//! `Option<R>`: `ChunkResult<Option<R>>::Success(None)` *is* Java's
//! `Success(null)`. The faithful static null-through is `or_else_nullable`
//! (Java's static `orElse`: `Success(Some(r))` → `r`, `Success(None)` / `Fail`
//! → the fallback). The instance `or_else` is the concrete-value form (it
//! wraps the `Success` value in `Option<T>` and returns the fallback on
//! `Fail`); when `T = Option<R>` it yields a nested `Option<Option<R>>`, so
//! Java's null-through on the *instance* form is intentionally not reproduced —
//! the pipeline uses `or_else_nullable` for that. The non-null static
//! `or_else_value` is the `Success(T)`-only form, used when the pipeline holds
//! a concrete value and the fallback is non-null.

/// `ChunkResult<T>` — `Success(T)` or `Fail(error supplier)`.
pub enum ChunkResult<T> {
    Success(T),
    Fail {
        error: Box<dyn Fn() -> String + Send + Sync>,
    },
}

impl<T> ChunkResult<T> {
    /// `ChunkResult.of(T value)` — `new ChunkResult.Success<>(value)`. `const`
    /// so a `Success` result can be a `static` item (the pipeline's unloaded
    /// chunk results; Paper's `error` statics need `LazyLock` in Rust — see
    /// the `Send`/`Sync` test).
    ///
    /// Java's `of(@Nullable T)` also permits `Success(null)`; pass the value as
    /// `Option<R>` to represent nullability (`of(None::<R>)` is Java's
    /// `Success(null)`) — see the module doc and `or_else_nullable`.
    pub const fn of(value: T) -> Self {
        Self::Success(value)
    }

    /// `ChunkResult.error(String)` — the eager form; the message is captured
    /// and returned lazily by `getError` (Java wraps the eager `String` in
    /// `() -> error`). The clone on each call models Java's re-read of the
    /// same message; a `Fn` closure cannot move the captured `String` out.
    pub fn error(message: impl Into<String> + Send + Sync + 'static) -> Self {
        let message = message.into();
        Self::error_lazy(move || message.clone())
    }

    /// `ChunkResult.error(Supplier<String>)` — the deferred form; the supplier
    /// runs on each `getError` call.
    pub fn error_lazy(error: impl Fn() -> String + Send + Sync + 'static) -> Self {
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

    /// Static `ChunkResult.orElse(result, orElse)` over a nullable value type:
    /// Java's exact null-through semantics. `result` is a `ChunkResult<Option<R>>`
    /// — `Success(None)` is Java's `Success(null)` — and `orElse` is the
    /// `@Nullable` fallback. Returns `r` for `Success(Some(r))`, and `or_else`
    /// for both `Success(None)` (the null-through branch) and `Fail`.
    pub fn or_else_nullable<R>(result: Self, or_else: R) -> R
    where
        T: Into<Option<R>>,
    {
        match result {
            Self::Success(value) => match value.into() {
                Some(r) => r,
                None => or_else,
            },
            Self::Fail { .. } => or_else,
        }
    }

    /// Static `ChunkResult.orElse(result, orElse)` — the value on `Success`,
    /// the fallback on `Fail`. This is the non-null form (the value type is a
    /// concrete `T`); the nullable form is `or_else_nullable`.
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
    use std::sync::LazyLock;

    /// The boxed supplier must cross async scheduler seams: `ChunkResult` is
    /// `Send + Sync` when `T` is. Paper's static unloaded-chunk results are
    /// `ChunkResult.error(...)` (Fail with a supplier); in Rust a heap-allocated
    /// `Box<dyn Fn>` static needs `LazyLock` (Java's `static final`), while
    /// `Success` statics work directly via `const of`.
    #[test]
    fn result_is_send_sync_and_static_construable() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ChunkResult<i32>>();

        // A Success result is a plain `const` static.
        static LOADED: ChunkResult<i32> = ChunkResult::of(7);
        assert!(LOADED.is_success());
        assert_eq!(LOADED.get_error(), None);

        // Paper's unloaded-chunk error constants (ChunkMap.
        // UNLOADED_CHUNK_LIST_RESULT, GenerationChunkHolder.UNLOADED_CHUNK) are
        // Fail-with-supplier; the heap allocation requires LazyLock.
        static UNLOADED: LazyLock<ChunkResult<i32>> =
            LazyLock::new(|| ChunkResult::error("unloaded chunk"));
        assert!(!UNLOADED.is_success());
        assert_eq!(UNLOADED.get_error(), Some("unloaded chunk".to_string()));
    }

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
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        // Java's `Supplier.get()` may have side effects; the `Fn` supplier
        // models that with interior mutability (`AtomicUsize`), and each
        // `getError` re-runs it, so the counter advances. The atomic also
        // keeps the closure `Send + Sync`, like the real pipeline's.
        let calls = Arc::new(AtomicUsize::new(0));
        let err = {
            let calls = Arc::clone(&calls);
            ChunkResult::<i32>::error_lazy(move || {
                let n = calls.fetch_add(1, Ordering::Relaxed) + 1;
                format!("call {n}")
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

    /// Java's static `orElse` falls through to the fallback when a `Success`
    /// holds null. `ChunkResult<Option<R>>::Success(None)` is that
    /// `Success(null)`; `or_else_nullable` reproduces the null-through exactly.
    #[test]
    fn or_else_nullable_models_java_success_null_through() {
        // Success(Some(r)) -> r.
        assert_eq!(
            ChunkResult::or_else_nullable(ChunkResult::of(Some(5)), 99),
            5
        );
        // Success(None) is Java's Success(null) -> the fallback.
        assert_eq!(
            ChunkResult::or_else_nullable(ChunkResult::of(None::<i32>), 99),
            99
        );
        // Fail -> the fallback.
        assert_eq!(
            ChunkResult::or_else_nullable::<i32>(ChunkResult::<Option<i32>>::error("x"), 99),
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
