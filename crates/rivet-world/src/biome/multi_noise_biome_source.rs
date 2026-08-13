//! Port of `net.minecraft.world.level.biome.MultiNoiseBiomeSource` (26.2) — the
//! `mc.world.level.biome.source` unit.
//!
//! The multi-noise source: either a direct `ParameterList<Holder<Biome>>` or a
//! `Holder<MultiNoiseBiomeSourceParameterList>` preset, resolving
//! `getNoiseBiome` by nearest-parameter search over the resolved list.
//!
//! ```text
//! ENTRY_CODEC   = Biome.CODEC.fieldOf("biome")
//! DIRECT_CODEC  = Climate.ParameterList.codec(ENTRY_CODEC).fieldOf("biomes")
//! PRESET_CODEC  = MultiNoiseBiomeSourceParameterList.CODEC.fieldOf("preset").withLifecycle(Lifecycle.stable())
//! CODEC         = Codec.mapEither(DIRECT_CODEC, PRESET_CODEC)
//!                   .xmap(MultiNoiseBiomeSource::new, o -> o.parameters)
//! ```
//!
//! `Codec.mapEither` is DFU's `EitherMapCodec` — the port has no generic
//! `map_either` in `rivet-serialization`, so [`MultiNoiseBiomeSourceMapEither`]
//! reproduces it faithfully here (decode tries `first`, then `second`, then
//! `firstRead.apply2((f, s) -> s, secondRead)`; encode dispatches on the
//! `Either` variant; keys concat). The final xmap to the source is
//! [`MultiNoiseBiomeSourceCodec`] (Java's `xmap(MultiNoiseBiomeSource::new,
//! o -> o.parameters)` — the `from` cannot be a value-cloning `map_codec::xmap`
//! because `ParameterList` is not `Clone`, so the encode delegates by reference).
//!
//! ## The `parameters()`/`stable()` seams
//!
//! Java resolves the preset holder through the owning lookup
//! (`preset.value().parameters()` / `preset.is(expected)`) and returns the
//! *same* stored `ParameterList` object every call (its RTree's warm
//! `lastResult` hint persists across queries). The port's source is `'static`
//! and cannot carry a borrowed lookup, so the search list is resolved once at
//! construction and stored ([`MultiNoiseBiomeSource::resolved_parameters`]) —
//! the stored-list identity: the same `ParameterList`/RTree instance serves
//! every query. A `Direct` list/preset reads its values inline (the list is
//! rebuilt once over the same values — `ParameterList` is not `Clone`); a
//! `Reference` preset is resolved at decode time through the ops' lookup
//! (Java's `preset.value()`, with `preset.is(key)` stored for `stable(key)`),
//! since only the decode has the lookup. The public [`MultiNoiseBiomeSource::create_from_preset`]
//! therefore takes the resolved `MultiNoiseBiomeSourceParameterList` value —
//! a `Reference` is only ever produced by the decode path.
//!
//! `addDebugInfo` reproduces Paper's `@VisibleForDebug` line — the
//! `OverworldBiomeBuilder` debug strings over the `unquantizeCoord`-ed sample
//! and `NoiseRouterData.peaksAndValleys(weirdness)`.

