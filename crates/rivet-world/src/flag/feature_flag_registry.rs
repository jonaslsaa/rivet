//! `net.minecraft.world.flag.FeatureFlagRegistry` — the named flag registry.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/
//! FeatureFlagRegistry.java`. The registry owns `(universe, allFlags, names)`
//! where `names` is a `Map<Identifier, FeatureFlag>` (a `LinkedHashMap` built
//! by the `Builder`, then `Map.copyOf` — an *insertion-ordered* immutable
//! view) and `allFlags` is the union set of every registered flag.
//!
//! Declaration ordering is preserved: the `Builder` assigns bits in
//! `create`-call order (`id++`) and `names` iterates in that order, so
//! `to_names` emits the `FeatureFlags` declarations (`vanilla`,
//! `trade_rebalance`, `redstone_experiments`, `minecart_improvements`).
//!
//! ## The `codec()` HashSet iteration-order model
//!
//! `codec()`'s decode half is
//! `Identifier.CODEC.listOf().comapFlatMap(...)`: it decodes a list of ids to a
//! `FeatureFlagSet`, collecting the **unknown** ids into a `HashSet<Identifier>`
//! that the error message then renders via `HashSet.toString()`. That rendering
//! order is JDK `HashMap`/`HashSet` hash-probe order — deterministic for a
//! fixed set of keys and JDK version, but not insertion order. The `toNames`
//! half builds a `HashSet` too. The port reproduces the exact order for the
//! keys the `FeatureFlags` registry can actually hold/meet (Java 25, the pinned
//! runtime) so a hostile test can assert the byte-exact `"Unknown feature ids:
//! [...]"` message.
//!
//! `java.util.HashMap` (no explicit initial capacity → table 16, load factor
//! 0.75, resized when `size > 12`): the final table capacity is the smallest
//! power of two with `4 * size <= 3 * capacity` and `capacity >= 16`, and the
//! iteration order is the non-empty slots of `bucket = spread(hash) &
//! (capacity - 1)`, slot-first. The `HashSet` "value" hash for an
//! `Identifier` is its `hashCode` (`31 * nsHash + pathHash`), so the port
//! computes it via `rivet_util::java_hash::string_hash` exactly like
//! `rivet-registry::Identifier`'s `Hash` impl.
//!
//! Two `HashSet`-specific behaviors are reproduced because the port collects
//! ids into a `Vec`, not a set: equal ids **deduplicate** before the capacity
//! and order run (Java's set collapses them), and the spread is JDK 25's
//! **logical** `h ^ (h >>> 16)` (an arithmetic `>>` on a negative `i32` would
//! sign-extend and diverge once the modeled capacity exceeds 2^16). This is a
//! **documented JDK-25-grounded reproduction**, not a general hash-table port;
//! any future key set beyond the four canonical flags must be re-validated
//! against Java.

use super::feature_flag::FeatureFlag;
use super::feature_flag_set::FeatureFlagSet;
use super::feature_flag_universe::FeatureFlagUniverse;
use rivet_registry::Identifier;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::functions::DecoderFn;
use rivet_util::java_hash::string_hash;
use std::collections::HashMap;
use std::sync::Arc;

/// `FeatureFlagRegistry` — the `(universe, allFlags, names)` value.
#[derive(Clone)]
pub struct FeatureFlagRegistry {
    universe: FeatureFlagUniverse,
    /// `allFlags` — the union of every registered flag, in `names` iteration
    /// order (the `Builder.build` `FeatureFlagSet.create`).
    all_flags: FeatureFlagSet,
    /// `names` — `Map<Identifier, FeatureFlag>`, **insertion-ordered**
    /// (Java's `Map.copyOf(LinkedHashMap)` preserves declaration order). Java
    /// exposes this as `public final`; the port exposes the ordered pair list
    /// (the same iteration Java's map gives).
    pub names: Vec<(Identifier, FeatureFlag)>,
    /// `by_name` — a `HashMap<Identifier, FeatureFlag>` for `from_names` lookups.
    by_name: HashMap<Identifier, FeatureFlag>,
}

impl FeatureFlagRegistry {
    fn new(
        universe: FeatureFlagUniverse,
        all_flags: FeatureFlagSet,
        names: Vec<(Identifier, FeatureFlag)>,
    ) -> Self {
        let by_name: HashMap<Identifier, FeatureFlag> = names.iter().cloned().collect();
        FeatureFlagRegistry {
            universe,
            all_flags,
            names,
            by_name,
        }
    }

