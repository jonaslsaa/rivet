//! Java-grounded tests for the `net.minecraft.network.chat` value model and
//! `ComponentSerialization` JSON codec.
//!
//! Expected values are taken from the Paper source of truth
//! (`working/Paper/paper-server/src/minecraft/java/net/minecraft/network/chat/`):
//! `toString`/`equals` contracts from `MutableComponent`/`Style`/
//! `TranslatableContents`, `TextColor.parseColor` messages, and the exact JSON
//! shapes `ComponentSerialization.CODEC` produces/accepts under `JsonOps`.

use crate::ChatFormatting;
use crate::component::Component;
use crate::component_contents::ComponentContents;
use crate::contents::{
    KeybindContents, PlainTextContents, ScoreContents, ScoreName, SelectorContents,
    TranslatableArg, TranslatableContents,
};
use crate::numbers::NumberFormat;
use crate::style::Style;
use crate::text_color::TextColor;
use rivet_serialization::codec::Codec;
use rivet_serialization::json_ops::JsonOps;

/// `ComponentSerialization.CODEC` as used in the tests below.
fn component_codec() -> std::sync::Arc<dyn Codec<Component, JsonOps>> {
    crate::component_serialization::codec()
}

// ---------------------------------------------------------------------------
// TextColor
// ---------------------------------------------------------------------------

#[test]
fn text_color_named_values_match_java() {
    assert_eq!(TextColor::BLACK.get_value(), 0x000000);
    assert_eq!(TextColor::DARK_RED.get_value(), 0xAA0000);
    assert_eq!(TextColor::GOLD.get_value(), 0xFFAA00);
    assert_eq!(TextColor::WHITE.get_value(), 0xFFFFFF);
    assert_eq!(TextColor::RED.get_value(), 0xFF5555);
}

#[test]
fn text_color_equality_is_value_only() {
    // Java `TextColor.equals` compares `value` only, so a named color equals
    // an un-named `fromRgb` with the same RGB.
    assert_eq!(TextColor::RED, TextColor::from_rgb(0xFF5555));
    assert_eq!(TextColor::BLACK, TextColor::from_rgb(0));
    assert_ne!(TextColor::RED, TextColor::from_rgb(0xFF0000));

    // Value-equal colors hash equal (Java's `Objects.hash(value, name)` breaks
    // the contract; the port hashes the value only).
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    fn hash_of(color: TextColor) -> u64 {
        let mut hasher = DefaultHasher::new();
        color.hash(&mut hasher);
        hasher.finish()
    }
    assert_eq!(
        hash_of(TextColor::RED),
        hash_of(TextColor::from_rgb(0xFF5555))
    );

    // A round-trip decode of a named color compares equal to the original.
    let decoded = TextColor::parse_color("red").unwrap();
    assert_eq!(decoded, TextColor::RED);
}

#[test]
fn text_color_serialize_uses_name_or_rgb() {
    // `TextColor.serialize()` returns the name when present.
    assert_eq!(TextColor::RED.serialize(), "red");
    assert_eq!(TextColor::WHITE.serialize(), "white");
    // Custom colors serialize as uppercase `#RRGGBB` (`String.format("#%06X")`).
    assert_eq!(TextColor::from_rgb(0x00_AA_FF).serialize(), "#00AAFF");
    assert_eq!(TextColor::from_rgb(0x12_34_56).serialize(), "#123456");
}

#[test]
fn text_color_parse_round_trip() {
    assert_eq!(
        TextColor::parse_color("#FFFFFF").unwrap(),
        TextColor::from_rgb(0xFFFFFF)
    );
    assert_eq!(
        TextColor::parse_color("#00aaFF").unwrap(),
        TextColor::from_rgb(0x00AAFF)
    );
    // Named colors parse to the named value.
    assert_eq!(TextColor::parse_color("red").unwrap(), TextColor::RED);
    assert_eq!(TextColor::parse_color("gold").unwrap(), TextColor::GOLD);
}

#[test]
fn text_color_parse_errors_match_java_messages() {
    // Java `TextColor.parseColor` exact error messages.
    assert_eq!(
        TextColor::parse_color("#FFFFFFF").unwrap_err(),
        "Color value out of range: #FFFFFFF"
    );
    assert_eq!(
        TextColor::parse_color("#zz").unwrap_err(),
        "Invalid color value: #zz"
    );
    assert_eq!(
        TextColor::parse_color("notacolor").unwrap_err(),
        "Invalid color name: notacolor"
    );
}

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

