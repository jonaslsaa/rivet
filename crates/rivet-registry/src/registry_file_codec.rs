//! `RegistryFileCodec` / `RegistryFixedCodec` / `HolderSetCodec` — the #126
//! holder codecs of `net.minecraft.resources` (MC 26.2).
//!
//! PROVENANCE: `RegistryFileCodec.java` (82 lines), `RegistryFixedCodec.java`
//! (73 lines), `HolderSetCodec.java` (108 lines), all leaves of the `mc.resources`
//! manifest unit.
//!
//! These codecs encode/decode `Holder<T>` / `HolderSet<T>` against a
//! `RegistryOps<T, D>` context (the lookup provider in the ops). Behavior
//! mirrors Java exactly:
//! - A `Reference` encodes as its identifier; a `Direct` falls through to the
//!   element codec (`RegistryFileCodec`) or errors (`RegistryFixedCodec`).
//!   `canSerializeIn` (the O(1) `RegistryId` owner check) gates both.
//! - Decode resolves the identifier through the ops' getter; an unknown element
//!   errors `"Failed to get element <key>"` / `"Failed to get element <id>"`.
//! - Without a `RegistryOps` context the behavior degrades to the element codec
//!   (`RegistryFileCodec`) or errors `"Can't access registry <key>"`
//!   (`RegistryFixedCodec`).
//!
//! Binding-model deviations (documented, PORTING.md drift checklist):
//! - Java's `ops instanceof RegistryOps<?>` runtime guard is a compile-time
//!   bound (`Ops: RegistryOpsLookup`, the `registry_ops` trait) — the Rust ops
//!   type pins the context, so a codec built for a `RegistryOps` is only ever
//!   used with one.
//! - The codecs are generic over the concrete ops type `Ops` (Java's
//!   `DynamicOps<T>` genericity maps to the Rust `Codec<E, Ops>` ops
//!   parameter), and the context trait is the crate-local
//!   `registry_ops::RegistryOpsLookup`.
//! - `HolderSetCodec`'s `homogenousListCodec` uses
//!   `ExtraCodecs.ensureHomogenous(Holder::kind)` and `compactListCodec`, which
//!   are not ported to `rivet-serialization` yet. STUB(mc.util): the registry
//!   path (`registryAwareCodec` — encode/unwrap and the tag-or-list decode) is
//!   fully ported; the non-registry fallback (`decodeWithoutRegistry`/
//!   `encodeWithoutRegistry`) is `todo!()` with a `blocked` note — it only runs
//!   when the ops are NOT a `RegistryOps`, which the compile-time bound already
//!   excludes.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::holder::Holder;
use crate::holder_lookup::{HolderGetter, HolderLookup, RegistryOwner};
use crate::holder_set::HolderSet;
use crate::registry::{Registry, RegistryKey};
use crate::registry_ops::RegistryOpsLookup;
use crate::{ResourceKey, TagKey};

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::encoder::Encoder;
use rivet_serialization::lifecycle::Lifecycle;

/// Erase the element type of a registry key.
fn erase_registry_key<E>(key: &ResourceKey<Registry<E>>) -> RegistryKey<()> {
    ResourceKey::create_registry_key(key.identifier().clone())
}

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
        // Java `ops instanceof RegistryOps<?>`: the ops carry a lookup provider.
        let owner = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .map(|info| RegistryOwner {
                registry_id: info.registry_id,
            });
        if let Some(owner) = owner {
            if !input.can_serialize_in(&owner) {
                return DataResult::error(format!(
                    "Element {} is not valid in current registry set",
                    input
                ));
            }
            // `input.unwrap().map(Identifier.CODEC.encode(id.identifier(), ...),
            // value -> elementCodec.encode(value, ...))`.
            return match input {
                Holder::Direct(value) => self.element_codec.encode(value, ops, prefix),
                Holder::Reference { .. } => {
                    // The reference's identifier is read through the ops' getter
                    // (back-reference rule). Build the getter, resolve the key.
                    let getter = ops
                        .lookup_provider()
                        .lookup_erased(&erase_registry_key(&self.registry_key))
                        .map(|info| {
                            crate::holder_lookup::RegistryGetter::new(
                                info.access.clone(),
                                self.registry_key.clone(),
                            )
                        });
                    let identifier = getter
                        .as_ref()
                        .and_then(|getter| getter.key_of(input))
                        .map(|key| key.identifier().clone());
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
            // STUB(mc.util): a Reference without a registry has no resolvable
            // value under the back-reference model (Java resolves it through the
            // holder's own registry). Not reachable under the compile-time bound.
            Holder::Reference { .. } => todo!(
                "RegistryFileCodec non-registry Reference encode needs a lookup \
                 (blocked: no registry context in the ops)"
            ),
        }
    }
}

