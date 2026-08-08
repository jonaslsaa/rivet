//! Port of `net.minecraft.network.chat.contents.TranslatableContents`.
//!
//! Holds a `key`, an optional `fallback`, and a list of format arguments. The
//! arguments are Java `Object`s; in Rust they are an enum of the
//! `TranslatableContents.isAllowedPrimitiveArgument` set (Number/Boolean/
//! String) plus nested `Component` values.
//!
//! `visit`/`visit_styled` perform the locale resolution: `decompose` reads the
//! translated format string (via the process-wide [`Language`]), decomposes it
//! with the `FORMAT_PATTERN` `%(?:(\\d+)\\$)?([A-Za-z%]|$)`, and visits the
//! parts in order, short-circuiting on the first `Some` exactly like Java. The
//! decomposition result is cached (`decomposedWith` / `decomposedParts`), so a
//! component re-visits without re-reading the language.

use super::super::ComponentContents;
use crate::locale;
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
use std::cell::Cell;
use std::sync::Arc;
use std::sync::OnceLock;

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
#[derive(Clone)]
pub struct TranslatableContents {
    key: String,
    fallback: Option<String>,
    args: Vec<TranslatableArg>,
    /// `decomposedWith` / `decomposedParts` — the cached decomposition. Java
    /// stores `Language decomposedWith` plus the `List<FormattedText>` parts
    /// and re-decomposes only when the language changes; the port's `OnceLock`
    /// computes once and the process-wide language is write-once in this slice
    /// (`Language.inject` is deferred), so the cache never goes stale and the
    /// identity field Java keys on is unnecessary. The parts `Arc` is shared
    /// across clones, like Java sharing the same contents on `copy()`.
    decompose_cache: OnceLock<DecomposeCache>,
}

/// The cached decomposition of a translated format string.
#[derive(Clone)]
struct DecomposeCache {
    /// `decomposedParts` — the ordered parts, visited on each `visit`.
    parts: Arc<[DecomposedPart]>,
}

/// One part of a decomposed format string — Java's `List<FormattedText>`.
/// A format argument that is a `Component` is stored as the component itself
/// (its `visit` walks contents + siblings); every other argument is stored as
/// its `toString` text (`%%` → `"%"`, null → `"null"`). The `Component` is
/// boxed so the enum stays small (`Component` is a large value type).
#[derive(Clone, Debug)]
enum DecomposedPart {
    Text(String),
    Component(Box<crate::Component>),
}

impl DecomposedPart {
    /// `FormattedText.visit(ContentConsumer)` — a `Text` part delivers the
    /// string; a `Component` part recurses via `Component.visit` (contents then
    /// siblings), passing the same consumer.
    fn visit_content<T>(&self, output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        match self {
            DecomposedPart::Text(text) => output(text),
            DecomposedPart::Component(component) => component.visit_content(output),
        }
    }

    /// `FormattedText.visit(StyledContentConsumer, Style)`.
    fn visit_styled<T>(
        &self,
        output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        style: &Style,
    ) -> Option<T> {
        match self {
            DecomposedPart::Text(text) => output(style, text),
            DecomposedPart::Component(component) => component.visit_styled(output, style),
        }
    }
}

/// One `FORMAT_PATTERN` match — Java's `Matcher` group/span results.
struct FormatMatch<'a> {
    /// `Matcher.start()` — byte index of the `%`.
    start: usize,
    /// `Matcher.end()` — byte index one past the match (at least `start + 1`,
    /// since the literal `%` always consumes a byte).
    end: usize,
    /// `Matcher.group(1)` — the `(\d+)` position-index digits, if the
    /// `(\d+\$)` group matched.
    index_group: Option<&'a str>,
    /// `Matcher.group(2)` — the conversion character (`s`, or `%` for `%%`),
    /// or the empty string for the end-anchor (`$`) branch.
    format_type: &'a str,
}

/// `FORMAT_PATTERN` — `%(?:(\\d+)\\$)?([A-Za-z%]|$)`, implemented as a byte
/// scanner (no regex dependency). `find_from(current)` mirrors
/// `Matcher.find(int)`: the first match at or after `current`. The tricky
/// branch is group 2: a single ASCII letter or `%`, or — at the very end of
/// the input — the empty `$` alternative, which consumes just the `%` and
/// yields `format_type == ""`.
struct FormatMatcher<'a> {
    template: &'a str,
}

impl<'a> FormatMatcher<'a> {
    fn new(template: &'a str) -> Self {
        FormatMatcher { template }
    }

