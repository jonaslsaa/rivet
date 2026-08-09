//! Port of the `ComponentSerialization` wire-codec family (`net.minecraft.network.chat`).
//!
//! Java: `ComponentSerialization.java` in `working/Paper` (vanilla 26.2). The
//! class holds the recursive `CODEC` (ported in `rivet-text` as
//! [`rivet_text::component_serialization`]) and the `STREAM_CODEC` family that
//! serialize a `Component` to NBT on the wire. The `CODEC` lives in
//! `rivet-text` (it is the value-model serializer); this module is the wire
//! half — the stream codec family over the NBT `Tag` via
//! [`byte_buf_codecs::from_codec`].
//!
//! The five Java constants, 1:1 — plus [`trusted_optional_component`], the
//! context-free trusted optional that mirrors
//! `TRUSTED_CONTEXT_FREE_STREAM_CODEC.apply(ByteBufCodecs::optional)` (used by
//! `ClientboundResourcePackPushPacket`; no standalone constant in
//! `ComponentSerialization`):
//! - [`component_with_registries`] — `STREAM_CODEC`
//!   (`tagCodec(defaultQuota)` over `RegistryFriendlyByteBuf`, with the codec
//!   run over `RegistryOps` from the buffer's `RegistryAccess`).
//! - [`trusted_component_with_registries`] — `TRUSTED_STREAM_CODEC`
//!   (`tagCodec(unlimitedHeap)`).
//! - [`trusted_context_free_component`] — `TRUSTED_CONTEXT_FREE_STREAM_CODEC`
//!   (`tagCodec(unlimitedHeap)` over plain `ByteBuf`, no registry context).
//!   This is the port of the former `byte_buf_codecs::trusted_component`
//!   (issue #207); it serves `ServerLinks` custom display names.
//! - [`optional_component_with_registries`] — `OPTIONAL_STREAM_CODEC`.
//! - [`trusted_optional_component_with_registries`] — `TRUSTED_OPTIONAL_STREAM_CODEC`.
//! - [`trusted_optional_component`] — `ByteBufCodecs.optional(
//!   trustedContextFreeComponent())`, the context-free trusted optional:
//!   `TRUSTED_CONTEXT_FREE_STREAM_CODEC.apply(ByteBufCodecs::optional)`, used by
//!   `ClientboundResourcePackPushPacket`'s prompt field.
//!
//! Cycle cost and caching: the `Component` codec graph is built with
//! [`rivet_serialization::codec::recursive`], whose lazily-initialized cell
//! embeds a strong `Arc` back to the parent once the first encode/decode runs —
//! a permanent strong cycle. The graph must be built once per process, so it is
//! cached behind a `static OnceLock` (Java's `static final`), and the stream
//! codecs that embed it are cached the same way: repeated calls return clones
//! of the single registration-time graph and cannot accumulate leaked graphs
//! per connection. Rust codecs are monomorphic in the ops type, so there are
//! exactly two graphs, one per ops the wire runs: `NbtOps` (the context-free
//! codecs) and `RegistryOps<Tag, NbtOps>` (the registry-aware codecs) — Java's
//! single `CODEC` is ops-polymorphic, so this is the minimal Rust equivalent.
//!
//! Error model follows [`byte_buf_codecs::from_codec`]: netty
//! `DecoderException`/`EncoderException` map to `Err(CodecError)` with the
//! Java `"Failed to decode/encode: ..."` messages. The encode half renders the
//! value with Rust `Debug`, not Java's `MutableComponent.toString()` — the
//! documented deviation already carried by `from_codec`. Raw NBT I/O failures
//! (a truncated payload) still panic via `read_nbt_with_accounter`, mirroring
//! Java's unchecked rethrow, and the untrusted codecs enforce the default 2MB
//! `NbtAccounter` quota on decode (`Tried to read NBT tag that was too big...`).
//!
//! Scope: `nbt`/`object` component contents are still unregistered in
//! `rivet-text` (RivetTodo(#89) there), so this wire family covers the five
//! ported contents exactly as Java would encode them; the Paper locale-aware
//! translation path (`adventure$locale`) is a server-side concern and is not
//! ported.

