//! Port of `com.mojang.serialization.Lifecycle`.
//!
//! A monoidal lifecycle marker carried by `DataResult`s. `add` follows the
//! Java reference semantics: experimental wins, then the older `Deprecated`.

/// `com.mojang.serialization.Lifecycle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// `Lifecycle.stable()` — the private `STABLE` singleton.
    Stable,
    /// `Lifecycle.experimental()` — the private `EXPERIMENTAL` singleton.
    Experimental,
    /// `Lifecycle.deprecated(int since)`.
    Deprecated(i32),
}

impl Lifecycle {
    pub fn experimental() -> Self {
        Lifecycle::Experimental
    }

    pub fn stable() -> Self {
        Lifecycle::Stable
    }

    pub fn deprecated(since: i32) -> Self {
        Lifecycle::Deprecated(since)
    }

    /// `Lifecycle.add(Lifecycle other)`.
    ///
    /// ```java
    /// if (this == EXPERIMENTAL || other == EXPERIMENTAL) return EXPERIMENTAL;
    /// if (this instanceof Deprecated d) {
    ///     if (other instanceof Deprecated od && od.since < d.since) return other;
    ///     return this;
    /// }
    /// if (other instanceof Deprecated) return other;
    /// return STABLE;
    /// ```
    pub fn add(&self, other: Lifecycle) -> Lifecycle {
        match (*self, other) {
            (Lifecycle::Experimental, _) | (_, Lifecycle::Experimental) => Lifecycle::Experimental,
            (Lifecycle::Deprecated(this_since), Lifecycle::Deprecated(other_since)) => {
                if other_since < this_since {
                    Lifecycle::Deprecated(other_since)
                } else {
                    *self
                }
            }
            (Lifecycle::Deprecated(_), _) => *self,
            (Lifecycle::Stable, Lifecycle::Deprecated(_)) => other,
            (Lifecycle::Stable, Lifecycle::Stable) => Lifecycle::Stable,
        }
    }
}

impl std::fmt::Display for Lifecycle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lifecycle::Stable => write!(f, "Stable"),
            Lifecycle::Experimental => write!(f, "Experimental"),
            Lifecycle::Deprecated(since) => write!(f, "Deprecated({})", since),
        }
    }
}
