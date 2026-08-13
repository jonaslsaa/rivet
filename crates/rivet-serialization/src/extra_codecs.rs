//! **Partial** port of `net.minecraft.util.ExtraCodecs`.
//!
//! PROVENANCE: `ExtraCodecs.java` is a leaf of the `mc.util` manifest unit
//! (net.minecraft.util -> rivet-util). This module ports the DFU combinators
//! that are pure `com.mojang.serialization` surface with no Minecraft
//! dependency, so they belong in `rivet-serialization` (the crate that owns
//! that surface). They were added by the slices that needed them:
//!
//! - `overrideLifecycle(Codec, Function, Function)` and its 1-arg variant —
//!   required by `Registry.referenceHolderWithLifecycle()` (#394, the by-name
//!   codec surface in `rivet-registry`).
//! - `retrieveContext(Function)` — required by `RegistryOps`.
//! - `orCompressed(Codec, Codec)` and `orCompressed(MapCodec, MapCodec)` —
//!   the Codec variant is a transitive dependency of
//!   `StringRepresentable.StringRepresentableCodec` (rivet-util); the MapCodec
//!   variant is required by `ComponentSerialization.createLegacyComponentMatcher`
//!   (rivet-text).
//! - `idResolverCodec(ToIntFunction, IntFunction, int)` and its typed
//!   `Codec<I>` overload — transitive dependency of
//!   `StringRepresentable.StringRepresentableCodec`, and the typed overload is
//!   required by `LateBoundIdMapper.codec` (rivet-text's content dispatch).
//! - `nonEmptyList(Codec<List<T>>)` — required by `ComponentSerialization`'s
//!   `"extra"` sibling field.
//! - `LateBoundIdMapper<I, V>` + the `late_bound_values`/`late_bound_entries`
//!   accessors — required by `ComponentSerialization`'s content-type bootstrap
//!   and `KeyDispatchCodec` discriminator.
//! - `nonNegativeIntCodec` (`NON_NEGATIVE_INT`, an `intRangeWithMessage(0,
//!   MAX)` — a `Codec.INT` + `validate`) — required by `Weighted.codec`
//!   (issue #353) for the exact `"Value must be non-negative: N"` decode
//!   error. One range member is ported here (ahead of the rest of the ranges)
//!   because it is a pure-DFU `validate` with no Minecraft dependency; the
//!   remaining range surface stays `mc.util` scope.
//! - `intervalCodec(Codec<P>, String, String, BiFunction, Function,
//!   Function)` — the interval/value codec — required by
//!   `Climate.Parameter.CODEC` (issue #178, the `mc.world.level.biome` unit).
//!   It is pure `com.mojang.serialization` surface (the `Util.fixedSize`
//!   list-of-2 validation is inlined so the module stays free of the
//!   Minecraft-bound `Util`), so it belongs here; when the full `mc.util`
//!   unit is ported it keeps this signature and semantics.
//!
//! RECONCILIATION: when the full `mc.util` unit is ported, these free functions
//! move into that unit's `extra_codecs.rs`; they keep the exact same signatures
//! and semantics documented here. The remaining `ExtraCodecs.java` surface
//! (`intRange`/`longRange`/`floatRange` and friends, `compactListCodec`,
//! `ensureHomogenous`, `orElsePartial`, ...) is not ported — that is future
//! `mc.util` scope, not this slice.

use crate::codec::{self, Codec, ResultFunction};
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::either::Either;
use crate::lifecycle::Lifecycle;
use crate::map_codec::MapCodec;
use crate::record_builder::{self, RecordCodecBuilder};
use std::fmt::Debug;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// overrideLifecycle
// ---------------------------------------------------------------------------

/// `ExtraCodecs.overrideLifecycle(Codec<E>, Function<E, Lifecycle>,
/// Function<E, Lifecycle>)` — `codec.mapResult(ResultFunction)` whose
/// `apply` overrides the decode lifecycle from the decoded value (only on a
/// full success; an error/partial result passes through untouched) and whose
/// `coApply` overrides the encode lifecycle from the input value.
pub fn override_lifecycle<E, Ops>(
    codec: Arc<dyn Codec<E, Ops>>,
    decode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle + Send + Sync>,
    encode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle + Send + Sync>,
) -> Arc<dyn Codec<E, Ops>>
where
    E: 'static,
    Ops: DynamicOps + 'static,
{
    codec::map_result(
        codec,
        Arc::new(OverrideLifecycleResultFunction {
            decode_lifecycle,
            encode_lifecycle,
        }),
    )
}

/// `ExtraCodecs.overrideLifecycle(Codec<E>, Function<E, Lifecycle>)` — both
/// halves share `lifecycleGetter`.
pub fn override_lifecycle_single<E, Ops>(
    codec: Arc<dyn Codec<E, Ops>>,
    lifecycle_getter: Arc<dyn Fn(&E) -> Lifecycle + Send + Sync>,
) -> Arc<dyn Codec<E, Ops>>
where
    E: 'static,
    Ops: DynamicOps + 'static,
{
    override_lifecycle(codec, lifecycle_getter.clone(), lifecycle_getter)
}

/// `overrideLifecycle`'s `ResultFunction` — `toString` is
/// `"WithLifecycle[" + decodeLifecycle + " " + encodeLifecycle + "]"`.
struct OverrideLifecycleResultFunction<E> {
    decode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle + Send + Sync>,
    encode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle + Send + Sync>,
}

impl<E> Debug for OverrideLifecycleResultFunction<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WithLifecycle[decodeLifecycle encodeLifecycle]")
    }
}

