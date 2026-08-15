//! Port of `net.minecraft.world.level.biome.FeatureSorter` (26.2).
//!
//! Faithful port of `FeatureSorter.buildFeaturesPerStep` and the
//! `StepFeatureData`/`indexMapping` output the biome decoration pass
//! (`ChunkGenerator.applyBiomeDecoration`) feeds `setFeatureSeed` with.
//!
//! The algorithm assigns every placed feature a **global first-appearance
//! index** (in source order, then step order, then holder-set order), builds a
//! dependency graph over `(step, globalIndex)` nodes — one node per
//! feature-per-step occurrence, with a directed edge from each feature to the
//! next in the same source's flattened list (so a source's features are
//! chained in step order) — then runs a DFS topological sort whose post-order,
//! reversed, yields the per-step feature ordering. `StepFeatureData` stores one
//! step's features (in that topological order) plus the identity
//! `indexMapping` (`Util.createIndexIdentityLookup`) used to turn a feature
//! back into its position in the step.
//!
//! ## Identity semantics
//!
//! Java keys the global index map (`Object2IntMap<PlacedFeature>`) and the
//! per-step `indexMapping` (`Util.createIndexIdentityLookup`) on the *value
//! object* — for a `Reference` holder the registry stores one value per
//! `(registry, id)`, and for a `Direct` holder one value per holder instance —
//! so Java's value identity is exactly the **holder identity**. The Rust port
//! therefore keys on [`PlacedFeatureKey`]: a `Reference` holder's
//! `(RegistryId, id)` pair, or a `Direct` holder's inline-value address (the
//! Rust spelling of Java's per-object identity). The same feature holder seen
//! in several biomes/steps collapses to one global index and one per-step
//! entry.
//!
//! A `Reference` holder's key is a pure `(registry, id)` value, so a clone is
//! the same identity — exactly Java's registry-backed behavior. A `Direct`
//! holder's key is the inline value's address, so only the same *instance*
//! collapses; a Rust clone of a `Direct` holder is a distinct identity. Java's
//! `Direct.value()` returns the same object for the same holder (identity
//! across clones), so this is a small deviation — but `Direct` placed-feature
//! holders are decode-only inline values that never occur in registry-loaded
//! biome settings (those are all `Reference`), so it is limited to synthetic
//! scenarios.
//!
//! The value `PlacedFeature` itself is not resolved (the placed-feature
//! `HolderLookup` is not threaded through this slice — it defers with #126 like
//! the rest of placement resolution), so [`StepFeatureData`] holds
//! `Holder<PlacedFeature>` rather than values. The stored holders are clones
//! (for `Direct` holders a Rust clone is a distinct allocation, hence a
//! distinct identity), so the per-step `index_mapping` registers *both*
//! spellings of each logical holder — the identity captured at first appearance
//! (so a holder borrowed from the source settings resolves) and each stored
//! clone's own identity (so `index_mapping(&features[i])` is `Some(i)`). Java
//! needs only one key per feature because its `Direct.value()` returns the same
//! object for the same holder and its `features()` list holds those same
//! objects; the two keys are the Rust spelling of that single observable
//! identity. `Reference` holders collapse both spellings to the same
//! `(registry, id)` key.
//!
//! ## Cycle diagnostics
//!
//! A cycle makes `build_features_per_step` panic. With `tryReducingError =
//! false` the panic is Java's `"Feature order cycle found"`. With
//! `tryReducingError = true` Java first runs the source-reduction diagnostic
//! (`buildFeaturesPerStep` recursively on the source list with each source
//! removed, keeping a source removed iff the residual still cycles) and panics
//! with `"Feature order cycle found, involved sources: <list>"`; both are
//! replicated exactly, including the `T: Clone` spelling of Java's reference
//! copy and the `T: Debug` rendering of the surviving-source list.
//!
//! Java catches `IllegalStateException` from the recursive reduction probe
//! (only the cycle diagnostics are ISEs); the port downcasts the panic payload
//! (`&str` for a literal panic, `String` for a formatted one) and re-throws
//! anything that is not a cycle diagnostic, mirroring that narrow catch.