#[test]
fn style_empty_collapses_to_constant() {
    // `Style.withX(null)` on an empty style collapses back to `EMPTY`
    // (`checkEmptyAfterChange`). Java returns the interned `Style.EMPTY`
    // singleton (`this`), observable by `==` identity; Rust models `Style` as a
    // value type with no singleton to compare by pointer, so the collapse is
    // asserted by value equality (the meaningful Rust translation).
    let cleared = Style::EMPTY.with_bold(None);
    assert_eq!(cleared, Style::EMPTY);
    assert!(cleared.is_empty());
}

#[test]
fn style_with_color_and_formats() {
    let style = Style::EMPTY.with_color_rgb(0x00AAFF);
    assert_eq!(style.get_color().unwrap().get_value(), 0x00AAFF);

    let bold = Style::EMPTY.with_bold(Some(true));
    assert!(bold.is_bold());
    assert!(!bold.is_italic());

    let named = Style::EMPTY.with_color_format(ChatFormatting::Red);
    assert_eq!(named.get_color().unwrap(), &TextColor::RED);

    // Java `Style.withColor` early-returns `this` when
    // `Objects.equals(this.color, color)`; value-equality makes a named color
    // and its un-named `fromRgb` twin a no-op, preserving the carried name.
    let named = Style::EMPTY.with_color_format(ChatFormatting::Red);
    let restyled = named.with_color(Some(TextColor::from_rgb(0xFF5555)));
    assert_eq!(restyled, named);
    assert_eq!(restyled.get_color().unwrap().serialize(), "red");
}

#[test]
fn style_apply_to_merges_with_parent() {
    // `Style.applyTo(other)`: this style's non-null fields win over `other`.
    let parent = Style::EMPTY.with_color_rgb(0x0000FF).with_bold(Some(true));
    let child = Style::EMPTY
        .with_color_rgb(0xFF0000)
        .with_italic(Some(true));
    let merged = child.apply_to(&parent);
    // Child's color wins; parent's bold survives; child's italic survives.
    assert_eq!(merged.get_color().unwrap().get_value(), 0xFF0000);
    assert!(merged.is_bold());
    assert!(merged.is_italic());
}

#[test]
fn style_apply_format_clear_and_apply_formats() {
    // `Style.applyFormats(ChatFormatting...)`.
    let multi = Style::EMPTY
        .with_color_format(ChatFormatting::Red)
        .apply_formats(&[ChatFormatting::Bold, ChatFormatting::Underline]);
    assert_eq!(multi.get_color().unwrap(), &TextColor::RED);
    assert!(multi.is_bold());
    assert!(multi.is_underlined());

    // RESET anywhere clears to `EMPTY`.
    let reset = Style::EMPTY
        .with_bold(Some(true))
        .apply_formats(&[ChatFormatting::Reset]);
    assert!(reset.is_empty());
}

#[test]
fn style_display_matches_java_field_order() {
    // Java `Style.toString()` renders `{color=...,bold,!italic,...}` with a
    // `!` prefix on false flags and the exact field order.
    let style = Style::EMPTY
        .with_color_format(ChatFormatting::Red)
        .with_bold(Some(true))
        .with_italic(Some(false));
    assert_eq!(style.to_string(), "{color=red,bold,!italic}");

    let shadow = Style::EMPTY.with_shadow_color(0x123456);
    assert_eq!(shadow.to_string(), "{shadowColor=1193046}");

    let insertion = Style::EMPTY.with_insertion(Some("hello".to_string()));
    assert_eq!(insertion.to_string(), "{insertion=hello}");
}

// ---------------------------------------------------------------------------
// Contents
// ---------------------------------------------------------------------------

#[test]
fn plain_text_contents_visit_and_display() {
    // `LiteralContents.toString()` = `"literal{text}"`; `EMPTY` = `"empty"`.
    let literal = PlainTextContents::create("hello".to_string());
    assert_eq!(literal.to_string(), "literal{hello}");

    let mut visited = Vec::new();
    literal.visit_content(&mut |text| {
        visited.push(text.to_owned());
        None::<()>
    });
    assert_eq!(visited, vec!["hello".to_string()]);

    // `create("")` returns the EMPTY singleton (equal, visits nothing).
    let empty = PlainTextContents::create(String::new());
    assert_eq!(empty, PlainTextContents::EMPTY);
    assert_eq!(empty.to_string(), "empty");
    assert_eq!(empty.text(), "");
}

