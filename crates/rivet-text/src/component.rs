//! Port of `net.minecraft.network.chat.Component` and its builder
//! `MutableComponent`.
//!
//! Java models `Component` as an interface with `MutableComponent` (the
//! concrete final builder) implementing it. The Rust port collapses the two
//! into one value type `Component` with a `MutableComponent` type alias, since
//! every value this crate produces is mutable and the interface's other
//! impls (`KeybindComponent`, `NbtComponent`, ... legacy classes) were removed
//! in 24w14a. The three fields — `contents`, `siblings`, `style` — are exactly
//! `MutableComponent`'s.
//!
//! STUB (epic #12): `visit`/`visitSelf` are not exposed as trait methods;
//! `get_string`/`flatten` walk the tree directly. Deferred: `getString(int
//! limit)`, `contains`, `toFlatList`, `getVisualOrderText`, the `Iterable`/
//! `stream` views, and the `translatable(...)` vararg factories
//! (`translatable(key, args...)`, `translatableWithFallback(key, fallback[,
//! args...])`, `translatableEscape`). `copy`/`plain_copy` are ported because
//! `ComponentSerialization.createFromList` needs them.

use crate::component_contents::ComponentContents;
use crate::contents::{
    KeybindContents, PlainTextContents, ScoreContents, ScoreName, SelectorContents,
    TranslatableContents,
};
use crate::style::Style;

/// Port of `net.minecraft.network.chat.Component` / `MutableComponent`.
///
/// `MutableComponent` is final and `Component` is an interface with a single
/// impl in current MC, so one struct models both. Field order matches
/// `MutableComponent`'s constructor `(contents, siblings, style)`. The derived
/// `PartialEq` compares all three fields; Java's `equals` compares the same
/// three (in the order contents, style, siblings), so value equality is
/// identical.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    /// `MutableComponent.contents`.
    contents: ComponentContents,
    /// `MutableComponent.siblings`.
    siblings: Vec<Component>,
    /// `MutableComponent.style`.
    style: Style,
}

/// `MutableComponent` — the Java builder type; the same value type as
/// `Component`.
pub type MutableComponent = Component;

impl Component {
    /// `MutableComponent.create(ComponentContents)`.
    pub fn create(contents: ComponentContents) -> MutableComponent {
        MutableComponent {
            contents,
            siblings: Vec::new(),
            style: Style::EMPTY,
        }
    }

    /// `new MutableComponent(contents, siblings, style)` — the
    /// `RecordCodecBuilder`/`copy` constructor.
    pub fn new(contents: ComponentContents, siblings: Vec<Component>, style: Style) -> Self {
        MutableComponent {
            contents,
            siblings,
            style,
        }
    }

    /// `Component.literal(String)`.
    pub fn literal(text: &str) -> MutableComponent {
        MutableComponent::create(ComponentContents::PlainText(PlainTextContents::create(
            text.to_owned(),
        )))
    }

    /// `Component.translatable(String)`.
    pub fn translatable(key: &str) -> MutableComponent {
        MutableComponent::create(ComponentContents::Translatable(TranslatableContents::new(
            key.to_owned(),
            None,
            Vec::new(),
        )))
    }

    /// `Component.empty()` — `MutableComponent.create(PlainTextContents.EMPTY)`.
    pub fn empty() -> MutableComponent {
        MutableComponent::create(ComponentContents::PlainText(PlainTextContents::EMPTY))
    }

    /// `Component.nullToEmpty(@Nullable String)` — `null` yields
    /// `CommonComponents.EMPTY`, otherwise `literal(text)`.
    pub fn null_to_empty(text: Option<&str>) -> MutableComponent {
        match text {
            Some(text) => Component::literal(text),
            None => Component::empty(),
        }
    }

    /// `Component.keybind(String)`.
    pub fn keybind(name: &str) -> MutableComponent {
        MutableComponent::create(ComponentContents::Keybind(KeybindContents::new(
            name.to_owned(),
        )))
    }

    /// `Component.score(String, String)` — the `Either.right` (plain name)
    /// form.
    pub fn score(name: &str, objective: &str) -> MutableComponent {
        MutableComponent::create(ComponentContents::Score(ScoreContents::new(
            ScoreName::Name(name.to_owned()),
            objective.to_owned(),
        )))
    }

    /// `Component.selector(String, Optional<Component>)` — the plain-source
    /// form.
    pub fn selector(pattern: &str, separator: Option<Component>) -> MutableComponent {
        MutableComponent::create(ComponentContents::Selector(SelectorContents::new(
            pattern.to_owned(),
            separator,
        )))
    }

    /// `Component.getContents()`.
    pub fn get_contents(&self) -> &ComponentContents {
        &self.contents
    }