use crate::levelgen::placement::PlacedFeature;
use rivet_registry::{Holder, HolderSet, RegistryId};
use std::any::Any;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

/// The global-index key — Java keys on the `PlacedFeature` value object, which
/// for a `Reference` holder is one object per `(registry, id)` and for a
/// `Direct` holder one object per instance; the Rust spelling of that identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PlacedFeatureKey {
    /// `Holder.Reference` — the `(RegistryId, id)` pair.
    Reference { registry: RegistryId, id: u32 },
    /// `Holder.Direct` — the inline value's address (Java per-object identity).
    Direct(usize),
}

/// The holder-identity key of a `&Holder<PlacedFeature>`.
fn placed_feature_key(holder: &Holder<PlacedFeature>) -> PlacedFeatureKey {
    match holder {
        Holder::Reference { registry, id } => PlacedFeatureKey::Reference {
            registry: *registry,
            id: *id,
        },
        Holder::Direct(feature) => {
            PlacedFeatureKey::Direct(feature as *const PlacedFeature as usize)
        }
    }
}

/// `FeatureSorter.StepFeatureData` — one decoration step's feature list plus
/// the identity index mapping (`Util.createIndexIdentityLookup`).
#[derive(Debug, Clone)]
pub struct StepFeatureData {
    /// `StepFeatureData.features` — the features whose `(step, globalIndex)`
    /// node falls in this step, in the reversed-DFS topological order.
    pub features: Vec<Holder<PlacedFeature>>,
    /// `StepFeatureData.indexMapping` — `feature -> position in features`,
    /// keyed on holder identity (see the module doc).
    index_mapping: HashMap<PlacedFeatureKey, usize>,
}

impl StepFeatureData {
    /// `StepFeatureData.indexMapping().applyAsInt(feature)` — the feature's
    /// position in `features` (the per-step global index `setFeatureSeed` is
    /// given, and the index used to `features().get(globalIndexOfFeature)`).
    /// Registers both the first-appearance identity and each stored clone's own
    /// identity, so `index_mapping(&features[i]) == Some(i)` and a holder
    /// borrowed from the source settings both resolve — Java's single-identity
    /// observable contract. Java's identity lookup returns `-1` for an absent
    /// feature; the `Option` is the Rust spelling.
    pub fn index_mapping(&self, feature: &Holder<PlacedFeature>) -> Option<usize> {
        self.index_mapping
            .get(&placed_feature_key(feature))
            .copied()
    }
}

/// `FeatureSorter.buildFeaturesPerStep(List<T>, Function<T,
/// List<HolderSet<PlacedFeature>>>, boolean)`.
///
/// `T: Clone` is the Rust spelling of Java's `new ArrayList<>(featureSources)`
/// reference copy (only exercised by the cycle-reduction diagnostic); `T: Debug`
/// renders the surviving-source list in the cycle error like Java's
/// `List.toString`.
pub fn build_features_per_step<T>(
    feature_sources: &[T],
    feature_getter: impl Fn(&T) -> &[HolderSet<PlacedFeature>],
    try_reducing_error: bool,
) -> Vec<StepFeatureData>
where
    T: Clone + fmt::Debug,
{
    // Forward through a `&dyn Fn` fat pointer so the cycle-reduction recursion
    // below stays within one monomorphization (an `impl Fn` parameter would
    // re-instantiate the generic per probe: `T, &F`, `T, &&F`, ... until the
    // recursion limit).
    build_features_per_step_inner(feature_sources, &feature_getter, try_reducing_error)
}