impl<E, Ops: DynamicOps + 'static> ResultFunction<E, Ops> for OverrideLifecycleResultFunction<E> {
    fn apply(
        &self,
        _ops: &Ops,
        _input: &Ops::Output,
        a: DataResult<(E, Ops::Output)>,
    ) -> DataResult<(E, Ops::Output)> {
        // Java: `a.result().map(r -> a.setLifecycle(decodeLifecycle.apply(r.getFirst())))
        //        .orElse(a)` — lifecycle override applies only to a full success.
        match a.result() {
            Some((value, _)) => {
                let lifecycle = (self.decode_lifecycle)(value);
                a.set_lifecycle(lifecycle)
            }
            None => a,
        }
    }

    fn co_apply(
        &self,
        _ops: &Ops,
        input: &E,
        t: DataResult<Ops::Output>,
    ) -> DataResult<Ops::Output> {
        t.set_lifecycle((self.encode_lifecycle)(input))
    }
}

// ---------------------------------------------------------------------------
// retrieveContext
// ---------------------------------------------------------------------------

/// The `retrieveContext` getter — `Function<DynamicOps<?>, DataResult<E>>`.
type ContextGetter<E, Ops> = Arc<dyn Fn(&Ops) -> DataResult<E> + Send + Sync>;

/// `ExtraCodecs.retrieveContext(Function<DynamicOps<?>, DataResult<E>>)` —
/// a `MapCodec` that ignores its input and derives the value purely from the
/// ops. Encoding is a no-op (the prefix `RecordBuilder` is returned
/// unchanged); `keys` is empty.
pub fn retrieve_context<E, Ops>(getter: ContextGetter<E, Ops>) -> Arc<dyn MapCodec<E, Ops>>
where
    E: 'static,
    Ops: DynamicOps + 'static,
{
    Arc::new(ContextRetrievalCodec { getter })
}

/// `retrieveContext`'s `MapCodec` — `toString` is
/// `"ContextRetrievalCodec[" + getter + "]"`.
struct ContextRetrievalCodec<E, Ops> {
    getter: ContextGetter<E, Ops>,
}

impl<E, Ops> Debug for ContextRetrievalCodec<E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ContextRetrievalCodec[getter]")
    }
}

impl<E, Ops: DynamicOps + 'static> MapCodec<E, Ops> for ContextRetrievalCodec<E, Ops> {
    fn decode(&self, ops: &Ops, _input: &dyn MapLike<Ops::Output>) -> DataResult<E> {
        (self.getter)(ops)
    }

    fn encode(
        &self,
        _input: &E,
        _ops: &Ops,
        _prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        // Java: `return prefix;` — no-op.
    }
}

impl<E, Ops: DynamicOps + 'static> Keyable<Ops> for ContextRetrievalCodec<E, Ops> {
    fn keys(&self, _ops: &Ops) -> Vec<Ops::Output> {
        // Java: `Stream.empty()`.
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// orCompressed (Codec variant only)
// ---------------------------------------------------------------------------

/// `ExtraCodecs.orCompressed(Codec<E>, Codec<E>)` — encode/decode route
/// through `compressed` when `ops.compressMaps()`, else `normal`. `toString`
/// is `normal + " orCompressed " + compressed`. (The `MapCodec` overload is
/// not ported — nothing in this slice needs it.)
pub fn or_compressed<E, Ops>(
    normal: Arc<dyn Codec<E, Ops>>,
    compressed: Arc<dyn Codec<E, Ops>>,
) -> Arc<dyn Codec<E, Ops>>
where
    E: 'static,
    Ops: DynamicOps + 'static,
{
    Arc::new(OrCompressedCodec { normal, compressed })
}

struct OrCompressedCodec<E, Ops> {
    normal: Arc<dyn Codec<E, Ops>>,
    compressed: Arc<dyn Codec<E, Ops>>,
}

impl<E, Ops> Debug for OrCompressedCodec<E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} orCompressed {:?}", self.normal, self.compressed)
    }
}

impl<E, Ops: DynamicOps + 'static> crate::Encoder<E, Ops> for OrCompressedCodec<E, Ops> {
    fn encode(&self, input: &E, ops: &Ops, prefix: &Ops::Output) -> DataResult<Ops::Output> {
        if ops.compress_maps() {
            self.compressed.encode(input, ops, prefix)
        } else {
            self.normal.encode(input, ops, prefix)
        }
    }
}

impl<E, Ops: DynamicOps + 'static> crate::Decoder<E, Ops> for OrCompressedCodec<E, Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(E, Ops::Output)> {
        if ops.compress_maps() {
            self.compressed.decode(ops, input)
        } else {
            self.normal.decode(ops, input)
        }
    }
}

impl<E, Ops: DynamicOps + 'static> Codec<E, Ops> for OrCompressedCodec<E, Ops> {}

// ---------------------------------------------------------------------------
// idResolverCodec (int variant only)
// ---------------------------------------------------------------------------

/// `ExtraCodecs.idResolverCodec(ToIntFunction<E>, IntFunction<E>, int)` —
/// `Codec.INT.flatXmap(id -> byId, e -> byCode)` with the `unknownId`
/// sentinel. Error messages match Java exactly: `"Unknown element id: " + id`
/// and `"Element with unknown id: " + e`.
pub fn id_resolver_codec<E, Ops>(
    to_int: Arc<dyn Fn(&E) -> i32 + Send + Sync>,
    from_int: Arc<dyn Fn(i32) -> Option<E> + Send + Sync>,
    unknown_id: i32,
) -> Arc<dyn Codec<E, Ops>>
where
    E: 'static + std::fmt::Display,
    Ops: DynamicOps + 'static,
{
    codec::flat_xmap(
        codec::int_codec::<Ops>(),
        Arc::new(move |id: &i32| match from_int(*id) {
            Some(e) => DataResult::success(e),
            None => DataResult::error(format!("Unknown element id: {}", id)),
        }),
        Arc::new(move |e: &E| {
            let id = to_int(e);
            if id == unknown_id {
                DataResult::error(format!("Element with unknown id: {}", e))
            } else {
                DataResult::success(id)
            }
        }),
    )
}

