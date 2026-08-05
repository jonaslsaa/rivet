//! Port of `com.mojang.datafixers.util.Either`.
//!
//! Java `Either<L, R>` is an abstract class with private `Left`/`Right`
//! subclasses; a Rust `enum` preserves the sum shape. The `Applicative`
//! `Instance` and `CocartesianLike`/`Traversable` kinds are not part of the
//! serialization surface and are omitted.

/// `com.mojang.datafixers.util.Either<L, R>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Either<L, R> {
    Left(L),
    Right(R),
}

impl<L, R> Either<L, R> {
    /// `Either.left(value)`.
    pub fn left(value: L) -> Either<L, R> {
        Either::Left(value)
    }

    /// `Either.right(value)`.
    pub fn right(value: R) -> Either<L, R> {
        Either::Right(value)
    }

    /// `Either.map(Function, Function)` — folds to one value.
    pub fn map<T>(self, l: impl FnOnce(L) -> T, r: impl FnOnce(R) -> T) -> T {
        match self {
            Either::Left(v) => l(v),
            Either::Right(v) => r(v),
        }
    }

    /// `Either.mapBoth(Function, Function)`.
    pub fn map_both<C, D>(self, f1: impl FnOnce(L) -> C, f2: impl FnOnce(R) -> D) -> Either<C, D> {
        match self {
            Either::Left(v) => Either::Left(f1(v)),
            Either::Right(v) => Either::Right(f2(v)),
        }
    }

    /// Borrowed `map` — used by `EitherCodec`/`XorCodec` encode, which receive
    /// `&Either<F, S>`.
    pub fn map_ref<T>(&self, l: impl FnOnce(&L) -> T, r: impl FnOnce(&R) -> T) -> T {
        match self {
            Either::Left(v) => l(v),
            Either::Right(v) => r(v),
        }
    }

    /// `Either.ifLeft(Consumer)` — the consumer receives the value by
    /// reference (consuming it would leave the returned `Either` partially
    /// moved).
    pub fn if_left(self, consumer: impl FnOnce(&L)) -> Either<L, R> {
        if let Either::Left(v) = &self {
            consumer(v);
        }
        self
    }

    /// `Either.ifRight(Consumer)` — the consumer receives the value by
    /// reference.
    pub fn if_right(self, consumer: impl FnOnce(&R)) -> Either<L, R> {
        if let Either::Right(v) = &self {
            consumer(v);
        }
        self
    }

    /// `Either.left()`.
    pub fn left_opt(&self) -> Option<&L> {
        match self {
            Either::Left(v) => Some(v),
            Either::Right(_) => None,
        }
    }

    /// `Either.right()`.
    pub fn right_opt(&self) -> Option<&R> {
        match self {
            Either::Left(_) => None,
            Either::Right(v) => Some(v),
        }
    }

    /// `Either.mapLeft(Function)`.
    pub fn map_left<T>(self, l: impl FnOnce(L) -> T) -> Either<T, R> {
        self.map(|v| Either::left(l(v)), Either::right)
    }

    /// `Either.mapRight(Function)`.
    pub fn map_right<T>(self, r: impl FnOnce(R) -> T) -> Either<L, T> {
        self.map(Either::left, |v| Either::right(r(v)))
    }

    /// `Either.orThrow()` — panics when right (Java: throws the right value or
    /// a `RuntimeException` of its `toString`).
    pub fn or_throw(self) -> L
    where
        R: std::fmt::Display,
    {
        self.map(|l| l, |r| panic!("{}", r))
    }

    /// `Either.swap()`.
    pub fn swap(self) -> Either<R, L> {
        self.map(Either::right, Either::left)
    }

    /// `Either.flatMap(Function)` — maps the left side only.
    pub fn flat_map<L2>(self, function: impl FnOnce(L) -> Either<L2, R>) -> Either<L2, R> {
        self.map(function, Either::right)
    }

    /// `Either.unwrap(Either<? extends U, ? extends U>)`.
    pub fn unwrap<U>(either: Either<U, U>) -> U {
        either.map(|u| u, |u| u)
    }

    /// Borrowed `unwrap` used by `Codec.withAlternative`'s xmap `to` closure
    /// (which receives `&Either<A, A>`).
    pub fn unwrap_ref<U>(either: &Either<U, U>) -> &U {
        match either {
            Either::Left(u) => u,
            Either::Right(u) => u,
        }
    }
}
