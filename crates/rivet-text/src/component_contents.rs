//! Port of `net.minecraft.network.chat.ComponentContents`.
//!
//! Java models contents as an interface with a `codec()` returning the
//! concrete `MapCodec<? extends ComponentContents>`; the Rust port models the
//! closed set of this slice's contents as an enum with a visitor surface and a
//! per-variant `MapCodec` lookup used by `ComponentSerialization`'s dispatch.
//!
//! Java's `visit` returns `Optional<T>` and stops at the first `Some`; the
//! port's `visit_content`/`visit_styled` take the consumer as `&mut dyn FnMut`,
//! returning `Option<T>` exactly like Java. `resolve` (translation/selector
//! resolution) is issue #92 scope and not part of this slice.

use crate::contents::{
    KeybindContents, PlainTextContents, ScoreContents, SelectorContents, TranslatableContents,
};
use crate::style::Style;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use std::sync::Arc;

/// Port of `net.minecraft.network.chat.ComponentContents`.
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentContents {
    /// `PlainTextContents` — a literal string or the `EMPTY` singleton.
    PlainText(PlainTextContents),
    /// `TranslatableContents` — a translation key + args.
    Translatable(TranslatableContents),
    /// `KeybindContents` — a keybind name.
    Keybind(KeybindContents),
    /// `ScoreContents` — a scoreboard objective read.
    Score(ScoreContents),
    /// `SelectorContents` — an entity selector (source + separator).
    Selector(SelectorContents),
}

impl ComponentContents {
    /// `ComponentContents.visit(FormattedText.ContentConsumer<T>)`.
    ///
    /// Calls `output` with the plain text this contents contributes, returning
    /// the consumer's short-circuit value if it returns `Some`. Contents that
    /// resolve asynchronously (translatable/keybind/score) contribute nothing.
    pub fn visit_content<T>(&self, output: &mut dyn FnMut(&str) -> Option<T>) -> Option<T> {
        match self {
            ComponentContents::PlainText(c) => c.visit_content(output),
            ComponentContents::Translatable(c) => c.visit_content(output),
            ComponentContents::Keybind(c) => c.visit_content(output),
            ComponentContents::Score(c) => c.visit_content(output),
            ComponentContents::Selector(c) => c.visit_content(output),
        }
    }

    /// `ComponentContents.visit(FormattedText.StyledContentConsumer<T>,
    /// Style)`.
    pub fn visit_styled<T>(
        &self,
        output: &mut dyn FnMut(&Style, &str) -> Option<T>,
        style: &Style,
    ) -> Option<T> {
        match self {
            ComponentContents::PlainText(c) => c.visit_styled(output, style),
            ComponentContents::Translatable(c) => c.visit_styled(output, style),
            ComponentContents::Keybind(c) => c.visit_styled(output, style),
            ComponentContents::Score(c) => c.visit_styled(output, style),
            ComponentContents::Selector(c) => c.visit_styled(output, style),
        }
    }

    /// `ComponentContents.codec()` — the `MapCodec<? extends ComponentContents>`
    /// for this variant, used by the dispatch in `ComponentSerialization`.
    pub fn codec<Ops: DynamicOps + 'static>(&self) -> Arc<dyn MapCodec<ComponentContents, Ops>> {
        match self {
            ComponentContents::PlainText(_) => PlainTextContents::map_codec(),
            ComponentContents::Translatable(_) => TranslatableContents::map_codec(),
            ComponentContents::Keybind(_) => KeybindContents::map_codec(),
            ComponentContents::Score(_) => ScoreContents::map_codec(),
            ComponentContents::Selector(_) => SelectorContents::map_codec(),
        }
    }
}

impl std::fmt::Display for ComponentContents {
    /// `ComponentContents.toString()` — the concrete type's `toString` (used
    /// by `Component.toString`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ComponentContents::PlainText(c) => write!(f, "{}", c),
            ComponentContents::Translatable(c) => write!(f, "{}", c),
            ComponentContents::Keybind(c) => write!(f, "{}", c),
            ComponentContents::Score(c) => write!(f, "{}", c),
            ComponentContents::Selector(c) => write!(f, "{}", c),
        }
    }
}