/// `Function<I, Optional<E>>` — `fromId` for `idResolverCodec`.
type FromIdFn<I, E> = Arc<dyn Fn(&I) -> Option<E> + Send + Sync>;
/// `Function<E, Optional<I>>` — `toId` for `idResolverCodec`.
type ToIdFn<I, E> = Arc<dyn Fn(&E) -> Option<I> + Send + Sync>;

/// `ExtraCodecs.idResolverCodec(Codec<I> value, Function<I, E>,
/// Function<E, I>)` — `value.flatXmap(id -> fromId(id) ?? error,
/// e -> toId(e) ?? error)`. The `Codec<I>`-parameterized variant (used by
/// `LateBoundIdMapper.codec(Codec<I>)`); the int variant above is the
/// `ToIntFunction` overload. Error messages match Java exactly.
pub fn id_resolver_codec_typed<I, E, Ops>(
    value: Arc<dyn Codec<I, Ops>>,
    from_id: FromIdFn<I, E>,
    to_id: ToIdFn<I, E>,
) -> Arc<dyn Codec<E, Ops>>
where
    I: 'static + std::fmt::Display + Clone + Send + Sync,
    E: 'static + std::fmt::Display + Clone + Send + Sync,
    Ops: DynamicOps + 'static,
{
    codec::flat_xmap(
        value,
        Arc::new(move |id: &I| match from_id(id) {
            Some(e) => DataResult::success(e),
            None => DataResult::error(format!("Unknown element id: {}", id)),
        }),
        Arc::new(move |e: &E| match to_id(e) {
            Some(id) => DataResult::success(id),
            None => DataResult::error(format!("Element with unknown id: {}", e)),
        }),
    )
}

// ---------------------------------------------------------------------------
// NON_NEGATIVE_INT
// ---------------------------------------------------------------------------

/// `ExtraCodecs.NON_NEGATIVE_INT` — `Codec.INT.validate(v -> v >= 0 ?
/// success : error("Value must be non-negative: " + v))`.
///
/// Java's `intRangeWithMessage(0, Integer.MAX_VALUE, n -> "Value must be
/// non-negative: " + n)` — a plain `Codec.INT` + `validate` with that message.
/// Note this is NOT `codec::int_range(0, i32::MAX)`: the message differs
/// (`"Value must be non-negative: N"` vs the generic range message), and this
/// is what `Weighted.codec` (issue #353) relies on for exact decode errors.
pub fn non_negative_int_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    codec::validate(
        codec::int_codec::<Ops>(),
        Arc::new(|value: &i32| {
            if *value >= 0 {
                DataResult::success(*value)
            } else {
                DataResult::error(format!("Value must be non-negative: {}", value))
            }
        }),
    )
}

// ---------------------------------------------------------------------------
// nonEmptyList
// ---------------------------------------------------------------------------

/// `ExtraCodecs.nonEmptyList(Codec<List<T>>)` — validates the decoded list is
/// non-empty, else `DataResult.error("List must have contents")`.
pub fn non_empty_list<E, Ops>(
    list_codec: Arc<dyn Codec<Vec<E>, Ops>>,
) -> Arc<dyn Codec<Vec<E>, Ops>>
where
    E: 'static + Clone,
    Ops: DynamicOps + 'static,
{
    codec::validate(
        list_codec,
        Arc::new(|list: &Vec<E>| {
            if list.is_empty() {
                DataResult::error("List must have contents")
            } else {
                DataResult::success(list.clone())
            }
        }),
    )
}

// ---------------------------------------------------------------------------
// orCompressed (MapCodec variant)
// ---------------------------------------------------------------------------

/// `ExtraCodecs.orCompressed(MapCodec<E>, MapCodec<E>)` — the MapCodec
/// overload: encode/decode route through `compressed` when
/// `ops.compressMaps()`, else `normal`; `keys` comes from `compressed`.
/// `toString` is `normal + " orCompressed " + compressed`.
pub fn or_compressed_map<E, Ops>(
    normal: Arc<dyn MapCodec<E, Ops>>,
    compressed: Arc<dyn MapCodec<E, Ops>>,
) -> Arc<dyn MapCodec<E, Ops>>
where
    E: 'static,
    Ops: DynamicOps + 'static,
{
    Arc::new(OrCompressedMapCodec { normal, compressed })
}

struct OrCompressedMapCodec<E, Ops> {
    normal: Arc<dyn MapCodec<E, Ops>>,
    compressed: Arc<dyn MapCodec<E, Ops>>,
}

impl<E, Ops> Debug for OrCompressedMapCodec<E, Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} orCompressed {:?}", self.normal, self.compressed)
    }
}

impl<E, Ops: DynamicOps + 'static> Keyable<Ops> for OrCompressedMapCodec<E, Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        // Java `orCompressed(MapCodec)` returns `compressed.keys()`.
        self.compressed.keys(ops)
    }
}

impl<E, Ops: DynamicOps + 'static> crate::map_encoder::MapEncoder<E, Ops>
    for OrCompressedMapCodec<E, Ops>
{
    fn encode(&self, input: &E, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        if ops.compress_maps() {
            self.compressed.encode(input, ops, prefix)
        } else {
            self.normal.encode(input, ops, prefix)
        }
    }
}

impl<E, Ops: DynamicOps + 'static> crate::map_decoder::MapDecoder<E, Ops>
    for OrCompressedMapCodec<E, Ops>
{
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<E> {
        if ops.compress_maps() {
            self.compressed.decode(ops, input)
        } else {
            self.normal.decode(ops, input)
        }
    }
}

