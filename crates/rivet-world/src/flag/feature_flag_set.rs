//! `net.minecraft.world.flag.FeatureFlagSet` — the immutable flag set value.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/
//! FeatureFlagSet.java`. A `final` value class over `(@Nullable
//! FeatureFlagUniverse universe, long mask)`. Java's `EMPTY` is `(null, 0L)`;
//! the port models the nullable universe as a `FeatureFlagSet` with
//! `universe: None`.
//!
//! Semantics preserved exactly (PORTING.md):
//! - `contains(flag)` — `universe == flag.universe && (mask & flag.mask) != 0`.
//! - `isEmpty` — equals `EMPTY`.
//! - `isSubsetOf` — `universe == null || universe == set.universe && (mask &
//!   ~set.mask) == 0`.
//! - `intersects` — both non-null, same universe, `(mask & set.mask) != 0`.
//! - `join` / `subtract` — mismatched (both non-null, different) universes
//!   panic with Java's exact message; `subtract` with a result of `0` returns
//!   `EMPTY`.
//! - `equals`/`hashCode` — structural `(universe, mask)`; Java's
//!   `HashCommon.mix(mask)` is ported for the mask half.
//!
//! Java's universe *reference* identity (`==`) is preserved via the universe's
//! `Arc` pointer identity (see `feature_flag_universe`): every `Builder` owns a
//! fresh universe allocation, and `clone()` shares it, so flags from one
//! builder are compatible and flags from two same-id builders are not — exactly
//! Java's `==`.
//!
//! The Java `create(universe, Collection<FeatureFlag>)` / `of(flag, ...)`
//! factories are the `from_*` constructors; the registry uses them to build
//! the empty set (an empty collection returns `EMPTY`, so the `universe` is
//! dropped — matching Java).

use super::feature_flag::FeatureFlag;
use super::feature_flag_universe::FeatureFlagUniverse;

/// `FeatureFlagSet.EMPTY` — the shared `(null, 0L)` empty set. Java's `of()`
/// returns this singleton; the port returns a fresh value (value semantics).
pub fn empty() -> FeatureFlagSet {
    FeatureFlagSet {
        universe: None,
        mask: 0,
    }
}

/// `FeatureFlagSet` — the immutable `(@Nullable universe, mask)` value.
///
/// Not `Copy`: the universe owns a `String`, so the set is `Clone`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FeatureFlagSet {
    universe: Option<FeatureFlagUniverse>,
    mask: u64,
}

impl FeatureFlagSet {
    /// `FeatureFlagSet.MAX_CONTAINER_SIZE` — `64`.
    pub const MAX_CONTAINER_SIZE: usize = 64;

    /// `FeatureFlagSet.of()` — `EMPTY`.
    pub fn of() -> Self {
        empty()
    }

    /// `FeatureFlagSet.of(FeatureFlag)` — `new FeatureFlagSet(flag.universe,
    /// flag.mask)`.
    pub fn of_flag(flag: &FeatureFlag) -> Self {
        FeatureFlagSet {
            universe: Some(flag.universe().clone()),
            mask: flag.mask(),
        }
    }

    /// `FeatureFlagSet.of(FeatureFlag, FeatureFlag...)` — the vararg factory
    /// (the `computeMask` path, so a mismatched-universe flag panics).
    pub fn of_flags(first: &FeatureFlag, rest: &[&FeatureFlag]) -> Self {
        let mut mask = first.mask();
        for flag in rest {
            if first.universe() != flag.universe() {
                panic!(
                    "Mismatched feature universe, expected '{}', but got '{}'",
                    first.universe(),
                    flag.universe()
                );
            }
            mask |= flag.mask();
        }
        FeatureFlagSet {
            universe: Some(first.universe().clone()),
            mask,
        }
    }

    /// `FeatureFlagSet.create(universe, Collection<FeatureFlag>)` — an empty
    /// collection yields `EMPTY` (universe dropped); otherwise the OR of every
    /// flag's mask with the mismatched-universe check.
    pub(crate) fn create(
        universe: &FeatureFlagUniverse,
        flags: impl IntoIterator<Item = FeatureFlag>,
    ) -> Self {
        let mut mask = 0u64;
        let mut any = false;
        for flag in flags {
            if universe != flag.universe() {
                panic!(
                    "Mismatched feature universe, expected '{}', but got '{}'",
                    universe,
                    flag.universe()
                );
            }
            mask |= flag.mask();
            any = true;
        }
        if !any {
            empty()
        } else {
            FeatureFlagSet {
                universe: Some(universe.clone()),
                mask,
            }
        }
    }

    /// `FeatureFlagSet.contains(FeatureFlag)`.
    pub fn contains(&self, flag: &FeatureFlag) -> bool {
        self.universe.as_ref() == Some(flag.universe()) && (self.mask & flag.mask()) != 0
    }

    /// `FeatureFlagSet.isEmpty()` — `this.equals(EMPTY)`.
    pub fn is_empty(&self) -> bool {
        self.universe.is_none() && self.mask == 0
    }

