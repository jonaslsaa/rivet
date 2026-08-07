//! `RegistryFileCodec` / `RegistryFixedCodec` / `HolderSetCodec` — the #126
//! holder codecs of `net.minecraft.resources` (MC 26.2).
//!
//! `RegistryDataLoader` (`net.minecraft.server.packs.resources.RegistryDataLoader`)
//! is deliberately NOT here: it is the server-side pack-loading driver built on
//! these codecs, not a codec itself, and defers with its owning unit.
//!
//! PROVENANCE: `RegistryFileCodec.java` (82 lines), `RegistryFixedCodec.java`
//! (73 lines), `HolderSetCodec.java` (108 lines), all leaves of the `mc.resources`
//! manifest unit. The two helpers come from `net.minecraft.util.ExtraCodecs.java`
//! (`compactListCodec` at line 418, `ensureHomogenous` at line 513); they are
//! ported here as private helpers because `HolderSetCodec` is their only
//! consumer in the #126 surface (a later `mc.util` port can lift them).
//!
//! These codecs encode/decode `Holder<T>` / `HolderSet<T>` against a
//! `RegistryOps<T, D>` context (the lookup provider in the ops). Behavior
//! mirrors Java exactly:
//! - A `Reference` encodes as its identifier; a `Direct` falls through to the
//!   element codec (`RegistryFileCodec`) or errors (`RegistryFixedCodec`).
//!   `canSerializeIn` (the O(1) `RegistryId` owner check) gates both.
//! - Decode resolves the identifier through the ops' getter; an unknown element
//!   errors `"Failed to get element <key>"` / `"Failed to get element <id>"`.
//!   `RegistryFixedCodec` propagates a malformed identifier's error verbatim
//!   (`Identifier.CODEC.decode(...).flatMap(...)` in Java), e.g. a non-string
//!   yields `"Not a string: ..."`, an invalid location
//!   `"Not a valid resource location: ..."`.
//! - `HolderSetCodec` is `either(TagKey.hashedCodec, homogenousListCodec)` where
//!   `homogenousListCodec = elementCodec.listOf().validate(ensureHomogenous
//!   (Holder::kind))`, compacted to a bare single value when `alwaysUseList` is
//!   false.
//!
//! The Java `ops instanceof RegistryOps<?>` runtime guard is a compile-time
//! bound (`Ops: RegistryOpsLookup`) — the Rust ops type pins the context, so a
//! codec built for a `RegistryOps` is only ever used with one. The registry
//! being *absent* from the provider is still reachable (a `RegistryOps` over a
//! `RegistryAccess::empty()` or any partial access), so every registry-absent
//! branch below returns a `DataResult` error — never a `todo!`/panic:
//! - `RegistryFileCodec` decode: `"Registry does not exist: <key>"`.
//! - `RegistryFixedCodec` encode/decode: `"Can't access registry <key>"`.
//! - `HolderSetCodec` decode: `decodeWithoutRegistry` (element-list fallback,
//!   every element must be a `Direct` else `"Can't decode element <holder>
//!   without registry"`).
//! - `HolderSetCodec` encode: `encodeWithoutRegistry` (`homogenousListCodec`).
//!
//! Binding-model deviations (documented, PORTING.md drift checklist):
//! - **R✗ encode of a `Reference` is unrepresentable.** Java's non-registry
//!   fallback `elementCodec.encode(input.value())` self-resolves the holder's
//!   stored value; the Rust `(RegistryId, u32)` reference stores no value, so it
//!   is unrecoverable without a lookup. `RegistryFileCodec` returns an honest
//!   `DataResult` error for this state. (`Direct` encode without a registry
//!   works in both.) The holder's key resolution *within* a registry context is
//!   a defensive-invariant panic (a lookup-constructed reference always resolves
//!   its key; only a hand-constructed invalid id reaches it).
//! - Every error that embeds a holder renders the id-form
//!   (`Reference{registry=id}`, `Direct{Debug}`) because a lookup-free `Display`
//!   cannot recover the key/value pair (see `holder.rs`). The message *shape*
//!   (`"Element {} is not valid in current registry set"`, `"HolderSet {} is not
//!   valid in current registry set"`, `"Can't decode element {} without
//!   registry"`) matches Java; only the embedded rendering diverges.
//! - `HolderSetCodec` decode lifecycle: both the tag and list paths end
//!   `experimental` in Java. `registryAwareCodec.decode` carries the string
//!   (experimental) or list (stable) lifecycle, then the inner
//!   `DataResult.success(HolderSet.direct(...))` / `lookupTag` results re-add
//!   `experimental` inside the outer `flatMap` (experimental wins the monoid).
//!   The port mirrors that composition, so the lifecycle flows through
//!   identically — including the error messages (`"Missing tag: ..."` keeps the
//!   tag codec's experimental lifecycle).
//! - `HolderSetCodec` has no Java `toString`; the Rust `Debug`
//!   (`"HolderSetCodec[<key>]"`) is only the trait bound's requirement.
//! - `Holder::key()` on a `Direct` panics `"Direct holder has no key"` — surface
//!   Java's `Holder` interface does not have (`key()` is `Reference`-only).

use std::marker::PhantomData;
use std::sync::Arc;

use crate::holder::Holder;
use crate::holder_lookup::{HolderGetter, HolderLookup, RegistryGetter, RegistryOwner};
use crate::holder_set::HolderSet;
use crate::registry::{Registry, RegistryKey};
use crate::registry_ops::RegistryOpsLookup;
use crate::{ResourceKey, TagKey};

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::either::Either;
use rivet_serialization::encoder::Encoder;
use rivet_serialization::functions::DecoderFn;
use rivet_serialization::lifecycle::Lifecycle;

/// Erase the element type of a registry key.
fn erase_registry_key<E>(key: &ResourceKey<Registry<E>>) -> RegistryKey<()> {
    ResourceKey::create_registry_key(key.identifier().clone())
}

/// The registry-aware either arm of `HolderSetCodec` — a hashed `TagKey<E>` or
/// a homogeneous holder list (`HolderSetCodec.registryAwareCodec`).
type RegistryAware<E, Ops> = Arc<dyn Codec<Either<TagKey<E>, Vec<Holder<E>>>, Ops>>;