impl<E, Ops: DynamicOps + 'static> MapCodec<E, Ops> for OrCompressedMapCodec<E, Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<E> {
        crate::map_decoder::MapDecoder::decode(self, ops, input)
    }

    fn encode(&self, input: &E, ops: &Ops, prefix: &mut dyn RecordBuilder<Output = Ops::Output>) {
        crate::map_encoder::MapEncoder::encode(self, input, ops, prefix)
    }
}

// ---------------------------------------------------------------------------
// LateBoundIdMapper
// ---------------------------------------------------------------------------

/// `ExtraCodecs.LateBoundIdMapper<I, V>` — a name → MapCodec registry built
/// up by `put` calls (the component-content/ObjectInfo/DataSource bootstrap).
/// Java's `idToValue` is a `HashBiMap`; the port keeps insertion order so the
/// FuzzyCodec decode loop is deterministic (the registered shapes are disjoint
/// in practice, so order is unobservable for valid input).
pub struct LateBoundIdMapper<K, V> {
    entries: std::sync::Mutex<Vec<(K, V)>>,
}

impl<K, V> Default for LateBoundIdMapper<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> LateBoundIdMapper<K, V> {
    /// `new LateBoundIdMapper<>()`.
    pub fn new() -> Self {
        LateBoundIdMapper {
            entries: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// `put(I id, V value)` — `Objects.requireNonNull(value)`; panics on a
    /// null (unrepresentable) value like Java.
    pub fn put(&self, id: K, value: V) {
        self.entries.lock().unwrap().push((id, value));
    }

    /// `codec(Codec<I> idCodec)` — `ExtraCodecs.idResolverCodec(idCodec,
    /// this.idToValue::get, valueToId::get)`.
    ///
    /// Java's `BiMap` resolves by value equality on both directions; the id
    /// codec type is always the map's own key type (callers pass
    /// `Codec.STRING` for a `LateBoundIdMapper<String, ...>`). The port keeps
    /// a `Vec<(K, V)>` in insertion order and compares by equality.
    pub fn codec<Ops>(&self, id_codec: Arc<dyn Codec<K, Ops>>) -> Arc<dyn Codec<V, Ops>>
    where
        K: PartialEq + Clone + Send + Sync + std::fmt::Display + 'static,
        V: PartialEq + Clone + Send + Sync + std::fmt::Display + 'static,
        Ops: DynamicOps + 'static,
    {
        let entries = self.entries.lock().unwrap().clone();
        let entries_for_encode = entries.clone();
        id_resolver_codec_typed(
            id_codec,
            Arc::new(move |id: &K| {
                entries
                    .iter()
                    .find(|(k, _)| k == id)
                    .map(|(_, v)| v.clone())
            }),
            Arc::new(move |v: &V| {
                entries_for_encode
                    .iter()
                    .find(|(_, val)| *val == *v)
                    .map(|(k, _)| k.clone())
            }),
        )
    }
}

/// `values()` — an immutable snapshot of the registered values (Java returns
/// `Collections.unmodifiableSet(this.idToValue.values())`; order is the
/// insertion order here).
pub fn late_bound_values<K, V>(mapper: &LateBoundIdMapper<K, V>) -> Vec<V>
where
    V: Clone,
{
    mapper
        .entries
        .lock()
        .unwrap()
        .iter()
        .map(|(_, v)| v.clone())
        .collect()
}

/// `entries()` — an immutable snapshot of the `(id, value)` pairs in insertion
/// order, used by `ComponentSerialization`'s discriminator codec to resolve a
/// type name to its `MapCodec`.
pub fn late_bound_entries<K: Clone, V: Clone>(mapper: &LateBoundIdMapper<K, V>) -> Vec<(K, V)> {
    mapper.entries.lock().unwrap().clone()
}

// ---------------------------------------------------------------------------
// intervalCodec
// ---------------------------------------------------------------------------

/// `Util.fixedSize(List<T>, int)` with `size = 2` — the `intervalCodec`
/// array-form validation. `"Input is not a list of 2 elements"`; a longer list
/// keeps the first 2 as a partial, a shorter list has no partial.
fn fixed_size_two<P: Clone>(input: &[P]) -> DataResult<Vec<P>> {
    if input.len() != 2 {
        if input.len() >= 2 {
            DataResult::error_with_partial(
                "Input is not a list of 2 elements".to_string(),
                input[..2].to_vec(),
            )
        } else {
            DataResult::error("Input is not a list of 2 elements")
        }
    } else {
        DataResult::success(input.to_vec())
    }
}

/// `ExtraCodecs.intervalCodec(Codec<P> pointCodec, String lowerBoundName,
/// String upperBoundName, BiFunction<P, P, DataResult<I>> makeInterval,
/// Function<I, P> getMin, Function<I, P> getMax)` — the interval/value codec
/// (required by `Climate.Parameter.CODEC`).
///
/// Java builds, in order:
/// 1. `arrayCodec` = `Codec.list(pointCodec).comapFlatMap(list ->
///    Util.fixedSize(list, 2).flatMap(l -> makeInterval(l[0], l[1])),
///    p -> ImmutableList.of(getMin(p), getMax(p)))` — a 2-element array form
///    (an interval with min/max);
/// 2. `objectCodec` = a `RecordCodecBuilder` over the `lowerBoundName` and
///    `upperBoundName` fields (a `Pair<P, P>`) `.comapFlatMap(p ->
///    makeInterval(p.first, p.second), i -> Pair.of(getMin(i), getMax(i)))` —
///    an object form;
/// 3. `arrayOrObjectCodec` = `Codec.withAlternative(arrayCodec, objectCodec)`;
/// 4. the result = `Codec.either(pointCodec, arrayOrObjectCodec)
///    .comapFlatMap(either -> either.map(min -> makeInterval(min, min),
///    DataResult::success), p -> equals(getMin(p), getMax(p)) ? Either.left(min)
///    : Either.right(p))` — a bare point decodes to the degenerate
///    `makeInterval(min, min)` interval; encode picks the point form when the
///    interval is degenerate, else the array-or-object form.
///
/// The `BiFunction<P, P, DataResult<I>>` is `Fn(&P, &P) -> DataResult<I>`
/// (references — Java's by-value call is unobservable for the port's
/// value-semantic `P`); `getMin`/`getMax` are `Fn(&I) -> P`.
///
/// The degenerate encode check calls Java's `Objects.equals(getMin(p),
/// getMax(p))`, whose semantics depend on `P`'s `equals` — for `Float` that is
/// `Float.equals` (all NaNs equal, `-0.0` distinct from `+0.0`), which is not
/// `PartialEq`. The caller therefore supplies `equals` explicitly; the
/// `Parameter` caller passes [`crate::float_format::java_float_equals`].
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn interval_codec<P, I, Ops>(
    point_codec: Arc<dyn Codec<P, Ops>>,
    lower_bound_name: String,
    upper_bound_name: String,
    make_interval: Arc<dyn Fn(&P, &P) -> DataResult<I> + Send + Sync>,
    get_min: Arc<dyn Fn(&I) -> P + Send + Sync>,
    get_max: Arc<dyn Fn(&I) -> P + Send + Sync>,
    equals: Arc<dyn Fn(&P, &P) -> bool + Send + Sync>,
) -> Arc<dyn Codec<I, Ops>>
where
    P: Clone + Send + Sync + 'static,
    I: Clone + Send + Sync + 'static,
    Ops: DynamicOps + 'static,
{
    let make_interval_for_array = make_interval.clone();
    let get_min_for_array = get_min.clone();
    let get_max_for_array = get_max.clone();
    let array_codec: Arc<dyn Codec<I, Ops>> = codec::comap_flat_map(
        codec::list(point_codec.clone()),
        Arc::new(move |list: &Vec<P>| {
            let make = make_interval_for_array.clone();
            fixed_size_two(list).flat_map(move |two: Vec<P>| make(&two[0], &two[1]))
        }),
        Arc::new(move |i: &I| vec![get_min_for_array(i), get_max_for_array(i)]),
    );

    let make_interval_for_object = make_interval.clone();
    let get_min_for_object = get_min.clone();
    let get_max_for_object = get_max.clone();
    let object_codec: Arc<dyn Codec<I, Ops>> = codec::comap_flat_map(
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|pair: &(P, P)| pair.0.clone()),
                    codec::field_of(point_codec.clone(), lower_bound_name),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|pair: &(P, P)| pair.1.clone()),
                    codec::field_of(point_codec.clone(), upper_bound_name),
                ))
                .apply(instance, Arc::new(|min: P, max: P| (min, max)))
        }),
        Arc::new(move |pair: &(P, P)| make_interval_for_object(&pair.0, &pair.1)),
        Arc::new(move |i: &I| (get_min_for_object(i), get_max_for_object(i))),
    );

    let array_or_object_codec = codec::with_alternative(array_codec, object_codec);
    let make_interval_for_either = make_interval.clone();
    let get_min_for_final = get_min.clone();
    let get_max_for_final = get_max.clone();
    let equals_for_final = equals.clone();
    codec::comap_flat_map(
        codec::either(point_codec, array_or_object_codec),
        Arc::new(move |e: &Either<P, I>| match e {
            Either::Left(min) => make_interval_for_either(min, min),
            Either::Right(i) => DataResult::success(i.clone()),
        }),
        Arc::new(move |i: &I| {
            let min = get_min_for_final(i);
            let max = get_max_for_final(i);
            if equals_for_final(&min, &max) {
                Either::left(min)
            } else {
                Either::right(i.clone())
            }
        }),
    )
}