    /// `Matcher.find(int current)`.
    fn find_from(&self, current: usize) -> Option<FormatMatch<'a>> {
        let bytes = self.template.as_bytes();
        let mut i = current;
        while i < bytes.len() {
            if bytes[i] == b'%' {
                // Optional `(\d+\$)` — digits then `$`; without the `$` the
                // group matches empty and scanning resumes right after `%`
                // (the regex backtracks past the digits).
                let mut j = i + 1;
                let digits_start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                let has_index = j > digits_start && j < bytes.len() && bytes[j] == b'$';
                let index_group = has_index.then(|| &self.template[digits_start..j]);
                let g2 = if has_index { j + 1 } else { i + 1 };

                if g2 < bytes.len() {
                    let c = bytes[g2];
                    if c.is_ascii_alphabetic() || c == b'%' {
                        return Some(FormatMatch {
                            start: i,
                            end: g2 + 1,
                            index_group,
                            format_type: &self.template[g2..g2 + 1],
                        });
                    }
                    // A non-letter/non-`%` (and not end-of-input) can't fill
                    // group 2; the pattern fails at this `%`, keep scanning.
                } else {
                    // End of input: `([A-Za-z%]|$)` matches the empty `$`
                    // branch; the match is the `%` alone.
                    return Some(FormatMatch {
                        start: i,
                        end: g2,
                        index_group,
                        format_type: "",
                    });
                }
            }
            i += 1;
        }
        None
    }
}

/// The failures `decomposeTemplate`/`getArgument` can throw. Java wraps them
/// in `TranslatableFormatException` ("Error parsing: <c>: ...",
/// "Invalid index %d requested for <c>", or "Error while parsing: <c>"), but
/// `decompose` catches every variant and falls back to a single part holding
/// the raw format string, so the message text is never surfaced and the
/// variants carry no payload.
enum FormatError {
    /// The raw `IllegalArgumentException` — an embedded `%` in literal text
    /// between matches.
    IllegalArgument,
    /// `"Unsupported format: '<formatString>'"`.
    Unsupported,
    /// `Integer.parseInt` failure (an index-group that overflows `int`).
    NumberFormat,
    /// `"Invalid index %d requested for <component>"`.
    InvalidIndex,
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
            decompose_cache: OnceLock::new(),
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

    /// `visit(ContentConsumer)` — `decompose()` then visit each part in order,
    /// returning the first `Some` the consumer produces. Paper wraps the
    /// consumer in `TranslatableContentConsumer`, which counts visited strings
    /// and throws `IllegalArgumentException("Too long")` once the count exceeds
    /// 32; the caught exception surfaces as `output.accept("...")`.
    ///
    /// The port models the counting consumer with a per-visit shared
    /// [`Cell`] counter and abort flag: the wrapped consumer increments the
    /// counter and, past the limit, drops the string (never calling `output`)
    /// and sets the abort flag, which the part loop checks to stop. This
    /// reproduces Java's observable output exactly — the strings accepted up to
    /// the limit reach `output`, everything past it is discarded, and a single
    /// `"..."` is appended. Each `visit_content` call (including a nested
    /// translatable reached through a `Component` arg) creates its own counter,
    /// matching Java's nested `TranslatableContentConsumer`; the nested level's
    /// `"..."` passes through this level's counting consumer, so the shared
    /// counter accumulates across the whole visit like `A.accept` does.
    pub fn visit_content<T>(&self, output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        let visited = Cell::new(0usize);
        let aborted = Cell::new(false);
        {
            let mut counted = |text: &str| -> Option<T> {
                // Paper: `if (this.visited++ > 32) throw` — the increment is
                // skipped on the aborting call, so 33 strings are accepted.
                let n = visited.get();
                if n > 32 {
                    aborted.set(true);
                    return None;
                }
                visited.set(n + 1);
                output(text)
            };
            self.decompose();
            for part in self.decomposed() {
                if aborted.get() {
                    break;
                }
                if let Some(result) = part.visit_content(&mut counted) {
                    return Some(result);
                }
            }
        }
        // Paper: `catch (IllegalArgumentException ignored) { return
        // output.accept("..."); }` — `output` is the original consumer, not the
        // counting wrapper.
        if aborted.get() {
            return output("...");
        }
        None
    }

    /// `visit(StyledContentConsumer, Style)` — `decompose()` then visit each
    /// part with `part.visit(output, currentStyle)`, short-circuiting on the
    /// first `Some`. Paper's "Too long" guard wraps only the unstyled `visit`;
    /// the styled path is uncounted.
    pub fn visit_styled<T>(
        &self,
        output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        style: &Style,
    ) -> Option<T> {
        self.decompose();
        for part in self.decomposed() {
            if let Some(result) = part.visit_styled(output, style) {
                return Some(result);
            }
        }
        None
    }