use crate::biome::biome_id_codec::biome_id_field_codec;
use crate::biome::biome_source::BiomeSource;
use crate::biome::biome_source::keys;
use crate::biome::biome_source_type::{BiomeSourceTypeId, BiomeSourceTypes};
use crate::biome::climate::{self, ParameterList, Sampler};
use crate::biome::multi_noise_biome_source_parameter_list::MultiNoiseBiomeSourceParameterList;
use crate::biome::overworld_biome_builder::OverworldBiomeBuilder;
use crate::levelgen::noisegen::noise_router_data::peaks_and_valleys_f32;
use rivet_registry::ResourceKey;
use rivet_registry::biome_id::BiomeId;
use rivet_registry::core::{BlockPos, QuartPos};
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderLookup;
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::either::Either;
use rivet_serialization::lifecycle::Lifecycle;
use rivet_serialization::map_codec::{self, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::sync::{Arc, OnceLock};

/// `MultiNoiseBiomeSource` — the direct-list or preset-backed multi-noise
/// source.
pub struct MultiNoiseBiomeSource {
    /// `this.parameters` — `Either<ParameterList<Holder<Biome>>, Holder<
    /// MultiNoiseBiomeSourceParameterList>>`. Retained for the codec's encode
    /// and for equality; the search list is [`MultiNoiseBiomeSource::resolved_parameters`].
    parameters: Either<ParameterList<Holder<BiomeId>>, Holder<MultiNoiseBiomeSourceParameterList>>,
    /// Java's stored `parameters()` list — the resolved search list, fixed at
    /// construction so every query reuses the same `ParameterList`/RTree
    /// instance (the warm `lastResult` hint persists across queries, exactly
    /// like Java returning the same list object). A `Direct` list/preset is
    /// rebuilt once over the same values (`ParameterList` is not `Clone`); a
    /// `Reference` preset is resolved at decode. Not part of equality (derived
    /// from `parameters`).
    resolved_parameters: ParameterList<Holder<BiomeId>>,
    /// The resolved preset key of a `Reference` preset source — Java
    /// `preset.get().is(expected)` for [`MultiNoiseBiomeSource::stable`] (a
    /// `Reference` is the only keyed preset; a `Direct` preset is never keyed
    /// and a direct-list source has no right arm). Not part of equality
    /// (derived from the `Reference` arm of `parameters`).
    preset_key: Option<ResourceKey<MultiNoiseBiomeSourceParameterList>>,
    /// The `possibleBiomes` memo — Java's `Suppliers.memoize` (computed once on
    /// first read). Not part of equality (the derived cache value).
    possible_biomes: OnceLock<Vec<Holder<BiomeId>>>,
}

impl std::fmt::Debug for MultiNoiseBiomeSource {
    /// `ParameterList` is not `Debug` (the RTree index), so the struct renders
    /// the resolved entry values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiNoiseBiomeSource")
            .field("parameters", &self.parameters().values())
            .finish()
    }
}

impl Clone for MultiNoiseBiomeSource {
    /// `ParameterList` is not `Clone` (the RTree index), so the resolved list
    /// is rebuilt from its values.
    fn clone(&self) -> Self {
        MultiNoiseBiomeSource {
            parameters: match &self.parameters {
                Either::Left(list) => Either::Left(ParameterList::new(list.values().to_vec())),
                Either::Right(holder) => Either::Right(holder.clone()),
            },
            resolved_parameters: ParameterList::new(self.resolved_parameters.values().to_vec()),
            preset_key: self.preset_key.clone(),
            possible_biomes: OnceLock::new(),
        }
    }
}

impl PartialEq for MultiNoiseBiomeSource {
    fn eq(&self, other: &Self) -> bool {
        match (&self.parameters, &other.parameters) {
            (Either::Left(a), Either::Left(b)) => a.values() == b.values(),
            (Either::Right(a), Either::Right(b)) => a == b,
            _ => false,
        }
    }
}

impl MultiNoiseBiomeSource {
    /// `new MultiNoiseBiomeSource(Either, resolved, presetKey)` — the private
    /// constructor (Java's `new MultiNoiseBiomeSource(Either)`); the resolved
    /// search list and preset key are fixed here (the `parameters()`/
    /// `stable(key)` seams, see the module docs).
    fn new(
        parameters: Either<
            ParameterList<Holder<BiomeId>>,
            Holder<MultiNoiseBiomeSourceParameterList>,
        >,
        resolved_parameters: ParameterList<Holder<BiomeId>>,
        preset_key: Option<ResourceKey<MultiNoiseBiomeSourceParameterList>>,
    ) -> Self {
        MultiNoiseBiomeSource {
            parameters,
            resolved_parameters,
            preset_key,
            possible_biomes: OnceLock::new(),
        }
    }

    /// `MultiNoiseBiomeSource.createFromList(Climate.ParameterList<Holder<
    /// Biome>>)`.
    pub fn create_from_list(parameters: ParameterList<Holder<BiomeId>>) -> Self {
        let resolved = ParameterList::new(parameters.values().to_vec());
        MultiNoiseBiomeSource::new(Either::left(parameters), resolved, None)
    }

    /// `MultiNoiseBiomeSource.createFromPreset(Holder<MultiNoiseBiomeSource
    /// ParameterList>)` for the direct-value subset — the only subset the port
    /// can resolve without a lookup.
    ///
    /// The source is `'static` and cannot carry the owning lookup, so a
    /// `Reference` preset (which the pure-id holder stores as `(RegistryId,
    /// id)`) cannot be resolved here. The API therefore takes the resolved
    /// [`MultiNoiseBiomeSourceParameterList`] value — a `Reference` is only
    /// ever produced by the decode path ([`from_decoded`]), which has the ops'
    /// lookup.
    pub fn create_from_preset(preset: MultiNoiseBiomeSourceParameterList) -> Self {
        let resolved = ParameterList::new(preset.parameters().values().to_vec());
        MultiNoiseBiomeSource::new(Either::right(Holder::Direct(preset)), resolved, None)
    }