/// The algorithm body, with the getter erased to `&dyn Fn` for the recursive
/// reduction probe.
fn build_features_per_step_inner<T>(
    feature_sources: &[T],
    feature_getter: &dyn Fn(&T) -> &[HolderSet<PlacedFeature>],
    try_reducing_error: bool,
) -> Vec<StepFeatureData>
where
    T: Clone + fmt::Debug,
{
    // `featureIndex: Object2IntMap<PlacedFeature>` + `nextFeatureIndex: MutableInt(0)`.
    let mut feature_index: HashMap<PlacedFeatureKey, usize> = HashMap::new();
    let mut next_feature_index = 0usize;
    // `features_by_index[i]` = (identity key, feature) of global index i. The
    // key is captured at first appearance so `StepFeatureData` maps holders
    // borrowed from the source settings even though the stored features are
    // clones.
    let mut features_by_index: Vec<(PlacedFeatureKey, Holder<PlacedFeature>)> = Vec::new();
    // `maxStep` — the largest `featuresForStep.size()` across sources.
    let mut max_step = 0usize;
    // `edges: TreeMap<FeatureData, Set<FeatureData>>` ordered by the
    // `(step, featureIndex)` comparator. A `(step, globalIndex)` node uniquely
    // identifies a feature (global indices are unique), so the node key is the
    // tuple; the TreeSet successor dedup (comparator equality) is the tuple
    // set's.
    let mut edges: BTreeMap<(usize, usize), BTreeSet<(usize, usize)>> = BTreeMap::new();

    for feature_source in feature_sources {
        let features_for_step = feature_getter(feature_source);
        max_step = max_step.max(features_for_step.len());
        // `featureList` — this source's `(globalIndex, step)` pairs flattened
        // in step order, then holder-set order.
        let mut feature_list: Vec<(usize, usize)> = Vec::new();

        for (step, holder_set) in features_for_step.iter().enumerate() {
            for holder in holder_set.iter() {
                // `featureIndex.computeIfAbsent(feature, f ->
                // nextFeatureIndex.getAndIncrement())` keyed on holder identity.
                let global_index = match feature_index.get(&placed_feature_key(holder)) {
                    Some(&global_index) => global_index,
                    None => {
                        let global_index = next_feature_index;
                        next_feature_index += 1;
                        features_by_index.push((placed_feature_key(holder), holder.clone()));
                        feature_index.insert(placed_feature_key(holder), global_index);
                        global_index
                    }
                };
                feature_list.push((global_index, step));
            }
        }

        for (i, &(global_index, step)) in feature_list.iter().enumerate() {
            // `edges.computeIfAbsent(...)` creates an entry for every node,
            // even the last.
            let successors = edges.entry((step, global_index)).or_default();
            if let Some(&(next_global_index, next_step)) = feature_list.get(i + 1) {
                successors.insert((next_step, next_global_index));
            }
        }
    }

    let mut discovered: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut currently_visiting: BTreeSet<(usize, usize)> = BTreeSet::new();
    let mut sorted_features: Vec<(usize, usize)> = Vec::new();

    let nodes: Vec<(usize, usize)> = edges.keys().copied().collect();
    for &node in &nodes {
        // Java's per-iteration invariant check (unreachable in practice; the
        // message's missing closing paren is Java's own typo).
        if !currently_visiting.is_empty() {
            panic!(
                "You somehow broke the universe; DFS bork (iteration finished with non-empty in-progress vertex set"
            );
        }

        if !discovered.contains(&node)
            && depth_first_search(
                &edges,
                &mut discovered,
                &mut currently_visiting,
                &mut sorted_features,
                node,
            )
        {
            if !try_reducing_error {
                panic!("Feature order cycle found");
            }

            // The source-reduction diagnostic: copy the source list and
            // repeatedly remove each source whose removal still leaves a cycle
            // (`buildFeaturesPerStep(..., false)` panics), re-adding any source
            // whose removal eliminates it.
            let mut reduced_sources: Vec<T> = feature_sources.to_vec();
            loop {
                let last_size = reduced_sources.len();
                let mut i = 0;
                while i < reduced_sources.len() {
                    let source = reduced_sources.remove(i);
                    let probe = catch_unwind(AssertUnwindSafe(|| {
                        build_features_per_step_inner(&reduced_sources, feature_getter, false)
                    }));
                    match probe {
                        Ok(_) => {
                            // Removal eliminated the cycle — not an involved
                            // source; put it back past the cursor.
                            reduced_sources.insert(i, source);
                            i += 1;
                        }
                        Err(payload) => {
                            if is_cycle_error(payload.as_ref()) {
                                // Removal still cycles — the source is
                                // involved; it stays removed.
                            } else {
                                resume_unwind(payload);
                            }
                        }
                    }
                }
                if last_size == reduced_sources.len() {
                    break;
                }
            }
            panic!(
                "Feature order cycle found, involved sources: {:?}",
                reduced_sources
            );
        }
    }

    // `Collections.reverse(sortedFeatures)` — reverse the DFS post-order.
    sorted_features.reverse();

    let mut result = Vec::with_capacity(max_step);
    for step in 0..max_step {
        let mut features = Vec::new();
        let mut index_mapping = HashMap::new();
        for &(node_step, global_index) in &sorted_features {
            if node_step == step {
                let (key, feature) = &features_by_index[global_index];
                let position = features.len();
                features.push(feature.clone());
                // Register both the first-appearance identity (source-borrowed
                // holders) and the stored clone's own identity (`features[i]`);
                // Java collapses these to one because its value objects are
                // shared. `Reference` holders collapse both to the same key.
                index_mapping.insert(*key, position);
                index_mapping.insert(placed_feature_key(&features[position]), position);
            }
        }
        result.push(StepFeatureData {
            features,
            index_mapping,
        });
    }
    result
}

