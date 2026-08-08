//! Port of `net.minecraft.network.chat.ComponentSerialization`.
//!
//! `ComponentSerialization.CODEC` is the recursive JSON codec for `Component`:
//!
//! ```text
//! recursive("Component", |topSerializer| {
//!   LateBoundIdMapper<String, MapCodec<? extends ComponentContents>> contentTypes;
//!   bootstrap(contentTypes);
//!   MapCodec<ComponentContents> compressedContents =
//!       createLegacyComponentMatcher(contentTypes, ComponentContents::codec, "type");
//!   Codec<Component> fullCodec = record {
//!       contents: compressedContents.forGetter(Component::getContents),
//!       extra:    nonEmptyList(topSerializer.listOf()).optionalFieldOf("extra", List.of()).forGetter(Component::getSiblings),
//!       style:    Style.Serializer.MAP_CODEC.forGetter(Component::getStyle),
//!   } apply MutableComponent::new;
//!   either(either(STRING, nonEmptyList(topSerializer.listOf())), fullCodec)
//!       .xmap(Component::literal / createFromList, tryCollapseToString)
//! })
//! ```
//!
//! The Rust port keeps the same shape. `createLegacyComponentMatcher` is
//! `orCompressed(StrictEither(typeFieldName, discriminator, fuzzy),
//! discriminator)`:
//!
//! - `fuzzy` = `FuzzyCodec(types.values(), codecGetter)` — tries each
//!   registered `MapCodec` and returns the first successful decode; encodes via
//!   the value's own codec (the `codecGetter`).
//! - `discriminator` = `types.codec(STRING).dispatchMap("type", codecGetter,
//!   c -> c)`. Java threads the `MapCodec` itself as the discriminator value; the
//!   Rust port threads the type **name** (`String`) instead, because
//!   `Arc<dyn MapCodec>` is not `PartialEq`/`Display`-comparable. The behavior
//!   is identical: `codec_fn` maps the name back to its registered `MapCodec`,
//!   and the encode side still uses the value's own codec.
//!
//! RivetTodo(#89): `nbt`/`object` contents are not registered (they need
//! `NbtOps`/path parsing and `ObjectInfo`); `ClickEvent`/`HoverEvent` codecs
//! error on encode when a style carries those fields. The NBT wire-codec family
//! (`STREAM_CODEC`/`TRUSTED_STREAM_CODEC`/`TRUSTED_CONTEXT_FREE_STREAM_CODEC`
//! and the optional variants) is ported in `rivet-protocol::chat`, which runs
//! this same graph over `NbtOps`/`RegistryOps`; the Adventure localization path
//! is a server-side concern and remains out of scope (epic #12).