    /// `Component.getStyle()`.
    pub fn get_style(&self) -> &Style {
        &self.style
    }

    /// `Component.getSiblings()`.
    pub fn get_siblings(&self) -> &[Component] {
        &self.siblings
    }

    /// `MutableComponent.setStyle(Style)`.
    pub fn set_style(&mut self, style: Style) -> &mut Self {
        self.style = style;
        self
    }

    /// `MutableComponent.withStyle(Style patch)` — `setStyle(patch.applyTo(
    /// this.style))`.
    pub fn with_style(mut self, patch: Style) -> MutableComponent {
        self.style = patch.apply_to(&self.style);
        self
    }

    /// `MutableComponent.withStyle(ChatFormatting)` — `setStyle(
    /// this.style.applyFormat(format))`.
    pub fn with_format(mut self, format: crate::ChatFormatting) -> MutableComponent {
        self.style = self.style.apply_format(format);
        self
    }

    /// `MutableComponent.withStyle(ChatFormatting...)`.
    pub fn with_formats(mut self, formats: &[crate::ChatFormatting]) -> MutableComponent {
        self.style = self.style.apply_formats(formats);
        self
    }

    /// `MutableComponent.append(Component)`.
    pub fn append_component(&mut self, component: Component) -> &mut Self {
        self.siblings.push(component);
        self
    }

    /// `MutableComponent.append(String)` — no-op for an empty string (Java
    /// returns `this` early).
    pub fn append_str(&mut self, text: &str) -> &mut Self {
        if !text.is_empty() {
            self.siblings.push(Component::literal(text));
        }
        self
    }

    /// `Component.getString()` — `FormattedText.getString()`: the plain text
    /// of this node then each sibling, in order.
    pub fn get_string(&self) -> String {
        let mut out = String::new();
        self.contents.visit_content(&mut |text| {
            out.push_str(text);
            None::<()>
        });
        for sibling in &self.siblings {
            out.push_str(&sibling.get_string());
        }
        out
    }

    /// `Component.tryCollapseToString()` — the plain text when this node is a
    /// bare `PlainTextContents` with no siblings and an empty style, else
    /// `None`.
    pub fn try_collapse_to_string(&self) -> Option<String> {
        if let ComponentContents::PlainText(text) = &self.contents
            && self.siblings.is_empty()
            && self.style.is_empty()
        {
            return Some(text.text().to_owned());
        }
        None
    }

    /// `Component.copy()` — shallow copy sharing nothing mutable.
    pub fn copy(&self) -> MutableComponent {
        MutableComponent {
            contents: self.contents.clone(),
            siblings: self.siblings.clone(),
            style: self.style.clone(),
        }
    }

    /// `Component.plainCopy()` — `MutableComponent.create(this.getContents())`.
    pub fn plain_copy(&self) -> MutableComponent {
        MutableComponent::create(self.contents.clone())
    }

    /// Test-support helper: flatten to `(text, style)` leaf pairs. Each node's
    /// own plain text (if non-empty) contributes one pair with the node's own
    /// style, then each sibling is flattened recursively. Used by
    /// `rivet-nbt`'s `TextComponentTagVisitor` tests to inspect per-leaf
    /// styling; not part of the Java `Component` surface. Unlike `toFlatList`
    /// (deferred), it does not apply parent-style inheritance — it reports the
    /// style each node carries directly, which is what the visitor's
    /// pre-styled leaves need.
    pub fn flatten(&self) -> Vec<(String, Style)> {
        let mut out = Vec::new();
        self.contents.visit_content(&mut |text| {
            if !text.is_empty() {
                out.push((text.to_owned(), self.style.clone()));
            }
            None::<()>
        });
        for sibling in &self.siblings {
            out.extend(sibling.flatten());
        }
        out
    }
}

impl std::fmt::Display for Component {
    /// `MutableComponent.toString()` — `contents` then `[style=..., siblings=
    /// ...]` when either is non-empty. Matches Java exactly (the `Style` and
    /// `List<Component>` displays).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.contents)?;
        let has_style = !self.style.is_empty();
        let has_siblings = !self.siblings.is_empty();
        if has_style || has_siblings {
            f.write_str("[")?;
            if has_style {
                f.write_str("style=")?;
                write!(f, "{}", self.style)?;
            }
            if has_style && has_siblings {
                f.write_str(", ")?;
            }
            if has_siblings {
                // Java appends `this.siblings` — a `List<Component>` whose
                // `toString()` is `[a, b]` (bracketed), not the bare elements.
                f.write_str("siblings=[")?;
                for (i, sibling) in self.siblings.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{}", sibling)?;
                }
                f.write_str("]")?;
            }
            f.write_str("]")?;
        }
        Ok(())
    }
}