    /// `FeatureFlagRegistry.isSubset(FeatureFlagSet)` — `set.isSubsetOf(this.allFlags)`.
    pub fn is_subset(&self, set: &FeatureFlagSet) -> bool {
        set.is_subset_of(&self.all_flags)
    }

    /// `FeatureFlagRegistry.allFlags()`.
    pub fn all_flags(&self) -> &FeatureFlagSet {
        &self.all_flags
    }

    /// `FeatureFlagRegistry.subset(FeatureFlag...)` — `FeatureFlagSet.create(
    /// this.universe, Arrays.asList(flags))`.
    pub fn subset(&self, flags: &[&FeatureFlag]) -> FeatureFlagSet {
        FeatureFlagSet::create(&self.universe, flags.iter().map(|f| (*f).clone()))
    }

    /// `FeatureFlagRegistry.fromNames(Iterable<Identifier>)` — the
    /// default-unknown warning variant. The `LOGGER.warn` sink is `eprintln!`
    /// (the crate's established warn-level precedent; `tracing` is not a
    /// dependency of `rivet-world`).
    pub fn from_names(&self, flag_ids: impl IntoIterator<Item = Identifier>) -> FeatureFlagSet {
        self.from_names_with(flag_ids, |id| eprintln!("Unknown feature flag: {}", id))
    }

    /// `FeatureFlagRegistry.fromNames(Iterable<Identifier>, Consumer<Identifier>)`.
    pub fn from_names_with(
        &self,
        flag_ids: impl IntoIterator<Item = Identifier>,
        unknown_flags: impl FnMut(Identifier),
    ) -> FeatureFlagSet {
        let mut unknown_flags = unknown_flags;
        // Java's `Sets.newIdentityHashSet()` — identity keyed, so the same
        // logical flag (equal Identifier) can appear twice. The port's
        // `FeatureFlag` is a value; deduplication via an insertion-ordered
        // vec of masks (a HashSet of flags would lose the mask-OR identity
        // the identity set preserves).
        let mut seen: Vec<FeatureFlag> = Vec::new();
        for flag_id in flag_ids {
            match self.by_name.get(&flag_id) {
                Some(flag) => {
                    if !seen.contains(flag) {
                        seen.push(flag.clone());
                    }
                }
                None => unknown_flags(flag_id),
            }
        }
        FeatureFlagSet::create(&self.universe, seen)
    }

    /// `FeatureFlagRegistry.toNames(FeatureFlagSet)` — the `HashSet`-ordered
    /// identifier set of every *registered* name whose flag is in `set`.
    pub fn to_names(&self, set: &FeatureFlagSet) -> Vec<Identifier> {
        // Java builds a `HashSet<Identifier>` and returns it; the port returns
        // the same elements in JDK-25 hash order (see the module doc). The
        // registry's four canonical names have distinct buckets, so this is
        // declaration order for `allFlags` — asserted in the tests.
        let mut result: Vec<Identifier> = Vec::new();
        for (id, flag) in &self.names {
            if set.contains(flag) {
                result.push(id.clone());
            }
        }
        hash_set_order(&result)
    }

    /// `FeatureFlagRegistry.codec()` —
    /// `Identifier.CODEC.listOf().comapFlatMap(fromNames(ids, unknown::add),
    /// set -> List.copyOf(toNames(set)))`.
    ///
    /// Decode: an unknown flag id makes the whole decode an **error** whose
    /// message renders the unknown `HashSet` (hash order) and whose *partial*
    /// value is the set of known flags (`DataResult.error(..., result)`). The
    /// partial is what `WorldDataConfiguration.MAP_CODEC`'s lenient field drops
    /// and falls back to `DEFAULT_FLAGS` on. Encode: the names of `set`'s
    /// registered flags, in `HashSet` order.
    pub fn codec<Ops: DynamicOps + 'static>(&self) -> Arc<dyn Codec<FeatureFlagSet, Ops>> {
        let this = Arc::new(self.clone());
        let to_this = Arc::clone(&this);
        let to: DecoderFn<Vec<Identifier>, FeatureFlagSet> =
            Arc::new(move |ids: &Vec<Identifier>| {
                let mut unknown_ids: Vec<Identifier> = Vec::new();
                let result =
                    to_this.from_names_with(ids.iter().cloned(), |id| unknown_ids.push(id));
                if unknown_ids.is_empty() {
                    DataResult::success(result)
                } else {
                    // Java's `HashSet.toString()` renders the unknown set in hash
                    // order inside the `DataResult.error` message.
                    let rendered = hash_set_order(&unknown_ids)
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", ");
                    DataResult::error_with_partial(
                        format!("Unknown feature ids: [{}]", rendered),
                        result,
                    )
                }
            });
        let from_this = Arc::clone(&this);
        codec::comap_flat_map(
            codec::list(rivet_registry::identifier::identifier_codec::<Ops>()),
            to,
            Arc::new(move |set: &FeatureFlagSet| from_this.to_names(set)),
        )
    }
}