use crate::codec::byte_buf_codecs::{self, from_codec};
use crate::codec::registry_byte_buf_codecs::lift;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, apply, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_registry::registry_ops::RegistryOps;
use rivet_serialization::codec::Codec;
use rivet_text::Component;
use std::sync::{Arc, OnceLock};

/// The shared `ComponentSerialization.CODEC` graph over `NbtOps` — one
/// process-wide instance shared by the context-free stream codecs (Java's
/// single `CODEC` used with `NbtOps.INSTANCE`). The permanent strong `Arc`
/// cycle is built exactly once here (see module docs).
fn component_codec() -> Arc<dyn Codec<Component, NbtOps>> {
    static CODEC: OnceLock<Arc<dyn Codec<Component, NbtOps>>> = OnceLock::new();
    CODEC
        .get_or_init(|| rivet_text::component_serialization::codec::<NbtOps>())
        .clone()
}

/// The shared `ComponentSerialization.CODEC` graph over
/// `RegistryOps<Tag, NbtOps>` — the ops the registry-aware stream codecs build
/// per call from the buffer's `RegistryAccess`. Distinct from the `NbtOps`
/// graph (Rust codecs are monomorphic in the ops), built once like it.
fn component_codec_with_registries() -> Arc<dyn Codec<Component, RegistryOps<Tag, NbtOps>>> {
    static CODEC: OnceLock<Arc<dyn Codec<Component, RegistryOps<Tag, NbtOps>>>> = OnceLock::new();
    CODEC
        .get_or_init(|| rivet_text::component_serialization::codec::<RegistryOps<Tag, NbtOps>>())
        .clone()
}

/// `ComponentSerialization.STREAM_CODEC` — `Component` over
/// `RegistryFriendlyByteBuf`, untrusted (`tagCodec(NbtAccounter::defaultQuota)`),
/// run over the buffer's `RegistryOps` serialization context.
pub fn component_with_registries() -> StreamCodec<RegistryFriendlyByteBuf, Component> {
    static CODEC: OnceLock<StreamCodec<RegistryFriendlyByteBuf, Component>> = OnceLock::new();
    CODEC
        .get_or_init(|| from_codec_with_registries(NbtAccounter::default_quota))
        .clone()
}

/// `ComponentSerialization.TRUSTED_STREAM_CODEC` — the registry-aware codec
/// with `tagCodec(NbtAccounter::unlimitedHeap)`.
pub fn trusted_component_with_registries() -> StreamCodec<RegistryFriendlyByteBuf, Component> {
    static CODEC: OnceLock<StreamCodec<RegistryFriendlyByteBuf, Component>> = OnceLock::new();
    CODEC
        .get_or_init(|| from_codec_with_registries(NbtAccounter::unlimited_heap))
        .clone()
}