// ---------------------------------------------------------------------------
// stringRgbColor
// ---------------------------------------------------------------------------

/// `ExtraCodecs.STRING_RGB_COLOR` — a color codec accepting either the primary
/// `#RRGGBB` hex-string form (`hexColor(6)` xmapped through `ARGB::opaque`/
/// `ARGB::transparent`) or the alternative `RGB_COLOR_CODEC` (a bare int, or a
/// `[r, g, b]` float vector). Required by `BiomeSpecialEffects.CODEC` (issue
/// #178, the `mc.world.level.biome` unit). Pure `com.mojang.serialization`
/// surface plus the ARGB bit arithmetic (inlined here so the DFU crate stays
/// free of the Minecraft-bound `ARGB`).
///
/// Decode tries the hex string first, then the int/vector forms; encode always
/// emits the hex string (`#RRGGBB`, lowercase, zero-padded — Java
/// `HexFormat.toHexDigits(value, 6)` over the low 24 bits).
pub fn string_rgb_color<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    let hex = codec::xmap(
        hex_color::<Ops>(6),
        // `ARGB.opaque` — decode applies `color | 0xFF000000`.
        Arc::new(|color: &i32| color | 0xFF000000u32 as i32),
        // `ARGB.transparent` — encode strips the alpha channel.
        Arc::new(|color: &i32| color & 0xFFFFFF),
    );
    codec::with_alternative(hex, rgb_color_codec::<Ops>())
}

/// `ExtraCodecs.RGB_COLOR_CODEC` — `Codec.withAlternative(Codec.INT, VECTOR3F,
/// v -> ARGB.colorFromFloat(1.0F, v.x(), v.y(), v.z()))`. Decode tries the bare
/// int first, then the `[r, g, b]` float vector; encode always uses the int.
fn rgb_color_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    let vector3f = codec::comap_flat_map(
        codec::list(codec::float_codec::<Ops>()),
        Arc::new(|list: &Vec<f32>| {
            // Java `Util.fixedSize(list, 3)` — the message shape only (the
            // partial-result `subList` is dropped; the alternative path fails).
            if list.len() != 3 {
                DataResult::error("Input is not a list of 3 elements".to_string())
            } else {
                DataResult::success(color_from_float(1.0f32, list[0], list[1], list[2]))
            }
        }),
        // Encode always goes through the primary int codec (`withAlternative`),
        // so this direction is never exercised; return a valid-but-unused triple.
        Arc::new(|_color: &i32| vec![0.0f32, 0.0, 0.0]),
    );
    codec::with_alternative(codec::int_codec::<Ops>(), vector3f)
}

