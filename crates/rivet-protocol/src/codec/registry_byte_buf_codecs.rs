//! Port of the registry-aware `ByteBufCodecs` methods (MC 26.2, #126 phase G) —
//! the `StreamCodec<RegistryFriendlyByteBuf, ...>` methods that resolve a
//! registry through the buffer's `RegistryAccess` — plus the key `StreamCodec`s
//! they compose over (`Identifier`/`ResourceKey`/`TagKey`/`BlockPos`/`GlobalPos`).
//!
//! Java: `ByteBufCodecs.java` in `working/Paper` (vanilla 26.2), the `registry`/
//! `holderRegistry`/`holder`/`holderSet` methods (lines 672-770). The key
//! codecs mirror `Identifier.STREAM_CODEC` / `ResourceKey.streamCodec` /
//! `TagKey.streamCodec` / `BlockPos.STREAM_CODEC` / `GlobalPos.STREAM_CODEC`.
//!
//! In Java the key codecs are `StreamCodec<ByteBuf, ...>` (generic over the base
//! `ByteBuf`), so they compose over both `FriendlyByteBuf` and
//! `RegistryFriendlyByteBuf` via the `? super B` wildcards; Rust codecs are
//! monomorphic in the buffer type, so here they are concrete over
//! [`RegistryFriendlyByteBuf`] (the only buffer that carries a `RegistryAccess`),
//! with the string-shaped ones lifting the already-ported
//! [`byte_buf_codecs::string`] (Java `STRING_UTF8`) through the inner buffer
//! ([`lift`]).
//!
//! Wire formats (exact):
//! - `registry(reg)`: varint id; decode `byIdOrThrow(id)`, encode
//!   `getIdOrThrow(value)` (the registry's own `IdMap<T>` — on a defaulted
//!   registry an out-of-range decode id falls back to the default element).
//! - `holderRegistry(reg)`: varint id; decode `byIdOrThrow(id)`, encode
//!   `getIdOrThrow(holder)`, through the `Registry.asHolderIdMap()` view whose
//!   `byId` is `Registry.get(int)` — the **strict** bounds-checked range
//!   (`MappedRegistry.get(int)`), so an out-of-range id on even a defaulted
//!   registry is absent, never the default.
//! - `holder(reg, directCodec)`: `DIRECT_HOLDER_ID = 0`. Decode
//!   `id == 0 ? Holder.direct(directCodec.decode(input)) : byIdOrThrow(id - 1)`;
//!   encode `REFERENCE → getIdOrThrow(holder) + 1`, `DIRECT → 0` then
//!   `directCodec`.
//! - `holderSet(reg)`: `NAMED_SET = -1`. Decode `count = VarInt.read - 1`;
//!   `-1` → the bound named set for `TagKey.create(reg, Identifier.decode)`
//!   (`orElseThrow` → `"No value present"`), else `count` holders via
//!   `holderRegistry` → `HolderSet.direct`. Encode named → varint `0` then the
//!   tag location identifier; else varint `size() + 1` then the members.
//!
//! Error model: `byIdOrThrow`/`getIdOrThrow` panics (`"No value with id {id}"` /
//! `"Can't find id for value in map {registry}"`) mirror Java's
//! `IllegalArgumentException`; `Identifier::parse` on a malformed key panics
//! with Java's `IdentifierException` message (a Java `RuntimeException`, per the
//! crate's panic-model convention). Codec-level structural errors (`decode`
//! underflow, over-length strings) return `Err(CodecError)` via the underlying
//! `byte_buf_codecs` codecs.

use crate::codec::byte_buf_codecs::{self, MAX_INITIAL_COLLECTION_SIZE};
use crate::codec::stream_codec::{StreamCodec, composite_2, map, of};
use crate::codec::stream_decoder::StreamDecoder;
use crate::codec::stream_encoder::StreamEncoder;
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::core::{BlockPos, GlobalPos};
use rivet_registry::holder::Holder;
use rivet_registry::holder_lookup::HolderGetter;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::id_map::IdMap;
use rivet_registry::registry::{Registry, RegistryKey};
use rivet_registry::{Identifier, ResourceKey, TagKey};

/// Lift a `StreamCodec<FriendlyByteBuf, T>` to run over [`RegistryFriendlyByteBuf`]
/// by delegating to the inner buffer — the Rust stand-in for Java's
/// `StreamCodec<ByteBuf, T>` values composing over both buffers through the
/// `? super B` wildcards.
fn lift<T: 'static>(
    codec: StreamCodec<FriendlyByteBuf, T>,
) -> StreamCodec<RegistryFriendlyByteBuf, T> {
    let encoder = codec.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, value: &T| {
            encoder.encode(output.inner_mut(), value)
        },
        move |input: &mut RegistryFriendlyByteBuf| codec.decode(input.inner_mut()),
    )
}

