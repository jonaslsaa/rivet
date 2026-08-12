//! `net.minecraft.world.level.levelgen.heightproviders` — the height-provider
//! value/codec layer (issue #181 leaf, unblocked by the merged VerticalAnchor
//! #388 and weighted-random #353).
//!
//! Owned by the `mc.world.level.levelgen.heightproviders` manifest unit (26.2):
//! `HeightProvider.java`, `HeightProviderType.java`, `ConstantHeight.java`,
//! `UniformHeight.java`, `BiasedToBottomHeight.java`,
//! `VeryBiasedToBottomHeight.java`, `TrapezoidHeight.java`,
//! `WeightedListHeight.java`, `package-info.java`.
//!
//! Java's `HeightProvider` is an abstract base class whose six concrete
//! subclasses are all in this package. The port mirrors the sealed surface as a
//! single [`HeightProvider`] enum over the six variants — the same shape the
//! codebase uses for sealed worldgen hierarchies (`VerticalAnchor`,
//! `GenerationStep.Decoration`). Each Java class keeps its own module file; the
//! dispatch hub (`type()`, `sample`, `HeightProvider.CODEC`) lives in
//! `height_provider`, and the type-registry identity (`HeightProviderTypeId`,
//! the six `HeightProviderTypes` constants in declaration order) in
//! `height_provider_type`.
//!
//! ## `HeightProvider.CODEC` — constant-or-dispatch, recursive
//!
//! ```java
//! CONSTANT_OR_DISPATCH_CODEC = Codec.either(
//!     VerticalAnchor.CODEC,
//!     BuiltInRegistries.HEIGHT_PROVIDER_TYPE.byNameCodec()
//!         .dispatch(HeightProvider::getType, HeightProviderType::codec));
//! CODEC = CONSTANT_OR_DISPATCH_CODEC.xmap(
//!     either -> either.map(ConstantHeight::of, f -> (HeightProvider)f),
//!     f -> f.getType() == HeightProviderType.CONSTANT
//!         ? Either.left(((ConstantHeight)f).getValue()) : Either.right(f));
//! ```
//!
//! A bare `VerticalAnchor` decodes/encodes as a `ConstantHeight` (the Left
//! branch); every other provider dispatches on the `"type"` key. The dispatch's
//! per-type `MapCodec`s resolve through [`height_provider_type_by_name`] with
//! Paper's exact `"Unknown registry key in ResourceKey[minecraft:root /
//! minecraft:height_provider_type]: {name}"` error. Because `WeightedListHeight`
//! embeds `WeightedList<HeightProvider>` (recursive), the whole codec is a
//! `codec::recursive` graph whose single `RecursiveSelf` threads into the
//! weighted-list element codec — the same pattern `BlockPredicate.CODEC` uses.
//!
//! ## Fidelity notes
//!
//! - Sampling arithmetic is Java-int wrapping throughout (PORTING.md); the
//!   `Mth.randomBetweenInclusive`/`Mth.nextInt` helpers come from
//!   `rivet-util::mth` (bit-exact LCG goldens).
//! - The `LOGGER.warn("Empty height range: {}", this)` warnings in
//!   `UniformHeight`/`BiasedToBottomHeight`/`VeryBiasedToBottomHeight`/
//!   `TrapezoidHeight` and `UniformHeight`'s `LongOpenHashSet warnedFor` dedup
//!   are dropped: the port's `log_and_pause_if_in_ide` is a documented no-op and
//!   the dedup only suppresses duplicate (no-op) warnings — it has no observable
//!   effect on the sampled value (the same precedent as the dropped IDE-only
//!   warnings in `rivet-util::weighted`).
//! - The `"inner"`/`"plateau"` optional fields are Java's
//!   `Codec.optionalFieldOf(name, default)` — the NON-lenient form (a
//!   present-but-malformed value is a decode error, unlike
//!   `lenientOptionalFieldOf`). The serialization crate only exposes the lenient
//!   default helper, so [`optional_field_of`] here rebuilds the non-lenient one
//!   (the `optionalField(name, this, false).xmap(...)` DFU body).

use rivet_serialization::codec;
use rivet_serialization::codec::Codec;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::{self, MapCodec};
use std::sync::Arc;

/// `Codec.optionalFieldOf(String, A default)` — the NON-lenient with-default
/// form of an optional field, as the ops-generic `optional_field_of::<F,
/// Ops>(name, codec, default)` factory.
///
/// Java (DFU 10.0.21, `Codec.optionalFieldOf(name, defaultValue)`):
/// `optionalField(name, this, false).xmap(o -> o.orElse(default), a ->
/// Objects.equals(a, default) ? Optional.empty() : Optional.of(a))`. Unlike
/// `lenientOptionalFieldOf`, a present-but-malformed value is a decode error
/// (the optional field is NOT lenient). The default value is omitted on encode.
fn optional_field_of<F, Ops: DynamicOps + 'static>(
    name: &str,
    element_codec: Arc<dyn Codec<F, Ops>>,
    default: F,
) -> Arc<dyn MapCodec<F, Ops>>
where
    F: 'static + Clone + PartialEq + Send + Sync,
{
    let inner = codec::optional_field(name.to_string(), element_codec, false);
    let default_for_decode = default.clone();
    let default_for_encode = default;
    map_codec::xmap(
        inner,
        Arc::new(move |o: &Option<F>| o.clone().unwrap_or_else(|| default_for_decode.clone())),
        Arc::new(move |a: &F| {
            if *a == default_for_encode {
                None
            } else {
                Some(a.clone())
            }
        }),
    )
}

pub mod biased_to_bottom_height;
pub mod constant_height;
pub mod height_provider;
pub mod height_provider_type;
pub mod trapezoid_height;
pub mod uniform_height;
pub mod very_biased_to_bottom_height;
pub mod weighted_list_height;

pub use biased_to_bottom_height::BiasedToBottomHeight;
pub use constant_height::ConstantHeight;
pub use height_provider::HeightProvider;
pub use height_provider_type::{HeightProviderTypeId, HeightProviderTypes};
pub use trapezoid_height::TrapezoidHeight;
pub use uniform_height::UniformHeight;
pub use very_biased_to_bottom_height::VeryBiasedToBottomHeight;
pub use weighted_list_height::WeightedListHeight;