// ---------------------------------------------------------------------------
// RegistryFileCodec
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.RegistryFileCodec<E>` — a `Codec<Holder<E>>` that
/// encodes a registered reference by identifier and decodes an identifier (or,
/// when `allow_inline`, an inline element value) as a holder.
pub struct RegistryFileCodec<E, Ops: DynamicOps + 'static> {
    /// `RegistryFileCodec.registryKey`.
    pub registry_key: ResourceKey<Registry<E>>,
    /// `RegistryFileCodec.elementCodec`.
    pub element_codec: Arc<dyn Codec<E, Ops>>,
    /// `RegistryFileCodec.allowInline`.
    pub allow_inline: bool,
}

impl<E, Ops> RegistryFileCodec<E, Ops>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    /// `RegistryFileCodec.create(registryKey, elementCodec)` — `allowInline = true`.
    pub fn create(
        registry_key: &ResourceKey<Registry<E>>,
        element_codec: Arc<dyn Codec<E, Ops>>,
    ) -> Self {
        RegistryFileCodec::create_with_inline(registry_key, element_codec, true)
    }

    /// `RegistryFileCodec.create(registryKey, elementCodec, allowInline)`.
    pub fn create_with_inline(
        registry_key: &ResourceKey<Registry<E>>,
        element_codec: Arc<dyn Codec<E, Ops>>,
        allow_inline: bool,
    ) -> Self {
        RegistryFileCodec {
            registry_key: registry_key.clone(),
            element_codec,
            allow_inline,
        }
    }
}

impl<E, Ops> std::fmt::Debug for RegistryFileCodec<E, Ops>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Java `toString()`: `"RegistryFileCodec[<key> <elementCodec>]"`.
        write!(
            f,
            "RegistryFileCodec[{} {:?}]",
            self.registry_key, self.element_codec
        )
    }
}

impl<E, Ops> Encoder<Holder<E>, Ops> for RegistryFileCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn encode(
        &self,
        input: &Holder<E>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let info = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key));
        if let Some(info) = info {
            let owner = RegistryOwner {
                registry_id: info.registry_id,
            };
            if !input.can_serialize_in(&owner) {
                return DataResult::error(format!(
                    "Element {} is not valid in current registry set",
                    input
                ));
            }
            // `input.unwrap().map(id -> Identifier.CODEC.encode(id.identifier(),
            // ops, prefix), value -> elementCodec.encode(value, ops, prefix))`.
            return match input {
                Holder::Direct(value) => self.element_codec.encode(value, ops, prefix),
                Holder::Reference { .. } => {
                    // The reference's identifier is read through the ops' getter
                    // (back-reference rule): resolve the key, encode its
                    // identifier. A lookup-constructed reference always resolves;
                    // the panic is the defensive-invariant path for a hand-built
                    // invalid id.
                    let getter =
                        RegistryGetter::new(info.access.clone(), self.registry_key.clone());
                    let identifier = getter.key_of(input).map(|key| key.identifier().clone());
                    match identifier {
                        Some(identifier) => crate::identifier::identifier_codec::<Ops>().encode(
                            &identifier,
                            ops,
                            prefix,
                        ),
                        None => panic!("Reference holder has no key: {}", input),
                    }
                }
            };
        }
        // No registry context: Java falls back to `elementCodec.encode(input.value())`.
        match input {
            Holder::Direct(value) => self.element_codec.encode(value, ops, prefix),
            // Java self-resolves the reference's stored value here; the ID model
            // cannot (no lookup), so this is an honest error for an
            // unrepresentable state (see module docs).
            Holder::Reference { .. } => DataResult::error(format!(
                "Can't encode reference holder {} without a registry context",
                input
            )),
        }
    }
}

impl<E, Ops> Decoder<Holder<E>, Ops> for RegistryFileCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Holder<E>, Ops::Output)> {
        let getter = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .map(|info| RegistryGetter::new(info.access.clone(), self.registry_key.clone()));
        let getter = match getter {
            Some(getter) => getter,
            None => {
                return DataResult::error(format!(
                    "Registry does not exist: {}",
                    self.registry_key
                ));
            }
        };
        // `Identifier.CODEC.decode(ops, input)`; if that fails, decode as the
        // element codec (if inline allowed) else error.
        let id_decoded = crate::identifier::identifier_codec::<Ops>().decode(ops, input);
        match id_decoded.result() {
            Some(pair) => {
                let (identifier, rest) = pair.clone();
                let element_key = ResourceKey::create(&self.registry_key, identifier);
                match getter.get(&element_key) {
                    Some(holder) => {
                        DataResult::success_with_lifecycle((holder, rest), Lifecycle::stable())
                    }
                    None => DataResult::error_with_lifecycle(
                        format!("Failed to get element {}", element_key),
                        Lifecycle::stable(),
                    ),
                }
            }
            None => {
                if !self.allow_inline {
                    return DataResult::error("Inline definitions not allowed here");
                }
                self.element_codec
                    .decode(ops, input)
                    .map_owned(|(value, rest)| (Holder::direct(value), rest))
            }
        }
    }
}

impl<E, Ops> Codec<Holder<E>, Ops> for RegistryFileCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
}

