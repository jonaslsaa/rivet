//! Port of `net.minecraft.network.chat.contents.TranslatableContents`.
//!
//! Holds a `key`, an optional `fallback`, and a list of format arguments. The
//! arguments are Java `Object`s; in Rust they are an enum of the
//! `TranslatableContents.isAllowedPrimitiveArgument` set (Number/Boolean/
//! String) plus nested `Component` values. Format-string decomposition and
//! per-locale resolution are NOT in this slice (issue #92) — `visit` visits
//! nothing, matching an unresolved component in a non-localized encode (the
//! encode path in `ComponentSerialization` never decomposes; Paper's localized
//! Adventure path does, and is out of scope).

use super::super::ComponentContents;
use crate::style::Style;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::decoder::Decoder;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::either::Either;
use rivet_serialization::encoder::Encoder;
use rivet_serialization::map_codec::{self as map_codec_mod, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use rivet_serialization::number::Number;
use std::sync::Arc;

/// One translatable argument — Java `Object` restricted to the allowed
/// primitive set plus `Component`.
///
/// `PartialEq` only (no `Eq`): `Float(f64)` cannot satisfy `Eq`, and Java's
/// `equals` on a `Double` uses IEEE equality anyway.
#[derive(Clone, Debug, PartialEq)]
pub enum TranslatableArg {
    Number(i64),
    Float(f64),
    Bool(bool),
    String(String),
    // Boxed so the enum stays small once `Component` carries the full
    // ClickEvent/HoverEvent codec tree (clippy `large_enum_variant`).
    Component(Box<crate::Component>),
}

impl std::fmt::Display for TranslatableArg {
    /// `Object.toString()` — `null` renders `"null"`; numbers use Java's
    /// formatting. A `Component` arg renders its full `Component.toString()`
    /// (`String.valueOf(component)`), not just the plain text.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TranslatableArg::Number(n) => write!(f, "{}", n),
            TranslatableArg::Float(v) => {
                write!(
                    f,
                    "{}",
                    rivet_util::java_float_format::java_double_to_string(*v)
                )
            }
            TranslatableArg::Bool(b) => write!(f, "{}", b),
            TranslatableArg::String(s) => f.write_str(s),
            TranslatableArg::Component(c) => write!(f, "{}", c),
        }
    }
}

/// Port of `net.minecraft.network.chat.contents.TranslatableContents`.
#[derive(Clone, Debug, PartialEq)]
pub struct TranslatableContents {
    key: String,
    fallback: Option<String>,
    args: Vec<TranslatableArg>,
}

impl TranslatableContents {
    /// `TranslatableContents.NO_ARGS`.
    pub const NO_ARGS: &'static [TranslatableArg] = &[];

    /// `new TranslatableContents(String key, String fallback, Object[] args)`.
    pub fn new(key: String, fallback: Option<String>, args: Vec<TranslatableArg>) -> Self {
        TranslatableContents {
            key,
            fallback,
            args,
        }
    }

    /// `TranslatableContents.getKey()`.
    pub fn get_key(&self) -> &str {
        &self.key
    }

    /// `TranslatableContents.getFallback()`.
    pub fn get_fallback(&self) -> Option<&str> {
        self.fallback.as_deref()
    }

    /// `TranslatableContents.getArgs()`.
    pub fn get_args(&self) -> &[TranslatableArg] {
        &self.args
    }

    /// `visit(ContentConsumer)` — resolution via `Language.decompose` is issue
    /// #92 scope; an unresolved translatable contributes no plain text.
    pub fn visit_content<T>(&self, _output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        None
    }

    /// `visit(StyledContentConsumer, Style)` — same deferral.
    pub fn visit_styled<T>(
        &self,
        _output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        _style: &Style,
    ) -> Option<T> {
        None
    }

