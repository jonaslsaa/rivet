// STUB(mc.nbt.text) — minimal port of `net.minecraft.network.chat.Component` /
// `MutableComponent` for `TextComponentTagVisitor`. Owned by the
// net.minecraft.network.chat package (rivet-text); replaced by the real port.

use crate::style::Style;

/// Port of `net.minecraft.network.chat.Component`. A literal text node plus an
/// ordered list of sibling components and a style — exactly the shape
/// `MutableComponent` keeps in Java.
#[derive(Clone, Debug, PartialEq)]
pub struct Component {
    text: String,
    style: Style,
    siblings: Vec<Component>,
}

/// `MutableComponent` — the Java builder type; in this stub it is the same
/// value type as `Component` (Java: `MutableComponent implements Component`).
pub type MutableComponent = Component;

impl Component {
    /// `Component.literal(text)`.
    pub fn literal(text: &str) -> MutableComponent {
        MutableComponent {
            text: text.to_owned(),
            style: Style::EMPTY,
            siblings: Vec::new(),
        }
    }

    /// `Component.empty()`.
    pub fn empty() -> MutableComponent {
        MutableComponent::literal("")
    }

    /// `Component.getStyle()`.
    pub fn get_style(&self) -> &Style {
        &self.style
    }

    /// `Component.getSiblings()`.
    pub fn get_siblings(&self) -> &[Component] {
        &self.siblings
    }

    /// `MutableComponent.setStyle(style)`.
    pub fn set_style(&mut self, style: Style) -> &mut Self {
        self.style = style;
        self
    }

    /// `MutableComponent.withStyle(Style)` — applies the patch to the current
    /// style (Java: `withStyle(Style patch)` = `setStyle(patch.applyTo(this.style))`).
    pub fn with_style(mut self, patch: Style) -> MutableComponent {
        self.style = self.style.apply_to(&patch);
        self
    }

    /// `MutableComponent.append(Component)`.
    pub fn append_component(&mut self, component: Component) -> &mut Self {
        self.siblings.push(component);
        self
    }

    /// `MutableComponent.append(String)`.
    pub fn append_str(&mut self, text: &str) -> &mut Self {
        if text.is_empty() {
            return self;
        }
        self.siblings.push(Component::literal(text));
        self
    }

    /// `Component.getString()` — concatenated text of this node and all
    /// descendants. Matches Java's `FormattedText.getString()` traversal
    /// (contents first, then each sibling).
    pub fn get_string(&self) -> String {
        let mut out = String::new();
        out.push_str(&self.text);
        for sibling in &self.siblings {
            out.push_str(&sibling.get_string());
        }
        out
    }

    /// Test-support helper on the stub: flatten to (text, style) leaf pairs for
    /// style inspection. Not part of the Java `Component`/`MutableComponent`
    /// surface — the real rivet-text port drops this (tests use it from
    /// `rivet-nbt`).
    pub fn flatten(&self) -> Vec<(String, Style)> {
        let mut out = Vec::new();
        if !self.text.is_empty() {
            out.push((self.text.clone(), self.style));
        }
        for sibling in &self.siblings {
            out.extend(sibling.flatten());
        }
        out
    }
}