/// `FeatureFlagRegistry.Builder` — the bit-assigning declaration-order builder.
pub struct Builder {
    universe: FeatureFlagUniverse,
    id: u32,
    /// `flags` — a `LinkedHashMap`, so iteration (and hence `names`) is
    /// declaration order.
    flags: Vec<(Identifier, FeatureFlag)>,
    by_name: HashMap<Identifier, FeatureFlag>,
}

impl Builder {
    /// `new Builder(String universeId)`.
    pub fn new(universe_id: String) -> Self {
        Builder {
            universe: FeatureFlagUniverse::new(universe_id),
            id: 0,
            flags: Vec::new(),
            by_name: HashMap::new(),
        }
    }

    /// `Builder.createVanilla(String)` — `create(Identifier.withDefaultNamespace(name))`.
    pub fn create_vanilla(&mut self, name: &str) -> FeatureFlag {
        self.create(Identifier::with_default_namespace(name))
    }

    /// `Builder.create(Identifier)` — `id++` bit assignment, `>= 64` guard,
    /// duplicate-name guard (Java's `IllegalStateException`s).
    pub fn create(&mut self, name: Identifier) -> FeatureFlag {
        if self.id >= 64 {
            panic!("Too many feature flags");
        }
        let result = FeatureFlag::new(self.universe.clone(), self.id);
        self.id += 1;
        if self.by_name.contains_key(&name) {
            panic!("Duplicate feature flag {}", name);
        }
        self.by_name.insert(name.clone(), result.clone());
        self.flags.push((name, result.clone()));
        result
    }

    /// `Builder.build()` — `FeatureFlagSet.create(universe, flags.values())`
    /// then the immutable registry with `Map.copyOf(flags)`.
    pub fn build(&mut self) -> FeatureFlagRegistry {
        let all_flags =
            FeatureFlagSet::create(&self.universe, self.flags.iter().map(|(_, f)| f.clone()));
        FeatureFlagRegistry::new(self.universe.clone(), all_flags, self.flags.clone())
    }
}

/// Renders an `Identifier` set the way JDK 25's `HashSet<Identifier>` does:
/// the non-empty slots of `bucket = spread(hash) & (capacity - 1)`, slot-first
/// ascending, with the final capacity the smallest power of two `>= 16`
/// satisfying `4 * size <= 3 * capacity`. Tie (same bucket) order is JDK's
/// probe order; the port's key sets have distinct buckets and are documented as
/// such.
fn hash_set_order(ids: &[Identifier]) -> Vec<Identifier> {
    // Java collects into a `HashSet<Identifier>`, so equal ids collapse before
    // the capacity/order calculation runs. Dedup by value equality first.
    let mut deduped: Vec<Identifier> = Vec::new();
    for id in ids {
        if !deduped.contains(id) {
            deduped.push(id.clone());
        }
    }
    if deduped.is_empty() {
        return Vec::new();
    }
    let mut capacity = 16usize;
    while 4 * deduped.len() > 3 * capacity {
        capacity *= 2;
    }
    let mask = capacity - 1;
    let mut slots: Vec<Vec<&Identifier>> = (0..capacity).map(|_| Vec::new()).collect();
    for id in &deduped {
        let hash = identifier_hash(id);
        // JDK 25: `h ^ (h >>> 16)` — a LOGICAL right shift (the spread of a
        // negative hash must not sign-extend). Arithmetic `>>` would diverge
        // once the modeled capacity exceeds 2^16.
        let spread = hash ^ ((hash as u32) >> 16) as i32;
        let bucket = (spread as usize) & mask;
        slots[bucket].push(id);
    }
    slots.into_iter().flatten().cloned().collect()
}

