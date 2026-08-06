//! Port of `net.minecraft.network.chat.contents.SelectorContents`.
//!
//! Holds a selector source string (`CompilableString<EntitySelector>.source`)
//! and an optional `separator` component. Resolution is out of scope; `visit`
//! renders the selector source text (matching `SelectorContents.visit`, which
//! visits `this.selector.source()` directly).

use super::super::ComponentContents;
use crate::style::Style;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::{self as map_codec_mod, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::sync::Arc;

/// Port of `net.minecraft.network.chat.contents.SelectorContents`.
///
/// `separator` is a `Box<Component>` to break the value-type cycle
/// `Component` → `ComponentContents` → `SelectorContents` → `Component`
/// (Java holds an `Optional<Component>` reference). Callers get
/// `Option<&Component>` back.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectorContents {
    /// `CompilableString<EntitySelector>.source()` — the selector pattern text.
    selector: String,
    /// `Optional<Component> separator`.
    separator: Option<Box<crate::Component>>,
}

impl SelectorContents {
    /// `new SelectorContents(selector, separator)`.
    pub fn new(selector: String, separator: Option<crate::Component>) -> Self {
        SelectorContents {
            selector,
            separator: separator.map(Box::new),
        }
    }

    /// `SelectorContents.selector()`.
    pub fn selector(&self) -> &str {
        &self.selector
    }

    /// `SelectorContents.separator()`.
    pub fn separator(&self) -> Option<&crate::Component> {
        self.separator.as_deref()
    }

    /// `visit(ContentConsumer)` — visits the selector source text.
    pub fn visit_content<T>(&self, output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        output(&self.selector)
    }

    /// `visit(StyledContentConsumer, Style)`.
    pub fn visit_styled<T>(
        &self,
        output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        style: &Style,
    ) -> Option<T> {
        output(style, &self.selector)
    }

    /// `SelectorContents.MAP_CODEC` — `RecordCodecBuilder.mapCodec` over
    /// `selector` (deferred compilable string → plain string) and the optional
    /// `separator` component, lifted to the `ComponentContents` enum.
    pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ComponentContents, Ops>> {
        map_codec_mod::xmap(
            Arc::new(SelectorMapCodec {
                _ops: std::marker::PhantomData,
            }),
            Arc::new(|c: &SelectorContents| ComponentContents::Selector(c.clone())),
            Arc::new(|c: &ComponentContents| match c {
                ComponentContents::Selector(inner) => inner.clone(),
                _ => panic!("selector codec applied to non-selector contents"),
            }),
        )
    }
}

/// `SelectorContents.MAP_CODEC`.
struct SelectorMapCodec<Ops: DynamicOps + 'static> {
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for SelectorMapCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SelectorContentsMapCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for SelectorMapCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string("selector".to_string())]
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<SelectorContents, Ops> for SelectorMapCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<SelectorContents> {
        let selector = match input.get_string("selector") {
            Some(selector) => codec::string_codec::<Ops>().parse(ops, &selector),
            None => return DataResult::error("No key selector in MapLike".to_string()),
        };
        // `ComponentSerialization.CODEC.optionalFieldOf("separator")`.
        let separator = match input.get_string("separator") {
            Some(value) => crate::component_serialization::codec::<Ops>()
                .parse(ops, &value)
                .map(|c| Some(c.clone())),
            None => DataResult::success(None),
        };
        selector.flat_map(move |selector| {
            separator
                .map(move |separator| SelectorContents::new(selector.clone(), separator.clone()))
        })
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<SelectorContents, Ops> for SelectorMapCodec<Ops> {
    fn encode(
        &self,
        input: &SelectorContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        prefix.add_string_result(
            "selector",
            codec::string_codec::<Ops>().encode_start(ops, &input.selector),
        );
        if let Some(separator) = &input.separator {
            prefix.add_string_result(
                "separator",
                crate::component_serialization::codec::<Ops>()
                    .encode_start(ops, separator.as_ref()),
            );
        }
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<SelectorContents, Ops> for SelectorMapCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<SelectorContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &SelectorContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

impl std::fmt::Display for SelectorContents {
    /// `SelectorContents.toString()` = `pattern{selector}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "pattern{{{}}}", self.selector)
    }
}