/// `ByteBufCodecs.fromCodecWithRegistries(codec, accounter)` inlined for
/// `Component` (Java's `ComponentSerialization.createTranslationAware`): decode
/// reads the tag with `tagCodec(accounter)`, then parses it over
/// `RegistryOps`; encode builds the tag over `RegistryOps`, then writes it with
/// `tagCodec(accounter)`.
fn from_codec_with_registries(
    accounter: impl Fn() -> NbtAccounter + Send + Sync + 'static,
) -> StreamCodec<RegistryFriendlyByteBuf, Component> {
    let tag_stream = lift(byte_buf_codecs::tag_codec(accounter));
    let codec = component_codec_with_registries();
    let encode_codec = codec.clone();
    let encode_tag = tag_stream.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, value: &Component| {
            let ops = RegistryOps::create_from_access(
                &NbtOps::instance(),
                output.registry_access().clone(),
            );
            let result = encode_codec.encode_start(&ops, value);
            let tag = match result.result() {
                Some(tag) => tag.clone(),
                None => {
                    let msg = result
                        .error_ref()
                        .map(|e| e.message().to_string())
                        .unwrap_or_default();
                    return Err(crate::codec::CodecError::new(format!(
                        "Failed to encode: {msg} {value:?}"
                    )));
                }
            };
            encode_tag.encode(output, &tag)
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            let tag = tag_stream.decode(input)?;
            let ops = RegistryOps::create_from_access(
                &NbtOps::instance(),
                input.registry_access().clone(),
            );
            let result = codec.parse(&ops, &tag);
            match result.result() {
                Some(value) => Ok((*value).clone()),
                None => {
                    let msg = result
                        .error_ref()
                        .map(|e| e.message().to_string())
                        .unwrap_or_default();
                    Err(crate::codec::CodecError::new(format!(
                        "Failed to decode: {msg} {tag}"
                    )))
                }
            }
        },
    )
}

/// `ByteBufCodecs.fromCodecTrusted(ComponentSerialization.CODEC)` —
/// `ComponentSerialization.TRUSTED_CONTEXT_FREE_STREAM_CODEC`:
/// `tagCodec(unlimitedHeap).apply(fromCodec(NbtOps.INSTANCE, CODEC))` over
/// plain `FriendlyByteBuf` (no registry context). Used by `ServerLinks`
/// custom display names, exactly as Java.
pub fn trusted_context_free_component() -> StreamCodec<FriendlyByteBuf, Component> {
    static CODEC: OnceLock<StreamCodec<FriendlyByteBuf, Component>> = OnceLock::new();
    CODEC
        .get_or_init(|| {
            apply(
                byte_buf_codecs::trusted_tag(),
                from_codec(NbtOps::instance(), component_codec()),
            )
        })
        .clone()
}

/// `OPTIONAL_STREAM_CODEC` — `STREAM_CODEC.apply(ByteBufCodecs::optional)`:
/// a boolean presence prefix, then the registry-aware component.
pub fn optional_component_with_registries()
-> StreamCodec<RegistryFriendlyByteBuf, Option<Component>> {
    static CODEC: OnceLock<StreamCodec<RegistryFriendlyByteBuf, Option<Component>>> =
        OnceLock::new();
    CODEC
        .get_or_init(|| optional_registry(component_with_registries()))
        .clone()
}

/// `TRUSTED_OPTIONAL_STREAM_CODEC` — `TRUSTED_STREAM_CODEC.apply(
/// ByteBufCodecs::optional)`.
pub fn trusted_optional_component_with_registries()
-> StreamCodec<RegistryFriendlyByteBuf, Option<Component>> {
    static CODEC: OnceLock<StreamCodec<RegistryFriendlyByteBuf, Option<Component>>> =
        OnceLock::new();
    CODEC
        .get_or_init(|| optional_registry(trusted_component_with_registries()))
        .clone()
}

/// `ByteBufCodecs.optional(trustedContextFreeComponent())` — the context-free
/// trusted optional:
/// `TRUSTED_CONTEXT_FREE_STREAM_CODEC.apply(ByteBufCodecs::optional)`, used by
/// `ClientboundResourcePackPushPacket`'s prompt field.
pub fn trusted_optional_component() -> StreamCodec<FriendlyByteBuf, Option<Component>> {
    static CODEC: OnceLock<StreamCodec<FriendlyByteBuf, Option<Component>>> = OnceLock::new();
    CODEC
        .get_or_init(|| byte_buf_codecs::optional(trusted_context_free_component()))
        .clone()
}