#[test]
fn component_null_to_empty_and_literal() {
    // `Component.nullToEmpty(null)` -> `CommonComponents.EMPTY`.
    let empty = Component::null_to_empty(None);
    assert_eq!(
        empty.get_contents(),
        &ComponentContents::PlainText(PlainTextContents::EMPTY)
    );
    assert_eq!(empty.get_string(), "");

    // `Component.nullToEmpty(text)` -> `literal(text)`.
    let literal = Component::null_to_empty(Some("hello"));
    assert_eq!(literal.get_string(), "hello");
    assert_eq!(literal, Component::literal("hello"));
}

#[test]
fn keybind_contents_display() {
    let keybind = KeybindContents::new("key.forward".to_string());
    assert_eq!(keybind.get_name(), "key.forward");
    assert_eq!(keybind.to_string(), "keybind{key.forward}");
}

#[test]
fn score_contents_display() {
    let score = ScoreContents::new(
        ScoreName::Name("Player".to_string()),
        "objective".to_string(),
    );
    assert_eq!(score.name(), &ScoreName::Name("Player".to_string()));
    assert_eq!(score.objective(), "objective");
    // Java `ScoreContents.toString()` interpolates the `name` Either directly:
    // `"score{name='" + this.name + "', objective='" + this.objective + "'}"`
    // where `Either.right(...).toString()` is `Right[Player]`.
    assert_eq!(
        score.to_string(),
        "score{name='Right[Player]', objective='objective'}"
    );

    // The deferred selector variant renders `Left[<source>]` (the Java
    // `CompilableString.toString` is the raw selector source).
    let selector = ScoreContents::new(
        ScoreName::Selector("@p".to_string()),
        "objective".to_string(),
    );
    assert_eq!(
        selector.to_string(),
        "score{name='Left[@p]', objective='objective'}"
    );
}

#[test]
fn selector_contents_display_and_separator() {
    let selector = SelectorContents::new("@p".to_string(), Some(Component::literal("separator")));
    assert_eq!(selector.selector(), "@p");
    assert_eq!(
        selector.separator().map(|c| c.get_string()),
        Some("separator".to_string())
    );
    assert_eq!(selector.to_string(), "pattern{@p}");
}

#[test]
fn translatable_contents_display_matches_java() {
    // Java `TranslatableContents.toString()`:
    // `translation{key='K'[, fallback='F'], args=[...]}`.
    let no_args = TranslatableContents::new("key.tip".to_string(), None, Vec::new());
    assert_eq!(no_args.to_string(), "translation{key='key.tip', args=[]}");

    let with_fallback = TranslatableContents::new(
        "key.tip".to_string(),
        Some("fallback".to_string()),
        Vec::new(),
    );
    assert_eq!(
        with_fallback.to_string(),
        "translation{key='key.tip', fallback='fallback', args=[]}"
    );

    let with_args = TranslatableContents::new(
        "key.with".to_string(),
        None,
        vec![
            TranslatableArg::String("a".to_string()),
            TranslatableArg::Number(42),
        ],
    );
    // `Arrays.toString(Object[])` uses `String.valueOf` per element: no quotes
    // around the string arg.
    assert_eq!(
        with_args.to_string(),
        "translation{key='key.with', args=[a, 42]}"
    );
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

#[test]
fn component_literal_and_get_string() {
    let component = Component::literal("hello");
    assert_eq!(component.get_string(), "hello");
    assert_eq!(
        component.get_contents(),
        &ComponentContents::PlainText(PlainTextContents::create("hello".to_string()))
    );
    assert!(component.get_style().is_empty());
    assert!(component.get_siblings().is_empty());
}

#[test]
fn component_append_and_get_string() {
    let mut component = Component::literal("a");
    component.append_str("b");
    component.append_component(Component::literal("c"));
    assert_eq!(component.get_string(), "abc");
    assert_eq!(component.get_siblings().len(), 2);
}

#[test]
fn component_with_style_and_display() {
    // `MutableComponent.toString()`: contents, then `[style=..., siblings=...]`.
    let styled = Component::literal("x").with_style(Style::EMPTY.with_bold(Some(true)));
    assert_eq!(styled.to_string(), "literal{x}[style={bold}]");

    let mut with_sibling = Component::literal("a");
    with_sibling.append_component(Component::literal("b"));
    assert_eq!(
        with_sibling.to_string(),
        "literal{a}[siblings=[literal{b}]]"
    );
}

#[test]
fn component_try_collapse_to_string() {
    // `tryCollapseToString` — a bare plain-text node with empty style/siblings.
    let bare = Component::literal("hello");
    assert_eq!(bare.try_collapse_to_string().as_deref(), Some("hello"));

    // A styled literal cannot collapse.
    let styled = Component::literal("x").with_style(Style::EMPTY.with_bold(Some(true)));
    assert_eq!(styled.try_collapse_to_string(), None);

    // An empty plain-text node (`EMPTY`) collapses to `""` (Java returns the
    // empty string — `PlainTextContents.text()` is `""`).
    let empty = Component::empty();
    assert_eq!(empty.try_collapse_to_string().as_deref(), Some(""));
}

#[test]
fn component_copy_and_plain_copy() {
    let component = Component::literal("x").with_style(Style::EMPTY.with_bold(Some(true)));
    let copied = component.copy();
    assert_eq!(copied, component);
    assert_eq!(copied.get_style(), component.get_style());

    // `plainCopy()` drops style/siblings.
    let plain = component.plain_copy();
    assert_eq!(plain.get_contents(), component.get_contents());
    assert!(plain.get_style().is_empty());
    assert!(plain.get_siblings().is_empty());
}

// ---------------------------------------------------------------------------
// ComponentSerialization JSON codec
// ---------------------------------------------------------------------------

#[test]
fn component_json_string_round_trip() {
    // A bare literal encodes to a JSON string and parses back.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let literal = Component::literal("hello");
    let encoded = codec
        .encode_start(&ops, &literal)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!("hello"));

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, literal);
}