    /// `MultiNoiseBiomeSource.parameters()` — Java's stored list: the same
    /// `ParameterList` (and its RTree warm cache) serves every query. The port
    /// fixes the resolved list at construction ([`MultiNoiseBiomeSource::resolved_parameters`])
    /// and returns a reference to it — no per-query RTree rebuild.
    fn parameters(&self) -> &ParameterList<Holder<BiomeId>> {
        &self.resolved_parameters
    }

    /// `MultiNoiseBiomeSource.stable(ResourceKey<MultiNoiseBiomeSourceParameter
    /// List>)` — `parameters.right().isPresent() && preset.get().is(expected)`.
    ///
    /// A `Direct` preset is never keyed (Java `Direct.is` returns false) and a
    /// direct-list source has no right arm, so both report `false`. A
    /// `Reference` preset carries its key — resolved at decode and stored as
    /// [`MultiNoiseBiomeSource::preset_key`] — so the check is the stored-key
    /// compare (Java `Holder.Reference.is(key)`).
    pub fn stable(&self, expected: &ResourceKey<MultiNoiseBiomeSourceParameterList>) -> bool {
        self.preset_key.as_ref() == Some(expected)
    }

    /// `MultiNoiseBiomeSource.CODEC` — the map-either over `DIRECT_CODEC` and
    /// `PRESET_CODEC`, xmapped to the source (see the module docs).
    pub fn map_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
    -> Arc<dyn MapCodec<MultiNoiseBiomeSource, Ops>> {
        // `ENTRY_CODEC = Biome.CODEC.fieldOf("biome")` — the id-element field.
        let entry_codec = biome_id_field_codec::<Ops>("biome");
        // `DIRECT_CODEC = Climate.ParameterList.codec(ENTRY_CODEC).fieldOf("biomes")`.
        let direct_codec = codec::field_of(
            ParameterList::codec::<Ops>(entry_codec),
            "biomes".to_string(),
        );
        // `PRESET_CODEC = MultiNoiseBiomeSourceParameterList.CODEC.fieldOf("preset")
        // .withLifecycle(Lifecycle.stable())`.
        let preset_codec = map_codec::with_lifecycle(
            codec::field_of(
                MultiNoiseBiomeSourceParameterList::codec::<Ops>(),
                "preset".to_string(),
            ),
            Lifecycle::stable(),
        );
        let map_either = Arc::new(MultiNoiseBiomeSourceMapEither {
            first: direct_codec,
            second: preset_codec,
            _marker: std::marker::PhantomData,
        });
        Arc::new(MultiNoiseBiomeSourceCodec {
            inner: map_either,
            _marker: std::marker::PhantomData,
        })
    }
}

/// `Codec.mapEither(DIRECT_CODEC, PRESET_CODEC)` — DFU's `EitherMapCodec` (see
/// the module docs; `rivet-serialization` has no generic `map_either`).
struct MultiNoiseBiomeSourceMapEither<F, S, Ops: DynamicOps + 'static>
where
    F: Send + Sync,
    S: Send + Sync,
{
    first: Arc<dyn MapCodec<F, Ops>>,
    second: Arc<dyn MapCodec<S, Ops>>,
    _marker: std::marker::PhantomData<(F, S)>,
}

impl<F, S, Ops: DynamicOps + 'static> std::fmt::Debug for MultiNoiseBiomeSourceMapEither<F, S, Ops>
where
    F: Send + Sync,
    S: Send + Sync,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "EitherMapCodec[{:?}, {:?}]", self.first, self.second)
    }
}

impl<F, S, Ops: DynamicOps + 'static> Keyable<Ops> for MultiNoiseBiomeSourceMapEither<F, S, Ops>
where
    F: Send + Sync,
    S: Send + Sync,
{
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        // Java `Stream.concat(first.keys(ops), second.keys(ops))`.
        let mut keys = self.first.keys(ops);
        keys.extend(self.second.keys(ops));
        keys
    }
}

impl<F, S, Ops: DynamicOps + 'static> MapDecoder<Either<F, S>, Ops>
    for MultiNoiseBiomeSourceMapEither<F, S, Ops>