// ---------------------------------------------------------------------------
// RegistryFixedCodec
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.RegistryFixedCodec<E>` — a `Codec<Holder<E>>` that
/// only encodes/decodes registered references by identifier; inline values and
/// missing contexts are errors.
pub struct RegistryFixedCodec<E, Ops: DynamicOps + 'static> {
    /// `RegistryFixedCodec.registryKey`.
    pub registry_key: ResourceKey<Registry<E>>,
    _marker: PhantomData<fn() -> Ops>,
}

impl<E, Ops> RegistryFixedCodec<E, Ops>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    /// `RegistryFixedCodec.create(registryKey)`.
    pub fn create(registry_key: &ResourceKey<Registry<E>>) -> Self {
        RegistryFixedCodec {
            registry_key: registry_key.clone(),
            _marker: PhantomData,
        }
    }
}

impl<E, Ops> std::fmt::Debug for RegistryFixedCodec<E, Ops>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RegistryFixedCodec[{}]", self.registry_key)
    }
}

impl<E, Ops> Encoder<Holder<E>, Ops> for RegistryFixedCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn encode(
        &self,
        input: &Holder<E>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let info = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key));
        if let Some(info) = info {
            let owner = RegistryOwner {
                registry_id: info.registry_id,
            };
            if !input.can_serialize_in(&owner) {
                return DataResult::error(format!(
                    "Element {} is not valid in current registry set",
                    input
                ));
            }
            // `input.unwrap().map(id -> Identifier.CODEC.encode(...), value ->
            // error)`.
            return match input {
                Holder::Reference { .. } => {
                    let getter =
                        RegistryGetter::new(info.access.clone(), self.registry_key.clone());
                    let identifier = getter.key_of(input).map(|key| key.identifier().clone());
                    match identifier {
                        Some(identifier) => crate::identifier::identifier_codec::<Ops>().encode(
                            &identifier,
                            ops,
                            prefix,
                        ),
                        None => panic!("Reference holder has no key: {}", input),
                    }
                }
                Holder::Direct(_) => DataResult::error(format!(
                    "Elements from registry {} can't be serialized to a value",
                    self.registry_key
                )),
            };
        }
        DataResult::error(format!("Can't access registry {}", self.registry_key))
    }
}

impl<E, Ops> Decoder<Holder<E>, Ops> for RegistryFixedCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Holder<E>, Ops::Output)> {
        let getter = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .map(|info| RegistryGetter::new(info.access.clone(), self.registry_key.clone()));
        let getter = match getter {
            Some(getter) => getter,
            None => {
                return DataResult::error(format!("Can't access registry {}", self.registry_key));
            }
        };
        // Java: `Identifier.CODEC.decode(ops, input).flatMap(...)`. The
        // `flatMap` propagates a malformed identifier's error verbatim (e.g.
        // `"Not a valid resource location: ..."`, `"Not a string: ..."`).
        crate::identifier::identifier_codec::<Ops>()
            .decode(ops, input)
            .flat_map(|(identifier, rest)| {
                let element_key = ResourceKey::create(&self.registry_key, identifier.clone());
                match getter.get(&element_key) {
                    Some(holder) => {
                        DataResult::success_with_lifecycle((holder, rest), Lifecycle::stable())
                    }
                    None => DataResult::error_with_lifecycle(
                        format!("Failed to get element {}", identifier),
                        Lifecycle::stable(),
                    ),
                }
            })
    }
}

impl<E, Ops> Codec<Holder<E>, Ops> for RegistryFixedCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
}

// ---------------------------------------------------------------------------
// ExtraCodecs helpers (compactListCodec / ensureHomogenous)
// ---------------------------------------------------------------------------

/// `ExtraCodecs.ensureHomogenous(Holder::kind)` (ExtraCodecs.java:513) — the
/// checker passed to `Codec.validate` on `HolderSetCodec`'s list codec.
///
/// Every element must share the first element's `Holder.Kind`; a mix errors
/// `"Mixed type list: element <next> had type <KIND>, but list is of type
/// <KIND>"` with the upper-case kind names. The homogeneous result carries
/// `Lifecycle.stable()` exactly like Java's
/// `DataResult.success(container, Lifecycle.stable())`.
fn ensure_homogenous<E>() -> DecoderFn<Vec<Holder<E>>, Vec<Holder<E>>>
where
    E: Clone + std::fmt::Debug,
{
    Arc::new(|container: &Vec<Holder<E>>| {
        let mut iter = container.iter();
        if let Some(first) = iter.next() {
            let first_kind = first.kind();
            for next in iter {
                let next_kind = next.kind();
                if next_kind != first_kind {
                    return DataResult::error(format!(
                        "Mixed type list: element {} had type {}, but list is of type {}",
                        next, next_kind, first_kind
                    ));
                }
            }
        }
        DataResult::success_with_lifecycle(container.clone(), Lifecycle::stable())
    })
}

/// `ExtraCodecs.compactListCodec(elementCodec, listCodec)` (ExtraCodecs.java:418)
/// — `either(listCodec, elementCodec).xmap(...)`.
///
/// A multi-element list encodes as a list; a single-element list encodes as the
/// bare element value. Decode accepts both forms (a bare value decodes to a
/// single-element list). This is what makes `HolderSetCodec`'s `alwaysUseList`
/// flag observable.
fn compact_list_codec<E, Ops: DynamicOps + 'static>(
    element_codec: Arc<dyn Codec<E, Ops>>,
    list_codec: Arc<dyn Codec<Vec<E>, Ops>>,
) -> Arc<dyn Codec<Vec<E>, Ops>>
where
    E: 'static + Clone,
{
    codec::xmap(
        codec::either(list_codec, element_codec),
        // `xmap` decode: `Either<List<E>, E> -> Vec<E>` (Right unwraps to a
        // singleton list).
        Arc::new(|e: &Either<Vec<E>, E>| {
            e.map_ref(|list| list.clone(), |single| vec![single.clone()])
        }),
        // `xmap` encode: `Vec<E> -> Either<List<E>, E>` (size 1 compacts to the
        // bare element).
        Arc::new(|v: &Vec<E>| {
            if v.len() == 1 {
                Either::right(v[0].clone())
            } else {
                Either::left(v.clone())
            }
        }),
    )
}

// ---------------------------------------------------------------------------
// HolderSetCodec
// ---------------------------------------------------------------------------

/// `net.minecraft.resources.HolderSetCodec<E>` — a `Codec<HolderSet<E>>` that
/// decodes either a hashed tag key (a bound `Named`) or a holder list (a
/// `Direct`), against a `RegistryOps` context.
pub struct HolderSetCodec<E, Ops: DynamicOps + 'static> {
    /// `HolderSetCodec.registryKey`.
    pub registry_key: ResourceKey<Registry<E>>,
    /// `HolderSetCodec.elementCodec` — the `Codec<Holder<E>>` list elements and
    /// the non-registry fallback decode through.
    pub element_codec: Arc<dyn Codec<Holder<E>, Ops>>,
    /// `HolderSetCodec.homogenousListCodec`.
    homogenous_list: Arc<dyn Codec<Vec<Holder<E>>, Ops>>,
    /// `HolderSetCodec.registryAwareCodec`.
    registry_aware: RegistryAware<E, Ops>,
}

