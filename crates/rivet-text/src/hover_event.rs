//! Port of `net.minecraft.network.chat.HoverEvent`.
//!
//! Java's `HoverEvent` is an interface dispatched on `HoverEvent.Action`
//! (`HoverEvent.CODEC = Action.CODEC.dispatch("action", HoverEvent::action,
//! action -> action.codec)`). The Rust port is the closed enum over `show_text`
//! — the only action whose payload (`Component`) is reachable from
//! `rivet-text`. The other two actions exist in Java but are deferred because
//! their payloads need deps `rivet-text` cannot take: `show_item` needs
//! `ItemStackTemplate` and `show_entity` needs `UUIDUtil` + the `EntityType`
//! registry. Their names are still recognized by the action codec, and the
//! dispatch lookup errors with an explicit "not yet ported" message.
//!
//! `ShowText` carries the recursive `ComponentSerialization.CODEC` (the `top`
//! of the `Component` graph), so building the `HoverEvent` codec threads `top`
//! through — the `show_text` "value" field reuses the single Component graph
//! and never constructs a second one (issue #207's `CODEC_BUILD_COUNT` stays
//! flat).

use crate::Component;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use std::sync::Arc;

/// `HoverEvent.Action` — the dispatch key (`StringRepresentable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoverEventAction {
    ShowText,
    ShowItem,
    ShowEntity,
}

impl HoverEventAction {
    /// `Action.getSerializedName()`.
    pub fn get_serialized_name(&self) -> &'static str {
        match self {
            HoverEventAction::ShowText => "show_text",
            HoverEventAction::ShowItem => "show_item",
            HoverEventAction::ShowEntity => "show_entity",
        }
    }

    /// `Action.isAllowedFromServer()` — all three are allowed.
    pub fn is_allowed_from_server(&self) -> bool {
        true
    }

    fn from_name(name: &str) -> Option<HoverEventAction> {
        Some(match name {
            "show_text" => HoverEventAction::ShowText,
            "show_item" => HoverEventAction::ShowItem,
            "show_entity" => HoverEventAction::ShowEntity,
            _ => return None,
        })
    }
}

impl std::fmt::Display for HoverEventAction {
    /// `Action.toString()` — `"<action show_text>"` (Java overrides it).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<action {}>", self.get_serialized_name())
    }
}

/// `HoverEvent.Action.CODEC` — `StringRepresentable.fromValues(...).validate(
/// filterForSerialization)` with the Java-exact messages: a disallowed action
/// errors `"Action not allowed: {action}"`, an unknown name errors
/// `"Unknown element name:{name}"` (DFU `Codec.stringResolver` decode message —
/// no space before the name, matching `rivet-serialization`'s `string_resolver`).
fn action_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<HoverEventAction, Ops>> {
    codec::flat_xmap(
        codec::string_codec(),
        Arc::new(|name: &String| match HoverEventAction::from_name(name) {
            Some(action) if action.is_allowed_from_server() => DataResult::success(action),
            Some(action) => DataResult::error(format!("Action not allowed: {}", action)),
            None => DataResult::error(format!("Unknown element name:{}", name)),
        }),
        Arc::new(|action: &HoverEventAction| {
            if action.is_allowed_from_server() {
                DataResult::success(action.get_serialized_name().to_string())
            } else {
                DataResult::error(format!("Action not allowed: {}", action))
            }
        }),
    )
}

/// `HoverEvent.ShowText` — `ComponentSerialization.CODEC.fieldOf("value")`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShowText {
    value: Box<Component>,
}

impl ShowText {
    /// `ShowText.value()`.
    pub fn value(&self) -> &Component {
        &self.value
    }
}

/// Port of `net.minecraft.network.chat.HoverEvent`.
#[derive(Clone, Debug, PartialEq)]
pub enum HoverEvent {
    ShowText(ShowText),
    /// STUB(mc.network.chat): `show_item` needs `ItemStackTemplate` (item
    /// crate dep) — the name is recognized but the dispatch lookup errors.
    ShowItem,
    /// STUB(mc.network.chat): `show_entity` needs `UUIDUtil` + the `EntityType`
    /// registry — the name is recognized but the dispatch lookup errors.
    ShowEntity,
}

impl HoverEvent {
    /// `HoverEvent::action()` — `Action.action()`.
    pub fn action(&self) -> HoverEventAction {
        match self {
            HoverEvent::ShowText(_) => HoverEventAction::ShowText,
            HoverEvent::ShowItem => HoverEventAction::ShowItem,
            HoverEvent::ShowEntity => HoverEventAction::ShowEntity,
        }
    }

    /// `HoverEvent.CODEC` — `Action.CODEC.dispatch("action", ...)` over the
    /// per-action record `MapCodec`s. `top` is the `RecursiveSelf` of the
    /// `Component` graph, threaded into the `ShowText` value codec so nested
    /// components reuse the one graph.
    pub fn codec<Ops: DynamicOps + 'static>(
        top: Arc<dyn Codec<Component, Ops>>,
    ) -> Arc<dyn Codec<HoverEvent, Ops>> {
        let show_text = show_text_codec(top);
        map_codec::codec_of(key_dispatch_codec::dispatch_map(
            "action",
            action_codec(),
            Arc::new(|event: &HoverEvent| DataResult::success(event.action())),
            Arc::new(move |action: &HoverEventAction| match action {
                HoverEventAction::ShowText => DataResult::success(show_text.clone()),
                HoverEventAction::ShowItem | HoverEventAction::ShowEntity => DataResult::error(
                    format!("HoverEvent action not yet ported: {action} (RivetTodo #85)"),
                ),
            }),
        ))
    }
}