    /// `FeatureFlagSet.isSubsetOf(FeatureFlagSet)` — an empty (null-universe)
    /// set is a subset of everything; otherwise same-universe AND no bits
    /// outside the other set.
    pub fn is_subset_of(&self, set: &FeatureFlagSet) -> bool {
        self.universe.is_none() || (self.universe == set.universe && (self.mask & !set.mask) == 0)
    }

    /// `FeatureFlagSet.intersects(FeatureFlagSet)` — both non-null, same
    /// universe, non-zero shared mask.
    pub fn intersects(&self, set: &FeatureFlagSet) -> bool {
        self.universe.is_some()
            && set.universe.is_some()
            && self.universe == set.universe
            && (self.mask & set.mask) != 0
    }

    /// `FeatureFlagSet.join(FeatureFlagSet)` — null-universe shortcuts, else
    /// mismatched-universe `IllegalArgumentException`, else the OR.
    pub fn join(&self, other: &FeatureFlagSet) -> FeatureFlagSet {
        match (&self.universe, &other.universe) {
            // `EMPTY.join(x)` = `x`; `x.join(EMPTY)` = `x`.
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(u), Some(other_u)) => {
                if u != other_u {
                    panic!("Mismatched set elements: '{}' != '{}'", u, other_u);
                }
                FeatureFlagSet {
                    universe: self.universe.clone(),
                    mask: self.mask | other.mask,
                }
            }
        }
    }

    /// `FeatureFlagSet.subtract(FeatureFlagSet)` — null shortcuts, else the
    /// `mask & ~other.mask`, returning `EMPTY` when it clears.
    pub fn subtract(&self, other: &FeatureFlagSet) -> FeatureFlagSet {
        match (&self.universe, &other.universe) {
            // A null universe (EMPTY) on either side leaves `self` unchanged.
            (None, _) => self.clone(),
            (_, None) => self.clone(),
            (Some(u), Some(other_u)) => {
                if u != other_u {
                    panic!("Mismatched set elements: '{}' != '{}'", u, other_u);
                }
                let new_mask = self.mask & !other.mask;
                if new_mask == 0 {
                    empty()
                } else {
                    FeatureFlagSet {
                        universe: self.universe.clone(),
                        mask: new_mask,
                    }
                }
            }
        }
    }

    /// `FeatureFlagSet.universe()` — `@Nullable`: `None` for the empty set.
    pub fn universe(&self) -> Option<&FeatureFlagUniverse> {
        self.universe.as_ref()
    }

    /// The raw `mask` (Java's private `long mask`). Exposed for the registry's
    /// `allFlags` bookkeeping and the codec's `to_names` iteration.
    pub fn mask(&self) -> u64 {
        self.mask
    }
}