/// `Identifier.STREAM_CODEC` — `STRING_UTF8.map(Identifier::parse, Identifier::toString)`.
///
/// A malformed key makes `Identifier::parse` panic with Java's
/// `IdentifierException` message (Java throws it from the `map` decoder as an
/// unchecked `RuntimeException`).
pub fn identifier_stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, Identifier> {
    map(
        lift(byte_buf_codecs::string()),
        |name: &String| Identifier::parse(name),
        |identifier: &Identifier| identifier.to_string(),
    )
}

/// `ResourceKey.streamCodec(registryName)` —
/// `Identifier.STREAM_CODEC.map(name -> create(registryName, name), ResourceKey::identifier)`.
pub fn resource_key_stream_codec<T: 'static>(
    registry_name: &RegistryKey<T>,
) -> StreamCodec<RegistryFriendlyByteBuf, ResourceKey<T>> {
    let registry_name = registry_name.clone();
    map(
        identifier_stream_codec(),
        move |name: &Identifier| ResourceKey::create(&registry_name, name.clone()),
        |key: &ResourceKey<T>| key.identifier().clone(),
    )
}

/// `TagKey.streamCodec(registryName)` —
/// `Identifier.STREAM_CODEC.map(location -> create(registryName, location), TagKey::location)`.
pub fn tag_key_stream_codec<T: 'static>(
    registry_name: &RegistryKey<T>,
) -> StreamCodec<RegistryFriendlyByteBuf, TagKey<T>> {
    let registry_name = registry_name.clone();
    map(
        identifier_stream_codec(),
        move |location: &Identifier| TagKey::create(&registry_name, location.clone()),
        |tag: &TagKey<T>| tag.location().clone(),
    )
}

/// `BlockPos.STREAM_CODEC` — the packed long (`BlockPos.asLong()`).
pub fn block_pos_stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, BlockPos> {
    of(
        |output: &mut RegistryFriendlyByteBuf, pos: &BlockPos| {
            output.write_long(pos.as_long());
            Ok(())
        },
        |input: &mut RegistryFriendlyByteBuf| Ok(BlockPos::of_long(input.read_long())),
    )
}

/// `GlobalPos.STREAM_CODEC` —
/// `composite(ResourceKey.streamCodec(Registries.DIMENSION), ::dimension,
/// BlockPos.STREAM_CODEC, ::pos, ::of)`.
pub fn global_pos_stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, GlobalPos> {
    composite_2(
        resource_key_stream_codec(&*rivet_registry::registries::DIMENSION),
        |pos: &GlobalPos| pos.dimension().clone(),
        block_pos_stream_codec(),
        |pos: &GlobalPos| pos.pos(),
        GlobalPos::of,
    )
}

/// `ByteBufCodecs.registry(registryKey)` — a varint element id mapped through
/// the registry's own `IdMap<T>`.
///
/// Decode returns a value-equal clone of the stored element (`Registry<T>`
/// stores `Arc<T>`; the codec boundary cannot hand out references). Encode uses
/// `getIdOrThrow`, which is identity-sensitive (`Registry::get_id` keys by
/// `Arc::as_ptr`), so like Java only elements the registry holds (or a
/// defaulted registry's default) encode.
pub fn registry<T>(registry_key: &RegistryKey<T>) -> StreamCodec<RegistryFriendlyByteBuf, T>
where
    T: Clone + Send + Sync + 'static,
{
    let encoder_key = registry_key.clone();
    let decoder_key = registry_key.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, value: &T| {
            let registry = output.registry_access().lookup_or_throw(&encoder_key);
            let id = IdMap::get_id_or_throw(registry, value);
            output.write_var_int(id);
            Ok(())
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            let id = input.read_var_int();
            let registry = input.registry_access().lookup_or_throw(&decoder_key);
            Ok(registry.by_id_or_throw(id).clone())
        },
    )
}

/// `ByteBufCodecs.holderRegistry(registryKey)` — a varint holder id through the
/// `Registry.asHolderIdMap()` view.
///
/// Decode uses the strict bounds check (`MappedRegistry.get(int)`), NOT the
/// `DefaultedRegistry` fallback: an out-of-range id on a defaulted registry is
/// absent (`"No value with id {id}"`), never the default element.
pub fn holder_registry<T>(
    registry_key: &RegistryKey<T>,
) -> StreamCodec<RegistryFriendlyByteBuf, Holder<T>>
where
    T: Send + Sync + 'static,
{
    let encoder_key = registry_key.clone();
    let decoder_key = registry_key.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, holder: &Holder<T>| {
            let registry = output.registry_access().lookup_or_throw(&encoder_key);
            let id = get_holder_id_or_throw(registry, holder);
            output.write_var_int(id);
            Ok(())
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            let id = input.read_var_int();
            let registry = input.registry_access().lookup_or_throw(&decoder_key);
            Ok(holder_by_id_or_throw(registry, id))
        },
    )
}