where
    F: Send + Sync,
    S: Send + Sync,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<Either<F, S>> {
        // Java: `first.decode(ops, input).map(Either::left)`; success returns.
        let first_read = self.first.decode(ops, input).map_owned(|v| Either::left(v));
        if first_read.is_success() {
            return first_read;
        }
        // Java: `second.decode(ops, input).map(Either::right)`; success returns.
        let second_read = self
            .second
            .decode(ops, input)
            .map_owned(|v| Either::right(v));
        if second_read.is_success() {
            return second_read;
        }
        // Java: `firstRead.apply2((f, s) -> s, secondRead)` — both reads failed
        // (reached only when neither is a success), so this is the curried
        // `Applicative.ap2` fallback. `ParameterList` is not `Clone`, so the
        // generic `DataResult::apply2` (which bounds `T: Clone`) is reproduced
        // inline: the message is `second + "; " + first`, and the partial is the
        // second's partial when both reads carry one (the `(f, s) -> s` function
        // discards the first).
        map_either_fallback(first_read, second_read)
    }
}

impl<F, S, Ops: DynamicOps + 'static> MapEncoder<Either<F, S>, Ops>
    for MultiNoiseBiomeSourceMapEither<F, S, Ops>
where
    F: Send + Sync,
    S: Send + Sync,
{
    fn encode(
        &self,
        input: &Either<F, S>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        // Java: `input.map(v1 -> first.encode(v1, ops, prefix), v2 -> second.
        // encode(v2, ops, prefix))` — a manual match (the closures would both
        // require unique access to `prefix`).
        match input {
            Either::Left(value1) => self.first.encode(value1, ops, prefix),
            Either::Right(value2) => self.second.encode(value2, ops, prefix),
        }
    }
}

impl<F, S, Ops: DynamicOps + 'static> MapCodec<Either<F, S>, Ops>
    for MultiNoiseBiomeSourceMapEither<F, S, Ops>
where
    F: Send + Sync,
    S: Send + Sync,
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<Either<F, S>> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &Either<F, S>,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

/// The `mapEither(...).xmap(MultiNoiseBiomeSource::new, o -> o.parameters)`
/// wrap — decode maps the `Either` to the source (the decode-built list is
/// owned, so no clone is needed); encode delegates the source's `parameters`
/// field by reference (Java's `comap` with `o -> o.parameters`).
#[allow(clippy::type_complexity)] // The map-either element type, mirroring Java's `EitherMapCodec`.
struct MultiNoiseBiomeSourceCodec<Ops: DynamicOps + 'static> {
    inner: Arc<
        dyn MapCodec<
                Either<ParameterList<Holder<BiomeId>>, Holder<MultiNoiseBiomeSourceParameterList>>,
                Ops,
            >,
    >,
    _marker: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for MultiNoiseBiomeSourceCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MultiNoiseBiomeSourceCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for MultiNoiseBiomeSourceCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        self.inner.keys(ops)
    }
}

impl<Ops: DynamicOps + 'static + RegistryOpsLookup> MapDecoder<MultiNoiseBiomeSource, Ops>
    for MultiNoiseBiomeSourceCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<MultiNoiseBiomeSource> {
        // Java's `xmap(MultiNoiseBiomeSource::new, ...)` — the decode-built
        // `Either` resolves any `Reference` preset through the ops' lookup
        // ([`from_decoded`]).
        self.inner
            .decode(ops, input)
            .flat_map(|either| from_decoded(either, ops))
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<MultiNoiseBiomeSource, Ops>
    for MultiNoiseBiomeSourceCodec<Ops>
{
    fn encode(
        &self,
        input: &MultiNoiseBiomeSource,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.inner.encode(&input.parameters, ops, prefix);
    }
}

impl<Ops: DynamicOps + 'static + RegistryOpsLookup> MapCodec<MultiNoiseBiomeSource, Ops>
    for MultiNoiseBiomeSourceCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<MultiNoiseBiomeSource> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &MultiNoiseBiomeSource,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

/// The decode-time construction — Java's `xmap(MultiNoiseBiomeSource::new, ...)`
/// with the preset holder resolved through the ops' lookup. A `Reference`
/// preset is resolved to its stored `MultiNoiseBiomeSourceParameterList`
/// (Java `preset.value().parameters()`) and key (Java `preset.is(key)`); a
/// `Direct` list/preset reads its values inline. An unresolvable reference
/// fails the decode with Java's unbound-value text (a decode-constructed
/// reference always resolves — `RegistryFileCodec` only yields references the
/// getter found — so the failure is defensive).
fn from_decoded<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    either: Either<ParameterList<Holder<BiomeId>>, Holder<MultiNoiseBiomeSourceParameterList>>,
    ops: &Ops,
) -> DataResult<MultiNoiseBiomeSource> {
    match either {
        Either::Left(direct) => {
            let resolved = ParameterList::new(direct.values().to_vec());
            DataResult::success(MultiNoiseBiomeSource::new(
                Either::left(direct),
                resolved,
                None,
            ))
        }
        Either::Right(Holder::Direct(list)) => {
            let resolved = ParameterList::new(list.parameters().values().to_vec());
            DataResult::success(MultiNoiseBiomeSource::new(
                Either::right(Holder::Direct(list)),
                resolved,
                None,
            ))
        }
        Either::Right(holder @ Holder::Reference { registry, .. }) => {
            match resolve_reference_preset(&holder, ops) {
                Some((resolved, preset_key)) => DataResult::success(MultiNoiseBiomeSource::new(
                    Either::right(holder),
                    resolved,
                    Some(preset_key),
                )),
                None => DataResult::error(format!(
                    "Trying to access unbound value 'null' from registry {}",
                    registry.0
                )),
            }
        }
    }
}