impl<E, Ops> HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static + Clone + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    /// `HolderSetCodec.create(registryKey, elementCodec, alwaysUseList)`.
    pub fn create(
        registry_key: &ResourceKey<Registry<E>>,
        element_codec: Arc<dyn Codec<Holder<E>, Ops>>,
        always_use_list: bool,
    ) -> Self {
        // `homogenousList(elementCodec, alwaysUseList)`
        // (HolderSetCodec.java:25): `elementCodec.listOf().validate(ensureHomogenous
        // (Holder::kind))`, compacted when `alwaysUseList` is false.
        let list_codec: Arc<dyn Codec<Vec<Holder<E>>, Ops>> =
            codec::validate(codec::list(element_codec.clone()), ensure_homogenous::<E>());
        let homogenous_list = if always_use_list {
            list_codec
        } else {
            compact_list_codec::<Holder<E>, Ops>(element_codec.clone(), list_codec)
        };
        // `Codec.either(TagKey.hashedCodec(registryKey), homogenousListCodec)`.
        let tag_codec: Arc<dyn Codec<TagKey<E>, Ops>> =
            crate::tag_key::tag_key_hashed_codec::<E, Ops>(registry_key);
        let registry_aware: RegistryAware<E, Ops> =
            codec::either(tag_codec, homogenous_list.clone());
        HolderSetCodec {
            registry_key: registry_key.clone(),
            element_codec,
            homogenous_list,
            registry_aware,
        }
    }

    /// `HolderSetCodec.decodeWithoutRegistry` (HolderSetCodec.java:89) — the
    /// element-list fallback when the ops' provider has no getter for the
    /// registry. Every decoded element must be a `Holder.Direct`; a reference
    /// errors `"Can't decode element <holder> without registry"`.
    fn decode_without_registry(
        &self,
        ops: &Ops,
        input: &Ops::Output,
    ) -> DataResult<(HolderSet<E>, Ops::Output)> {
        // Java uses the plain `elementCodec.listOf()` here (no validate/compact).
        let list_codec: Arc<dyn Codec<Vec<Holder<E>>, Ops>> =
            codec::list(self.element_codec.clone());
        list_codec.decode(ops, input).flat_map(|(holders, rest)| {
            for holder in &holders {
                if !matches!(holder, Holder::Direct(_)) {
                    return DataResult::error(format!(
                        "Can't decode element {} without registry",
                        holder
                    ));
                }
            }
            DataResult::success((HolderSet::direct(holders), rest))
        })
    }
}

/// `HolderSetCodec.lookupTag` (HolderSetCodec.java:67) — resolve a tag to its
/// bound `Named` set through the getter, erroring
/// `"Missing tag: '<location>' in '<registry>'"` when absent.
fn lookup_tag<E>(getter: &impl HolderGetter<E>, key: TagKey<E>) -> DataResult<HolderSet<E>> {
    match getter.get_tag(&key) {
        Some(set) => DataResult::success(set),
        None => DataResult::error(format!(
            "Missing tag: '{}' in '{}'",
            key.location(),
            key.registry().identifier()
        )),
    }
}

impl<E, Ops> std::fmt::Debug for HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HolderSetCodec[{}]", self.registry_key)
    }
}

impl<E, Ops> Encoder<HolderSet<E>, Ops> for HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static + Clone + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn encode(
        &self,
        input: &HolderSet<E>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let info = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key));
        if let Some(info) = info {
            let owner = RegistryOwner {
                registry_id: info.registry_id,
            };
            if !input.can_serialize_in(&owner) {
                return DataResult::error(format!(
                    "HolderSet {} is not valid in current registry set",
                    input
                ));
            }
            // `registryAwareCodec.encode(input.unwrap().mapRight(List::copyOf), ...)`.
            let either: Either<TagKey<E>, Vec<Holder<E>>> = match input.unwrap() {
                Either::Left(tag) => Either::left(tag),
                Either::Right(holders) => Either::right(holders.to_vec()),
            };
            return self.registry_aware.encode(&either, ops, prefix);
        }
        // `encodeWithoutRegistry` (HolderSetCodec.java:105):
        // `homogenousListCodec.encode(input.stream().toList(), ...)`.
        let members = input.stream();
        self.homogenous_list.encode(&members, ops, prefix)
    }
}

impl<E, Ops> Decoder<HolderSet<E>, Ops> for HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static + Clone + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(HolderSet<E>, Ops::Output)> {
        let getter = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .map(|info| RegistryGetter::new(info.access.clone(), self.registry_key.clone()));
        if let Some(getter) = getter {
            // `registryAwareCodec.decode(ops, input).flatMap(...)` — the exact
            // Java composition, so the lifecycle flows through identically: the
            // tag-or-list either decode's lifecycle is combined with the inner
            // `DataResult::success`/`lookupTag` experimental lifecycle.
            return self
                .registry_aware
                .decode(ops, input)
                .flat_map(|(either, rest)| {
                    let result: DataResult<HolderSet<E>> = match either {
                        Either::Left(tag) => lookup_tag(&getter, tag),
                        Either::Right(holders) => DataResult::success(HolderSet::direct(holders)),
                    };
                    result.map_owned(|set| (set, rest))
                });
        }
        self.decode_without_registry(ops, input)
    }
}