/// `ByteBufCodecs.holder(registryKey, directCodec)` — `DIRECT_HOLDER_ID = 0`,
/// references encoded as `id + 1` so id `0` is free for the direct form.
pub fn holder<T>(
    registry_key: &RegistryKey<T>,
    direct_codec: StreamCodec<RegistryFriendlyByteBuf, T>,
) -> StreamCodec<RegistryFriendlyByteBuf, Holder<T>>
where
    T: Send + Sync + 'static,
{
    let encoder_key = registry_key.clone();
    let decoder_key = registry_key.clone();
    let encoder_direct = direct_codec.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, holder: &Holder<T>| match holder {
            Holder::Direct(value) => {
                output.write_var_int(0);
                encoder_direct.encode(output, value)
            }
            Holder::Reference { .. } => {
                let registry = output.registry_access().lookup_or_throw(&encoder_key);
                let id = get_holder_id_or_throw(registry, holder);
                output.write_var_int(id.wrapping_add(1));
                Ok(())
            }
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            let id = input.read_var_int();
            if id == 0 {
                let value = direct_codec.decode(input)?;
                Ok(Holder::Direct(value))
            } else {
                let registry = input.registry_access().lookup_or_throw(&decoder_key);
                Ok(holder_by_id_or_throw(registry, id.wrapping_sub(1)))
            }
        },
    )
}

/// `ByteBufCodecs.holderSet(registryKey)` — `NAMED_SET = -1`: a varint `count`
/// where `count - 1 == -1` (varint `0`) selects a bound named tag set and
/// otherwise prefixes `count` `holderRegistry` members.
pub fn holder_set<T>(
    registry_key: &RegistryKey<T>,
) -> StreamCodec<RegistryFriendlyByteBuf, HolderSet<T>>
where
    T: Send + Sync + 'static,
{
    let encoder_key = registry_key.clone();
    let decoder_key = registry_key.clone();
    let holder_encoder = holder_registry(&encoder_key);
    let holder_decoder = holder_registry(&decoder_key);
    of(
        move |output: &mut RegistryFriendlyByteBuf, value: &HolderSet<T>| match value.unwrap_key() {
            Some(key) => {
                output.write_var_int(0);
                identifier_stream_codec().encode(output, key.location())?;
                Ok(())
            }
            None => {
                output.write_var_int((value.size() as i32).wrapping_add(1));
                for holder in value.iter() {
                    holder_encoder.encode(output, holder)?;
                }
                Ok(())
            }
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            let count = input.read_var_int().wrapping_sub(1);
            if count == -1 {
                // Java evaluates `Identifier.STREAM_CODEC.decode(input)` before
                // `registry.get(...)`, so decode the location (which needs `&mut
                // input`) before borrowing the registry through the access.
                let location = identifier_stream_codec().decode(input)?;
                let registry = input.registry_access().lookup_or_throw(&decoder_key);
                let tag = TagKey::create(&decoder_key, location);
                let set = <Registry<T> as HolderGetter<T>>::get_tag(registry, &tag)
                    .unwrap_or_else(|| panic!("No value present"));
                Ok(set)
            } else {
                // Java `new ArrayList<>(Math.min(count, 65536))` throws
                // `IllegalArgumentException("Illegal Capacity: -n")` on a
                // negative count; the loop then decodes `count` members.
                let capacity = count.min(MAX_INITIAL_COLLECTION_SIZE);
                if capacity < 0 {
                    panic!("Illegal Capacity: {capacity}");
                }
                let mut holders = Vec::with_capacity(capacity as usize);
                for _ in 0..count {
                    holders.push(holder_decoder.decode(input)?);
                }
                Ok(HolderSet::direct(holders))
            }
        },
    )
}

/// `IdMap<Holder<T>>.byIdOrThrow` (the `Registry.asHolderIdMap()` view) in the
/// pure-ID holder model. Java's `byId` is `Registry.get(int)`, the strict
/// bounds-checked range (`MappedRegistry.get(int)`); an out-of-range id is
/// absent — even on a defaulted registry, never the default.
fn holder_by_id_or_throw<T>(registry: &Registry<T>, id: i32) -> Holder<T> {
    if id >= 0 && (id as usize) < registry.size() as usize {
        Holder::Reference {
            registry: registry.registry_id(),
            id: id as u32,
        }
    } else {
        panic!("No value with id {id}");
    }
}