/// `ExtraCodecs.hexColor(int expectedDigits)` — the `#RRGGBB` string form.
/// Decode requires the `#` prefix and exactly `expectedDigits` hex digits;
/// encode emits `#` + `expectedDigits` lowercase zero-padded hex digits of the
/// low bits.
fn hex_color<Ops: DynamicOps + 'static>(expected_digits: u32) -> Arc<dyn Codec<i32, Ops>> {
    let max_value: u64 = (1u64 << (expected_digits * 4)) - 1;
    codec::comap_flat_map(
        codec::string_codec::<Ops>(),
        Arc::new(move |string: &String| {
            if !string.starts_with('#') {
                return DataResult::error("Hex color must begin with #".to_string());
            }
            // Java `string.length() - "#".length()` counts UTF-16 code units
            // (`String.length()`), not bytes — so a malformed non-ASCII input
            // reports the same "got N" as Paper (`encode_utf16` counts
            // surrogate pairs as 2, matching Java exactly).
            let digits = string[1..].encode_utf16().count();
            if digits != expected_digits as usize {
                return DataResult::error(format!(
                    "Hex color is wrong size, expected {} digits but got {}",
                    expected_digits, digits
                ));
            }
            // Java `HexFormat.fromHexDigitsToLong` — hex digits only, no sign,
            // no whitespace, case-insensitive.
            let digits_str = &string[1..];
            if digits_str.is_empty() || !digits_str.chars().all(|c| c.is_ascii_hexdigit()) {
                return DataResult::error(format!("Invalid color value: {}", string));
            }
            match u64::from_str_radix(digits_str, 16) {
                Ok(value) if value <= max_value => DataResult::success(value as i32),
                Ok(_) => DataResult::error(format!("Color value out of range: {}", string)),
                Err(_) => DataResult::error(format!("Invalid color value: {}", string)),
            }
        }),
        Arc::new(move |value: &i32| {
            // Java `HexFormat.of().toHexDigits(value.intValue(), expectedDigits)`
            // — the low `expectedDigits*4` bits, lowercase, zero-padded. The
            // mask is computed in `u64` so `expected_digits = 8` (the full
            // 32-bit ARGB surface) doesn't overflow `1 << 32` on a 32-bit
            // shift (`HexFormat` rejects digits > 8 for `int`).
            let mask = (1u64 << (expected_digits * 4)) - 1;
            format!(
                "#{:0width$x}",
                (*value as i64) & mask as i64,
                width = expected_digits as usize
            )
        }),
    )
}

/// `ARGB.colorFromFloat(float alpha, float red, float green, float blue)` —
/// `color(as8BitChannel(...) ×4)`, inlined so the DFU crate stays free of the
/// Minecraft-bound `ARGB` (issue #206 owns the full `ARGB` port).
fn color_from_float(alpha: f32, red: f32, green: f32, blue: f32) -> i32 {
    argb_color(
        as8_bit_channel(alpha),
        as8_bit_channel(red),
        as8_bit_channel(green),
        as8_bit_channel(blue),
    )
}

/// `ARGB.as8BitChannel(float)` — `Mth.floor(value * 255.0F)`.
fn as8_bit_channel(value: f32) -> i32 {
    (value * 255.0f32).floor() as i32
}

/// `ARGB.color(int alpha, int red, int green, int blue)`.
fn argb_color(alpha: i32, red: i32, green: i32, blue: i32) -> i32 {
    (alpha & 0xFF) << 24 | (red & 0xFF) << 16 | (green & 0xFF) << 8 | blue & 0xFF
}

#[cfg(test)]
mod interval_codec_tests {
    use super::*;
    use crate::float_format::{java_float_compare, java_float_equals, java_float_to_string};
    use crate::json_ops::JsonOps;
    use serde_json::json;

    /// A trivial quantized interval used to exercise the codec's forms.
    /// `makeInterval` quantizes to `(min*10000, max*10000)` (i64).
    type TestInterval = (i64, i64);

    /// A float-pass-through interval whose points are the raw f32 bounds — the
    /// only way to exercise `-0.0`/NaN degenerate-encode semantics (the i64
    /// `TestInterval` cannot represent them).
    fn float_interval_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<(f32, f32), Ops>> {
        interval_codec(
            codec::float_codec::<Ops>(),
            "min".to_string(),
            "max".to_string(),
            Arc::new(|min: &f32, max: &f32| {
                if java_float_compare(*min, *max) == std::cmp::Ordering::Greater {
                    DataResult::error(format!(
                        "min > max ({} > {})",
                        java_float_to_string(*min),
                        java_float_to_string(*max)
                    ))
                } else {
                    DataResult::success((*min, *max))
                }
            }),
            Arc::new(|i: &(f32, f32)| i.0),
            Arc::new(|i: &(f32, f32)| i.1),
            // Java `Objects.equals(getMin(p), getMax(p))` → `Float.equals`.
            Arc::new(|a: &f32, b: &f32| java_float_equals(*a, *b)),
        )
    }

    fn test_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TestInterval, Ops>> {
        interval_codec(
            codec::float_codec::<Ops>(),
            "min".to_string(),
            "max".to_string(),
            Arc::new(|min: &f32, max: &f32| {
                if min > max {
                    DataResult::error(format!("min > max ({} > {})", min, max))
                } else {
                    DataResult::success(((min * 10000.0f32) as i64, (max * 10000.0f32) as i64))
                }
            }),
            Arc::new(|i: &TestInterval| i.0 as f32 / 10000.0f32),
            Arc::new(|i: &TestInterval| i.1 as f32 / 10000.0f32),
            // The points are f32 (`float_codec`); for the finite values this
            // codec decodes, `Float.equals` and `PartialEq` agree, so the plain
            // `==` is the faithful equality.
            Arc::new(|a: &f32, b: &f32| a == b),
        )
    }

