//! `net.minecraft.world.flag.FeatureFlags` — the vanilla flag registry + the
//! world-configuration statics (#387).
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/flag/
//! FeatureFlags.java`. The `static` initializer block runs once, in
//! declaration order, and every constant is a `static final` built from it.
//! The Rust port mirrors that with `LazyLock` statics (the registry owns an
//! `Identifier` — a `String` — and an `Arc`, so nothing here is `const`, the
//! same pattern as `rivet-registry::registries`).
//!
//! The `CODEC`/`VANILLA_SET`/`DEFAULT_FLAGS` accessors are the consumers the
//! `WorldDataConfiguration.MAP_CODEC` and `FeatureFlags.CODEC` reference in
//! Java; `REGISTRY` is the `codec()`/`fromNames`/`toNames` owner. The Java
//! static's per-flag bit positions are implicitly observable through
//! `allFlags`/`isSubset`/the codec; the port exposes them exactly as the
//! builder assigned them (declaration order, `vanilla` = bit 0).

use super::feature_flag::FeatureFlag;
use super::feature_flag_registry::{Builder, FeatureFlagRegistry};
use super::feature_flag_set::FeatureFlagSet;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::{Arc, LazyLock};

/// `FeatureFlags.REGISTRY` — the `"main"` registry with all four vanilla
/// flags, in declaration order.
pub static REGISTRY: LazyLock<FeatureFlagRegistry> = LazyLock::new(|| {
    let mut builder = Builder::new("main".to_string());
    builder.create_vanilla("vanilla");
    builder.create_vanilla("trade_rebalance");
    builder.create_vanilla("redstone_experiments");
    builder.create_vanilla("minecart_improvements");
    builder.build()
});

/// `FeatureFlags.VANILLA` — `builder.createVanilla("vanilla")`, bit 0.
pub static VANILLA: LazyLock<FeatureFlag> = LazyLock::new(|| REGISTRY.names[0].1.clone());

/// `FeatureFlags.TRADE_REBALANCE` — bit 1.
pub static TRADE_REBALANCE: LazyLock<FeatureFlag> = LazyLock::new(|| REGISTRY.names[1].1.clone());

/// `FeatureFlags.REDSTONE_EXPERIMENTS` — bit 2.
pub static REDSTONE_EXPERIMENTS: LazyLock<FeatureFlag> =
    LazyLock::new(|| REGISTRY.names[2].1.clone());

/// `FeatureFlags.MINECART_IMPROVEMENTS` — bit 3.
pub static MINECART_IMPROVEMENTS: LazyLock<FeatureFlag> =
    LazyLock::new(|| REGISTRY.names[3].1.clone());

/// `FeatureFlags.CODEC` — `REGISTRY.codec()`, ops-generic.
pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FeatureFlagSet, Ops>> {
    REGISTRY.codec()
}

/// `FeatureFlags.VANILLA_SET` — `FeatureFlagSet.of(VANILLA)`.
pub fn vanilla_set() -> FeatureFlagSet {
    FeatureFlagSet::of_flag(&VANILLA)
}

/// `FeatureFlags.DEFAULT_FLAGS` — `VANILLA_SET`.
pub fn default_flags() -> FeatureFlagSet {
    vanilla_set()
}

/// `FeatureFlags.printMissingFlags(FeatureFlagSet, FeatureFlagSet)` — the
/// `REGISTRY`-rooted comma-joined list of requested flags absent from the
/// allowed set. Not yet consumed by the #323 slice; ported for completeness.
pub fn print_missing_flags(allowed: &FeatureFlagSet, requested: &FeatureFlagSet) -> String {
    print_missing_flags_in(&REGISTRY, allowed, requested)
}