#[test]
fn component_json_typed_map_round_trip() {
    // Under non-compressed `JsonOps`, `orCompressed` encodes through
    // `StrictEither`'s fuzzy half, which writes the contents codec's keys
    // directly (no `type` discriminator), and `Style.Serializer.MAP_CODEC`
    // merges the style keys flat into the same record. Java emits
    // `{"translate":"key.tip","bold":true}`.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let component =
        Component::translatable("key.tip").with_style(Style::EMPTY.with_bold(Some(true)));
    let encoded = codec
        .encode_start(&ops, &component)
        .get_or_throw("encode")
        .clone();
    assert_eq!(
        encoded,
        serde_json::json!({
            "translate": "key.tip",
            "bold": true
        })
    );

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, component);
}

#[test]
fn component_json_text_contents_shape() {
    // `{"type":"text","text":"hi"}` is the legacy full form of a literal.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();
    let input = serde_json::json!({"type": "text", "text": "hi"});
    let decoded = codec.parse(&ops, &input).get_or_throw("parse").clone();
    assert_eq!(decoded, Component::literal("hi"));

    // Encoding that literal back produces the string form (Java collapses the
    // plain-text contents).
    let encoded = codec
        .encode_start(&ops, &decoded)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!("hi"));
}

#[test]
fn component_json_extra_siblings_round_trip() {
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let mut component = Component::literal("a");
    component.append_component(Component::literal("b"));

    // Java: a literal with siblings cannot `tryCollapseToString`, so it encodes
    // via the full record: contents (`{"text":"a"}`) merged flat with the
    // `extra` sibling list — no `type`, no brackets.
    let encoded = codec
        .encode_start(&ops, &component)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({"text": "a", "extra": ["b"]}));

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, component);
}

#[test]
fn component_json_nested_list_round_trip() {
    // Nested component in the sibling list round-trips through the full codec.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let mut outer = Component::literal("x");
    let mut inner = Component::literal("y");
    inner.append_component(Component::literal("z"));
    outer.append_component(inner);

    let encoded = codec
        .encode_start(&ops, &outer)
        .get_or_throw("encode")
        .clone();
    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, outer);
}

#[test]
fn component_json_selector_and_separator_round_trip() {
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let selector = Component::selector("@p", Some(Component::literal(",")));
    // Java: `SelectorContents.MAP_CODEC` is `{selector, separator?}` with no
    // `type`; the separator literal collapses to the string form under JsonOps.
    let encoded = codec
        .encode_start(&ops, &selector)
        .get_or_throw("encode")
        .clone();
    assert_eq!(
        encoded,
        serde_json::json!({
            "selector": "@p",
            "separator": ","
        })
    );

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, selector);
}

#[test]
fn component_json_keybind_round_trip() {
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let keybind = Component::keybind("key.forward");
    // Java: `KeybindContents.MAP_CODEC` is just `{keybind}` — no `type`.
    let encoded = codec
        .encode_start(&ops, &keybind)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({"keybind": "key.forward"}));

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, keybind);
}