/// `ByteBufCodecs.optional(StreamCodec)` over [`RegistryFriendlyByteBuf`] —
/// `FriendlyByteBuf` and the registry buffer cannot share the mono `optional`,
/// so the registry-aware optional forms use this (Java's `optional` is generic
/// over `B extends ByteBuf`).
fn optional_registry<T: 'static>(
    original: StreamCodec<RegistryFriendlyByteBuf, T>,
) -> StreamCodec<RegistryFriendlyByteBuf, Option<T>> {
    let encoder_codec = original.clone();
    of(
        move |output: &mut RegistryFriendlyByteBuf, value: &Option<T>| {
            match value {
                Some(v) => {
                    output.write_boolean(true);
                    encoder_codec.encode(output, v)?;
                }
                None => output.write_boolean(false),
            }
            Ok(())
        },
        move |input: &mut RegistryFriendlyByteBuf| {
            if input.read_boolean() {
                Ok(Some(original.decode(input)?))
            } else {
                Ok(None)
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use rivet_registry::RegistryAccess;
    use rivet_text::component_contents::ComponentContents;
    use rivet_text::component_serialization::CODEC_BUILD_COUNT;
    use rivet_text::contents::{TranslatableArg, TranslatableContents};
    use rivet_text::style::Style;

    fn buf() -> FriendlyByteBuf {
        FriendlyByteBuf::new(BytesMut::new())
    }

    fn written(b: FriendlyByteBuf) -> Vec<u8> {
        b.into_inner().to_vec()
    }

    fn round_trip<T: PartialEq + std::fmt::Debug>(
        codec: &StreamCodec<FriendlyByteBuf, T>,
        value: &T,
    ) {
        let mut out = buf();
        codec.encode(&mut out, value).unwrap();
        let mut input = FriendlyByteBuf::new(BytesMut::from(written(out).as_slice()));
        assert_eq!(&codec.decode(&mut input).unwrap(), value);
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn context_free_literal_golden_wire_bytes() {
        // A bare literal collapses through the string-either branch of
        // `ComponentSerialization.CODEC` to an NBT StringTag: type 8, then
        // writeUTF("hello") (u16 length + ASCII bytes).
        let mut out = buf();
        trusted_context_free_component()
            .encode(&mut out, &Component::literal("hello"))
            .unwrap();
        assert_eq!(
            written(out),
            vec![0x08, 0x00, 0x05, b'h', b'e', b'l', b'l', b'o']
        );
        round_trip(
            &trusted_context_free_component(),
            &Component::literal("hello"),
        );
    }

    #[test]
    fn context_free_styled_compound_golden_wire_bytes() {
        // A styled literal cannot collapse (style non-empty), so it takes the
        // fullCodec branch: a compound {text, extra?, style}. Encode order is
        // the RecordCodecBuilder field order: contents first ("text"), then
        // extra (absent -> skipped), then style ("bold"). `write_any_tag`
        // writes the compound id, then each entry `writeNamedTag`
        // (type + writeUTF(key) + payload), then the end byte.
        let value = Component::literal("x").with_style(Style::EMPTY.with_bold(Some(true)));
        let mut out = buf();
        trusted_context_free_component()
            .encode(&mut out, &value)
            .unwrap();
        assert_eq!(
            written(out),
            vec![
                0x0A, // compound id
                0x08, 0x00, 0x04, b't', b'e', b'x', b't', // type 8, key "text"
                0x00, 0x01, b'x', // StringTag "x"
                0x01, 0x00, 0x04, b'b', b'o', b'l', b'd', // type 1, key "bold"
                0x01, // ByteTag true
                0x00, // end
            ]
        );
        round_trip(&trusted_context_free_component(), &value);
    }

    #[test]
    fn optional_context_free_wire_form() {
        // None -> boolean false. Some(literal "hi") -> true, then the string tag.
        let mut out = buf();
        trusted_optional_component()
            .encode(&mut out, &None)
            .unwrap();
        assert_eq!(written(out), vec![0x00]);

        let mut out = buf();
        trusted_optional_component()
            .encode(&mut out, &Some(Component::literal("hi")))
            .unwrap();
        assert_eq!(written(out), vec![0x01, 0x08, 0x00, 0x02, b'h', b'i']);

        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x00].as_slice()));
        assert_eq!(
            trusted_optional_component().decode(&mut input).unwrap(),
            None
        );
    }

    #[test]
    fn registry_aware_empty_access_matches_context_free_wire() {
        // With an empty RegistryAccess, RegistryOps delegates to NbtOps, so the
        // registry-aware codec must emit the same bytes as the context-free one.
        let value = Component::literal("x").with_style(Style::EMPTY.with_bold(Some(true)));
        let mut context_free = buf();
        trusted_context_free_component()
            .encode(&mut context_free, &value)
            .unwrap();

        let access = RegistryAccess::empty();
        let mut reg = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        trusted_component_with_registries()
            .encode(&mut reg, &value)
            .unwrap();
        assert_eq!(reg.as_slice(), written(context_free).as_slice());

        // And the trusted + untrusted registry-aware codecs encode identically.
        let mut untrusted = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        component_with_registries()
            .encode(&mut untrusted, &value)
            .unwrap();
        assert_eq!(untrusted.as_slice(), reg.as_slice());

        // Round-trip through the registry-aware codec.
        let mut input =
            RegistryFriendlyByteBuf::new(BytesMut::from(reg.as_slice()), RegistryAccess::empty());
        assert_eq!(
            trusted_component_with_registries()
                .decode(&mut input)
                .unwrap(),
            value
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn hostile_non_component_tag_errors_not_panics() {
        // A structurally valid NBT IntTag is not a valid Component: the codec
        // returns Err with Java's "Failed to decode:" message (the payload tag
        // renders after the message), it does not panic.
        let wire = vec![0x03, 0x00, 0x00, 0x00, 0x05]; // IntTag(5)
        let mut input = FriendlyByteBuf::new(BytesMut::from(wire.as_slice()));
        let err = trusted_context_free_component()
            .decode(&mut input)
            .unwrap_err();
        assert!(
            err.message.starts_with("Failed to decode:"),
            "unexpected message: {}",
            err.message
        );
    }

    #[test]
    fn truncated_component_panics_like_java() {
        // A compound id byte with no payload: the first entry type byte hits
        // EOF inside `CompoundTag.load`, which Java wraps as
        // `ReportedNbtException` (CrashReport "Loading NBT data") — the
        // unchecked rethrow of the IOException inside readNbt.
        let mut input = FriendlyByteBuf::new(BytesMut::from(vec![0x0A].as_slice()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = trusted_context_free_component().decode(&mut input);
        }));
        let payload = result.expect_err("truncated NBT must panic");
        let msg = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .expect("panic payload is a String");
        assert!(
            msg.starts_with("Loading NBT data"),
            "unexpected panic: {msg}"
        );
    }

    #[test]
    fn untrusted_default_quota_rejects_oversize_trusted_accepts() {
        // Build a component whose NBT wire payload exceeds the 2MB default
        // quota: a root with many large literal siblings (each collapses to a
        // ~60KB StringTag in the `extra` list).
        let big = "x".repeat(60_000);
        let mut root = Component::literal("root");
        for _ in 0..60 {
            root.append_component(Component::literal(&big));
        }

        let mut out = buf();
        trusted_context_free_component()
            .encode(&mut out, &root)
            .unwrap();
        let bytes = written(out);
        assert!(
            bytes.len() > rivet_nbt::nbt_accounter::DEFAULT_NBT_QUOTA as usize,
            "test needs a >2MB payload, got {} bytes",
            bytes.len()
        );

        // Trusted decode: the unlimited heap accounter accepts it.
        let mut trusted_input = FriendlyByteBuf::new(BytesMut::from(bytes.as_slice()));
        assert_eq!(
            trusted_context_free_component()
                .decode(&mut trusted_input)
                .unwrap(),
            root
        );

        // Untrusted decode: the default quota panics with Java's
        // `NbtAccounterException` message. The untrusted context-free codec was
        // removed as speculative; the surviving untrusted codec is the
        // registry-aware `STREAM_CODEC` (`component_with_registries`), which
        // reads the same NBT bytes through an empty-access registry buffer.
        let mut untrusted_input =
            RegistryFriendlyByteBuf::new(BytesMut::from(bytes.as_slice()), RegistryAccess::empty());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            component_with_registries().decode(&mut untrusted_input)
        }));
        let payload = result.expect_err("untrusted decode must panic on oversize NBT");
        let msg = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .expect("panic payload is a String");
        assert!(
            msg.contains("Tried to read NBT tag that was too big"),
            "unexpected panic: {msg}"
        );
    }

    #[test]
    fn nested_components_do_not_rebuild_the_recursive_graph() {
        // The codec graph behind each stream codec must be built once and
        // reused. `CODEC_BUILD_COUNT` counts `component_serialization::codec()`
        // calls on this thread; it must stay flat across repeated encodes after
        // the first round-trip forces the lazy recursive factory.
        let graph_count = || CODEC_BUILD_COUNT.with(|c| c.get());

        let mut root = Component::literal("root");
        root.append_component(Component::create(ComponentContents::Translatable(
            TranslatableContents::new(
                "key.tip".to_string(),
                None,
                vec![TranslatableArg::Component(Box::new(
                    Component::literal("arg").with_style(Style::EMPTY.with_bold(Some(true))),
                ))],
            ),
        )));
        root.append_component(Component::selector("@p", Some(Component::literal(","))));

        // Context-free graph (NbtOps): warm the codec, then snapshot.
        round_trip(&trusted_context_free_component(), &root);
        let after_context_free = graph_count();
        for _ in 0..50 {
            round_trip(&trusted_context_free_component(), &root);
        }
        assert_eq!(
            graph_count(),
            after_context_free,
            "context-free component codec rebuilt the recursive graph per use"
        );

        // Registry-aware graph (RegistryOps): separate cache, same guarantee.
        let access = RegistryAccess::empty();
        let mut reg = RegistryFriendlyByteBuf::new(BytesMut::new(), access);
        trusted_component_with_registries()
            .encode(&mut reg, &root)
            .unwrap();
        let mut reg_input =
            RegistryFriendlyByteBuf::new(BytesMut::from(reg.as_slice()), RegistryAccess::empty());
        assert_eq!(
            trusted_component_with_registries()
                .decode(&mut reg_input)
                .unwrap(),
            root
        );
        let after_registries = graph_count();
        for _ in 0..50 {
            let mut reg = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
            trusted_component_with_registries()
                .encode(&mut reg, &root)
                .unwrap();
            let mut reg_input = RegistryFriendlyByteBuf::new(
                BytesMut::from(reg.as_slice()),
                RegistryAccess::empty(),
            );
            assert_eq!(
                trusted_component_with_registries()
                    .decode(&mut reg_input)
                    .unwrap(),
                root
            );
        }
        assert_eq!(
            graph_count(),
            after_registries,
            "registry-aware component codec rebuilt the recursive graph per use"
        );
    }

    #[test]
    fn registry_optional_wire_form() {
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        optional_component_with_registries()
            .encode(&mut out, &None)
            .unwrap();
        assert_eq!(out.as_slice(), &[0x00]);

        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), RegistryAccess::empty());
        trusted_optional_component_with_registries()
            .encode(&mut out, &Some(Component::literal("hi")))
            .unwrap();
        assert_eq!(out.as_slice(), &[0x01, 0x08, 0x00, 0x02, b'h', b'i']);

        let mut input = RegistryFriendlyByteBuf::new(
            BytesMut::from(vec![0x01, 0x08, 0x00, 0x02, b'h', b'i'].as_slice()),
            RegistryAccess::empty(),
        );
        assert_eq!(
            trusted_optional_component_with_registries()
                .decode(&mut input)
                .unwrap(),
            Some(Component::literal("hi"))
        );
    }
}