    fn encode<I: Clone>(
        codec: &Arc<dyn Codec<I, JsonOps>>,
        value: &I,
    ) -> Option<serde_json::Value> {
        codec
            .encode_start(&JsonOps::INSTANCE, value)
            .result()
            .cloned()
    }

    fn decode<I: Clone>(
        codec: &Arc<dyn Codec<I, JsonOps>>,
        input: &serde_json::Value,
    ) -> DataResult<I> {
        codec.parse(&JsonOps::INSTANCE, input)
    }

    #[test]
    fn degenerate_interval_round_trips_as_bare_point() {
        // A degenerate interval encodes as a bare point (Either.left) and a
        // bare point decodes to the degenerate interval.
        let codec = test_codec::<JsonOps>();
        let value = (5000, 5000);
        let encoded = encode(&codec, &value).expect("encode should succeed");
        assert_eq!(encoded, json!(0.5));
        let result = decode(&codec, &encoded);
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, value);
    }

    #[test]
    fn wide_interval_encodes_as_two_element_array() {
        // A non-degenerate interval encodes as `[min, max]` (the array form is
        // `withAlternative`'s primary, so encode always produces it).
        let codec = test_codec::<JsonOps>();
        let value = (0, 10000);
        let encoded = encode(&codec, &value).expect("encode should succeed");
        assert_eq!(encoded, json!([0.0, 1.0]));
    }

    #[test]
    fn wide_interval_decodes_from_array() {
        let codec = test_codec::<JsonOps>();
        let result = decode(&codec, &json!([0.0, 1.0]));
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, (0, 10000));
    }

    #[test]
    fn interval_decodes_from_object() {
        // The object form is the `withAlternative` fallback: a map that the
        // array form cannot decode.
        let codec = test_codec::<JsonOps>();
        let result = decode(&codec, &json!({"min": 0.0, "max": 1.0}));
        let decoded = result.result().expect("decode should succeed");
        assert_eq!(*decoded, (0, 10000));
    }

    #[test]
    fn array_of_wrong_length_is_an_error() {
        // `Util.fixedSize(list, 2)`: a one-element list has no partial and
        // errors.
        let codec = test_codec::<JsonOps>();
        let result = decode(&codec, &json!([0.0]));
        assert!(result.is_error());
    }

    #[test]
    fn min_greater_than_max_is_an_error() {
        let codec = test_codec::<JsonOps>();
        let result = decode(&codec, &json!([1.0, 0.0]));
        assert!(result.is_error());
    }

    #[test]
    fn float_interval_encode_signed_zero_uses_float_equals() {
        // Java `Objects.equals(getMin(p), getMax(p))` → `Float.equals`, which
        // treats `-0.0` and `+0.0` as DISTINCT. Under `PartialEq` they'd be
        // equal and `(-0.0, 0.0)` would wrongly encode as a bare point.
        let codec = float_interval_codec::<JsonOps>();
        // Degenerate (0.0, 0.0) → bare point (a single JSON number).
        let encoded = encode(&codec, &(0.0, 0.0)).expect("encode");
        assert!(
            encoded.is_number(),
            "(0.0, 0.0) must encode as the bare point, got {encoded}"
        );
        // Degenerate (-0.0, -0.0) → bare point too.
        let encoded = encode(&codec, &(-0.0, -0.0)).expect("encode");
        assert!(
            encoded.is_number(),
            "(-0.0, -0.0) must be a bare point, got {encoded}"
        );
        // Non-degenerate (-0.0, 0.0) → the two-element array form.
        let encoded = encode(&codec, &(-0.0, 0.0)).expect("encode");
        assert!(
            encoded.is_array(),
            "(-0.0, 0.0) must encode as the array form, got {encoded}"
        );
        let result = decode(&codec, &encoded);
        let decoded = result.result().expect("decode should succeed");
        assert!(java_float_equals(decoded.0, -0.0));
        assert!(java_float_equals(decoded.1, 0.0));
    }

    #[test]
    fn float_interval_encode_nan_payloads_are_all_equal() {
        // `Float.equals`: ALL NaN payloads are equal, so a degenerate NaN
        // interval encodes as the bare point even for distinct payloads.
        // JsonOps renders a NaN as `Null` (documented deviation), so the bare
        // point is `Null` while the array form would be `[Null, Null]`.
        let codec = float_interval_codec::<JsonOps>();
        let nan_a = f32::from_bits(0x7fc00001);
        let nan_b = f32::from_bits(0x7ff12345);
        assert!(nan_a.is_nan() && nan_b.is_nan());

        let encoded = encode(&codec, &(f32::NAN, f32::NAN)).expect("encode");
        assert!(
            encoded.is_null(),
            "degenerate NaN interval must encode as the bare point, got {encoded}"
        );
        let encoded = encode(&codec, &(nan_a, nan_b)).expect("encode");
        assert!(
            encoded.is_null(),
            "distinct NaN payloads are equal under Float.equals -> bare point, got {encoded}"
        );
    }
}

#[cfg(test)]
mod string_rgb_color_tests {
    use super::*;
    use crate::json_ops::JsonOps;
    use serde_json::json;

    fn error_message<T: std::fmt::Debug>(result: &DataResult<T>) -> String {
        result
            .error_ref()
            .unwrap_or_else(|| panic!("expected an error, got {:?}", result.result()))
            .message()
            .to_string()
    }