#[test]
fn component_json_score_round_trip() {
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let score = Component::score("Player", "objective");
    // Java: `ScoreContents.MAP_CODEC = INNER_CODEC.fieldOf("score")` — the
    // `{name, objective}` inner record under a `"score"` key, no `type`.
    let encoded = codec
        .encode_start(&ops, &score)
        .get_or_throw("encode")
        .clone();
    assert_eq!(
        encoded,
        serde_json::json!({
            "score": {"name": "Player", "objective": "objective"}
        })
    );

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, score);
}

#[test]
fn component_json_empty_extra_field_is_omitted() {
    // `optionalFieldOf("extra", List.of())`: encoding a component with no
    // siblings must NOT write an `extra` key (Java `OptionalFieldCodec` writes
    // only when present). This is observable on the full-record encode path —
    // a plain literal with no siblings collapses to the string form, so use a
    // styled literal (cannot collapse) and assert no `extra` key appears.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let styled = Component::literal("a").with_style(Style::EMPTY.with_bold(Some(true)));
    let encoded = codec
        .encode_start(&ops, &styled)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({"text": "a", "bold": true}));

    // A full typed object with a present-but-empty `extra` array is rejected
    // by `nonEmptyList` ("List must have contents").
    let empty_extra = serde_json::json!({
        "type": "text",
        "text": "hi",
        "extra": []
    });
    let result = codec.parse(&ops, &empty_extra);
    assert!(result.result().is_none(), "empty extra list must fail");
}

#[test]
fn component_json_typed_dispatch_forms() {
    // The `{"type": ...}` legacy full forms route through the KeyDispatchCodec
    // discriminator (StrictEither's typed half). Every registered content type
    // must decode from its typed form and encode back to it.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    let translatable = serde_json::json!({
        "type": "translatable",
        "translate": "key.tip",
        "with": [42]
    });
    let decoded = codec
        .parse(&ops, &translatable)
        .get_or_throw("parse")
        .clone();
    match decoded.get_contents() {
        ComponentContents::Translatable(contents) => {
            assert_eq!(contents.get_key(), "key.tip");
            assert_eq!(contents.get_args(), &[TranslatableArg::Number(42)]);
        }
        other => panic!("expected translatable, got {other:?}"),
    }

    let score = serde_json::json!({
        "type": "score",
        "score": {"name": "Player", "objective": "obj"}
    });
    let decoded = codec.parse(&ops, &score).get_or_throw("parse").clone();
    assert_eq!(decoded, Component::score("Player", "obj"));

    let keybind = serde_json::json!({"type": "keybind", "keybind": "key.forward"});
    let decoded = codec.parse(&ops, &keybind).get_or_throw("parse").clone();
    assert_eq!(decoded, Component::keybind("key.forward"));

    let selector = serde_json::json!({"type": "selector", "selector": "@p"});
    let decoded = codec.parse(&ops, &selector).get_or_throw("parse").clone();
    assert_eq!(decoded, Component::selector("@p", None));
}

#[test]
fn component_json_translatable_lenient_fallback() {
    // Java `TranslatableContents.MAP_CODEC` uses
    // `Codec.STRING.lenientOptionalFieldOf("fallback")`: a present `fallback`
    // that the string codec rejects (here a JSON number) decodes to
    // `Optional.empty()` — it must NOT fail the whole decode.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();
    let input = serde_json::json!({
        "translate": "key.tip",
        "fallback": 42
    });
    let decoded = codec.parse(&ops, &input).get_or_throw("parse").clone();
    // `key.tip` is not a real key, so `getOrDefault` returns the key itself
    // (`Language.getOrDefault(id, id)`); `get_string` resolves the translatable
    // via the locale and yields `"key.tip"`.
    assert_eq!(decoded.get_string(), "key.tip");
    match decoded.get_contents() {
        ComponentContents::Translatable(contents) => assert_eq!(contents.get_fallback(), None),
        other => panic!("expected translatable, got {other:?}"),
    }
}

#[test]
fn component_json_translatable_args_round_trip() {
    // Translatable with primitive + component args, exercised through the
    // `ARG_CODEC` either (string/number on the primitive side, literal string
    // collapsed to a string on the component side).
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();

    // Rebuild via ComponentContents to attach args (translatable factory takes
    // none; use the raw contents constructor).
    let contents = TranslatableContents::new(
        "key.with".to_string(),
        None,
        vec![
            TranslatableArg::String("a".to_string()),
            TranslatableArg::Number(42),
        ],
    );
    let component = Component::new(
        ComponentContents::Translatable(contents),
        Vec::new(),
        Style::EMPTY,
    );

    // Java: `TranslatableContents.MAP_CODEC` writes `{translate, fallback?,
    // with?}` flat — no `type`.
    let encoded = codec
        .encode_start(&ops, &component)
        .get_or_throw("encode")
        .clone();
    assert_eq!(
        encoded,
        serde_json::json!({
            "translate": "key.with",
            "with": ["a", 42]
        })
    );

    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, component);
}