fn show_text_codec<Ops: DynamicOps + 'static>(
    top: Arc<dyn Codec<Component, Ops>>,
) -> Arc<dyn MapCodec<HoverEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(top, "value".to_string()),
        Arc::new(|value: &Component| {
            HoverEvent::ShowText(ShowText {
                value: Box::new(value.clone()),
            })
        }),
        Arc::new(|event: &HoverEvent| match event {
            HoverEvent::ShowText(show) => (*show.value).clone(),
            _ => unreachable!("ShowText codec only used for ShowText events"),
        }),
    )
}

impl std::fmt::Display for HoverEvent {
    /// `HoverEvent.toString()` — the record `toString()` (`ShowText[value=...]`
    /// where the value is the component's `toString`). The deferred
    /// `show_item`/`show_entity` variants have no payload in Rust and are never
    /// constructed through the codec (the dispatch lookup errors); the
    /// placeholder keeps the match exhaustive.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HoverEvent::ShowText(e) => write!(f, "ShowText[value={}]", e.value()),
            HoverEvent::ShowItem => f.write_str("ShowItem[item=<unported>]"),
            HoverEvent::ShowEntity => f.write_str("ShowEntity[entity=<unported>]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::component_serialization;
    use rivet_serialization::codec::Codec;
    use rivet_serialization::data_result::DataResult;
    use rivet_serialization::json_ops::JsonOps;
    use std::sync::Arc;

    fn codec() -> Arc<dyn Codec<HoverEvent, JsonOps>> {
        let top: Arc<dyn Codec<crate::Component, JsonOps>> = component_serialization::codec();
        HoverEvent::codec(top)
    }

    /// Decode a hover-event object under non-compressed `JsonOps`.
    fn decode(input: &str) -> DataResult<HoverEvent> {
        let value: serde_json::Value = serde_json::from_str(input).expect("valid JSON");
        codec().parse(&JsonOps::INSTANCE, &value)
    }

    fn error_message<T: std::fmt::Debug>(result: &DataResult<T>) -> String {
        result
            .error_ref()
            .unwrap_or_else(|| panic!("expected an error, got {:?}", result.result()))
            .message()
            .to_string()
    }

    /// `show_text` decodes a plain-string value and re-encodes the dispatch map
    /// with `action` first (Paper's `Style.Serializer` output order).
    #[test]
    fn show_text_plain_value_round_trips() {
        let decoded = decode("{\"action\":\"show_text\",\"value\":\"hover!\"}")
            .result()
            .cloned()
            .expect("must decode");
        let HoverEvent::ShowText(show) = &decoded else {
            panic!("expected ShowText, got {decoded:?}");
        };
        assert_eq!(show.value().get_string(), "hover!");

        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("must re-encode");
        assert_eq!(
            encoded,
            serde_json::json!({"action": "show_text", "value": "hover!"})
        );
    }

    /// `show_text` with a nested styled component recurses through the threaded
    /// `top` codec (`ComponentSerialization.CODEC`), not a rebuilt graph.
    #[test]
    fn show_text_nested_component_round_trips() {
        let input = "{\"action\":\"show_text\",\"value\":{\"text\":\"nested\",\"bold\":true}}";
        let decoded = decode(input).result().cloned().expect("must decode");
        let HoverEvent::ShowText(show) = &decoded else {
            panic!("expected ShowText, got {decoded:?}");
        };
        assert!(
            show.value().get_style().is_bold(),
            "nested style must survive"
        );
        assert_eq!(show.value().get_string(), "nested");

        let encoded = codec()
            .encode_start(&JsonOps::INSTANCE, &decoded)
            .result()
            .cloned()
            .expect("must re-encode");
        assert_eq!(
            encoded,
            serde_json::json!({"action": "show_text", "value": {"text": "nested", "bold": true}})
        );
    }

    /// `Action.CODEC` decode of an unknown action name errors with the DFU
    /// `stringResolver` no-space message `"Unknown element name:{name}"`.
    #[test]
    fn unknown_action_name_uses_no_space_message() {
        let err = error_message(&decode("{\"action\":\"bogus\",\"value\":\"x\"}"));
        assert_eq!(err, "Unknown element name:bogus");
    }

    /// `show_text`'s value field is `value` (Paper 26.2) — a wrong key is a
    /// malformed field, rejected by the record codec with DFU's `FieldDecoder`
    /// `"No key value in ..."` message.
    #[test]
    fn show_text_wrong_value_key_is_rejected() {
        let err = error_message(&decode("{\"action\":\"show_text\",\"contents\":\"x\"}"));
        assert!(
            err.starts_with("No key value in"),
            "wrong field name must be a missing-value error, got {err:?}"
        );
    }

    /// The deferred actions (`show_item`, `show_entity`) are recognized by the
    /// action codec but rejected at the dispatch lookup with the explicit
    /// "not yet ported" message, never an ambiguous unknown-name error.
    #[test]
    fn deferred_actions_error_with_not_yet_ported_message() {
        for action in ["show_item", "show_entity"] {
            let input = format!("{{\"action\":\"{action}\"}}");
            let err = error_message(&decode(&input));
            assert!(
                err.contains("not yet ported"),
                "{action}: expected not-yet-ported error, got {err:?}"
            );
        }
    }
}
