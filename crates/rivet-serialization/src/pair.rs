//! Port of `com.mojang.datafixers.util.Pair`.

/// `com.mojang.datafixers.util.Pair<F, S>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair<F, S> {
    pub first: F,
    pub second: S,
}

impl<F, S> Pair<F, S> {
    pub fn of(first: F, second: S) -> Self {
        Pair { first, second }
    }

    /// `Pair.getFirst()`.
    pub fn get_first(&self) -> &F {
        &self.first
    }

    /// `Pair.getSecond()`.
    pub fn get_second(&self) -> &S {
        &self.second
    }

    /// `Pair.swap()`.
    pub fn swap(self) -> Pair<S, F> {
        Pair::of(self.second, self.first)
    }

    /// `Pair.mapFirst(Function)`.
    pub fn map_first<F2>(self, function: impl FnOnce(F) -> F2) -> Pair<F2, S> {
        Pair::of(function(self.first), self.second)
    }

    /// `Pair.mapSecond(Function)`.
    pub fn map_second<S2>(self, function: impl FnOnce(S) -> S2) -> Pair<F, S2> {
        Pair::of(self.first, function(self.second))
    }
}

impl<F, S> Pair<F, S> {
    /// `Pair.mapFirst(Function)` over borrowed values (used by `decode` chains).
    pub fn map_first_ref<F2>(&self, function: impl FnOnce(&F) -> F2) -> Pair<F2, S>
    where
        S: Clone,
    {
        Pair::of(function(&self.first), self.second.clone())
    }
}

impl<F, S> std::fmt::Display for Pair<F, S>
where
    F: std::fmt::Display,
    S: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.first, self.second)
    }
}