/// `IdMap<Holder<T>>.getIdOrThrow` (the `asHolderIdMap` view). Java resolves
/// `holder.value()` then `Registry.getId`; a `Holder::Reference` resolves to its
/// own id (element id == holder id == insertion index) once the owner and range
/// check pass. Panic messages follow Java's two failure modes:
/// - a reference into another registry (or a direct value): the registry's
///   identity map misses it — `"Can't find id for value in map {registry}"`;
/// - an out-of-range reference in its own registry: unbound — Java's
///   `Reference.value()` throws `"Trying to access unbound value '<key>' from
///   registry <id>"` with the unresolvable key rendered `"null"`.
fn get_holder_id_or_throw<T>(registry: &Registry<T>, holder: &Holder<T>) -> i32 {
    match holder {
        Holder::Reference {
            registry: owner,
            id,
        } => {
            if *owner == registry.registry_id() {
                if (*id as usize) < registry.size() as usize {
                    *id as i32
                } else {
                    panic!(
                        "Trying to access unbound value 'null' from registry {}",
                        owner.0
                    )
                }
            } else {
                panic!("Can't find id for value in map {}", registry);
            }
        }
        Holder::Direct(_) => panic!("Can't find id for value in map {}", registry),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
    use bytes::BytesMut;
    use rivet_registry::holder::RegistryId;
    use rivet_registry::registry::RegistryKey;
    use rivet_registry::{
        Identifier, RegistrationInfo, RegistryAccess, RegistryBuilder, ResourceKey, TagKey,
    };
    use std::panic::catch_unwind;
    use std::sync::Arc;

    #[derive(Debug, Clone, PartialEq, Eq)]
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

    /// `{air: 0, stone: 1, dirt: 2}` plus the tag `group = [air, dirt]`.
    fn tagged_registry() -> Registry<TestElement> {
        let mut builder = RegistryBuilder::new(&registry_key());
        let air = builder.register(
            &element_key("air"),
            Arc::new(TestElement(0)),
            RegistrationInfo::BUILT_IN,
        );
        let _stone = builder.register(
            &element_key("stone"),
            Arc::new(TestElement(1)),
            RegistrationInfo::BUILT_IN,
        );
        let dirt = builder.register(
            &element_key("dirt"),
            Arc::new(TestElement(2)),
            RegistrationInfo::BUILT_IN,
        );
        builder.bind_tags(vec![(tag_key("group"), vec![air, dirt])]);
        builder.freeze()
    }

    /// `{air: 0, stone: 1}` with default key `air` (the asymmetric
    /// `DefaultedRegistry` fallback is live on the raw id surface).
    fn defaulted_registry() -> Registry<TestElement> {
        let mut builder = RegistryBuilder::new_defaulted(
            &Identifier::with_default_namespace("air"),
            &registry_key(),
        );
        builder.register(
            &element_key("air"),
            Arc::new(TestElement(0)),
            RegistrationInfo::BUILT_IN,
        );
        builder.register(
            &element_key("stone"),
            Arc::new(TestElement(1)),
            RegistrationInfo::BUILT_IN,
        );
        builder.freeze()
    }

    fn access(registry: Registry<TestElement>) -> RegistryAccess {
        RegistryAccess::from_single_registry(registry_key(), registry)
    }

    fn buffer(access: &RegistryAccess) -> RegistryFriendlyByteBuf {
        RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone())
    }

    /// Decode `bytes` through `codec` against `access`; asserts the round trip
    /// value equals `value`.
    fn round_trip<T: PartialEq + std::fmt::Debug + 'static>(
        access: &RegistryAccess,
        codec: &StreamCodec<RegistryFriendlyByteBuf, T>,
        value: &T,
    ) {
        let mut out = buffer(access);
        codec.encode(&mut out, value).unwrap();
        let mut input = RegistryFriendlyByteBuf::new(
            BytesMut::from(out.into_inner().to_vec().as_slice()),
            access.clone(),
        );
        assert_eq!(&codec.decode(&mut input).unwrap(), value);
    }

    /// Encode `value`, return the wire bytes.
    fn written<T: 'static>(
        access: &RegistryAccess,
        codec: &StreamCodec<RegistryFriendlyByteBuf, T>,
        value: &T,
    ) -> Vec<u8> {
        let mut out = buffer(access);
        codec.encode(&mut out, value).unwrap();
        out.into_inner().to_vec()
    }

    fn panic_message<F: FnOnce() -> R, R>(f: F) -> String {
        let err = match catch_unwind(std::panic::AssertUnwindSafe(f)) {
            Ok(_) => panic!("expected the closure to panic"),
            Err(err) => err,
        };
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    /// Alias of `super::registry` so tests can name it while a local variable
    /// shadows the imported function name.
    fn registry_codec<T: Clone + Send + Sync + 'static>(
        registry_key: &RegistryKey<T>,
    ) -> StreamCodec<RegistryFriendlyByteBuf, T> {
        super::registry(registry_key)
    }

    fn test_element_codec() -> StreamCodec<RegistryFriendlyByteBuf, TestElement> {
        of(
            |output: &mut RegistryFriendlyByteBuf, value: &TestElement| {
                output.write_var_int(value.0 as i32);
                Ok(())
            },
            |input: &mut RegistryFriendlyByteBuf| Ok(TestElement(input.read_var_int() as u8)),
        )
    }

    // -----------------------------------------------------------------------
    // Identifier / ResourceKey / TagKey (String wire form)
    // -----------------------------------------------------------------------

    #[test]
    fn identifier_stream_codec_round_trips_and_wire_form() {
        let access = access(tagged_registry());
        let codec = identifier_stream_codec();
        let id = Identifier::parse("minecraft:stone");
        round_trip(&access, &codec, &id);
        // Wire form: `STRING_UTF8` — varint byte-length then the UTF-8 bytes.
        // "minecraft:stone" is 15 chars.
        assert_eq!(
            written(&access, &codec, &id),
            vec![
                15, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b's', b't', b'o',
                b'n', b'e'
            ]
        );
    }

    #[test]
    fn identifier_stream_codec_normalizes_default_namespace() {
        // `Identifier.parse("stone")` -> `minecraft:stone` (Java default ns).
        let access = access(tagged_registry());
        let mut out = buffer(&access);
        identifier_stream_codec()
            .encode(&mut out, &Identifier::parse("minecraft:stone"))
            .unwrap();
        let mut input = RegistryFriendlyByteBuf::new(
            BytesMut::from(out.into_inner().to_vec().as_slice()),
            access.clone(),
        );
        assert_eq!(
            identifier_stream_codec().decode(&mut input).unwrap(),
            Identifier::parse("minecraft:stone")
        );
    }

    #[test]
    fn identifier_stream_codec_malformed_key_panics_like_java() {
        // `Identifier.parse` throws `IdentifierException` (unchecked) with the
        // `assertValidPath` message — a space is not in `[a-z0-9/._-]`.
        let access = access(tagged_registry());
        let mut input = buffer(&access);
        input.write_utf("a b");
        let msg = panic_message(|| {
            let _ = identifier_stream_codec().decode(&mut input);
        });
        assert_eq!(
            msg,
            "Non [a-z0-9/._-] character in path of location: minecraft:a b"
        );
    }

    #[test]
    fn resource_key_stream_codec_round_trips() {
        let access = access(tagged_registry());
        let codec = resource_key_stream_codec::<TestElement>(&registry_key());
        let key = element_key("dirt");
        round_trip(&access, &codec, &key);
        // Wire form is just the identifier string. "minecraft:dirt" is 14 chars.
        assert_eq!(
            written(&access, &codec, &key),
            vec![
                14, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'd', b'i', b'r',
                b't'
            ]
        );
    }

    #[test]
    fn tag_key_stream_codec_round_trips() {
        let access = access(tagged_registry());
        let codec = tag_key_stream_codec::<TestElement>(&registry_key());
        let tag = tag_key("group");
        round_trip(&access, &codec, &tag);
        // "minecraft:group" is 15 chars.
        assert_eq!(
            written(&access, &codec, &tag),
            vec![
                15, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'g', b'r', b'o',
                b'u', b'p'
            ]
        );
    }

    // -----------------------------------------------------------------------
    // registry() — the element id codec
    // -----------------------------------------------------------------------

    #[test]
    fn registry_codec_round_trips_and_wire_form() {
        let registry = tagged_registry();
        let access = access(registry);
        let codec = registry_codec::<TestElement>(&registry_key());
        // Encode needs the value reference the registry holds (identity map).
        let value = access
            .lookup(&registry_key())
            .unwrap()
            .get_value(&element_key("stone"))
            .unwrap();
        assert_eq!(written(&access, &codec, value), vec![1]); // element id 1
        // Decode the wire id back to the value.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(vec![1].as_slice()), access.clone());
        assert_eq!(codec.decode(&mut input).unwrap(), TestElement(1));
    }

    #[test]
    fn registry_codec_unknown_id_panics_with_java_message() {
        let access = access(tagged_registry());
        let codec = registry_codec::<TestElement>(&registry_key());
        let mut input = buffer(&access);
        input.write_var_int(99);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "No value with id 99");
    }

    #[test]
    fn registry_codec_unregistered_value_panics_get_id_or_throw() {
        // `Registry.getId` is identity-sensitive: a fresh value was never
        // registered, so `getIdOrThrow` panics with the registry in the message.
        let access = access(tagged_registry());
        let codec = registry_codec::<TestElement>(&registry_key());
        let mut out = buffer(&access);
        let msg = panic_message(|| {
            let _ = codec.encode(&mut out, &TestElement(9));
        });
        assert_eq!(
            msg,
            format!(
                "Can't find id for value in map {}",
                access.lookup(&registry_key()).unwrap()
            )
        );
    }

    #[test]
    fn registry_codec_defaulted_registry_falls_back_to_default_element() {
        // `registry()` resolves through `Registry.byId` which inherits the
        // `DefaultedRegistry` fallback: an out-of-range id decodes to the default.
        let access = access(defaulted_registry());
        let codec = registry_codec::<TestElement>(&registry_key());
        let mut input = buffer(&access);
        input.write_var_int(99);
        assert_eq!(codec.decode(&mut input).unwrap(), TestElement(0)); // the default "air"
    }

    #[test]
    fn registry_codec_missing_registry_context_panics() {
        // Context mismatch: the buffer's access does not carry the registry key.
        let empty = RegistryAccess::empty();
        let mut input = RegistryFriendlyByteBuf::new(BytesMut::new(), empty);
        input.write_var_int(0);
        let msg = panic_message(|| {
            let _ = registry_codec::<TestElement>(&registry_key()).decode(&mut input);
        });
        assert_eq!(msg, format!("Missing registry: {}", registry_key()));
    }

    // -----------------------------------------------------------------------
    // holderRegistry() — the holder id codec (strict bounds, no default fallback)
    // -----------------------------------------------------------------------

    #[test]
    fn holder_registry_round_trips() {
        let access = access(tagged_registry());
        let codec = holder_registry::<TestElement>(&registry_key());
        let registry = access.lookup(&registry_key()).unwrap();
        let holder = registry.get(&element_key("dirt")).unwrap();
        round_trip(&access, &codec, &holder);
        assert_eq!(written(&access, &codec, &holder), vec![2]); // element id 2
        // Decode id 0 -> the air reference.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(vec![0].as_slice()), access.clone());
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(
            decoded,
            Holder::Reference {
                registry: registry.registry_id(),
                id: 0
            }
        );
    }

    #[test]
    fn holder_registry_unknown_id_panics_strictly_even_on_defaulted_registry() {
        // `holderRegistry` uses `Registry.asHolderIdMap()` whose `byId` is
        // `MappedRegistry.get(int)` — the bounds-checked range. On a defaulted
        // registry an out-of-range id is absent (Java `Optional.empty`), NEVER
        // the default element.
        let access = access(defaulted_registry());
        let codec = holder_registry::<TestElement>(&registry_key());
        let mut input = buffer(&access);
        input.write_var_int(99);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "No value with id 99");
    }

    #[test]
    fn holder_registry_direct_holder_encode_panics() {
        // `asHolderIdMap().getIdOrThrow(direct)` — Java `getId(holder.value())`
        // misses, then `getIdOrThrow` throws. The value part is unreproducible
        // (`T` unbounded), so the registry-only message matches `Registry<T>`'s
        // override.
        let access = access(tagged_registry());
        let codec = holder_registry::<TestElement>(&registry_key());
        let mut out = buffer(&access);
        let msg = panic_message(|| {
            let _ = codec.encode(&mut out, &Holder::Direct(TestElement(1)));
        });
        assert_eq!(
            msg,
            format!(
                "Can't find id for value in map {}",
                access.lookup(&registry_key()).unwrap()
            )
        );
    }

    #[test]
    fn holder_registry_out_of_range_reference_encode_panics_unbound() {
        // Java `getId(holder.value())` throws on the unbound reference before
        // the identity lookup; the unresolvable key renders as "null".
        let access = access(tagged_registry());
        let codec = holder_registry::<TestElement>(&registry_key());
        let registry = access.lookup(&registry_key()).unwrap();
        let mut out = buffer(&access);
        let msg = panic_message(|| {
            let _ = codec.encode(
                &mut out,
                &Holder::Reference {
                    registry: registry.registry_id(),
                    id: 42,
                },
            );
        });
        assert_eq!(
            msg,
            format!(
                "Trying to access unbound value 'null' from registry {}",
                registry.registry_id().0
            )
        );
    }

    #[test]
    fn holder_registry_wrong_owner_encode_panics() {
        // A reference into another registry (a different RegistryId) is not
        // found by this registry's identity map.
        let access = access(tagged_registry());
        let codec = holder_registry::<TestElement>(&registry_key());
        let registry = access.lookup(&registry_key()).unwrap();
        let mut out = buffer(&access);
        let msg = panic_message(|| {
            let _ = codec.encode(
                &mut out,
                &Holder::<TestElement>::Reference {
                    registry: RegistryId(u32::MAX),
                    id: 0,
                },
            );
        });
        assert_eq!(msg, format!("Can't find id for value in map {}", registry));
    }

    // -----------------------------------------------------------------------
    // holder() — direct id 0, references encoded id + 1
    // -----------------------------------------------------------------------

    #[test]
    fn holder_reference_round_trips_with_id_plus_one_wire_form() {
        let access = access(tagged_registry());
        let codec = holder::<TestElement>(&registry_key(), test_element_codec());
        let registry = access.lookup(&registry_key()).unwrap();
        let holder = registry.get(&element_key("stone")).unwrap();
        round_trip(&access, &codec, &holder);
        // References encode as `getIdOrThrow(holder) + 1` (DIRECT_HOLDER_ID = 0
        // must be free); element id 1 -> varint 2.
        assert_eq!(written(&access, &codec, &holder), vec![2]);
        // Decode varint 2 -> `byIdOrThrow(2 - 1)` = the id-1 reference.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(vec![2].as_slice()), access.clone());
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            Holder::Reference {
                registry: registry.registry_id(),
                id: 1
            }
        );
    }

    #[test]
    fn holder_direct_round_trips_with_zero_then_direct_codec() {
        let access = access(tagged_registry());
        let codec = holder::<TestElement>(&registry_key(), test_element_codec());
        let direct = Holder::Direct(TestElement(7));
        round_trip(&access, &codec, &direct);
        // Direct encodes varint 0, then the direct codec payload.
        assert_eq!(written(&access, &codec, &direct), vec![0, 7]);
        // Decode varint 0 -> decode the direct codec.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(vec![0, 7].as_slice()), access.clone());
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            Holder::Direct(TestElement(7))
        );
    }

    #[test]
    fn holder_decode_id_zero_takes_direct_even_with_registry_ids() {
        // The `id == 0` check happens BEFORE the registry lookup, so a direct
        // holder decodes even when the registry has an element at id 0.
        let access = access(tagged_registry());
        let codec = holder::<TestElement>(&registry_key(), test_element_codec());
        let mut input = buffer(&access);
        input.write_var_int(0);
        input.write_var_int(3);
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            Holder::Direct(TestElement(3))
        );
    }

    #[test]
    fn holder_decode_reference_id_zero_is_never_confused() {
        // A reference to element id 0 is wire varint 1 (0 + 1), which decodes
        // back to the id-0 reference, not a direct.
        let access = access(tagged_registry());
        let codec = holder::<TestElement>(&registry_key(), test_element_codec());
        let registry = access.lookup(&registry_key()).unwrap();
        let mut input = buffer(&access);
        input.write_var_int(1);
        assert_eq!(
            codec.decode(&mut input).unwrap(),
            Holder::Reference {
                registry: registry.registry_id(),
                id: 0
            }
        );
    }

    #[test]
    fn holder_unknown_reference_id_panics() {
        // varint 100 -> `byIdOrThrow(99)` -> strict bounds, panics.
        let access = access(tagged_registry());
        let codec = holder::<TestElement>(&registry_key(), test_element_codec());
        let mut input = buffer(&access);
        input.write_var_int(100);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "No value with id 99");
    }

    #[test]
    fn holder_direct_encode_on_registry_without_direct_support_still_works() {
        // Direct is decode-only, but the encode path writes varint 0 + the
        // direct codec regardless of the registry contents.
        let empty = RegistryAccess::empty();
        let codec = holder::<TestElement>(&registry_key(), test_element_codec());
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), empty);
        codec
            .encode(&mut out, &Holder::Direct(TestElement(5)))
            .unwrap();
        assert_eq!(out.into_inner().to_vec(), vec![0, 5]);
    }

    // -----------------------------------------------------------------------
    // holderSet() — NAMED_SET = -1 sentinel
    // -----------------------------------------------------------------------

    #[test]
    fn holder_set_direct_round_trips_with_count_plus_one() {
        let access = access(tagged_registry());
        let codec = holder_set::<TestElement>(&registry_key());
        let registry = access.lookup(&registry_key()).unwrap();
        let members = vec![
            registry.get(&element_key("air")).unwrap(),
            registry.get(&element_key("dirt")).unwrap(),
        ];
        let set = HolderSet::direct(members.clone());
        round_trip(&access, &codec, &set);
        // Direct: varint `size() + 1` then the members via holderRegistry.
        assert_eq!(written(&access, &codec, &set), vec![3, 0, 2]);
        // Decode varint 3 -> count 2 -> two holder references.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(vec![3, 0, 2].as_slice()), access.clone());
        let decoded = codec.decode(&mut input).unwrap();
        assert_eq!(decoded, set);
    }

    #[test]
    fn holder_set_empty_direct_round_trips() {
        let access = access(tagged_registry());
        let codec = holder_set::<TestElement>(&registry_key());
        let empty = HolderSet::<TestElement>::empty();
        round_trip(&access, &codec, &empty);
        assert_eq!(written(&access, &codec, &empty), vec![1]); // 0 + 1
    }

    #[test]
    fn holder_set_named_round_trips_with_varint_zero_and_tag_location() {
        let access = access(tagged_registry());
        let codec = holder_set::<TestElement>(&registry_key());
        let registry = access.lookup(&registry_key()).unwrap();
        // A bound Named set from the registry's tag binding.
        let named = <Registry<TestElement> as HolderGetter<TestElement>>::get_tag(
            registry,
            &tag_key("group"),
        )
        .unwrap();
        round_trip(&access, &codec, &named);
        // Named: varint 0, then the tag location identifier string.
        // "minecraft:group" is 15 chars.
        let mut expected = vec![0];
        expected.extend_from_slice(&[
            15, b'm', b'i', b'n', b'e', b'c', b'r', b'a', b'f', b't', b':', b'g', b'r', b'o', b'u',
            b'p',
        ]);
        assert_eq!(written(&access, &codec, &named), expected);
        // Decode varint 0 -> the bound tag set.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(expected.as_slice()), access.clone());
        let decoded = codec.decode(&mut input).unwrap();
        assert!(decoded.is_bound());
        assert_eq!(decoded.unwrap_key(), Some(tag_key("group")));
        assert_eq!(decoded.size(), 2);
    }

    #[test]
    fn holder_set_named_unknown_tag_panics() {
        // Java `Optional.orElseThrow()` -> `NoSuchElementException("No value present")`.
        let access = access(tagged_registry());
        let codec = holder_set::<TestElement>(&registry_key());
        let mut input = buffer(&access);
        input.write_var_int(0);
        input.write_utf("minecraft:nope");
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "No value present");
    }

    #[test]
    fn holder_set_negative_count_panics_like_arraylist() {
        // varint -1 -> count = -2 -> `new ArrayList<>(-2)` throws
        // `IllegalArgumentException("Illegal Capacity: -2")`.
        let access = access(tagged_registry());
        let codec = holder_set::<TestElement>(&registry_key());
        let mut input = buffer(&access);
        input.write_var_int(-1);
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert_eq!(msg, "Illegal Capacity: -2");
    }

    #[test]
    fn holder_set_hostile_member_count_still_bounds_initial_capacity() {
        // A hostile count (`2^31 - 2`, a 5-byte VarInt) passes the
        // ArrayList-capacity check (`Math.min(count, 65536)` = 65536, bounded —
        // the OOM the raw Java `new ArrayList<>(count)` would pre-size is
        // prevented) and then the member decode runs out of buffer; Java throws
        // `ArrayIndexOutOfBounds` from the truncated VarInt, surfaced here as a
        // panic (the raw-buffer VarInt reader on the empty tail).
        let access = access(tagged_registry());
        let codec = holder_set::<TestElement>(&registry_key());
        let mut input = buffer(&access);
        input.write_var_int(i32::MAX); // count = i32::MAX - 1 -> capacity 65536
        let msg = panic_message(|| {
            let _ = codec.decode(&mut input);
        });
        assert!(
            msg.contains("advance out of bounds"),
            "unexpected panic: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // BlockPos / GlobalPos
    // -----------------------------------------------------------------------

    #[test]
    fn block_pos_stream_codec_round_trips_as_long() {
        let access = access(tagged_registry());
        let codec = block_pos_stream_codec();
        let pos = BlockPos::new(1, -2, 3);
        round_trip(&access, &codec, &pos);
        assert_eq!(
            written(&access, &codec, &pos),
            pos.as_long().to_be_bytes().to_vec()
        );
    }

    #[test]
    fn global_pos_stream_codec_round_trips() {
        let access = access(tagged_registry());
        let codec = global_pos_stream_codec();
        let pos = GlobalPos::of(
            ResourceKey::create(
                &*rivet_registry::registries::DIMENSION,
                Identifier::with_default_namespace("overworld"),
            ),
            BlockPos::new(10, 64, -20),
        );
        round_trip(&access, &codec, &pos);
        // Wire: identifier string then the packed long. "minecraft:overworld"
        // is 19 chars.
        let bytes = written(&access, &codec, &pos);
        let mut expected = vec![19];
        expected.extend_from_slice(b"minecraft:overworld");
        expected.extend_from_slice(&BlockPos::new(10, 64, -20).as_long().to_be_bytes());
        assert_eq!(bytes, expected);
    }

    #[test]
    fn codecs_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<StreamCodec<RegistryFriendlyByteBuf, Identifier>>();
        assert_send_sync::<StreamCodec<RegistryFriendlyByteBuf, ResourceKey<TestElement>>>();
        assert_send_sync::<StreamCodec<RegistryFriendlyByteBuf, Holder<TestElement>>>();
        assert_send_sync::<StreamCodec<RegistryFriendlyByteBuf, HolderSet<TestElement>>>();
        assert_send_sync::<StreamCodec<RegistryFriendlyByteBuf, GlobalPos>>();
    }
}