/// Resolve a `Reference` preset through the ops' lookup — Java's
/// `preset.value().parameters()` (the search list) and `preset.get().key()`
/// (for `stable(key)`).
///
/// The pure-id holder stores no value, so the resolution must go through the
/// owning registry's `HolderLookup` (`RegistryAccess::lookup` at the sanctioned
/// erased boundary). `None` when the registry is absent, the holder references
/// a different registry, or the id does not resolve.
fn resolve_reference_preset<Ops: DynamicOps + 'static + RegistryOpsLookup>(
    holder: &Holder<MultiNoiseBiomeSourceParameterList>,
    ops: &Ops,
) -> Option<(
    ParameterList<Holder<BiomeId>>,
    ResourceKey<MultiNoiseBiomeSourceParameterList>,
)> {
    let registry_key = &*keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST;
    let erased =
        rivet_registry::ResourceKey::create_registry_key(registry_key.identifier().clone());
    let info = ops.lookup_provider().lookup_erased(&erased)?;
    // The holder must reference THIS registry (the pure-id analogue of Java's
    // `Reference` carrying its owning registry's key/value).
    if holder.registry_id() != Some(info.registry_id) {
        return None;
    }
    let registry = info
        .access
        .lookup::<MultiNoiseBiomeSourceParameterList>(registry_key)?;
    let list = registry.value_of(holder)?;
    let preset_key = registry.key_of(holder)?;
    Some((
        ParameterList::new(list.parameters().values().to_vec()),
        preset_key,
    ))
}

/// The `firstRead.apply2((f, s) -> s, secondRead)` fallback, specialized to the
/// non-`Clone` `Either<F, S>` (see [`MultiNoiseBiomeSourceMapEither::decode`]).
///
/// Both reads are errors here (the success returns already happened), so the
/// fast path of `Applicative.ap2` is unreachable and only the curried fallback
/// runs: `second.ap(first.ap(curried))` yields `appendMessages(second, first)`
/// and a partial only when *both* reads carry one (the `(f, s) -> s` function
/// keeps the second's).
fn map_either_fallback<F, S>(
    first: DataResult<Either<F, S>>,
    second: DataResult<Either<F, S>>,
) -> DataResult<Either<F, S>> {
    let first_lifecycle = first.lifecycle();
    let second_lifecycle = second.lifecycle();
    let first_message = first
        .error_ref()
        .map(|e| e.message().to_string())
        .unwrap_or_default();
    let second_message = second
        .error_ref()
        .map(|e| e.message().to_string())
        .unwrap_or_default();
    // Consume for the partials (borrowed messages were extracted first).
    let first_partial = first.result_or_partial_silent();
    let second_partial = second.result_or_partial_silent();
    let partial = match (&first_partial, second_partial) {
        (Some(_), Some(second_value)) => Some(second_value),
        _ => None,
    };
    let combined = Lifecycle::experimental()
        .add(first_lifecycle)
        .add(second_lifecycle);
    DataResult::error_with_partial_lifecycle(
        format!("{}; {}", second_message, first_message),
        partial,
        combined,
    )
}

impl BiomeSource for MultiNoiseBiomeSource {
    fn type_id(&self) -> BiomeSourceTypeId {
        BiomeSourceTypes::MULTI_NOISE
    }

