//! **Partial** port of `net.minecraft.util.ExtraCodecs`.
//!
//! PROVENANCE: `ExtraCodecs.java` is a leaf of the `mc.util` manifest unit
//! (net.minecraft.util -> rivet-util). This module ports ONLY the four methods
//! the registry-core slice needs, because they are pure DFU combinators with
//! no Minecraft dependency and therefore belong in `rivet-serialization` (the
//! crate that owns the `com.mojang.serialization` surface):
//!
//! - `overrideLifecycle(Codec, Function, Function)` and its 1-arg variant —
//!   required by `Registry.referenceHolderWithLifecycle()`.
//! - `retrieveContext(Function)` — required by `RegistryOps`.
//! - `orCompressed(Codec, Codec)` — transitive dependency of
//!   `StringRepresentable.StringRepresentableCodec` (rivet-util).
//! - `idResolverCodec(ToIntFunction, IntFunction, int)` — transitive
//!   dependency of `StringRepresentable.StringRepresentableCodec`.
//!
//! RECONCILIATION: when the full `mc.util` unit is ported, these four free
//! functions move into that unit's `extra_codecs.rs`; they keep the exact same
//! signatures and semantics documented here. Nothing else from
//! `ExtraCodecs.java` (ranges, `nonEmptyList`, `compactListCodec`,
//! `ensureHomogenous`, `orElsePartial`, ...) is ported — that is future
//! `mc.util` scope, not this slice.

use crate::codec::{self, Codec, ResultFunction};
use crate::data_result::DataResult;
use crate::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use crate::lifecycle::Lifecycle;
use crate::map_codec::MapCodec;
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
    decode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle>,
    encode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle>,
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
    lifecycle_getter: Arc<dyn Fn(&E) -> Lifecycle>,
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
    decode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle>,
    encode_lifecycle: Arc<dyn Fn(&E) -> Lifecycle>,
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
type ContextGetter<E, Ops> = Arc<dyn Fn(&Ops) -> DataResult<E>>;

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
    to_int: Arc<dyn Fn(&E) -> i32>,
    from_int: Arc<dyn Fn(i32) -> Option<E>>,
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