use crate::Component;
use crate::component_contents::ComponentContents;
use crate::contents::{
    KeybindContents, PlainTextContents, ScoreContents, SelectorContents, TranslatableContents,
};
use crate::style::Style;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::either::Either;
use rivet_serialization::extra_codecs;
use rivet_serialization::json_ops::JsonOps;
use rivet_serialization::map_codec::{self as map_codec, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::cell::Cell;
use std::sync::Arc;

// Test instrument: how many times this thread has called [`codec`], i.e. how
// many distinct `Codec.recursive` Component graphs it has constructed. Each
// call to [`codec`] builds a fresh graph (a permanent strong cycle), so the
// count must stay flat while a single graph is reused for nested components.
// Load-bearing tests (issue #207) snapshot this before/after repeated
// translatable-arg / selector-separator encodes to catch a per-use rebuild.
#[doc(hidden)]
thread_local! {
    pub static CODEC_BUILD_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// `ComponentContents::codec` — resolves a contents value to its registered
/// `MapCodec` (Java's `codecGetter` in `ComponentSerialization`).
type CodecGetter<T, Ops> = Arc<dyn Fn(&T) -> Arc<dyn MapCodec<T, Ops>> + Send + Sync>;

/// `ComponentSerialization.CODEC`.
pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Component, Ops>> {
    CODEC_BUILD_COUNT.with(|c| c.set(c.get() + 1));
    codec::recursive("Component".to_string(), Arc::new(|top| create_codec(top)))
}

/// Decode `input` as JSON through the given `CODEC` under non-compressed
/// `JsonOps`, re-encode, and serialize compactly (insertion order, no HTML
/// escaping) — the byte-identical mirror of Paper's `component.json` oracle op
/// (issue #98). Shared by the offline corpus tests (`corpus_tests.rs`) and the
/// live `rivet-parity` differential so the decode->re-encode compare can never
/// drift between the two.
pub fn json_canonical(
    codec: &Arc<dyn Codec<Component, JsonOps>>,
    input: &str,
) -> Result<String, String> {
    let ops = JsonOps::INSTANCE;
    let value: serde_json::Value =
        serde_json::from_str(input).map_err(|e| format!("invalid JSON: {e}"))?;
    let decoded = codec.parse(&ops, &value);
    let component = decoded
        .result()
        .ok_or_else(|| "component decode failed".to_string())?;
    let encoded = codec.encode_start(&ops, component);
    let encoded = encoded
        .result()
        .ok_or_else(|| "component encode failed".to_string())?;
    serde_json::to_string(encoded).map_err(|e| format!("serialize: {e}"))
}

/// The non-recursive body of `CODEC` given the `topSerializer`.
fn create_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Component, Ops>>,
) -> Arc<dyn Codec<Component, Ops>> {
    // Every nested `Component` reference inside the graph (the `extra` sibling
    // list, the `top` special form, and the translatable-arg / selector-
    // separator codecs) is threaded `top` — the `RecursiveSelf` of this graph —
    // so the whole graph shares the single `recursive` codec and no nested
    // `codec()` graph is ever built per use (Java's `ComponentSerialization.CODEC`
    // static singleton is threaded the same way).
    let content_types = bootstrap(top.clone());
    let contents_codec = create_legacy_component_matcher(top.clone(), &content_types);

    // `record { contents, extra: optionalFieldOf("extra", List.of()), style }`
    // apply `MutableComponent::new` — `RecordCodecBuilder.create(...)` returns
    // the `Codec<Component>`.
    let top_inner = top.clone();
    let full_codec =
        rivet_serialization::record_builder::create::<Component, Ops>(move |instance| {
            let contents = map_codec::for_getter(
                contents_codec,
                Arc::new(|c: &Component| c.get_contents().clone()),
            );
            let extra = map_codec::for_getter(
                codec::optional_field(
                    "extra".to_string(),
                    extra_codecs::non_empty_list(codec::list(top_inner.clone())),
                    false,
                ),
                Arc::new(|c: &Component| {
                    if c.get_siblings().is_empty() {
                        None
                    } else {
                        Some(c.get_siblings().to_vec())
                    }
                }),
            );
            let style = map_codec::for_getter(
                crate::style::serializer::map_codec(),
                Arc::new(|c: &Component| c.get_style().clone()),
            );
            instance.group(contents).and(extra).and(style).apply(
                instance,
                Arc::new(
                    |contents: ComponentContents, extra: Option<Vec<Component>>, style: Style| {
                        Component::new(contents, extra.unwrap_or_default(), style)
                    },
                ),
            )
        });

    // `either(either(STRING, nonEmptyList(top.listOf())), fullCodec)`.
    let special = codec::either(
        codec::either(
            codec::string_codec(),
            extra_codecs::non_empty_list(codec::list(top)),
        ),
        full_codec,
    );
    codec::xmap(
        special,
        Arc::new(
            |e: &Either<Either<String, Vec<Component>>, Component>| match e {
                Either::Left(inner) => match inner {
                    Either::Left(text) => Component::literal(text),
                    Either::Right(list) => create_from_list(list),
                },
                Either::Right(c) => c.clone(),
            },
        ),
        Arc::new(|c: &Component| match c.try_collapse_to_string() {
            Some(text) => Either::Left(Either::Left(text)),
            None => Either::Right(c.clone()),
        }),
    )
}

/// `createFromList(List<Component>)` — `result = list[0].copy(); append rest`.
fn create_from_list(list: &[Component]) -> Component {
    let mut result = list[0].copy();
    for item in &list[1..] {
        result.append_component(item.clone());
    }
    result
}

/// `bootstrap(LateBoundIdMapper<String, MapCodec<? extends ComponentContents>>)`.
///
/// `nbt` and `object` are deferred (RivetTodo(#89) at module scope); the five
/// registered contents match the ported slice. `top` is threaded into the
/// contents whose codecs recurse into `Component` (translatable args, selector
/// separator) so they reuse this graph instead of building their own.
fn bootstrap<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Component, Ops>>,
) -> extra_codecs::LateBoundIdMapper<String, Arc<dyn MapCodec<ComponentContents, Ops>>> {
    let mapper = extra_codecs::LateBoundIdMapper::new();
    mapper.put("text".to_string(), PlainTextContents::map_codec());
    mapper.put(
        "translatable".to_string(),
        TranslatableContents::map_codec(top.clone()),
    );
    mapper.put("keybind".to_string(), KeybindContents::map_codec());
    mapper.put("score".to_string(), ScoreContents::map_codec());
    mapper.put(
        "selector".to_string(),
        SelectorContents::map_codec(top.clone()),
    );
    mapper
}

