//! Port of `net.minecraft.network.chat.contents.PlainTextContents`.
//!
//! A `PlainTextContents` is either the `EMPTY` singleton (empty text) or a
//! `LiteralContents` holding a `String`. The factory `create(text)` returns
//! `EMPTY` for an empty string. Equality is by text (the record's `String`
//! equals), so the `EMPTY` singleton is equal to a `LiteralContents("")` that
//! can never exist. The `visit` methods pass the text to the consumer.

use super::super::ComponentContents;
use crate::style::Style;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::{self as map_codec_mod, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::sync::Arc;

/// Port of `net.minecraft.network.chat.contents.PlainTextContents`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlainTextContents {
    /// `PlainTextContents.EMPTY`.
    Empty,
    /// `PlainTextContents.LiteralContents(String)`.
    Literal(String),
}

impl PlainTextContents {
    /// `PlainTextContents.EMPTY`.
    pub const EMPTY: PlainTextContents = PlainTextContents::Empty;

    /// `PlainTextContents.create(String)` — `text.isEmpty() ? EMPTY :
    /// LiteralContents`.
    pub fn create(text: String) -> PlainTextContents {
        if text.is_empty() {
            PlainTextContents::Empty
        } else {
            PlainTextContents::Literal(text)
        }
    }

    /// `PlainTextContents.text()` — the text, `""` for `EMPTY`.
    pub fn text(&self) -> &str {
        match self {
            PlainTextContents::Empty => "",
            PlainTextContents::Literal(text) => text,
        }
    }

    /// `LiteralContents.visit(ContentConsumer)` — `EMPTY` visits nothing.
    pub fn visit_content<T>(&self, output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        match self {
            PlainTextContents::Empty => None,
            PlainTextContents::Literal(text) => output(text),
        }
    }

    /// `LiteralContents.visit(StyledContentConsumer, Style)` — `EMPTY` visits
    /// nothing.
    pub fn visit_styled<T>(
        &self,
        output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        style: &Style,
    ) -> Option<T> {
        match self {
            PlainTextContents::Empty => None,
            PlainTextContents::Literal(text) => output(style, text),
        }
    }

    /// `PlainTextContents.MAP_CODEC` — `RecordCodecBuilder.mapCodec` with the
    /// single `fieldOf("text")` decoded via `PlainTextContents::create`,
    /// lifted to the `ComponentContents` enum (Java's
    /// `MapCodec<? extends ComponentContents>`).
    pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ComponentContents, Ops>> {
        map_codec_mod::xmap(
            Arc::new(PlainTextMapCodec {
                _ops: std::marker::PhantomData,
            }),
            Arc::new(|c: &PlainTextContents| ComponentContents::PlainText(c.clone())),
            Arc::new(|c: &ComponentContents| match c {
                ComponentContents::PlainText(inner) => inner.clone(),
                _ => panic!("plain-text codec applied to non-plain-text contents"),
            }),
        )
    }
}

/// `PlainTextContents.MAP_CODEC`.
struct PlainTextMapCodec<Ops: DynamicOps + 'static> {
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for PlainTextMapCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PlainTextContentsMapCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for PlainTextMapCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string("text".to_string())]
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<PlainTextContents, Ops> for PlainTextMapCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<PlainTextContents> {
        match input.get_string("text") {
            Some(text) => codec::string_codec::<Ops>()
                .parse(ops, &text)
                .map(|t| PlainTextContents::create(t.clone())),
            None => DataResult::error("No key text in MapLike".to_string()),
        }
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<PlainTextContents, Ops> for PlainTextMapCodec<Ops> {
    fn encode(
        &self,
        input: &PlainTextContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        prefix.add_string_result(
            "text",
            codec::string_codec::<Ops>().encode_start(ops, &input.text().to_string()),
        );
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<PlainTextContents, Ops> for PlainTextMapCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<PlainTextContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &PlainTextContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

impl std::fmt::Display for PlainTextContents {
    /// `LiteralContents.toString()` = `"literal{text}"`; `EMPTY` =
    /// `"empty"`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlainTextContents::Empty => f.write_str("empty"),
            PlainTextContents::Literal(text) => write!(f, "literal{{{}}}", text),
        }
    }
}