    /// `TranslatableContents.MAP_CODEC` — `RecordCodecBuilder.mapCodec` over
    /// `translate`, the lenient `fallback`, and the optional `with` list,
    /// lifted to the `ComponentContents` enum. `top` is the enclosing
    /// `ComponentSerialization.CODEC`; the `with` args' component branch
    /// (`ARG_CODEC`) reuses it so encoding a translatable does not build a
    /// fresh recursive Component graph per use (Java's `ARG_CODEC` references
    /// the same static `ComponentSerialization.CODEC`).
    pub fn map_codec<Ops: DynamicOps + 'static>(
        top: Arc<dyn Codec<crate::Component, Ops>>,
    ) -> Arc<dyn MapCodec<ComponentContents, Ops>> {
        map_codec_mod::xmap(
            Arc::new(TranslatableMapCodec { top }),
            Arc::new(|c: &TranslatableContents| ComponentContents::Translatable(c.clone())),
            Arc::new(|c: &ComponentContents| match c {
                ComponentContents::Translatable(inner) => inner.clone(),
                _ => panic!("translatable codec applied to non-translatable contents"),
            }),
        )
    }
}

/// `TranslatableContents.MAP_CODEC`.
struct TranslatableMapCodec<Ops: DynamicOps + 'static> {
    /// `ComponentSerialization.CODEC` — reused for nested `Component` args.
    top: Arc<dyn Codec<crate::Component, Ops>>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for TranslatableMapCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TranslatableContentsMapCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for TranslatableMapCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![
            ops.create_string("translate".to_string()),
            ops.create_string("fallback".to_string()),
            ops.create_string("with".to_string()),
        ]
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<TranslatableContents, Ops>
    for TranslatableMapCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<TranslatableContents> {
        let translate = match input.get_string("translate") {
            Some(translate) => codec::string_codec::<Ops>().parse(ops, &translate),
            None => return DataResult::error("No key translate in MapLike".to_string()),
        };
        // Lenient `fallback` (`Codec.STRING.lenientOptionalFieldOf("fallback")`):
        // an absent field, or a present field the string codec rejects, decodes
        // to None (Java `OptionalFieldCodec` with lenient=true returns
        // `Optional.empty()` on a present-but-invalid value).
        let fallback = match input.get_string("fallback") {
            Some(value) => match codec::string_codec::<Ops>().parse(ops, &value).result() {
                Some(s) => DataResult::success(Some(s.clone())),
                None => DataResult::success(None),
            },
            None => DataResult::success(None),
        };
        // Optional `with` list via `ARG_CODEC.listOf()`.
        let args = match input.get_string("with") {
            Some(value) => {
                let list = codec::list::<TranslatableArg, Ops>(arg_codec(self.top.clone()))
                    .parse(ops, &value);
                list.map(|list| {
                    if list.is_empty() {
                        Vec::new()
                    } else {
                        list.to_vec()
                    }
                })
            }
            None => DataResult::success(Vec::new()),
        };
        translate.flat_map(move |translate| {
            fallback.flat_map(move |fallback| {
                args.map(move |args| {
                    TranslatableContents::new(translate.clone(), fallback.clone(), args.clone())
                })
            })
        })
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<TranslatableContents, Ops>
    for TranslatableMapCodec<Ops>
{
    fn encode(
        &self,
        input: &TranslatableContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        prefix.add_string_result(
            "translate",
            codec::string_codec::<Ops>().encode_start(ops, &input.key),
        );
        if let Some(fallback) = &input.fallback {
            prefix.add_string_result(
                "fallback",
                codec::string_codec::<Ops>().encode_start(ops, fallback),
            );
        }
        // Java writes `"with"` only when `args` is non-empty (`adjustArgs`).
        if !input.args.is_empty() {
            let list = codec::list::<TranslatableArg, Ops>(arg_codec(self.top.clone()))
                .encode_start(ops, &input.args);
            prefix.add_string_result("with", list);
        }
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<TranslatableContents, Ops> for TranslatableMapCodec<Ops> {
    fn decode(
        &self,
        ops: &Ops,
        input: &dyn MapLike<Ops::Output>,
    ) -> DataResult<TranslatableContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &TranslatableContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

impl std::fmt::Display for TranslatableContents {
    /// `TranslatableContents.toString()` —
    /// `translation{key='K'[, fallback='F'], args=[...]}`. The `args` render via
    /// `Arrays.toString(Object[])` — `String.valueOf` per element, no quotes —
    /// so `["a", 42]` displays as `[a, 42]`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "translation{{key='{}'", self.key)?;
        if let Some(fallback) = &self.fallback {
            write!(f, ", fallback='{}'", fallback)?;
        }
        f.write_str(", args=[")?;
        for (i, arg) in self.args.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{}", arg)?;
        }
        f.write_str("]}")
    }
}

/// `TranslatableContents.ARG_CODEC` — `Codec.either(PRIMITIVE_ARG_CODEC,
/// ComponentSerialization.CODEC)` xmapped to `TranslatableArg`.
///
/// Decode collapses a plain-text `Component` argument to its `String` (Java's
/// `component -> Objects.requireNonNullElse(component.tryCollapseToString(),
/// component)`), so a component arg that is a bare literal round-trips as a
/// string. `top` is the enclosing `ComponentSerialization.CODEC`; the component
/// branch reuses it instead of building a fresh recursive graph per arg (Java's
/// `ARG_CODEC` references the same static `ComponentSerialization.CODEC`).
pub fn arg_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<crate::Component, Ops>>,
) -> Arc<dyn Codec<TranslatableArg, Ops>> {
    codec::xmap(
        codec::either(
            Arc::new(PrimitiveArgCodec {
                _ops: std::marker::PhantomData,
            }),
            top,
        ),
        Arc::new(|e: &Either<TranslatableArg, crate::Component>| match e {
            Either::Left(a) => a.clone(),
            Either::Right(c) => match c.try_collapse_to_string() {
                Some(text) => TranslatableArg::String(text),
                None => TranslatableArg::Component(Box::new(c.clone())),
            },
        }),
        Arc::new(|a: &TranslatableArg| match a {
            TranslatableArg::Component(c) => Either::Right((**c).clone()),
            other => Either::Left(other.clone()),
        }),
    )
}

/// `TranslatableContents.PRIMITIVE_ARG_CODEC` — a codec over the non-Component
/// `TranslatableArg` values. Java uses `ExtraCodecs.JAVA` (a passthrough
/// accepting any value) validated to the primitive set; the port's `decode`
/// mirrors the observable result: a JSON string → `String`, number → number,
/// bool → `Bool`, anything else errors so the either falls through to the
/// component branch.
struct PrimitiveArgCodec<Ops: DynamicOps + 'static> {
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for PrimitiveArgCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PrimitiveArgCodec")
    }
}