/// `FeatureFlags.printMissingFlags(FeatureFlagRegistry, ...)` — `requested
/// names` minus `allowed names`, joined with ", " in the requested-iteration
/// order (the `toNames` hash order), matching Java's `Collectors.joining`.
pub fn print_missing_flags_in(
    registry: &FeatureFlagRegistry,
    allowed: &FeatureFlagSet,
    requested: &FeatureFlagSet,
) -> String {
    let requested_ids = registry.to_names(requested);
    let allowed_ids = registry.to_names(allowed);
    requested_ids
        .iter()
        .filter(|id| !allowed_ids.contains(id))
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// `FeatureFlags.isExperimental(FeatureFlagSet)` — `!features.isSubsetOf(
/// VANILLA_SET)`.
pub fn is_experimental(features: &FeatureFlagSet) -> bool {
    !features.is_subset_of(&vanilla_set())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statics_have_declaration_bits() {
        assert_eq!(VANILLA.mask(), 1 << 0);
        assert_eq!(TRADE_REBALANCE.mask(), 1 << 1);
        assert_eq!(REDSTONE_EXPERIMENTS.mask(), 1 << 2);
        assert_eq!(MINECART_IMPROVEMENTS.mask(), 1 << 3);
        // Vanilla flags come from the "main" registry.
        assert_eq!(VANILLA.universe().to_string(), "main");
        assert_eq!(VANILLA.universe(), TRADE_REBALANCE.universe());
    }

    #[test]
    fn registry_all_flags_is_the_union() {
        let all = REGISTRY.all_flags();
        assert_eq!(all.mask(), 0b1111);
        assert!(REGISTRY.is_subset(all));
        assert_eq!(
            all,
            &REGISTRY.subset(&[
                &VANILLA,
                &TRADE_REBALANCE,
                &REDSTONE_EXPERIMENTS,
                &MINECART_IMPROVEMENTS
            ])
        );
    }

    #[test]
    fn vanilla_set_and_default_flags() {
        let v = vanilla_set();
        assert_eq!(v.mask(), 1 << 0);
        assert_eq!(default_flags(), v);
        assert_eq!(default_flags(), FeatureFlagSet::of_flag(&VANILLA));
        // DEFAULT_FLAGS is a subset of the registry's allFlags.
        assert!(REGISTRY.is_subset(&default_flags()));
    }

    #[test]
    fn codec_decodes_default_flags_list() {
        use rivet_serialization::json_ops::JsonOps;
        let ops = JsonOps::INSTANCE;
        let codec = codec::<JsonOps>();
        let input = ops.create_list(vec![ops.create_string("minecraft:vanilla".to_string())]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, default_flags());
    }

    #[test]
    fn experimental_detection() {
        assert!(!is_experimental(&default_flags()));
        assert!(is_experimental(&FeatureFlagSet::of_flag(&TRADE_REBALANCE)));
        assert!(is_experimental(&FeatureFlagSet::of_flags(
            &VANILLA,
            &[&REDSTONE_EXPERIMENTS]
        )));
        assert!(is_experimental(REGISTRY.all_flags()));
    }

    #[test]
    fn print_missing_flags_joins_requested_order() {
        // requested = {vanilla, trade_rebalance}, allowed = {vanilla} ->
        // missing "trade_rebalance".
        let requested = FeatureFlagSet::of_flags(&VANILLA, &[&TRADE_REBALANCE]);
        let allowed = vanilla_set();
        assert_eq!(
            print_missing_flags(&allowed, &requested),
            "minecraft:trade_rebalance"
        );
        // Nothing missing when allowed covers requested.
        assert_eq!(print_missing_flags(&requested, &requested), "");
        // All except vanilla missing, in the requested `toNames` (hash) order:
        // [redstone_experiments, vanilla, trade_rebalance, minecart_improvements]
        // minus the allowed vanilla -> redstone, trade, minecart.
        assert_eq!(
            print_missing_flags(&allowed, REGISTRY.all_flags()),
            "minecraft:redstone_experiments, minecraft:trade_rebalance, minecraft:minecart_improvements"
        );
    }
}