impl<E, Ops> Codec<HolderSet<E>, Ops> for HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static + Clone + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::RegistryAccess;
    use crate::builder::RegistryBuilder;
    use crate::holder::{Holder, HolderId, RegistryId};
    use crate::registration_info::RegistrationInfo;
    use crate::registry_ops::RegistryOps;
    use crate::root::AnyBox;
    use crate::{Identifier, ResourceKey, TagKey};

    use rivet_serialization::json_ops::JsonOps;
    use serde_json::json;

    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq)]
    struct TestElement(u8);

    fn registry_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn element_key(id: &str) -> ResourceKey<TestElement> {
        ResourceKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    fn tag_key(id: &str) -> TagKey<TestElement> {
        TagKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    type TestOps = RegistryOps<serde_json::Value, JsonOps>;

    /// A TestElement codec over identifiers: encodes `TestElement` as
    /// `"minecraft:e"`, decodes a valid identifier to `TestElement(0)` (used by
    /// the FileCodec tests and the identifier-grounded HolderSetCodec tests).
    fn element_codec() -> Arc<dyn Codec<TestElement, TestOps>> {
        codec::xmap(
            crate::identifier::identifier_codec::<TestOps>(),
            Arc::new(|_id: &Identifier| TestElement(0)),
            Arc::new(|_e: &TestElement| Identifier::with_default_namespace("e")),
        )
    }

    /// A TestElement codec over integers: decodes a number to `TestElement(n)`
    /// (the inline path of `RegistryFileCodec` for non-identifier values, so a
    /// list element can decode as a `Direct` — needed for the mixed-kinds test).
    fn inline_element_codec() -> Arc<dyn Codec<TestElement, TestOps>> {
        codec::xmap(
            codec::int_codec::<TestOps>(),
            Arc::new(|n: &i32| TestElement(*n as u8)),
            Arc::new(|_e: &TestElement| 0i32),
        )
    }

    fn holder_element_codec() -> Arc<dyn Codec<Holder<TestElement>, TestOps>> {
        // Java passes a `RegistryFileCodec` as `HolderSetCodec`'s element codec,
        // so a list element decodes as a `Holder.Reference` by identifier.
        //
        // The concrete codec is not `Send + Sync`: its `RegistryOps` carries the
        // single-threaded `HolderLookupAdapter` (`RefCell` memo, OWNERSHIP's
        // single sync tick). The `Arc` here is test-local and never crosses
        // threads.
        #[allow(clippy::arc_with_non_send_sync)]
        Arc::new(RegistryFileCodec::create(&registry_key(), element_codec()))
    }

    fn inline_holder_element_codec() -> Arc<dyn Codec<Holder<TestElement>, TestOps>> {
        #[allow(clippy::arc_with_non_send_sync)]
        Arc::new(RegistryFileCodec::create(
            &registry_key(),
            inline_element_codec(),
        ))
    }

    /// A `Codec<Holder<TestElement>>` that produces a `Reference` from any
    /// identifier *without* a registry — exercises the
    /// `"Can't decode element ... without registry"` branch, which needs a
    /// reference holder from a registry-less decode.
    fn reference_holder_element_codec() -> Arc<dyn Codec<Holder<TestElement>, TestOps>> {
        codec::xmap(
            crate::identifier::identifier_codec::<TestOps>(),
            Arc::new(|_id: &Identifier| Holder::reference(RegistryId(0), 0)),
            Arc::new(|_h: &Holder<TestElement>| Identifier::with_default_namespace("x")),
        )
    }

    /// A registry-less `Codec<Holder<TestElement>>` that produces a `Direct`
    /// from any number — exercises `decodeWithoutRegistry`'s success path (a
    /// non-registry element codec, the only way Java's R✗ fallback yields
    /// `Direct`s).
    fn direct_holder_element_codec() -> Arc<dyn Codec<Holder<TestElement>, TestOps>> {
        codec::xmap(
            codec::int_codec::<TestOps>(),
            Arc::new(|n: &i32| Holder::direct(TestElement(*n as u8))),
            Arc::new(|_h: &Holder<TestElement>| 0i32),
        )
    }

    fn access_with_registry() -> RegistryAccess {
        let mut builder = RegistryBuilder::new(&registry_key());
        builder.register(
            &element_key("a"),
            Arc::new(TestElement(1)),
            RegistrationInfo::BUILT_IN,
        );
        let b = builder.register(
            &element_key("b"),
            Arc::new(TestElement(2)),
            RegistrationInfo::BUILT_IN,
        );
        builder.bind_tags(vec![(tag_key("group"), vec![b])]);
        let registry = builder.freeze();
        RegistryAccess::from_pairs(vec![(
            ResourceKey::create_registry_key(Identifier::with_default_namespace("test")),
            Box::new(registry) as AnyBox,
        )])
    }

    fn ops(access: RegistryAccess) -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, access)
    }

    fn ops_compressed(access: RegistryAccess) -> TestOps {
        RegistryOps::create_from_access(&JsonOps::COMPRESSED, access)
    }

    fn empty_ops() -> TestOps {
        RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty())
    }

    /// The frozen registry's per-instance `RegistryId` (the builder assigns a
    /// real id; tests must read it rather than assume `0`).
    fn registry_id_of(access: &RegistryAccess) -> RegistryId {
        access
            .lookup::<TestElement>(&registry_key())
            .expect("frozen registry")
            .registry_id()
    }

    // -----------------------------------------------------------------------
    // RegistryFileCodec
    // -----------------------------------------------------------------------

    #[test]
    fn registry_file_codec_encodes_a_reference_as_its_identifier() {
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let holder = Holder::reference(owner, 0);
        let encoded = codec
            .encode(&holder, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        // A Reference encodes as its identifier string (Java
        // `Identifier.CODEC.encode`).
        assert_eq!(encoded, ops.create_string("minecraft:a".to_string()));
    }

    #[test]
    fn registry_file_codec_decodes_an_identifier_to_a_reference() {
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let input = ops.create_string("minecraft:b".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, Holder::reference(owner, 1));
    }

    #[test]
    fn registry_file_codec_decode_lifecycle_is_stable() {
        // Java: `.setLifecycle(Lifecycle.stable())` after the getter lookup.
        let ops = ops(access_with_registry());
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let input = ops.create_string("minecraft:a".to_string());
        let decoded = codec.decode(&ops, &input);
        assert!(decoded.is_success());
        assert_eq!(decoded.lifecycle(), Lifecycle::Stable);
    }

    #[test]
    fn registry_file_codec_encodes_a_reference_from_a_different_owner_as_invalid() {
        // Java: `input.canSerializeIn(owner)` fails for a reference whose
        // RegistryId differs from the ops' registry, erroring
        // `"Element <holder> is not valid in current registry set"`.
        let ops = ops(access_with_registry());
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let holder: Holder<TestElement> = Holder::reference(RegistryId(u32::MAX), 0);
        let result = codec.encode(&holder, &ops, &ops.empty());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Element {} is not valid in current registry set", holder)
        );
    }

    #[test]
    fn registry_file_codec_decodes_unknown_identifier_to_error() {
        let ops = ops(access_with_registry());
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let input = ops.create_string("minecraft:nope".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Failed to get element"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn registry_file_codec_allow_inline_false_rejects_inline_values() {
        let ops = ops(access_with_registry());
        let codec = RegistryFileCodec::create_with_inline(&registry_key(), element_codec(), false);
        // A non-identifier (here a number) fails the identifier decode; with
        // allowInline=false Java errors "Inline definitions not allowed here".
        let result = codec.decode(&ops, &ops.create_int(7));
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            "Inline definitions not allowed here"
        );
    }

    #[test]
    fn registry_file_codec_allow_inline_true_decodes_an_inline_value() {
        // allowInline=true (the default): a non-identifier value falls through
        // to the element codec and wraps as a Direct holder (Java
        // `elementCodec.decode(...).map(p -> p.mapFirst(Holder::direct))`).
        let ops = ops(access_with_registry());
        let codec = RegistryFileCodec::create(&registry_key(), inline_element_codec());
        let decoded = codec
            .decode(&ops, &ops.create_int(7))
            .get_or_throw("decode")
            .clone();
        assert_eq!(decoded.0, Holder::direct(TestElement(7)));
    }

    #[test]
    fn registry_file_codec_missing_registry_decode_errors() {
        // A `RegistryOps` over an empty provider: the getter is absent, so Java
        // errors `"Registry does not exist: <key>"`.
        let ops = empty_ops();
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let result = codec.decode(&ops, &ops.create_string("minecraft:a".to_string()));
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Registry does not exist: {}", registry_key())
        );
    }

    #[test]
    fn registry_file_codec_missing_registry_encode_direct_still_works() {
        // Java's non-registry fallback `elementCodec.encode(input.value())` — a
        // Direct holder encodes through the element codec even with no registry.
        let ops = empty_ops();
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let encoded = codec
            .encode(&Holder::direct(TestElement(9)), &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_string("minecraft:e".to_string()));
    }

    #[test]
    fn registry_file_codec_missing_registry_encode_reference_errors() {
        // A Reference stores no value in the ID model, so the non-registry
        // encode is unrecoverable — an honest error, not a panic/todo.
        let ops = empty_ops();
        let codec = RegistryFileCodec::create(&registry_key(), element_codec());
        let holder = Holder::reference(RegistryId(0), 0);
        let result = codec.encode(&holder, &ops, &ops.empty());
        assert!(result.is_error());
        assert!(
            result
                .error_ref()
                .unwrap()
                .message()
                .contains("without a registry context"),
            "unexpected message: {}",
            result.error_ref().unwrap().message()
        );
    }

    // -----------------------------------------------------------------------
    // RegistryFixedCodec
    // -----------------------------------------------------------------------

    #[test]
    fn registry_fixed_codec_decodes_an_identifier_to_a_reference() {
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = RegistryFixedCodec::create(&registry_key());
        let input = ops.create_string("minecraft:a".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0, Holder::reference(owner, 0));
    }

    #[test]
    fn registry_fixed_codec_decode_lifecycle_is_stable() {
        let ops = ops(access_with_registry());
        let codec = RegistryFixedCodec::create(&registry_key());
        let input = ops.create_string("minecraft:a".to_string());
        let decoded = codec.decode(&ops, &input);
        assert!(decoded.is_success());
        assert_eq!(decoded.lifecycle(), Lifecycle::Stable);
    }

    #[test]
    fn registry_fixed_codec_rejects_a_reference_from_a_different_owner_on_encode() {
        // `canSerializeIn` gates RegistryFixedCodec encode too (Java's
        // `"Element <holder> is not valid in current registry set"`).
        let ops = ops(access_with_registry());
        let codec = RegistryFixedCodec::create(&registry_key());
        let holder: Holder<TestElement> = Holder::reference(RegistryId(u32::MAX), 0);
        let result = codec.encode(&holder, &ops, &ops.empty());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Element {} is not valid in current registry set", holder)
        );
    }

    #[test]
    fn registry_fixed_codec_decodes_unknown_identifier_to_error() {
        // Java: `"Failed to get element <id>"` — the bare Identifier, distinct
        // from RegistryFileCodec's `"Failed to get element <key>"` (full
        // ResourceKey) form.
        let ops = ops(access_with_registry());
        let codec = RegistryFixedCodec::create(&registry_key());
        let input = ops.create_string("minecraft:nope".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!(
                "Failed to get element {}",
                Identifier::with_default_namespace("nope")
            )
        );
    }

    #[test]
    fn registry_fixed_codec_encodes_a_reference_as_its_identifier() {
        // The positive encode path: a reference from the ops' registry encodes
        // as its identifier (Java `Identifier.CODEC.encode(id.identifier())`).
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = RegistryFixedCodec::create(&registry_key());
        let holder = Holder::reference(owner, 1);
        let encoded = codec
            .encode(&holder, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_string("minecraft:b".to_string()));
    }

    #[test]
    fn registry_fixed_codec_rejects_a_direct_holder_on_encode() {
        let ops = ops(access_with_registry());
        let codec = RegistryFixedCodec::create(&registry_key());
        let holder = Holder::direct(TestElement(9));
        let result = codec.encode(&holder, &ops, &ops.empty());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!(
                "Elements from registry {} can't be serialized to a value",
                registry_key()
            )
        );
    }

    #[test]
    fn registry_fixed_codec_malformed_identifier_propagates_identifier_error() {
        // Java `Identifier.CODEC.decode(ops, input).flatMap(...)`: a malformed
        // identifier keeps the identifier decode's error instead of replacing it
        // with a registry message.
        let ops = ops(access_with_registry());
        let codec = RegistryFixedCodec::create(&registry_key());
        // A non-string (JsonOps "Not a string: 42").
        let result = codec.decode(&ops, &ops.create_int(42));
        assert!(result.is_error());
        assert_eq!(result.error_ref().unwrap().message(), "Not a string: 42");
        // An invalid resource location (Identifier::read's message).
        let input = ops.create_string("a b:c".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        assert!(
            result
                .error_ref()
                .unwrap()
                .message()
                .contains("Not a valid resource location"),
            "unexpected message: {}",
            result.error_ref().unwrap().message()
        );
    }

    #[test]
    fn registry_fixed_codec_missing_registry_decode_errors() {
        let ops = empty_ops();
        let codec = RegistryFixedCodec::create(&registry_key());
        let result = codec.decode(&ops, &ops.create_string("minecraft:a".to_string()));
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Can't access registry {}", registry_key())
        );
    }

    #[test]
    fn registry_fixed_codec_missing_registry_encode_errors() {
        let ops = empty_ops();
        let codec = RegistryFixedCodec::create(&registry_key());
        let result = codec.encode(&Holder::reference(RegistryId(0), 0), &ops, &ops.empty());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("Can't access registry {}", registry_key())
        );
    }

    // -----------------------------------------------------------------------
    // HolderSetCodec — registry present
    // -----------------------------------------------------------------------

    #[test]
    fn holder_set_codec_encodes_a_named_set_as_a_hashed_tag_key() {
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let set = HolderSet::named_from_ids(owner, tag_key("group"), &[HolderId(1)]);
        let encoded = codec
            .encode(&set, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        // A Named set encodes as "#<location>" (TagKey.hashedCodec).
        assert_eq!(encoded, ops.create_string("#minecraft:group".to_string()));
    }

    #[test]
    fn holder_set_codec_decodes_a_hashed_tag_to_a_bound_named_set() {
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let input = ops.create_string("#minecraft:group".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        let set = decoded.0;
        assert!(set.is_bound());
        assert_eq!(set.unwrap_key(), Some(tag_key("group")));
        // The Named set belongs to the registry's owner id.
        assert!(matches!(
            set,
            HolderSet::Named { owner: o, .. } if o == owner
        ));
    }

    #[test]
    fn holder_set_codec_tag_decode_lifecycle_is_experimental() {
        // Java: `lookupTag`'s `DataResult::success` (experimental) re-added over
        // the tag codec's string decode lifecycle (experimental for JsonOps).
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let input = ops.create_string("#minecraft:group".to_string());
        let decoded = codec.decode(&ops, &input);
        assert!(decoded.is_success());
        assert_eq!(decoded.lifecycle(), Lifecycle::Experimental);
    }

    #[test]
    fn holder_set_codec_list_decode_lifecycle_is_experimental() {
        // Java: the list path's `DataResult.success(HolderSet.direct(...))`
        // (experimental) re-adds experimental over the list decode's stable
        // lifecycle inside the outer flatMap.
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let input = json!(["minecraft:a", "minecraft:b"]);
        let decoded = codec.decode(&ops, &input);
        assert!(decoded.is_success());
        assert_eq!(decoded.lifecycle(), Lifecycle::Experimental);
    }

    #[test]
    fn holder_set_codec_decodes_a_holder_list_to_a_direct_set() {
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        // A JSON list of identifiers decodes as a Direct holder set.
        let input = json!(["minecraft:a", "minecraft:b"]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        let set = decoded.0;
        match set {
            HolderSet::Direct(holders) => {
                assert_eq!(holders.len(), 2);
                assert!(
                    matches!(holders[0], Holder::Reference { registry: r, id: 0 } if r == owner)
                );
                assert!(
                    matches!(holders[1], Holder::Reference { registry: r, id: 1 } if r == owner)
                );
            }
            HolderSet::Named { .. } => panic!("a list must decode as Direct, not Named"),
        }
    }

    #[test]
    fn holder_set_codec_decodes_a_bare_singleton_to_a_single_element_direct_set() {
        // alwaysUseList=false: the compact list codec decodes a bare identifier
        // as a single-element Direct set (Java's element arm of
        // `compactListCodec`).
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let input = ops.create_string("minecraft:a".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        match decoded.0 {
            HolderSet::Direct(holders) => {
                assert_eq!(holders.len(), 1);
                assert_eq!(holders[0], Holder::reference(owner, 0));
            }
            HolderSet::Named { .. } => panic!("a bare value must decode as Direct, not Named"),
        }
    }

    #[test]
    fn holder_set_codec_always_use_list_rejects_a_bare_singleton() {
        // alwaysUseList=true: the compact element arm is disabled, so a bare
        // value fails both the tag codec and the list codec → EitherCodec error.
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), true);
        let input = ops.create_string("minecraft:a".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Failed to parse either"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn holder_set_codec_encodes_a_single_element_direct_set_compactly() {
        // alwaysUseList=false: a single-element Direct set encodes as the bare
        // element value (compactListCodec's size-1 right arm).
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let set = HolderSet::direct(vec![Holder::direct(TestElement(9))]);
        let encoded = codec
            .encode(&set, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_string("minecraft:e".to_string()));
    }

    #[test]
    fn holder_set_codec_encodes_a_single_element_direct_set_of_references_compactly() {
        // A Direct set of references (the registry-grounded list form) also
        // compacts to the bare identifier when alwaysUseList=false — the member
        // encodes through the RegistryFileCodec element codec.
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let set = HolderSet::direct(vec![Holder::reference(owner, 0)]);
        let encoded = codec
            .encode(&set, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_string("minecraft:a".to_string()));
    }

    #[test]
    fn holder_set_codec_encodes_a_multi_element_direct_set_of_references_as_a_list() {
        // alwaysUseList=false but two members: no compaction, the list arm
        // encodes each reference by identifier.
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let ops = ops(access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let set = HolderSet::direct(vec![
            Holder::reference(owner, 0),
            Holder::reference(owner, 1),
        ]);
        let encoded = codec
            .encode(&set, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            ops.create_list(vec![
                ops.create_string("minecraft:a".to_string()),
                ops.create_string("minecraft:b".to_string()),
            ])
        );
    }

    #[test]
    fn holder_set_codec_always_use_list_encodes_a_single_element_as_a_list() {
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), true);
        let set = HolderSet::direct(vec![Holder::direct(TestElement(9))]);
        let encoded = codec
            .encode(&set, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(
            encoded,
            ops.create_list(vec![ops.create_string("minecraft:e".to_string())])
        );
    }

    #[test]
    fn holder_set_codec_encodes_a_named_set_from_a_different_owner_as_invalid() {
        // Java: `input.canSerializeIn(owner)` fails for a Named set whose owner
        // RegistryId differs from the ops' registry, erroring
        // `"HolderSet <holder set> is not valid in current registry set"`.
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let set = HolderSet::named_from_ids(RegistryId(u32::MAX), tag_key("group"), &[HolderId(1)]);
        let result = codec.encode(&set, &ops, &ops.empty());
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            format!("HolderSet {} is not valid in current registry set", set)
        );
    }

    #[test]
    fn holder_set_codec_mixed_kinds_are_rejected() {
        // ensureHomogenous errors when a list mixes Reference and Direct
        // holders (the inline element codec lets the number 5 decode as a
        // Direct).
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), inline_holder_element_codec(), false);
        let input = json!(["minecraft:a", 5]);
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Mixed type list: element")
                && msg.contains("had type DIRECT")
                && msg.contains("but list is of type REFERENCE"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn holder_set_codec_missing_tag_errors() {
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let input = ops.create_string("#minecraft:nope".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        assert_eq!(
            result.error_ref().unwrap().message(),
            "Missing tag: 'minecraft:nope' in 'minecraft:test'"
        );
    }

    #[test]
    fn holder_set_codec_malformed_input_produces_either_error() {
        // A number with the identifier-grounded element codec fails the tag arm
        // (not a string) and the list arm (not a json array / element decode) —
        // EitherCodec aggregates into "Failed to parse either".
        let ops = ops(access_with_registry());
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let result = codec.decode(&ops, &ops.create_int(42));
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Failed to parse either"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn holder_set_codec_decodes_a_holder_list_under_compressed_ops() {
        // Holder codecs touch only strings/lists, so `JsonOps.COMPRESSED`
        // produces identical output to `JsonOps.INSTANCE`.
        let access = access_with_registry();
        let owner = registry_id_of(&access);
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let plain = ops(access.clone());
        let compressed = ops_compressed(access);
        let input = json!(["minecraft:a", "minecraft:b"]);
        let plain_decoded = codec.decode(&plain, &input).get_or_throw("decode").clone();
        let compressed_decoded = codec
            .decode(&compressed, &input)
            .get_or_throw("decode")
            .clone();
        assert_eq!(plain_decoded.0, compressed_decoded.0);
        match compressed_decoded.0 {
            HolderSet::Direct(holders) => {
                assert_eq!(
                    holders,
                    vec![Holder::reference(owner, 0), Holder::reference(owner, 1)]
                );
            }
            HolderSet::Named { .. } => panic!("a list must decode as Direct, not Named"),
        }
    }

    // -----------------------------------------------------------------------
    // HolderSetCodec — registry absent (decodeWithoutRegistry /
    // encodeWithoutRegistry)
    // -----------------------------------------------------------------------

    #[test]
    fn holder_set_codec_missing_registry_decode_uses_element_list_fallback() {
        // `decodeWithoutRegistry` decodes a list via the plain element codec and
        // requires every element to be Direct. The Direct-producing element
        // codec (a non-registry element codec) yields Directs for bare numbers.
        let ops = empty_ops();
        let codec = HolderSetCodec::create(&registry_key(), direct_holder_element_codec(), false);
        let input = json!([5, 6]);
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        match decoded.0 {
            HolderSet::Direct(holders) => {
                assert_eq!(holders.len(), 2);
                assert!(matches!(holders[0], Holder::Direct(TestElement(5))));
                assert!(matches!(holders[1], Holder::Direct(TestElement(6))));
            }
            HolderSet::Named { .. } => panic!("a list must decode as Direct, not Named"),
        }
    }

    #[test]
    fn holder_set_codec_missing_registry_with_registry_element_codec_errors() {
        // The practical Java construction passes a `RegistryFileCodec` as the
        // element codec; with the registry absent from the ops provider its
        // decode errors `"Registry does not exist"`, so `decodeWithoutRegistry`
        // propagates that error (Java's listOf accumulates the element failures).
        let ops = empty_ops();
        let codec = HolderSetCodec::create(&registry_key(), inline_holder_element_codec(), false);
        let input = json!([5, 6]);
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Registry does not exist"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn holder_set_codec_missing_registry_decode_rejects_references() {
        // A registry-less decode that yields a Reference holder errors Java's
        // `"Can't decode element <holder> without registry"`.
        let ops = empty_ops();
        let codec =
            HolderSetCodec::create(&registry_key(), reference_holder_element_codec(), false);
        let input = json!(["minecraft:a"]);
        let result = codec.decode(&ops, &input);
        assert!(result.is_error());
        let msg = result.error_ref().unwrap().message().to_string();
        assert!(
            msg.contains("Can't decode element") && msg.contains("without registry"),
            "unexpected message: {}",
            msg
        );
    }

    #[test]
    fn holder_set_codec_missing_registry_encode_uses_homogenous_list() {
        // `encodeWithoutRegistry` = `homogenousListCodec.encode(input.stream().
        // toList(), ...)` — a single Direct element compacts to the bare value.
        let ops = empty_ops();
        let codec = HolderSetCodec::create(&registry_key(), holder_element_codec(), false);
        let set = HolderSet::direct(vec![Holder::direct(TestElement(9))]);
        let encoded = codec
            .encode(&set, &ops, &ops.empty())
            .get_or_throw("encode")
            .clone();
        assert_eq!(encoded, ops.create_string("minecraft:e".to_string()));
    }
}
