//! Port of `net.minecraft.util.valueproviders` (17 classes, 26.2).
//!
//! The value-provider framework: `IntProvider` / `FloatProvider` sampled
//! distributions used by worldgen (features, surface rules, loot) and the
//! `SampledFloat` float-sampling leaf (`MultipliedFloats`). The dispatch roots
//! `IntProviders.CODEC` / `FloatProviders.CODEC` deserialize either a bare
//! constant (`Codec.INT` / `Codec.FLOAT`) or a discriminated record whose
//! `"type"` key names the concrete provider via the provider-type registry.
//!
//! The Java class hierarchy is an open interface set; all implementors live in
//! this package and are known statically, so the port collapses each root to a
//! closed enum — the same shape `rivet-world`'s `HeightProvider`/`BlockPredicate`
//! dispatch roots take (see the per-module provenance headers).
//!
//! ## Registry cycle avoidance
//!
//! Java resolves the provider-type registry through
//! `BuiltInRegistries.INT_PROVIDER_TYPE` / `FLOAT_PROVIDER_TYPE`. `rivet-util`
//! cannot depend on `rivet-registry` (the `Identifier` / `Registry` types), so
//! the port uses a closed string-based namespaced type lookup
//! ([`int_provider_type::int_provider_type_by_name`] /
//! [`float_provider_type::float_provider_type_by_name`]) that reproduces the
//! by-name codec's observable behavior: bare names default to `minecraft:`, and
//! the unknown-key error is byte-identical to Paper's
//! (`Unknown registry key in ResourceKey[minecraft:root /
//! minecraft:int_provider_type]: {name}`). The only divergence is the
//! `Identifier` malformed-string diagnostic: Paper's `Identifier` codec rejects
//! structurally invalid names (empty path, illegal characters) before the
//! registry lookup, while the closed lookup accepts any string and fails with
//! the unknown-key message. Documented divergence; nothing in Paper's usage of
//! these codecs feeds malformed identifiers.
//!
//! ## Recursive `IntProvider` dispatch
//!
//! `IntProviders.CODEC` is recursive (`ClampedInt` embeds a `source:
//! IntProvider`, `WeightedListInt` a `WeightedList<IntProvider>`), threaded
//! through a `codec::recursive` graph exactly like `BlockPredicate.CODEC` and
//! `HeightProvider.CODEC`. `FloatProviders.CODEC` has no recursion (no float
//! provider embeds a `FloatProvider`), matching Java.
//!
//! ## Float-field JSON encode
//!
//! A `Codec.FLOAT` field encodes through `JsonOps.createFloat`, which stores the
//! `f64` nearest Java's `Float.toString` literal — Gson renders a
//! `JsonPrimitive(Float)` with `Float.toString`, so `0.05f` writes `0.05`, not
//! the widened `0.05000000074505806`. This matches Paper for `UniformFloat`,
//! `TrapezoidFloat`, `ClampedNormalFloat`, and bare `ConstantFloat` encodes (see
//! `create_float_uses_float_to_string_literal` in `rivet-serialization`'s
//! `json_ops_tests`; `float_provider_round_trips` pins the shape end-to-end).

pub mod biased_to_bottom_int;
pub mod clamped_int;
pub mod clamped_normal_float;
pub mod clamped_normal_int;
pub mod constant_float;
pub mod constant_int;
pub mod float_provider;
pub mod float_provider_type;
pub mod int_provider;
pub mod int_provider_type;
pub mod multiplied_floats;
pub mod sampled_float;
pub mod trapezoid_float;
pub mod trapezoid_int;
pub mod uniform_float;
pub mod uniform_int;
pub mod weighted_list_int;

#[cfg(test)]
mod tests;

/// `Identifier` default-namespace normalization — the `minecraft:` default
/// namespace, exactly like Java's `Identifier.withDefaultNamespace` (the
/// `bySeparator` no-namespace / empty-namespace path). A bare `path` becomes
/// `minecraft:path`; a leading colon (`:path`) is stripped and also becomes
/// `minecraft:path` (Java treats a separator at index 0 as an empty namespace);
/// names already carrying a namespace pass through unchanged.
pub(crate) fn default_namespace(name: &str) -> String {
    if let Some(path) = name.strip_prefix(':') {
        format!("minecraft:{path}")
    } else if name.contains(':') {
        name.to_string()
    } else {
        format!("minecraft:{name}")
    }
}