impl<Ops: DynamicOps + 'static> Decoder<TranslatableArg, Ops> for PrimitiveArgCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &Ops::Output) -> DataResult<(TranslatableArg, Ops::Output)> {
        if let Some(s) = ops.get_string_value(input).result() {
            return DataResult::success((TranslatableArg::String(s.clone()), ops.empty()));
        }
        if let Some(n) = ops.get_number_value(input).result() {
            return DataResult::success((number_to_arg(*n), ops.empty()));
        }
        if let Some(b) = ops.get_boolean_value(input).result() {
            return DataResult::success((TranslatableArg::Bool(*b), ops.empty()));
        }
        DataResult::error("This value needs to be parsed as component".to_string())
    }
}

impl<Ops: DynamicOps + 'static> Encoder<TranslatableArg, Ops> for PrimitiveArgCodec<Ops> {
    fn encode(
        &self,
        input: &TranslatableArg,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        match input {
            TranslatableArg::Number(n) => ops.merge_to_primitive(prefix, ops.create_long(*n)),
            TranslatableArg::Float(v) => ops.merge_to_primitive(prefix, ops.create_double(*v)),
            TranslatableArg::Bool(b) => ops.merge_to_primitive(prefix, ops.create_boolean(*b)),
            TranslatableArg::String(s) => {
                ops.merge_to_primitive(prefix, ops.create_string(s.clone()))
            }
            TranslatableArg::Component(_) => {
                DataResult::error("Component argument must use the component branch".to_string())
            }
        }
    }
}

impl<Ops: DynamicOps + 'static> Codec<TranslatableArg, Ops> for PrimitiveArgCodec<Ops> {}

/// Map a typed `Number` to the `TranslatableArg` number variants. Integral
/// values (Byte/Short/Int/Long) become `Number(i64)`; floating values become
/// `Float(f64)`.
fn number_to_arg(number: Number) -> TranslatableArg {
    match number {
        Number::Byte(v) => TranslatableArg::Number(v as i64),
        Number::Short(v) => TranslatableArg::Number(v as i64),
        Number::Int(v) => TranslatableArg::Number(v as i64),
        Number::Long(v) => TranslatableArg::Number(v),
        Number::Float(v) => TranslatableArg::Float(v as f64),
        Number::Double(v) => TranslatableArg::Float(v),
    }
}