// ---------------------------------------------------------------------------
// network.chat.numbers NumberFormat
// ---------------------------------------------------------------------------

#[test]
fn number_format_formats_score_values_like_java() {
    // `BlankFormat.format` → `Component.empty()`.
    assert_eq!(NumberFormat::Blank.format(42), Component::empty());
    assert_eq!(NumberFormat::Blank.format(-7), Component::empty());

    // `StyledFormat.format` → `Component.literal(Integer.toString(value))
    // .withStyle(style)`.
    let styled = NumberFormat::Styled(Style::EMPTY.with_bold(Some(true)));
    let rendered = styled.format(42);
    assert_eq!(rendered.get_string(), "42");
    assert_eq!(rendered.get_style(), &Style::EMPTY.with_bold(Some(true)));
    // Java `Integer.toString` has no sign/leading-zero surprises for negatives.
    assert_eq!(styled.format(-7).get_string(), "-7");
    // `StyledFormat.NO_STYLE` renders plain text.
    assert_eq!(
        NumberFormat::Styled(Style::EMPTY).format(0).get_string(),
        "0"
    );

    // `FixedFormat.format` → `value.copy()` (a fresh equal component).
    let fixed = NumberFormat::Fixed(Component::literal("fixed"));
    assert_eq!(fixed.format(99), Component::literal("fixed"));
    assert_eq!(fixed.format(99).get_string(), "fixed");
}

#[test]
fn number_format_type_names_match_bootstrap() {
    // The three registry names in `NumberFormatTypes.bootstrap`.
    assert_eq!(NumberFormat::Blank.type_().name(), "blank");
    assert_eq!(NumberFormat::Styled(Style::EMPTY).type_().name(), "styled");
    assert_eq!(
        NumberFormat::Fixed(Component::literal("x")).type_().name(),
        "fixed"
    );
}

#[test]
fn number_format_json_round_trip_all_types() {
    // `NumberFormatTypes.MAP_CODEC` dispatches on the `"type"` field. Under
    // non-compressed `JsonOps`, `KeyDispatchCodec` writes the element codec's
    // keys then the `"type"` discriminator.
    let ops = JsonOps::INSTANCE;
    let top = component_codec();
    let codec = crate::numbers::number_format_types::codec(top);

    // Blank — `MapCodec.unit(INSTANCE)` encodes nothing, so only `type`.
    let blank = NumberFormat::Blank;
    let encoded = codec
        .encode_start(&ops, &blank)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({"type": "blank"}));
    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, NumberFormat::Blank);

    // Styled — `Style.Serializer.MAP_CODEC` writes the style keys inline.
    let styled = NumberFormat::Styled(Style::EMPTY.with_bold(Some(true)));
    let encoded = codec
        .encode_start(&ops, &styled)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({"bold": true, "type": "styled"}));
    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, styled);

    // Fixed — `ComponentSerialization.CODEC.fieldOf("value")`; a bare literal
    // encodes to a JSON string.
    let fixed = NumberFormat::Fixed(Component::literal("hi"));
    let encoded = codec
        .encode_start(&ops, &fixed)
        .get_or_throw("encode")
        .clone();
    assert_eq!(encoded, serde_json::json!({"value": "hi", "type": "fixed"}));
    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    assert_eq!(decoded, fixed);
}

#[test]
fn number_format_json_unknown_type_errors() {
    // A `type` outside the bootstrap set → `DataResult.error("Unknown element
    // id: ...")` (the `NUMBER_FORMAT_TYPE` registry's unknown-name error).
    let ops = JsonOps::INSTANCE;
    let codec = crate::numbers::number_format_types::codec(component_codec());
    let bad = serde_json::json!({"type": "not-a-format"});
    let result = codec.parse(&ops, &bad);
    assert!(
        result.result().is_none(),
        "expected an error, got {result:?}"
    );
}