/// `Graph.depthFirstSearch` — the recursive DFS that appends `current` to the
/// post-order after its successors, reporting (by `true`) the first back-edge
/// into `currentlyVisiting`.
fn depth_first_search(
    edges: &BTreeMap<(usize, usize), BTreeSet<(usize, usize)>>,
    discovered: &mut BTreeSet<(usize, usize)>,
    currently_visiting: &mut BTreeSet<(usize, usize)>,
    reverse_topological_order: &mut Vec<(usize, usize)>,
    current: (usize, usize),
) -> bool {
    if discovered.contains(&current) {
        return false;
    }
    if currently_visiting.contains(&current) {
        return true;
    }
    currently_visiting.insert(current);
    // `edges.getOrDefault(current, ImmutableSet.of())` — an absent node has no
    // successors.
    if let Some(next_nodes) = edges.get(&current) {
        for &next in next_nodes {
            if depth_first_search(
                edges,
                discovered,
                currently_visiting,
                reverse_topological_order,
                next,
            ) {
                return true;
            }
        }
    }
    currently_visiting.remove(&current);
    discovered.insert(current);
    reverse_topological_order.push(current);
    false
}

/// Java's reduction probe catches `IllegalStateException` (the cycle
/// diagnostics); anything else propagates. Both cycle messages start with this
/// prefix; the plain cycle panic's payload is a `&str` literal, the
/// involved-sources one a formatted `String`.
fn is_cycle_error(payload: &(dyn Any + Send)) -> bool {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.starts_with("Feature order cycle found");
    }
    payload
        .downcast_ref::<&'static str>()
        .is_some_and(|message| message.starts_with("Feature order cycle found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
    use crate::levelgen::feature::{ConfiguredFeatureErased, FeatureId};
    use std::sync::Arc;

    /// The test feature source — a `List<HolderSet<PlacedFeature>>` standing in
    /// for a biome's `BiomeGenerationSettings.features`. No `PartialEq`
    /// (`PlacedFeature` derives only `Debug`/`Clone`), like the production type.
    #[derive(Debug, Clone)]
    struct Source(Vec<HolderSet<PlacedFeature>>);

    impl Source {
        fn steps(&self) -> &[HolderSet<PlacedFeature>] {
            &self.0
        }
    }

    /// A registry-backed feature holder in the (fabricated) placed-feature
    /// registry 1. `id` doubles as the distinguishing feature identity.
    fn feature(id: u32) -> Holder<PlacedFeature> {
        Holder::Reference {
            registry: RegistryId(1),
            id,
        }
    }

    fn step(features: &[Holder<PlacedFeature>]) -> HolderSet<PlacedFeature> {
        HolderSet::direct(features.to_vec())
    }

    fn build(sources: &[Source], try_reducing_error: bool) -> Vec<StepFeatureData> {
        build_features_per_step(sources, |s: &Source| s.steps(), try_reducing_error)
    }

    /// The holder ids of a `StepFeatureData.features` list, so orderings can be
    /// asserted without `PlacedFeature` equality (it has none).
    fn ids(holders: &[Holder<PlacedFeature>]) -> Vec<u32> {
        holders
            .iter()
            .map(|h| match h {
                Holder::Reference { id, .. } => *id,
                Holder::Direct(_) => panic!("test sources use Reference holders"),
            })
            .collect()
    }

    /// A `PlacedFeature` wrapping a Direct no-op configured feature.
    fn no_op_placed(feature_id: u32) -> PlacedFeature {
        PlacedFeature::new(
            Holder::direct(ConfiguredFeatureErased {
                feature: FeatureId::new(feature_id),
                config: Arc::new(NoneFeatureConfiguration),
            }),
            Vec::new(),
        )
    }

    /// The Debug rendering of one surviving source in the cycle message —
    /// `Source([Direct([Reference { registry: RegistryId(1), id: n }, ...])])`.
    fn source_pat(ids: &[u32]) -> String {
        let inner = ids
            .iter()
            .map(|&id| format!("Reference {{ registry: RegistryId(1), id: {id} }}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("Source([Direct([{inner}])])")
    }

    fn panic_message(result: std::thread::Result<Vec<StepFeatureData>>) -> String {
        match result {
            Ok(_) => panic!("expected a panic, got Ok"),
            Err(payload) => {
                if let Some(message) = payload.downcast_ref::<String>() {
                    message.clone()
                } else if let Some(message) = payload.downcast_ref::<&str>() {
                    (*message).to_string()
                } else {
                    format!("{:?}", payload)
                }
            }
        }
    }

    #[test]
    fn cross_biome_ordering_matches_hand_traced_paper() {
        // Paper-grounded: hand-trace of `buildFeaturesPerStep` over
        //   B1 = [[A, B], [C]]   (A and B in step 0, C in step 1)
        //   B2 = [[C]]           (C in step 0)
        // with `featureIndex` assigned in source/step/holder order:
        //   A -> 0, B -> 1 (from B1 step 0), C -> 2 (from B1 step 1); B2's C
        //   reuses 2.
        // Edges (source chains), nodes as (step, globalIndex):
        //   B1 A->B->C = (0,0)->(0,1)->(1,2); B2 C = (0,2)->{}.
        // DFS from the sorted keys [(0,0),(0,1),(0,2),(1,2)] visits
        //   (0,0): post-order (1,2),(0,1),(0,0); then (0,2): post-order (0,2);
        //   (0,1) and (1,2) are already discovered.
        // Reversed: [(0,2),(0,0),(1,0),(2,1)] -> step 0 = [C, A, B], step 1 = [C].
        let sources = [
            Source(vec![step(&[feature(0), feature(1)]), step(&[feature(2)])]),
            Source(vec![step(&[feature(2)])]),
        ];
        let result = build(&sources, false);
        assert_eq!(result.len(), 2);
        assert_eq!(ids(&result[0].features), vec![2, 0, 1]);
        assert_eq!(ids(&result[1].features), vec![2]);
    }

    #[test]
    fn duplicate_holder_identity_collapses_to_one_global_index() {
        // The same `Reference` holder seen in several biomes and steps maps to
        // one global first-appearance index, and to one per-step entry.
        let shared = feature(7);
        let other = feature(3);
        let sources = [
            // Step 0: [shared, other]; step 1: [shared].
            Source(vec![
                step(&[shared.clone(), other.clone()]),
                step(std::slice::from_ref(&shared)),
            ]),
            // Step 2: [shared] again — new index only for other features.
            Source(vec![
                step(std::slice::from_ref(&other)),
                step(&[]),
                step(std::slice::from_ref(&shared)),
            ]),
        ];
        let result = build(&sources, false);
        // maxStep = 3.
        assert_eq!(result.len(), 3);
        // Step 0's features both appear once despite shared appearing in two
        // sources' step 0.
        assert_eq!(result[0].features.len(), 2);
        assert_eq!(
            result[0].index_mapping(&shared),
            Some(0),
            "shared is first in step 0's topological order"
        );
        assert_eq!(result[0].index_mapping(&other), Some(1));
        assert_eq!(result[2].features.len(), 1);
        assert_eq!(result[2].index_mapping(&shared), Some(0));
        // The same holder maps to the same per-step global index regardless of
        // which source's step it came from — the identity `setFeatureSeed` is
        // seeded with.
        assert_eq!(
            result[0].index_mapping(&shared),
            result[1].index_mapping(&shared)
        );
    }

    #[test]
    fn per_step_global_index_mapping_feeds_set_feature_seed() {
        // The decoration pass computes, per step, the sorted set of global
        // indices via `indexMapping().applyAsInt`, then seeds
        // `random.setFeatureSeed(seed, globalIndexOfFeature, stepIndex)` and
        // places `stepFeatureData.features().get(globalIndexOfFeature)`. Pin
        // that the mapping is a bijection into `features` for every step.
        let sources = [Source(vec![
            step(&[feature(30), feature(10)]),
            step(&[feature(20)]),
        ])];
        let result = build(&sources, false);
        for step_data in &result {
            for (position, holder) in step_data.features.iter().enumerate() {
                assert_eq!(
                    step_data.index_mapping(holder),
                    Some(position),
                    "index_mapping must round-trip through features().get"
                );
            }
            assert!(step_data.index_mapping(&feature(999)).is_none());
        }
        // Exhaustively: the two-step source yields step 0 = [z, x], step 1 = [y]
        // (z gets global index 0, x index 1, y index 2; DFS over the chain
        // (0,0)->(0,1)->(1,2) reversed gives step 0 [z, x] and step 1 [y]).
        assert_eq!(ids(&result[0].features), vec![30, 10]);
        assert_eq!(ids(&result[1].features), vec![20]);
    }

    #[test]
    fn same_feature_in_adjacent_steps_groups_by_step() {
        // A feature in step 0 and step 1 of one source is two nodes chained in
        // the graph; each step lists it once.
        let sources = [Source(vec![step(&[feature(0)]), step(&[feature(0)])])];
        let result = build(&sources, false);
        assert_eq!(result.len(), 2);
        assert_eq!(ids(&result[0].features), vec![0]);
        assert_eq!(ids(&result[1].features), vec![0]);
    }

    #[test]
    fn direct_holder_identity_is_per_instance() {
        // Java keys `Object2IntMap<PlacedFeature>` on the value object's
        // reference identity: two distinct `PlacedFeature` objects are distinct
        // keys even with equal contents. The Rust spelling keys a `Direct`
        // holder on its inline value's address (Java's per-object identity), so
        // distinct Direct instances are distinct identities. (`Reference`
        // holders — the production path — are keyed on the (registry, id) pair
        // instead, so clones there collapse.)
        let a = Holder::direct(no_op_placed(0));
        let b = Holder::direct(no_op_placed(0)); // equal contents, distinct instance
        let sources = [Source(vec![step(&[a.clone(), b.clone()])])];
        let result = build(&sources, false);
        // Distinct Direct instances are distinct identities: no content dedup.
        assert_eq!(result[0].features.len(), 2);
        // The mapping round-trips for the holder instances the build saw (the
        // ones inside the source's holder set) and for the clones stored in
        // `StepFeatureData.features` (both spellings are registered).
        let set = &sources[0].0[0];
        assert_eq!(result[0].index_mapping(set.get(0)), Some(0));
        assert_eq!(result[0].index_mapping(set.get(1)), Some(1));
        for (position, holder) in result[0].features.iter().enumerate() {
            assert_eq!(result[0].index_mapping(holder), Some(position));
        }
    }

    #[test]
    fn empty_sources_produce_no_steps() {
        assert!(build(&[], false).is_empty());
    }

    #[test]
    fn max_step_keeps_empty_leading_and_trailing_steps() {
        // A source with an empty step still contributes its length to maxStep,
        // so empty `StepFeatureData` entries are produced for steps with no
        // features (matching the per-source `maxStep`).
        let sources = [Source(vec![step(&[]), step(&[feature(0)]), step(&[])])];
        let result = build(&sources, false);
        assert_eq!(result.len(), 3);
        assert!(result[0].features.is_empty());
        assert_eq!(ids(&result[1].features), vec![0]);
        assert!(result[2].features.is_empty());
        assert!(result[0].index_mapping(&feature(0)).is_none());
    }

    #[test]
    fn same_step_duplicate_is_a_cycle() {
        // A feature appearing twice in the same source/step self-chains the
        // node (the flattened list's second element is the first's successor),
        // which DFS reads as a back-edge — Java reports a cycle too.
        let a = feature(0);
        let sources = [Source(vec![step(&[a.clone(), a])])];
        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&sources, false)));
        assert_eq!(panic_message(panic), "Feature order cycle found");
    }

    #[test]
    fn two_source_cycle_reports_involved_sources() {
        // A -> B (source 0), B -> A (source 1): a 2-cycle. The reduction
        // removes neither (removing either leaves an acyclic residual), so the
        // involved-source list is both sources.
        let sources = [
            Source(vec![step(&[feature(0), feature(1)])]),
            Source(vec![step(&[feature(1), feature(0)])]),
        ];
        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&sources, true)));
        let message = panic_message(panic);
        assert!(message.starts_with("Feature order cycle found, involved sources: ["));
        assert!(message.contains(&source_pat(&[0, 1])));
        assert!(message.contains(&source_pat(&[1, 0])));
    }

    #[test]
    fn redundant_cycle_source_is_dropped_by_reduction() {
        // s0 = A->B->C->D, s1 = D->A, s2 = B->A. Removing s1 leaves the
        // residual [s0, s2], which still has the A->B->A cycle; removing s0 or
        // s2 leaves an acyclic residual. So s1 is dropped and the message
        // reports the surviving [s0, s2].
        let sources = [
            Source(vec![step(&[
                feature(0),
                feature(1),
                feature(2),
                feature(3),
            ])]),
            Source(vec![step(&[feature(3), feature(0)])]),
            Source(vec![step(&[feature(1), feature(0)])]),
        ];
        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&sources, true)));
        let message = panic_message(panic);
        assert!(message.starts_with("Feature order cycle found, involved sources: ["));
        assert!(message.contains(&source_pat(&[0, 1, 2, 3]))); // s0 survives
        assert!(message.contains(&source_pat(&[1, 0]))); // s2 survives
        assert!(!message.contains(&source_pat(&[3, 0]))); // s1 was dropped
    }

    #[test]
    fn try_reducing_error_false_reports_plain_cycle() {
        // The same graph with `tryReducingError = false` throws the plain cycle
        // message, not the involved-sources one.
        let sources = [
            Source(vec![step(&[feature(0), feature(1)])]),
            Source(vec![step(&[feature(1), feature(0)])]),
        ];
        let panic =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&sources, false)));
        assert_eq!(panic_message(panic), "Feature order cycle found");
    }
}