impl<E, Ops> Decoder<Holder<E>, Ops> for RegistryFileCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(Holder<E>, Ops::Output)> {
        let lookup = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key));
        match lookup {
            Some(_info) => {
                // `Identifier.CODEC.decode(ops, input)`; if that fails, decode as
                // the element codec (if inline allowed) else error.
                let id_decoded = crate::identifier::identifier_codec::<Ops>().decode(ops, input);
                match id_decoded.result() {
                    Some((identifier, _rest)) => {
                        let element_key =
                            ResourceKey::create(&self.registry_key, identifier.clone());
                        let holder = ops
                            .lookup_provider()
                            .lookup_erased(&erase_registry_key(&self.registry_key))
                            .and_then(|info| {
                                crate::holder_lookup::RegistryGetter::new(
                                    info.access.clone(),
                                    self.registry_key.clone(),
                                )
                                .get(&element_key)
                            });
                        match holder {
                            Some(holder) => {
                                let pair = (holder, ops.empty());
                                DataResult::success_with_lifecycle(pair, Lifecycle::stable())
                            }
                            None => {
                                DataResult::error(format!("Failed to get element {}", element_key))
                            }
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
            None => DataResult::error(format!("Registry does not exist: {}", self.registry_key)),
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
        let owner = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .map(|info| RegistryOwner {
                registry_id: info.registry_id,
            });
        if let Some(owner) = owner {
            if !input.can_serialize_in(&owner) {
                return DataResult::error(format!(
                    "Element {} is not valid in current registry set",
                    input
                ));
            }
            // `input.unwrap().map(Identifier.CODEC.encode(...), value -> error)`.
            return match input {
                Holder::Reference { .. } => {
                    let getter = ops
                        .lookup_provider()
                        .lookup_erased(&erase_registry_key(&self.registry_key))
                        .map(|info| {
                            crate::holder_lookup::RegistryGetter::new(
                                info.access.clone(),
                                self.registry_key.clone(),
                            )
                        });
                    let identifier = getter
                        .as_ref()
                        .and_then(|getter| getter.key_of(input))
                        .map(|key| key.identifier().clone());
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
        if ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .is_some()
        {
            let id_decoded = crate::identifier::identifier_codec::<Ops>().decode(ops, input);
            let (identifier, rest) = match id_decoded.result() {
                Some(pair) => pair.clone(),
                None => {
                    return DataResult::error(format!(
                        "Can't access registry {}",
                        self.registry_key
                    ));
                }
            };
            let element_key = ResourceKey::create(&self.registry_key, identifier.clone());
            let holder = ops
                .lookup_provider()
                .lookup_erased(&erase_registry_key(&self.registry_key))
                .and_then(|info| {
                    crate::holder_lookup::RegistryGetter::new(
                        info.access.clone(),
                        self.registry_key.clone(),
                    )
                    .get(&element_key)
                });
            match holder {
                Some(holder) => {
                    let pair = (holder, rest);
                    DataResult::success_with_lifecycle(pair, Lifecycle::stable())
                }
                None => DataResult::error(format!("Failed to get element {}", identifier)),
            }
        } else {
            DataResult::error(format!("Can't access registry {}", self.registry_key))
        }
    }
}

impl<E, Ops> Codec<Holder<E>, Ops> for RegistryFixedCodec<E, Ops>
where
    E: Send + Sync + 'static + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
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
    /// `HolderSetCodec.elementCodec`.
    pub element_codec: Arc<dyn Codec<Holder<E>, Ops>>,
    /// `HolderSetCodec.alwaysUseList`.
    pub always_use_list: bool,
}

impl<E, Ops> HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    /// `HolderSetCodec.create(registryKey, elementCodec, alwaysUseList)`.
    pub fn create(
        registry_key: &ResourceKey<Registry<E>>,
        element_codec: Arc<dyn Codec<Holder<E>, Ops>>,
        always_use_list: bool,
    ) -> Self {
        HolderSetCodec {
            registry_key: registry_key.clone(),
            element_codec,
            always_use_list,
        }
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
        let owner = ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .map(|info| RegistryOwner {
                registry_id: info.registry_id,
            });
        if let Some(owner) = owner {
            if !input.can_serialize_in(&owner) {
                return DataResult::error(format!(
                    "HolderSet {} is not valid in current registry set",
                    input
                ));
            }
            // `registryAwareCodec.encode(input.unwrap().mapRight(List::copyOf))`.
            return match input.unwrap() {
                rivet_serialization::either::Either::Left(tag) => {
                    crate::tag_key::tag_key_hashed_codec::<E, Ops>(&self.registry_key)
                        .encode(&tag, ops, prefix)
                }
                rivet_serialization::either::Either::Right(holders) => {
                    let holder_list = codec::list::<Holder<E>, Ops>(self.element_codec.clone());
                    holder_list.encode(&holders.to_vec(), ops, prefix)
                }
            };
        }
        // STUB(mc.util): `encodeWithoutRegistry` uses `homogenousListCodec`
        // (`ExtraCodecs.ensureHomogenous`/`compactListCodec`, not ported). Not
        // reachable under the compile-time bound.
        todo!(
            "HolderSetCodec non-registry encode needs homogenousListCodec \
             (blocked: ExtraCodecs.ensureHomogenous/compactListCodec)"
        )
    }
}

impl<E, Ops> Decoder<HolderSet<E>, Ops> for HolderSetCodec<E, Ops>
where
    E: Send + Sync + 'static + Clone + std::fmt::Debug,
    Ops: DynamicOps + 'static + RegistryOpsLookup,
{
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(HolderSet<E>, Ops::Output)> {
        if ops
            .lookup_provider()
            .lookup_erased(&erase_registry_key(&self.registry_key))
            .is_some()
        {
            // `registryAwareCodec.decode` = `Codec.either(TagKey.hashedCodec,
            // homogenousListCodec)`.
            let tag_codec: Arc<dyn Codec<TagKey<E>, Ops>> =
                crate::tag_key::tag_key_hashed_codec::<E, Ops>(&self.registry_key);
            let list_codec: Arc<dyn Codec<Vec<Holder<E>>, Ops>> =
                codec::list(self.element_codec.clone());
            let either_codec =
                codec::either::<TagKey<E>, Vec<Holder<E>>, Ops>(tag_codec, list_codec);
            let decoded = either_codec.decode(ops, input);
            return match decoded.result() {
                Some((either, rest)) => {
                    let set = match either {
                        rivet_serialization::either::Either::Left(tag) => {
                            // `lookupTag(registry, tag)`.
                            let getter = ops
                                .lookup_provider()
                                .lookup_erased(&erase_registry_key(&self.registry_key))
                                .map(|info| {
                                    crate::holder_lookup::RegistryGetter::new(
                                        info.access.clone(),
                                        self.registry_key.clone(),
                                    )
                                });
                            match getter {
                                Some(getter) => getter.get_tag(tag).ok_or_else(|| {
                                    format!(
                                        "Missing tag: '{}' in '{}'",
                                        tag.location(),
                                        tag.registry().identifier()
                                    )
                                }),
                                None => Err(format!(
                                    "Missing tag: '{}' in '{}'",
                                    tag.location(),
                                    tag.registry().identifier()
                                )),
                            }
                        }
                        rivet_serialization::either::Either::Right(holders) => {
                            Ok(HolderSet::direct(holders.to_vec()))
                        }
                    };
                    match set {
                        Ok(set) => DataResult::success_with_lifecycle(
                            (set, rest.clone()),
                            Lifecycle::stable(),
                        ),
                        Err(msg) => DataResult::error(msg),
                    }
                }
                None => {
                    // STUB(mc.util): `decodeWithoutRegistry` (element-list fallback
                    // when the ops have no registry). Not reachable under the
                    // compile-time bound.
                    todo!(
                        "HolderSetCodec non-registry decode needs element-list \
                         fallback (blocked: no registry context in the ops)"
                    )
                }
            };
        }
        // STUB(mc.util): `decodeWithoutRegistry`. Not reachable under the
        // compile-time bound.
        todo!(
            "HolderSetCodec non-registry decode needs element-list fallback \
             (blocked: no registry context in the ops)"
        )
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

    fn element_codec() -> Arc<dyn Codec<TestElement, TestOps>> {
        // Encode a TestElement as its numeric value; decode a number back.
        codec::xmap(
            crate::identifier::identifier_codec::<TestOps>(),
            Arc::new(|_id: &Identifier| TestElement(0)),
            Arc::new(|_e: &TestElement| Identifier::with_default_namespace("e")),
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

    // -----------------------------------------------------------------------
    // HolderSetCodec
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
}