    #[test]
    fn hex_form_round_trips() {
        let codec = string_rgb_color::<JsonOps>();
        // Decode "#7d8c6e" -> opaque 0xFF7D8C6E.
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!("#7d8c6e"))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, 0xFF7D8C6Eu32 as i32);
        // Encode back to the same lowercase hex form.
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(encoded, json!("#7d8c6e"));
    }

    #[test]
    fn hex_form_is_case_insensitive_and_pads() {
        let codec = string_rgb_color::<JsonOps>();
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!("#Ff0000"))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, 0xFFFF0000u32 as i32);
        // Encode pads to 6 digits.
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &(0xFF000001u32 as i32))
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(encoded, json!("#000001"));
    }

    #[test]
    fn hex_form_rejects_malformed() {
        let codec = string_rgb_color::<JsonOps>();
        // A malformed value falls through both alternatives; the `either`
        // error composes the two inner messages (Java `EitherCodec.decode`).
        assert_eq!(
            error_message(&codec.parse(&JsonOps::INSTANCE, &json!("7d8c6e"))),
            "Failed to parse either. First: Hex color must begin with #; Second: Failed to parse either. First: Not a number: \"7d8c6e\"; Second: Not a json array: \"7d8c6e\""
        );
        assert_eq!(
            error_message(&codec.parse(&JsonOps::INSTANCE, &json!("#7d8c6"))),
            "Failed to parse either. First: Hex color is wrong size, expected 6 digits but got 5; Second: Failed to parse either. First: Not a number: \"#7d8c6\"; Second: Not a json array: \"#7d8c6\""
        );
        assert_eq!(
            error_message(&codec.parse(&JsonOps::INSTANCE, &json!("#7d8c6g"))),
            "Failed to parse either. First: Invalid color value: #7d8c6g; Second: Failed to parse either. First: Not a number: \"#7d8c6g\"; Second: Not a json array: \"#7d8c6g\""
        );
        // A leading '+' is not a hex digit (Java HexFormat rejects it), but
        // the size check (5 digits) fires first, exactly as the Java
        // `HexFormat` boundary check does.
        assert_eq!(
            error_message(&codec.parse(&JsonOps::INSTANCE, &json!("#+7d8c"))),
            "Failed to parse either. First: Hex color is wrong size, expected 6 digits but got 5; Second: Failed to parse either. First: Not a number: \"#+7d8c\"; Second: Not a json array: \"#+7d8c\""
        );
    }

    #[test]
    fn hex_form_wrong_size_counts_utf16_units_like_java() {
        let codec = string_rgb_color::<JsonOps>();
        // `"#7d8c6e\u{e9}"` is 8 UTF-16 code units (1 prefix + 6 hex + 1 for
        // the BMP 'é'), so Java `string.length() - 1` reports 7 — NOT 8 (which
        // a byte count would produce for the 2-byte UTF-8 'é'). The message
        // must match Paper's exact "got 7".
        assert_eq!(
            error_message(&codec.parse(&JsonOps::INSTANCE, &json!("#7d8c6e\u{e9}"))),
            "Failed to parse either. First: Hex color is wrong size, expected 6 digits but got 7; Second: Failed to parse either. First: Not a number: \"#7d8c6e\u{e9}\"; Second: Not a json array: \"#7d8c6e\u{e9}\""
        );
    }

    #[test]
    fn hex_color_8_digit_argb_round_trips_without_overflow() {
        // `hexColor(8)` is the `#RRGGBBAA`-style full 32-bit surface
        // (`STRING_ARGB_COLOR` when ported). The encode mask must not overflow
        // `1 << 32` and must preserve the high (alpha) bits of a negative i32.
        let codec = hex_color::<JsonOps>(8);
        // 0xFFFF0000 (negative i32) — the high bit survives the encode mask.
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!("#ffff0000"))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, 0xFFFF0000u32 as i32);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(encoded, json!("#ffff0000"));
        // Full 8-digit round trip with alpha bits set.
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!("#ff334455"))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, 0xFF334455u32 as i32);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(encoded, json!("#ff334455"));
        // All-ones (0xFFFFFFFF, i32 -1).
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!("#ffffffff"))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, -1);
        let encoded = codec
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("encode");
        assert_eq!(encoded, json!("#ffffffff"));
    }

    #[test]
    fn int_alternative_decodes() {
        // `RGB_COLOR_CODEC` — a bare int falls back to the int form.
        let codec = string_rgb_color::<JsonOps>();
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!(0x7D8C6E))
            .result()
            .cloned()
            .expect("decode");
        // Hex form is NOT hit (int input); the int is already a color, and the
        // outer hex xmap only wraps the hex path — so the raw int is returned
        // unchanged (Java `RGB_COLOR_CODEC`'s INT arm).
        assert_eq!(decoded, 0x7D8C6E);
    }

    #[test]
    fn vector_alternative_decodes() {
        let codec = string_rgb_color::<JsonOps>();
        // [1.0, 0.5, 0.0] -> color(255, floor(0.5*255)=127, 0) = 0xFFFF7F00.
        let decoded = codec
            .parse(&JsonOps::INSTANCE, &json!([1.0, 0.5, 0.0]))
            .result()
            .cloned()
            .expect("decode");
        assert_eq!(decoded, 0xFFFF7F00u32 as i32);
    }

    #[test]
    fn vector_alternative_rejects_wrong_length() {
        let codec = string_rgb_color::<JsonOps>();
        // A wrong-length vector falls through both alternatives; the `either`
        // error composes the nested messages (Java `EitherCodec.decode`).
        assert_eq!(
            error_message(&codec.parse(&JsonOps::INSTANCE, &json!([1.0, 0.0]))),
            "Failed to parse either. First: Not a string: [1.0,0.0]; Second: Failed to parse either. First: Not a number: [1.0,0.0]; Second: Input is not a list of 3 elements"
        );
    }
}