/// `createLegacyComponentMatcher(types, codecGetter, typeFieldName)` —
/// `orCompressed(StrictEither(type, discriminator, fuzzy), discriminator)`.
fn create_legacy_component_matcher<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Component, Ops>>,
    types: &extra_codecs::LateBoundIdMapper<String, Arc<dyn MapCodec<ComponentContents, Ops>>>,
) -> Arc<dyn MapCodec<ComponentContents, Ops>> {
    let values = extra_codecs::late_bound_values(types);
    let entries = extra_codecs::late_bound_entries(types);
    let codec_getter: CodecGetter<ComponentContents, Ops> =
        Arc::new(move |c: &ComponentContents| c.codec(top.clone()));

    // `FuzzyCodec(types.values(), codecGetter)`.
    let fuzzy: Arc<dyn MapCodec<ComponentContents, Ops>> = Arc::new(FuzzyCodec {
        codecs: values,
        encoder_getter: codec_getter,
    });

    // `types.codec(STRING).dispatchMap(typeFieldName, codecGetter, c -> c)`.
    let discriminator = key_dispatch(
        codec::string_codec(),
        Arc::new(|c: &ComponentContents| DataResult::success(type_name(c))),
        Arc::new(move |name: &String| {
            entries
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| DataResult::success(v.clone()))
                .unwrap_or_else(|| DataResult::error(format!("Unknown element id: {}", name)))
        }),
    );

    // `StrictEither(typeFieldName, discriminator, fuzzy)`.
    let strict: Arc<dyn MapCodec<ComponentContents, Ops>> = Arc::new(StrictEither {
        type_field_name: "type".to_string(),
        typed: discriminator.clone(),
        fuzzy,
    });
    // `ExtraCodecs.orCompressed(contentsCodec, discriminator)`.
    extra_codecs::or_compressed_map(strict, discriminator)
}

/// `types.codec(STRING).dispatchMap("type", codecGetter, c -> c)` — the
/// `fieldOf("type", STRING)`-discriminated map codec. `K = String` (the type
/// name; Java threads the `MapCodec` value — see module docs).
fn key_dispatch<Ops: DynamicOps + 'static>(
    id_codec: Arc<dyn Codec<String, Ops>>,
    type_fn: rivet_serialization::key_dispatch_codec::TypeFn<String, ComponentContents>,
    codec_fn: rivet_serialization::key_dispatch_codec::CodecFn<String, ComponentContents, Ops>,
) -> Arc<dyn MapCodec<ComponentContents, Ops>> {
    rivet_serialization::key_dispatch_codec::dispatch_map("type", id_codec, type_fn, codec_fn)
}

/// `ComponentContents::codec` — the type name registered in `bootstrap`.
fn type_name(c: &ComponentContents) -> String {
    match c {
        ComponentContents::PlainText(_) => "text".to_string(),
        ComponentContents::Translatable(_) => "translatable".to_string(),
        ComponentContents::Keybind(_) => "keybind".to_string(),
        ComponentContents::Score(_) => "score".to_string(),
        ComponentContents::Selector(_) => "selector".to_string(),
    }
}

/// `ComponentSerialization.FuzzyCodec<T>` — tries every registered codec and
/// returns the first successful decode; encodes via the value's own codec
/// (`encoderGetter.apply(input)`).
struct FuzzyCodec<Ops: DynamicOps + 'static> {
    codecs: Vec<Arc<dyn MapCodec<ComponentContents, Ops>>>,
    encoder_getter: CodecGetter<ComponentContents, Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for FuzzyCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FuzzyCodec[{:?}]", self.codecs)
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for FuzzyCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = Vec::new();
        for codec in &self.codecs {
            keys.extend(codec.keys(ops));
        }
        keys.dedup();
        keys
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<ComponentContents, Ops> for FuzzyCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ComponentContents> {
        for codec in &self.codecs {
            let result = codec.decode(ops, input);
            if result.result().is_some() {
                return result;
            }
        }
        DataResult::error("No matching codec found".to_string())
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<ComponentContents, Ops> for FuzzyCodec<Ops> {
    fn encode(
        &self,
        input: &ComponentContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        let encoder = (self.encoder_getter)(input);
        encoder.encode(input, ops, prefix);
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<ComponentContents, Ops> for FuzzyCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ComponentContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &ComponentContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

/// `ComponentSerialization.StrictEither<T>` — decodes through the typed
/// discriminator when the `typeFieldName` key is present, else the fuzzy codec;
/// encodes via the fuzzy codec.
struct StrictEither<Ops: DynamicOps + 'static> {
    type_field_name: String,
    typed: Arc<dyn MapCodec<ComponentContents, Ops>>,
    fuzzy: Arc<dyn MapCodec<ComponentContents, Ops>>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for StrictEither<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "StrictEither")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for StrictEither<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        let mut keys = self.typed.keys(ops);
        keys.extend(self.fuzzy.keys(ops));
        keys.dedup();
        keys
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<ComponentContents, Ops> for StrictEither<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ComponentContents> {
        if input
            .get(&ops.create_string(self.type_field_name.clone()))
            .is_some()
        {
            self.typed.decode(ops, input)
        } else {
            self.fuzzy.decode(ops, input)
        }
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<ComponentContents, Ops> for StrictEither<Ops> {
    fn encode(
        &self,
        input: &ComponentContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        self.fuzzy.encode(input, ops, prefix);
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<ComponentContents, Ops> for StrictEither<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<ComponentContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &ComponentContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}