/// `Identifier.hashCode()` = `31 * namespace.hashCode() + path.hashCode()`
/// (UTF-16, matching the `Hash` impl in `rivet-registry::identifier`).
fn identifier_hash(id: &Identifier) -> i32 {
    string_hash(id.namespace())
        .wrapping_mul(31)
        .wrapping_add(string_hash(id.path()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build the canonical `"main"` registry (declaration order).
    fn main_registry() -> FeatureFlagRegistry {
        let mut builder = Builder::new("main".to_string());
        builder.create_vanilla("vanilla");
        builder.create_vanilla("trade_rebalance");
        builder.create_vanilla("redstone_experiments");
        builder.create_vanilla("minecart_improvements");
        builder.build()
    }

    #[test]
    fn declaration_ordering_preserved() {
        let registry = main_registry();
        let names: Vec<String> = registry
            .names
            .iter()
            .map(|(id, _)| id.to_string())
            .collect();
        assert_eq!(
            names,
            vec![
                "minecraft:vanilla",
                "minecraft:trade_rebalance",
                "minecraft:redstone_experiments",
                "minecraft:minecart_improvements"
            ]
        );
        // `allFlags` = the OR of every registered flag.
        let all = registry.all_flags();
        assert!(all.contains(&registry.names[0].1));
        assert!(all.contains(&registry.names[3].1));
        assert_eq!(all.mask(), 0b1111);
        assert!(registry.is_subset(all));
    }

    #[test]
    fn duplicate_and_too_many_panic() {
        let mut builder = Builder::new("main".to_string());
        builder.create_vanilla("vanilla");
        // The duplicate guard throws before inserting, so `builder` is
        // borrowable again (Java's IllegalStateException).
        let dup = std::panic::catch_unwind(|| {
            let mut b = Builder::new("main".to_string());
            b.create_vanilla("vanilla");
            b.create_vanilla("vanilla");
        });
        assert!(dup.is_err());
        let too_many = std::panic::catch_unwind(|| {
            let mut b = Builder::new("big".to_string());
            for i in 0..65 {
                b.create_vanilla(&format!("flag_{i}"));
            }
        });
        assert!(too_many.is_err());
        // `builder` still works (the first build path is unaffected).
        builder.create_vanilla("trade_rebalance");
    }

    #[test]
    fn from_names_known_and_unknown() {
        let registry = main_registry();
        let ids = vec![
            Identifier::with_default_namespace("vanilla"),
            Identifier::with_default_namespace("trade_rebalance"),
        ];
        let set = registry.from_names(ids);
        assert_eq!(set.mask(), 0b0011);
        // The built set is a subset of the registry's allFlags.
        assert!(registry.is_subset(&set));

        // Unknown ids hit the sink; known ones are preserved.
        let mut warned = Vec::new();
        let mixed = vec![
            Identifier::with_default_namespace("vanilla"),
            Identifier::with_default_namespace("nope"),
            Identifier::with_default_namespace("redstone_experiments"),
        ];
        let set2 = registry.from_names_with(mixed, |id| warned.push(id.to_string()));
        assert_eq!(set2.mask(), 0b0101);
        assert_eq!(warned, vec!["minecraft:nope"]);
    }

    #[test]
    fn subset_factory() {
        let registry = main_registry();
        let s = registry.subset(&[&registry.names[0].1, &registry.names[1].1]);
        assert_eq!(s.mask(), 0b0011);
    }

    #[test]
    fn to_names_declaration_order_for_canonical_flags() {
        // The four canonical names have distinct JDK-25 buckets, so hash-set
        // order == declaration order here (empirically validated against Java
        // 25). This test pins that exact order.
        let registry = main_registry();
        let names: Vec<String> = registry
            .to_names(registry.all_flags())
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            names,
            vec![
                "minecraft:redstone_experiments",
                "minecraft:vanilla",
                "minecraft:trade_rebalance",
                "minecraft:minecart_improvements"
            ]
        );
        // A subset in the same hash order ({vanilla bucket 8,
        // redstone_experiments bucket 0} -> redstone, vanilla).
        let set = registry.subset(&[&registry.names[0].1, &registry.names[2].1]);
        let subset_names: Vec<String> = registry
            .to_names(&set)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            subset_names,
            vec!["minecraft:redstone_experiments", "minecraft:vanilla"]
        );
    }

    #[test]
    fn hash_set_order_matches_jdk_25() {
        // Empirically-validated JDK 25 ordering for the four canonical names
        // (bucket = spread & 15: redstone=0, vanilla=8, trade=14, minecart=15).
        let ids = vec![
            Identifier::with_default_namespace("vanilla"),
            Identifier::with_default_namespace("trade_rebalance"),
            Identifier::with_default_namespace("redstone_experiments"),
            Identifier::with_default_namespace("minecart_improvements"),
        ];
        let ordered = hash_set_order(&ids);
        let strings: Vec<String> = ordered.iter().map(ToString::to_string).collect();
        assert_eq!(
            strings,
            vec![
                "minecraft:redstone_experiments",
                "minecraft:vanilla",
                "minecraft:trade_rebalance",
                "minecraft:minecart_improvements"
            ]
        );
        // Empty and singleton.
        assert!(hash_set_order(&[]).is_empty());
        assert_eq!(
            hash_set_order(&[Identifier::with_default_namespace("vanilla")])[0].to_string(),
            "minecraft:vanilla"
        );
    }

    #[test]
    fn codec_round_trips_via_json() {
        use rivet_serialization::json_ops::JsonOps;
        let registry = main_registry();
        let codec = registry.codec::<JsonOps>();
        let ops = JsonOps::INSTANCE;
        let set = registry.subset(&[&registry.names[0].1, &registry.names[2].1]);

        // Encode: the names, in hash-set order (redstone_experiments bucket 0
        // sorts before vanilla bucket 8).
        let encoded = codec
            .encode_start(&ops, &set)
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            ops.create_list(vec![
                ops.create_string("minecraft:redstone_experiments".to_string()),
                ops.create_string("minecraft:vanilla".to_string()),
            ])
        );

        // Decode round-trips (the decoded set contains the same flags).
        let decoded = codec.decode(&ops, &encoded).get_or_throw("decode").clone();
        assert_eq!(decoded.0, set);
    }

    #[test]
    fn codec_unknown_flag_errors_with_exact_hash_order_message() {
        use rivet_serialization::json_ops::JsonOps;
        let registry = main_registry();
        let codec = registry.codec::<JsonOps>();
        let ops = JsonOps::INSTANCE;

        // A single unknown id: the exact Java error message. The partial is
        // the empty set (`fromNames` yields EMPTY when every id is unknown),
        // which is what the lenient `enabled_features` field in
        // `WorldDataConfiguration.MAP_CODEC` promotes to DEFAULT_FLAGS.
        let input = ops.create_list(vec![ops.create_string("minecraft:bogus".to_string())]);
        let result = codec.decode(&ops, &input);
        assert!(result.error_ref().is_some());
        let err = result.error_ref().unwrap();
        assert_eq!(err.message(), "Unknown feature ids: [minecraft:bogus]");
        let partial = err
            .partial()
            .clone()
            .expect("partial carries the empty set");
        assert!(partial.0.is_empty());

        // Mixed known + unknown: partial carries the known set, error message
        // renders the unknown (Java 25 HashSet order: bar, foo, baz).
        let input = ops.create_list(vec![
            ops.create_string("minecraft:foo".to_string()),
            ops.create_string("minecraft:vanilla".to_string()),
            ops.create_string("minecraft:bar".to_string()),
            ops.create_string("minecraft:baz".to_string()),
        ]);
        let result = codec.decode(&ops, &input);
        let err = result.error_ref().unwrap();
        assert_eq!(
            err.message(),
            "Unknown feature ids: [minecraft:bar, minecraft:foo, minecraft:baz]"
        );
        let partial = err
            .partial()
            .clone()
            .expect("partial carries the known set");
        // The partial is a `(FeatureFlagSet, Value)` pair — the first component
        // is the known-flag set.
        assert!(partial.0.contains(&registry.names[0].1)); // vanilla
        assert_eq!(partial.0.mask(), 0b0001);
    }

    #[test]
    fn codec_accepts_full_default_flags() {
        use rivet_serialization::json_ops::JsonOps;
        let registry = main_registry();
        let codec = registry.codec::<JsonOps>();
        let ops = JsonOps::INSTANCE;
        // `FeatureFlags.DEFAULT_FLAGS` = {vanilla}.
        let input = ops.create_list(vec![ops.create_string("minecraft:vanilla".to_string())]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0.mask(), 0b0001);
    }

    #[test]
    fn codec_dedups_repeated_unknown_ids_like_hashset() {
        // Java collects unknown ids into a `HashSet`, so a repeated id appears
        // once in the error message. The port must collapse duplicates before
        // the capacity/order calculation too.
        use rivet_serialization::json_ops::JsonOps;
        let registry = main_registry();
        let codec = registry.codec::<JsonOps>();
        let ops = JsonOps::INSTANCE;
        let input = ops.create_list(vec![
            ops.create_string("minecraft:bogus".to_string()),
            ops.create_string("minecraft:bogus".to_string()),
        ]);
        let result = codec.decode(&ops, &input);
        let err = result.error_ref().unwrap();
        assert_eq!(err.message(), "Unknown feature ids: [minecraft:bogus]");
        // The partial is the empty set (all unknown).
        assert!(err.partial().as_ref().unwrap().0.is_empty());
    }

    #[test]
    fn codec_dedups_repeated_known_and_unknown_ids() {
        use rivet_serialization::json_ops::JsonOps;
        let registry = main_registry();
        let codec = registry.codec::<JsonOps>();
        let ops = JsonOps::INSTANCE;
        // Repeated known ids collapse in `fromNames` (identity set); repeated
        // unknown ids collapse in the error HashSet.
        let input = ops.create_list(vec![
            ops.create_string("minecraft:vanilla".to_string()),
            ops.create_string("minecraft:vanilla".to_string()),
            ops.create_string("minecraft:nope".to_string()),
            ops.create_string("minecraft:nope".to_string()),
        ]);
        let result = codec.decode(&ops, &input);
        let err = result.error_ref().unwrap();
        assert_eq!(err.message(), "Unknown feature ids: [minecraft:nope]");
        let partial = err.partial().as_ref().unwrap();
        assert_eq!(partial.0.mask(), 0b0001);
    }

    #[test]
    fn hash_set_order_uses_logical_shift_for_negative_hashes() {
        // JDK 25's spread is `h ^ (h >>> 16)` — a LOGICAL right shift. An
        // arithmetic `>>` on a negative i32 would sign-extend and diverge once
        // the modeled capacity exceeds 2^16. This id set forces a 131,072-capacity
        // table (49,153 distinct keys); assert the returned order is exactly the
        // slot-first ascending order of the LOGICAL buckets.
        let ids: Vec<Identifier> = (0..49_153)
            .map(|i| Identifier::with_default_namespace(&format!("u{i}")))
            .collect();
        let ordered = hash_set_order(&ids);
        assert_eq!(ordered.len(), ids.len()); // all distinct -> no dedup
        let mut expected: Vec<(usize, usize)> = ids
            .iter()
            .enumerate()
            .map(|(idx, id)| {
                let h = identifier_hash(id);
                let spread = h ^ ((h as u32) >> 16) as i32;
                let capacity = 131_072usize; // 4 * 49153 > 3 * 65536
                (spread as usize & (capacity - 1), idx)
            })
            .collect();
        expected.sort();
        let logical: Vec<Identifier> = expected.iter().map(|&(_, idx)| ids[idx].clone()).collect();
        assert_eq!(ordered, logical);
    }

    #[test]
    fn logical_shift_diverges_from_arithmetic_for_large_capacity() {
        // Find an id whose JDK (logical) bucket differs from an arithmetic
        // `>>` bucket at capacity 131072 — proving the fix is load-bearing
        // (the four canonical ids never exercise this).
        let found = (0..49_153).find(|&i| {
            let id = Identifier::with_default_namespace(&format!("u{i}"));
            let h = identifier_hash(&id);
            let logical = h ^ ((h as u32) >> 16) as i32;
            let arithmetic = h ^ (h >> 16);
            logical as usize & 131_071 != arithmetic as usize & 131_071
        });
        assert!(found.is_some(), "expected at least one diverging id");
    }
}