    /// `decompose()` — resolve the format string from the process-wide
    /// [`Language`], decompose it into parts, and cache them. On a format error
    /// Java falls back to a single part holding the raw format string. The
    /// `OnceLock` computes once; Java re-decomposes only when the language
    /// instance changes, and the port's default language is write-once in this
    /// slice (`Language.inject` is deferred), so once-only is equivalent.
    fn decompose(&self) {
        let lang = locale::get_instance();
        self.decompose_cache.get_or_init(|| {
            let format = match &self.fallback {
                Some(fallback) => lang.get_or_default_with(&self.key, fallback).to_owned(),
                None => lang.get_or_default(&self.key).to_owned(),
            };
            let parts = match self.decompose_template(&format) {
                Ok(parts) => parts,
                Err(_) => vec![DecomposedPart::Text(format)],
            };
            DecomposeCache {
                parts: parts.into(),
            }
        });
    }

    /// The cached decomposed parts (after [`decompose`](Self::decompose)).
    fn decomposed(&self) -> &[DecomposedPart] {
        &self
            .decompose_cache
            .get()
            .expect("decompose() runs before decomposed()")
            .parts
    }

    /// `decomposeTemplate(template, consumer)` — the `FORMAT_PATTERN`
    /// `%(?:(\\d+)\\$)?([A-Za-z%]|$)` scan. Plain text between matches is
    /// emitted verbatim; `%%` emits `"%"`; `%s`/`%<n>$s` emits the argument
    /// (component or `toString`); any other conversion, an embedded `%` in
    /// literal text, or an out-of-range index throws
    /// [`TranslatableFormatException`] (the caller falls back to the raw
    /// format).
    fn decompose_template(&self, template: &str) -> Result<Vec<DecomposedPart>, FormatError> {
        let mut parts = Vec::new();
        let mut current = 0;
        // `replacementIndex` — the implicit 0-based position of the next
        // unindexed `%s` (`replacementIndex++` in Java's loop body).
        let mut replacement_index = 0usize;

        let scan = FormatMatcher::new(template);
        while let Some(FormatMatch {
            start,
            end,
            index_group,
            format_type,
        }) = scan.find_from(current)
        {
            if start > current {
                let prefix = &template[current..start];
                if prefix.contains('%') {
                    return Err(FormatError::IllegalArgument);
                }
                parts.push(DecomposedPart::Text(prefix.to_owned()));
            }

            let format_string = &template[start..end];
            if format_type == "%" && format_string == "%%" {
                parts.push(DecomposedPart::Text("%".to_owned()));
            } else {
                if format_type != "s" {
                    return Err(FormatError::Unsupported);
                }
                let index = match index_group {
                    Some(digits) => {
                        digits
                            .parse::<i32>()
                            .map_err(|_| FormatError::NumberFormat)? as i64
                            - 1
                    }
                    None => {
                        let idx = replacement_index;
                        replacement_index += 1;
                        idx as i64
                    }
                };
                parts.push(self.get_argument(index)?);
            }

            current = end;
        }

        if current < template.len() {
            let tail = &template[current..];
            if tail.contains('%') {
                return Err(FormatError::IllegalArgument);
            }
            parts.push(DecomposedPart::Text(tail.to_owned()));
        }

        Ok(parts)
    }

    /// `getArgument(int)` — the part for a format argument. A `Component` arg
    /// is the component itself; a null-ish arg is `"null"`; otherwise the
    /// `toString`. An out-of-range index throws `Invalid index N requested
    /// for <component>`.
    fn get_argument(&self, index: i64) -> Result<DecomposedPart, FormatError> {
        if index >= 0 && (index as usize) < self.args.len() {
            let arg = &self.args[index as usize];
            match arg {
                TranslatableArg::Component(component) => {
                    Ok(DecomposedPart::Component(component.clone()))
                }
                other => Ok(DecomposedPart::Text(other.to_string())),
            }
        } else {
            Err(FormatError::InvalidIndex)
        }
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

impl PartialEq for TranslatableContents {
    /// `TranslatableContents.equals(Object)` — compares only `key`, `fallback`,
    /// and `args` (`Arrays.equals`); the decomposition cache is derived state
    /// and not part of equality.
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.fallback == other.fallback && self.args == other.args
    }
}

impl std::fmt::Debug for TranslatableContents {
    /// Manual `Debug` (the `OnceLock` cache field has none); mirrors the value
    /// fields, like `Component`'s derived `Debug`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TranslatableContents")
            .field("key", &self.key)
            .field("fallback", &self.fallback)
            .field("args", &self.args)
            .finish()
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