    fn collect_possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        // Java: `parameters().values().stream().map(Pair::getSecond)`.
        self.parameters()
            .values()
            .iter()
            .map(|(_point, value)| value.clone())
            .collect()
    }

    fn possible_biomes(&self) -> Vec<Holder<BiomeId>> {
        // Java's `Suppliers.memoize` — the collect+dedup runs once and is
        // cached (the source's parameter list is immutable, so the memo is
        // observationally identical to recomputing, but `collectPossibleBiomes`
        // rebuilds the search tree per call).
        self.possible_biomes
            .get_or_init(|| {
                crate::biome::biome_source::dedupe_possible_biomes(self.collect_possible_biomes())
            })
            .clone()
    }

    fn get_noise_biome(
        &self,
        quart_x: i32,
        quart_y: i32,
        quart_z: i32,
        sampler: &Sampler,
    ) -> Holder<BiomeId> {
        // Java: `getNoiseBiome(sampler.sample(quartX, quartY, quartZ))` →
        // `parameters().findValue(target)`.
        let target = sampler.sample(quart_x, quart_y, quart_z);
        self.parameters().find_value(&target)
    }

    fn add_debug_info(&self, result: &mut Vec<String>, feet_pos: &BlockPos, sampler: &Sampler) {
        let quart_x = QuartPos::from_block(feet_pos.get_x());
        let quart_y = QuartPos::from_block(feet_pos.get_y());
        let quart_z = QuartPos::from_block(feet_pos.get_z());
        let sample_quantized = sampler.sample(quart_x, quart_y, quart_z);
        let continentalness = climate::unquantize_coord(sample_quantized.continentalness);
        let erosion = climate::unquantize_coord(sample_quantized.erosion);
        let temperature = climate::unquantize_coord(sample_quantized.temperature);
        let humidity = climate::unquantize_coord(sample_quantized.humidity);
        let weirdness = climate::unquantize_coord(sample_quantized.weirdness);
        // Java `NoiseRouterData.peaksAndValleys(float)` returns float, widened
        // into the `double peaksAndValleys` local.
        let peaks_and_valleys = peaks_and_valleys_f32(weirdness) as f64;
        let biome_builder = OverworldBiomeBuilder::new();
        // The `getDebugStringFor*` methods take `double` in Java; the float
        // samples widen implicitly (the Rust signatures are `f64`, so the
        // widening is explicit here).
        result.push(format!(
            "Biome builder PV: {} C: {} E: {} T: {} H: {}",
            OverworldBiomeBuilder::get_debug_string_for_peaks_and_valleys(peaks_and_valleys),
            biome_builder.get_debug_string_for_continentalness(continentalness as f64),
            biome_builder.get_debug_string_for_erosion(erosion as f64),
            biome_builder.get_debug_string_for_temperature(temperature as f64),
            biome_builder.get_debug_string_for_humidity(humidity as f64),
        ));
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::biome::climate::ParameterPoint;
    use rivet_registry::biome_id::BiomeId;
    use rivet_registry::registry_ops::RegistryOps;
    use rivet_serialization::json_ops::JsonOps;
    use serde_json::Value;

    /// The registry-backed ops the `MultiNoiseBiomeSource` codecs require (the
    /// `"biomes"` list / `"preset"` field resolve the biome registry).
    type TestOps = RegistryOps<Value, JsonOps>;

    fn holder(id: u16) -> Holder<BiomeId> {
        Holder::direct(BiomeId::from_id(id))
    }

    /// The five-entry NETHER parameter list resolved to direct id holders.
    fn nether_list() -> ParameterList<Holder<BiomeId>> {
        ParameterList::new(vec![
            (params(0.0, 0.0, 0.0), holder(34)),
            (params(0.0, -0.5, 0.0), holder(49)),
            (params(0.4, 0.0, 0.0), holder(7)),
            (params(0.0, 0.5, 0.375), holder(60)),
            (params(-0.5, 0.0, 0.175), holder(2)),
        ])
    }

    fn params(t: f32, h: f32, w: f32) -> ParameterPoint {
        crate::biome::climate::Climate::parameters(t, h, 0.0, 0.0, 0.0, 0.0, w)
    }

    #[test]
    fn get_noise_biome_finds_the_nearest_entry() {
        let src = MultiNoiseBiomeSource::create_from_list(nether_list());
        // `Climate.empty()` samples all zeros → the nether_wastes entry
        // (0, 0, 0, 0, 0, 0, offset 0) wins (every other entry carries a
        // nonzero offset or parameter).
        assert_eq!(
            src.get_noise_biome(0, 0, 0, &crate::biome::climate::Climate::empty()),
            holder(34)
        );
    }

    #[test]
    fn collect_possible_biomes_preserves_declaration_order() {
        let src = MultiNoiseBiomeSource::create_from_list(nether_list());
        let biomes = src.collect_possible_biomes();
        assert_eq!(
            biomes,
            vec![holder(34), holder(49), holder(7), holder(60), holder(2)]
        );
    }

    #[test]
    fn clone_and_eq_rebuild_the_list() {
        let a = MultiNoiseBiomeSource::create_from_list(nether_list());
        let b = a.clone();
        assert_eq!(a, b);
        let c = MultiNoiseBiomeSource::create_from_list(nether_list());
        assert_eq!(a, c);
        let different = MultiNoiseBiomeSource::create_from_list(ParameterList::new(vec![(
            params(0.0, 0.0, 0.0),
            holder(34),
        )]));
        assert_ne!(a, different);
    }

    /// A `HolderGetter<BiomeId>` that resolves the generated biome names to
    /// `Direct` id holders (the nether preset's keys).
    struct NameGetter;

    impl rivet_registry::holder_lookup::HolderGetter<BiomeId> for NameGetter {
        fn get(&self, key: &rivet_registry::ResourceKey<BiomeId>) -> Option<Holder<BiomeId>> {
            BiomeId::from_name(&key.identifier().to_string()).map(Holder::direct)
        }
        fn get_tag(
            &self,
            _tag: &rivet_registry::TagKey<BiomeId>,
        ) -> Option<rivet_registry::HolderSet<BiomeId>> {
            None
        }
    }

    #[test]
    fn stable_is_false_for_direct_list_and_direct_preset() {
        // A direct-list source: `parameters.right()` is empty.
        let list_src = MultiNoiseBiomeSource::create_from_list(nether_list());
        let preset_key = rivet_registry::ResourceKey::create(
            &crate::biome::biome_source::keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST,
            rivet_registry::Identifier::with_default_namespace("nether"),
        );
        assert!(!list_src.stable(&preset_key));
        // A direct preset is never keyed — Java `Holder.Direct.is(key)` is
        // false; the port takes the resolved list value (no holder wrapper).
        let direct_preset = crate::biome::multi_noise_biome_source_parameter_list::MultiNoiseBiomeSourceParameterList::new(
            crate::biome::multi_noise_biome_source_parameter_list::BY_NAME
                .get("minecraft:nether")
                .expect("nether preset present")
                .clone(),
            &NameGetter,
        )
        .expect("the nether preset is never deferred");
        let preset_src = MultiNoiseBiomeSource::create_from_preset(direct_preset);
        assert!(!preset_src.stable(&preset_key));
    }

    #[test]
    fn list_codec_round_trips_and_encodes_biomes_key() {
        let codec = map_codec::codec_of(MultiNoiseBiomeSource::map_codec::<TestOps>());
        let (ops, owner) = biome_ops();
        // Decode resolves each `"biome"` entry to a `Reference`, so round-trip a
        // source built from references.
        let src = MultiNoiseBiomeSource::create_from_list(ParameterList::new(vec![
            (params(0.0, 0.0, 0.0), Holder::reference(owner, 0)),
            (params(0.0, -0.5, 0.0), Holder::reference(owner, 1)),
        ]));
        let encoded = codec
            .encode_start(&ops, &src)
            .result()
            .expect("encode should succeed")
            .clone();
        // The DIRECT_CODEC branch writes the `"biomes"` field.
        let map = encoded.as_object().expect("encoded map");
        assert!(map.contains_key("biomes"));
        let decoded = codec
            .parse(&ops, &encoded)
            .result()
            .expect("decode should succeed")
            .clone();
        assert_eq!(decoded, src);
    }

    #[test]
    fn preset_codec_decode_resolves_a_reference_preset() {
        let codec = map_codec::codec_of(MultiNoiseBiomeSource::map_codec::<TestOps>());
        let (ops, _preset_owner, nether_key) = preset_ops();
        // `{"preset": "minecraft:nether"}` decodes through the
        // `MultiNoiseBiomeSourceParameterList` `RegistryFileCodec` to a
        // `Reference` preset holder; the decode resolves it through the ops'
        // lookup (Java `preset.value().parameters()` / `preset.is(key)`).
        let decoded = codec
            .parse(&ops, &serde_json::json!({"preset": "minecraft:nether"}))
            .result()
            .expect("decode should succeed")
            .clone();
        // The resolved preset key drives `stable(key)`.
        assert!(decoded.stable(&nether_key));
        let overworld_key = rivet_registry::ResourceKey::create(
            &keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST,
            rivet_registry::Identifier::with_default_namespace("overworld"),
        );
        assert!(!decoded.stable(&overworld_key));
        // `get_noise_biome` resolves through the stored search list (the nether
        // preset's first entry — nether_wastes, the all-zero sample winner).
        assert_eq!(
            decoded.get_noise_biome(0, 0, 0, &crate::biome::climate::Climate::empty()),
            holder(34)
        );
        // Encode preserves the Reference preset form (the `Either` is retained).
        let re_encoded = codec
            .encode_start(&ops, &decoded)
            .result()
            .expect("encode should succeed")
            .clone();
        assert_eq!(
            re_encoded,
            serde_json::json!({"preset": "minecraft:nether"})
        );
    }

    #[test]
    fn map_either_both_fail_reports_the_second_then_first_diagnostic() {
        // Neither the DIRECT_CODEC (`"biomes"`) nor the PRESET_CODEC
        // (`"preset"`) field is present — Java's `EitherMapCodec.decode` runs
        // `first.apply2((f, s) -> s, second)`, so the `Applicative.ap2` fallback
        // concatenates `second + "; " + first` (the curried function keeps the
        // second's message first). The port's `map_either_fallback` reproduces
        // that ordering.
        let codec = map_codec::codec_of(MultiNoiseBiomeSource::map_codec::<TestOps>());
        let (ops, _owner) = biome_ops();
        let result = codec.parse(&ops, &serde_json::json!({}));
        let message = result
            .error_ref()
            .map(|e| e.message().to_string())
            .expect("both fields missing must fail");
        assert!(
            message.starts_with("No key preset in MapLike"),
            "second message first: {message}"
        );
        assert!(
            message.contains("; No key biomes in MapLike"),
            "first message second: {message}"
        );
    }

    /// A `RegistryOps` whose access carries a biome registry with
    /// `plains`/`river` registered at element ids 0..1 — the `"biomes"` list
    /// decode resolves each identifier to a `Reference`.
    fn biome_ops() -> (TestOps, rivet_registry::holder::RegistryId) {
        let key = rivet_registry::registries::BIOME.clone();
        let mut builder = rivet_registry::RegistryBuilder::<BiomeId>::new(&key);
        for (i, name) in ["plains", "river"].iter().enumerate() {
            builder.register(
                &rivet_registry::ResourceKey::create(
                    &key,
                    rivet_registry::Identifier::with_default_namespace(name),
                ),
                std::sync::Arc::new(BiomeId::from_id(40 + i as u16)),
                rivet_registry::RegistrationInfo::BUILT_IN,
            );
        }
        let registry = builder.freeze();
        let owner = registry.registry_id();
        let access = rivet_registry::RegistryAccess::from_single_registry(key, registry);
        (
            RegistryOps::create_from_access(&JsonOps::INSTANCE, access),
            owner,
        )
    }

    /// A `RegistryOps` whose access carries a `multi_noise_biome_source_parameter_list`
    /// registry with the `nether` preset registered — the `"preset"` field
    /// decode resolves `"minecraft:nether"` to a `Reference` preset holder.
    fn preset_ops() -> (
        TestOps,
        rivet_registry::holder::RegistryId,
        rivet_registry::ResourceKey<MultiNoiseBiomeSourceParameterList>,
    ) {
        let preset_registry_key = keys::MULTI_NOISE_BIOME_SOURCE_PARAMETER_LIST.clone();
        let nether_key = rivet_registry::ResourceKey::create(
            &preset_registry_key,
            rivet_registry::Identifier::with_default_namespace("nether"),
        );
        let value = MultiNoiseBiomeSourceParameterList::new(
            crate::biome::multi_noise_biome_source_parameter_list::BY_NAME
                .get("minecraft:nether")
                .expect("nether preset present")
                .clone(),
            &NameGetter,
        )
        .expect("the nether preset is never deferred");
        let mut builder =
            rivet_registry::RegistryBuilder::<MultiNoiseBiomeSourceParameterList>::new(
                &preset_registry_key,
            );
        builder.register(
            &nether_key,
            std::sync::Arc::new(value),
            rivet_registry::RegistrationInfo::BUILT_IN,
        );
        let registry = builder.freeze();
        let owner = registry.registry_id();
        let access =
            rivet_registry::RegistryAccess::from_single_registry(preset_registry_key, registry);
        (
            RegistryOps::create_from_access(&JsonOps::INSTANCE, access),
            owner,
            nether_key,
        )
    }
}