#[test]
fn number_format_style_codec_rejects_non_map_fixed() {
    // `FixedFormat.TYPE` reads `value` via `ComponentSerialization.CODEC`; a
    // non-component value there fails decode. Java surfaces the underlying
    // error through the field codec.
    let ops = JsonOps::INSTANCE;
    let codec = crate::numbers::number_format_types::codec(component_codec());
    let bad = serde_json::json!({"type": "fixed", "value": {"not": ["a component"]}});
    let result = codec.parse(&ops, &bad);
    assert!(
        result.result().is_none(),
        "expected an error, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// TranslatableContents locale resolution
// ---------------------------------------------------------------------------

/// A translatable whose key is not in `en_us` and whose `fallback` is used.
/// Java `getOrDefault(key, fallback)` returns the fallback for a missing key.
fn translatable_with_fallback(fallback: &str, args: Vec<TranslatableArg>) -> TranslatableContents {
    TranslatableContents::new("no.such.key".to_string(), Some(fallback.to_string()), args)
}

#[test]
fn translatable_resolves_real_locale_key() {
    // `chat.type.text` = "<%s> %s"; two positional args in order.
    let contents = TranslatableContents::new(
        "chat.type.text".to_string(),
        None,
        vec![
            TranslatableArg::String("Steve".to_string()),
            TranslatableArg::String("Hello".to_string()),
        ],
    );
    let component = Component::create(ComponentContents::Translatable(contents));
    assert_eq!(component.get_string(), "<Steve> Hello");
}

#[test]
fn translatable_missing_key_uses_fallback() {
    // A missing key resolves the fallback string (Java `getOrDefault(key,
    // fallback)`); `%s` is replaced by the single arg.
    let component = Component::create(ComponentContents::Translatable(translatable_with_fallback(
        "Hello %s!",
        vec![TranslatableArg::String("world".to_string())],
    )));
    assert_eq!(component.get_string(), "Hello world!");
}

#[test]
fn translatable_missing_key_and_no_fallback_uses_key() {
    // `getOrDefault(key)` = `getOrDefault(key, key)`: a wholly unknown key
    // resolves to itself.
    let contents = TranslatableContents::new("totally.unknown.key".to_string(), None, Vec::new());
    let component = Component::create(ComponentContents::Translatable(contents));
    assert_eq!(component.get_string(), "totally.unknown.key");
}

#[test]
fn translatable_escaped_percent_and_indexed_args() {
    // `%%` → a literal `%`; `%2$s` picks arg index 1 (explicit index, so the
    // implicit `replacementIndex` is not consumed).
    let contents = TranslatableContents::new(
        "escaped.key".to_string(),
        Some("100%% of %2$s and %s".to_string()),
        vec![
            TranslatableArg::Number(7),
            TranslatableArg::String("score".to_string()),
        ],
    );
    let component = Component::create(ComponentContents::Translatable(contents));
    assert_eq!(component.get_string(), "100% of score and 7");
}

#[test]
fn translatable_component_arg_recurses() {
    // A `Component` argument is visited as the component itself (contents then
    // siblings), so its plain text is inserted in order.
    let mut nested = Component::literal("nested");
    nested.append_component(Component::literal("+sib"));
    let contents = TranslatableContents::new(
        "nested.key".to_string(),
        Some("pre [%s] post".to_string()),
        vec![TranslatableArg::Component(Box::new(nested))],
    );
    let component = Component::create(ComponentContents::Translatable(contents));
    assert_eq!(component.get_string(), "pre [nested+sib] post");
}

#[test]
fn translatable_malformed_format_falls_back_to_raw() {
    // `translation.test.invalid2` = "hi %  s": the `%` in literal text between
    // matches throws `IllegalArgumentException`, and `decompose` falls back to
    // a single `Text` part holding the raw format string (Java's
    // `TranslatableFormatException` path).
    let contents =
        TranslatableContents::new("translation.test.invalid2".to_string(), None, Vec::new());
    let component = Component::create(ComponentContents::Translatable(contents));
    assert_eq!(component.get_string(), "hi %  s");
}

#[test]
fn translatable_out_of_range_index_falls_back_to_raw() {
    // `%s` with no arg → `getArgument(0)` out of range → `Invalid index`
    // exception → raw-format fallback.
    let contents = TranslatableContents::new(
        "oob.key".to_string(),
        Some("need %s here".to_string()),
        Vec::new(),
    );
    let component = Component::create(ComponentContents::Translatable(contents));
    assert_eq!(component.get_string(), "need %s here");
}

#[test]
fn translatable_paper_too_long_guard_appends_ellipsis() {
    // Paper's `TranslatableContentConsumer` accepts 33 strings (post-increment
    // `visited++ > 32`) then throws; `visit` catches and appends "...". Build a
    // fallback with 40 `%s` slots and 40 args → 40 visited strings.
    let format = std::iter::repeat_n("%s", 40).collect::<Vec<_>>().join("");
    let args = (0..40)
        .map(|i| TranslatableArg::Number(i as i64))
        .collect::<Vec<_>>();
    let contents = translatable_with_fallback(&format, args);
    let component = Component::create(ComponentContents::Translatable(contents));
    // 33 accepted numbers (0..=32) then "...".
    let expected = (0..33).map(|i| i.to_string()).collect::<Vec<_>>().join("") + "...";
    assert_eq!(component.get_string(), expected);
}

#[test]
fn translatable_styled_visit_short_circuits() {
    // `visit_styled` visits parts in order and stops at the first `Some`; a
    // consumer that matches the second part sees both styles applied.
    let contents = TranslatableContents::new(
        "chat.type.text".to_string(),
        None,
        vec![
            TranslatableArg::String("a".to_string()),
            TranslatableArg::String("b".to_string()),
        ],
    );
    let mut seen = Vec::new();
    let style = Style::EMPTY.with_bold(Some(true));
    let result = contents.visit_styled(
        &mut |s, text| {
            seen.push((s.clone(), text.to_owned()));
            None::<()>
        },
        &style,
    );
    assert!(result.is_none());
    // `<`, arg0, `> `, arg1 — 4 parts visited, each carrying the (unchanged)
    // current style.
    assert_eq!(seen.len(), 4);
    assert_eq!(
        seen[0],
        (Style::EMPTY.with_bold(Some(true)), "<".to_string())
    );
    assert_eq!(
        seen[1],
        (Style::EMPTY.with_bold(Some(true)), "a".to_string())
    );
    assert_eq!(
        seen[2],
        (Style::EMPTY.with_bold(Some(true)), "> ".to_string())
    );
    assert_eq!(
        seen[3],
        (Style::EMPTY.with_bold(Some(true)), "b".to_string())
    );

    // Short-circuit: the first part matches.
    let mut hit = 0;
    let style = Style::EMPTY;
    let result = contents.visit_styled(
        &mut |_, _| {
            hit += 1;
            Some(())
        },
        &style,
    );
    assert_eq!(result, Some(()));
    assert_eq!(hit, 1);
}

#[test]
fn translatable_equality_ignores_decomposition_cache() {
    // `TranslatableContents.equals` compares key/fallback/args only; two equal
    // contents are equal even though one has decomposed (populated cache) and
    // the other has not.
    let mk = || {
        TranslatableContents::new(
            "chat.type.text".to_string(),
            None,
            vec![TranslatableArg::String("a".to_string())],
        )
    };
    let a = mk();
    let b = mk();
    assert_eq!(a, b);
    let component = Component::create(ComponentContents::Translatable(a));
    let _ = component.get_string(); // decompose the cache on the stored contents.
    match component.get_contents() {
        ComponentContents::Translatable(stored) => assert_eq!(stored, &b),
        other => panic!("expected translatable, got {other:?}"),
    }
}

#[test]
fn translatable_reuses_component_codec_for_args() {
    // The `with` list encodes args through `ComponentSerialization.CODEC`
    // (threaded as `top`); a nested component arg round-trips via the shared
    // graph, not a fresh per-value codec.
    let ops = JsonOps::INSTANCE;
    let codec = component_codec();
    let nested = Component::literal("nested");
    let contents = TranslatableContents::new(
        "nested.key".to_string(),
        Some("x %s".to_string()),
        vec![TranslatableArg::Component(Box::new(nested))],
    );
    let component = Component::new(
        ComponentContents::Translatable(contents),
        Vec::new(),
        Style::EMPTY,
    );
    let encoded = codec
        .encode_start(&ops, &component)
        .get_or_throw("encode")
        .clone();
    assert_eq!(
        encoded,
        serde_json::json!({"translate": "nested.key", "fallback": "x %s", "with": ["nested"]})
    );
    // Decode collapses the literal component arg to its String (Java
    // `ARG_CODEC` `Objects.requireNonNullElse(component.tryCollapseToString(),
    // component)`), so the round-tripped args hold `String("nested")`.
    let decoded = codec.parse(&ops, &encoded).get_or_throw("parse").clone();
    let collapsed = Component::new(
        ComponentContents::Translatable(TranslatableContents::new(
            "nested.key".to_string(),
            Some("x %s".to_string()),
            vec![TranslatableArg::String("nested".to_string())],
        )),
        Vec::new(),
        Style::EMPTY,
    );
    assert_eq!(decoded, collapsed);
}

// `Component.visit`/`flatten` helpers used above.
fn _noop() {}