/// `HashCommon.mix(long)` (fastutil 8.5.18) — the mask half of Java's
/// `FeatureFlagSet.hashCode`.
///
/// `FeatureFlagSet.hashCode` is `(int)HashCommon.mix(mask)` — fastutil's
/// `mix(long)`, verified against the pinned 8.5.18 jar's bytecode:
/// `h = x * 0x9E3779B97F4A7C15; h ^= h >>> 32; h ^= h >>> 16`. (fastutil's
/// newer `mix()`/`mix64()` variants differ; the pinned version is the
/// two-xor one.)
#[cfg(test)]
pub(crate) fn hash_common_mix(x: u64) -> u64 {
    let h = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let h = h ^ (h >> 32);
    h ^ (h >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn universe(name: &str) -> FeatureFlagUniverse {
        FeatureFlagUniverse::new(name.to_string())
    }

    fn flag(u: &FeatureFlagUniverse, bit: u32) -> FeatureFlag {
        FeatureFlag::new(u.clone(), bit)
    }

    #[test]
    fn empty_semantics() {
        let e = FeatureFlagSet::of();
        assert!(e.is_empty());
        assert!(e.universe().is_none());
        // Empty is a subset of everything, intersects nothing.
        let u = universe("main");
        let s = FeatureFlagSet::of_flag(&flag(&u, 0));
        assert!(e.is_subset_of(&s));
        assert!(!e.intersects(&s));
        assert_eq!(e.join(&s), s);
        assert_eq!(s.join(&e), s);
        assert_eq!(s.subtract(&e), s);
        assert_eq!(e.subtract(&s), e);
    }

    #[test]
    fn contains_and_mask() {
        let u = universe("main");
        let a = flag(&u, 0);
        let b = flag(&u, 1);
        let set = FeatureFlagSet::of_flags(&a, &[&b]);
        assert!(set.contains(&a));
        assert!(set.contains(&b));
        assert_eq!(set.mask(), 0b11);
        let other_u = universe("other");
        let c = flag(&other_u, 0);
        assert!(!set.contains(&c));
        // A set built from a single flag.
        let one = FeatureFlagSet::of_flag(&a);
        assert!(one.contains(&a));
        assert!(!one.contains(&b));
        assert_eq!(one.mask(), 1);
    }

    #[test]
    fn of_flag_uses_flag_universe_even_if_not_first() {
        let u = universe("main");
        let a = flag(&u, 0);
        let b = flag(&u, 5);
        let set = FeatureFlagSet::of_flags(&b, &[&a]);
        assert_eq!(set.universe(), Some(&u));
        assert_eq!(set.mask(), (1 << 0) | (1 << 5));
    }

    #[test]
    fn mismatched_universe_panics_with_java_message() {
        let u1 = universe("main");
        let u2 = universe("other");
        let a = flag(&u1, 0);
        let b = flag(&u2, 1);
        // of(flag, ...): `computeMask` throws on the first mismatched flag.
        let result = std::panic::catch_unwind(|| FeatureFlagSet::of_flags(&a, &[&b]));
        assert!(result.is_err());
        // create: the same check.
        let result = std::panic::catch_unwind(|| {
            FeatureFlagSet::create(&u1, [a.clone(), b.clone()]);
        });
        assert!(result.is_err());
    }

    #[test]
    fn subset_and_intersect_math() {
        let u = universe("main");
        let a = flag(&u, 0);
        let b = flag(&u, 1);
        let s1 = FeatureFlagSet::of_flag(&a);
        let s2 = FeatureFlagSet::of_flags(&a, &[&b]);
        assert!(s1.is_subset_of(&s2));
        assert!(!s2.is_subset_of(&s1));
        assert!(s2.is_subset_of(&s2));
        assert!(s1.intersects(&s2));
        assert!(s2.intersects(&s1));
        let s3 = FeatureFlagSet::of_flag(&b);
        assert!(!s3.intersects(&s1));
        assert!(s3.intersects(&s2));
        assert!(s3.is_subset_of(&s2));
    }

    #[test]
    fn join_and_subtract() {
        let u = universe("main");
        let a = flag(&u, 0);
        let b = flag(&u, 1);
        let c = flag(&u, 2);
        let s_a = FeatureFlagSet::of_flag(&a);
        let s_b = FeatureFlagSet::of_flag(&b);
        let s_ab = FeatureFlagSet::of_flags(&a, &[&b]);
        let s_abc = FeatureFlagSet::of_flags(&a, &[&b, &c]);
        assert_eq!(s_a.join(&s_b), s_ab);
        assert_eq!(s_ab.join(&s_a), s_ab);
        assert_eq!(s_abc.subtract(&s_a), FeatureFlagSet::of_flags(&b, &[&c]));
        assert_eq!(s_ab.subtract(&s_ab), FeatureFlagSet::of());
        assert_eq!(s_a.subtract(&FeatureFlagSet::of()), s_a);
        assert_eq!(FeatureFlagSet::of().subtract(&s_a), FeatureFlagSet::of());
    }

    #[test]
    fn mismatched_join_subtract_panics() {
        let u1 = universe("main");
        let u2 = universe("other");
        let s1 = FeatureFlagSet::of_flag(&flag(&u1, 0));
        let s2 = FeatureFlagSet::of_flag(&flag(&u2, 1));
        let r1 = std::panic::catch_unwind(|| s1.join(&s2));
        assert!(r1.is_err());
        let r2 = std::panic::catch_unwind(|| s1.subtract(&s2));
        assert!(r2.is_err());
    }

    #[test]
    fn equality_is_structural() {
        let u = universe("main");
        let s1 = FeatureFlagSet::of_flag(&flag(&u, 0));
        let s2 = FeatureFlagSet::of_flag(&flag(&u, 0));
        assert_eq!(s1, s2);
        assert_eq!(FeatureFlagSet::of(), FeatureFlagSet::of());
        // Same mask, different universe: NOT equal.
        let u2 = universe("other");
        assert_ne!(s1, FeatureFlagSet::of_flag(&flag(&u2, 0)));
        // Java reference identity: two same-id universes (separate `new`) are
        // also NOT equal, so sets built from them are incompatible.
        let u3 = universe("main");
        let s3 = FeatureFlagSet::of_flag(&flag(&u3, 0));
        assert_ne!(s1, s3);
        // A clone shares the allocation and IS equal (same universe identity).
        let s1_clone = s1.clone();
        assert_eq!(s1, s1_clone);
    }

    #[test]
    fn hash_common_mix_matches_fastutil() {
        // Golden values from the fastutil 8.5.18 `HashCommon.mix(long)`,
        // computed by a Java probe against the pinned jar's bytecode
        // (`h = x * 0x9E3779B97F4A7C15; h ^= h >>> 32; h ^= h >>> 16`):
        //   mix(0) = 0
        //   mix(1) = 0x9e37e78e98c4e4d1
        //   mix(3) = 0xdaa6b78aca55be6a
        assert_eq!(hash_common_mix(0), 0);
        assert_eq!(hash_common_mix(1), 0x9e37_e78e_98c4_e4d1);
        assert_eq!(hash_common_mix(3), 0xdaa6_b78a_ca55_be6a);
        // Determinism + that a differing mask differs.
        assert_eq!(hash_common_mix(0b11), hash_common_mix(0b11));
        assert_ne!(hash_common_mix(0b01), hash_common_mix(0b10));
    }
}
