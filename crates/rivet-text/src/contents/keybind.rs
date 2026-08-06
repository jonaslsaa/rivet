//! Port of `net.minecraft.network.chat.contents.KeybindContents`.
//!
//! Holds a keybind `name` (e.g. `"key.forward"`). Resolution via
//! `KeybindResolver` is out of scope (issue #92); `visit` contributes nothing,
//! and `toString` is `keybind{name}`.

use super::super::ComponentContents;
use crate::style::Style;
use rivet_serialization::codec;
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::{DynamicOps, Keyable, MapLike, RecordBuilder};
use rivet_serialization::map_codec::{self as map_codec_mod, MapCodec};
use rivet_serialization::map_decoder::MapDecoder;
use rivet_serialization::map_encoder::MapEncoder;
use std::sync::Arc;

/// Port of `net.minecraft.network.chat.contents.KeybindContents`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeybindContents {
    name: String,
}

impl KeybindContents {
    /// `new KeybindContents(String name)`.
    pub fn new(name: String) -> Self {
        KeybindContents { name }
    }

    /// `KeybindContents.getName()`.
    pub fn get_name(&self) -> &str {
        &self.name
    }

    /// `visit(ContentConsumer)` — resolution via `KeybindResolver` is out of
    /// scope, so this contributes nothing (Java would visit the resolved key).
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

    /// `KeybindContents.MAP_CODEC` — `RecordCodecBuilder.mapCodec` with the
    /// single `fieldOf("keybind")`, lifted to the `ComponentContents` enum.
    pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ComponentContents, Ops>> {
        map_codec_mod::xmap(
            Arc::new(KeybindMapCodec {
                _ops: std::marker::PhantomData,
            }),
            Arc::new(|c: &KeybindContents| ComponentContents::Keybind(c.clone())),
            Arc::new(|c: &ComponentContents| match c {
                ComponentContents::Keybind(inner) => inner.clone(),
                _ => panic!("keybind codec applied to non-keybind contents"),
            }),
        )
    }
}

/// `KeybindContents.MAP_CODEC`.
struct KeybindMapCodec<Ops: DynamicOps + 'static> {
    _ops: std::marker::PhantomData<Ops>,
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for KeybindMapCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeybindContentsMapCodec")
    }
}

impl<Ops: DynamicOps + 'static> Keyable<Ops> for KeybindMapCodec<Ops> {
    fn keys(&self, ops: &Ops) -> Vec<Ops::Output> {
        vec![ops.create_string("keybind".to_string())]
    }
}

impl<Ops: DynamicOps + 'static> MapDecoder<KeybindContents, Ops> for KeybindMapCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<KeybindContents> {
        match input.get_string("keybind") {
            Some(name) => codec::string_codec::<Ops>()
                .parse(ops, &name)
                .map(|n| KeybindContents::new(n.clone())),
            None => DataResult::error("No key keybind in MapLike".to_string()),
        }
    }
}

impl<Ops: DynamicOps + 'static> MapEncoder<KeybindContents, Ops> for KeybindMapCodec<Ops> {
    fn encode(
        &self,
        input: &KeybindContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        prefix.add_string_result(
            "keybind",
            codec::string_codec::<Ops>().encode_start(ops, &input.name),
        );
    }
}

impl<Ops: DynamicOps + 'static> MapCodec<KeybindContents, Ops> for KeybindMapCodec<Ops> {
    fn decode(&self, ops: &Ops, input: &dyn MapLike<Ops::Output>) -> DataResult<KeybindContents> {
        MapDecoder::decode(self, ops, input)
    }

    fn encode(
        &self,
        input: &KeybindContents,
        ops: &Ops,
        prefix: &mut dyn RecordBuilder<Output = Ops::Output>,
    ) {
        MapEncoder::encode(self, input, ops, prefix)
    }
}

impl std::fmt::Display for KeybindContents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "keybind{{{}}}", self.name)
    }
}
