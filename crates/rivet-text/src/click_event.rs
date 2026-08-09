//! Port of `net.minecraft.network.chat.ClickEvent`.
//!
//! Java's `ClickEvent` is an interface dispatched on `ClickEvent.Action`
//! (`ClickEvent.CODEC = Action.CODEC.dispatch("action", ClickEvent::action,
//! action -> action.codec)`). The Rust port is the closed enum over the six
//! reachable record actions — `open_url`, `open_file`, `run_command`,
//! `suggest_command`, `change_page`, `copy_to_clipboard`. The remaining two
//! actions exist in Java but are deferred because their payloads need deps
//! `rivet-text` cannot take: `show_dialog` needs `Holder<Dialog>` (server
//! crate) and `custom` needs `Identifier` + NBT `Tag` (registry/nbt crates).
//! The action name codec still *recognizes* them, and the dispatch lookup
//! errors with an explicit "not yet ported" message rather than an ambiguous
//! unknown-name error.

use crate::extra_codecs;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::key_dispatch_codec;
use rivet_serialization::map_codec::{self, MapCodec};
use std::sync::Arc;

/// `ClickEvent.Action` — the dispatch key (`StringRepresentable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickEventAction {
    OpenUrl,
    OpenFile,
    RunCommand,
    SuggestCommand,
    ShowDialog,
    ChangePage,
    CopyToClipboard,
    Custom,
}

impl ClickEventAction {
    /// `Action.getSerializedName()`.
    pub fn get_serialized_name(&self) -> &'static str {
        match self {
            ClickEventAction::OpenUrl => "open_url",
            ClickEventAction::OpenFile => "open_file",
            ClickEventAction::RunCommand => "run_command",
            ClickEventAction::SuggestCommand => "suggest_command",
            ClickEventAction::ShowDialog => "show_dialog",
            ClickEventAction::ChangePage => "change_page",
            ClickEventAction::CopyToClipboard => "copy_to_clipboard",
            ClickEventAction::Custom => "custom",
        }
    }

    /// `Action.isAllowedFromServer()` — `open_file` is the only action a
    /// server may not send to a client.
    pub fn is_allowed_from_server(&self) -> bool {
        !matches!(self, ClickEventAction::OpenFile)
    }

    fn from_name(name: &str) -> Option<ClickEventAction> {
        Some(match name {
            "open_url" => ClickEventAction::OpenUrl,
            "open_file" => ClickEventAction::OpenFile,
            "run_command" => ClickEventAction::RunCommand,
            "suggest_command" => ClickEventAction::SuggestCommand,
            "show_dialog" => ClickEventAction::ShowDialog,
            "change_page" => ClickEventAction::ChangePage,
            "copy_to_clipboard" => ClickEventAction::CopyToClipboard,
            "custom" => ClickEventAction::Custom,
            _ => return None,
        })
    }
}

impl std::fmt::Display for ClickEventAction {
    /// `Action.toString()` — the default `Enum.toString()` (`"OPEN_FILE"`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ClickEventAction::OpenUrl => "OPEN_URL",
            ClickEventAction::OpenFile => "OPEN_FILE",
            ClickEventAction::RunCommand => "RUN_COMMAND",
            ClickEventAction::SuggestCommand => "SUGGEST_COMMAND",
            ClickEventAction::ShowDialog => "SHOW_DIALOG",
            ClickEventAction::ChangePage => "CHANGE_PAGE",
            ClickEventAction::CopyToClipboard => "COPY_TO_CLIPBOARD",
            ClickEventAction::Custom => "CUSTOM",
        })
    }
}

/// `ClickEvent.Action.CODEC` — `StringRepresentable.fromEnum(...).validate(
/// filterForSerialization)` with the Java-exact messages: a disallowed action
/// errors `"Click event type not allowed: {ACTION}"`, an unknown name errors
/// `"Unknown element name:{name}"` (DFU `Codec.stringResolver` decode message —
/// no space before the name, matching `rivet-serialization`'s `string_resolver`).
fn action_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ClickEventAction, Ops>> {
    codec::flat_xmap(
        codec::string_codec(),
        Arc::new(|name: &String| match ClickEventAction::from_name(name) {
            Some(action) if action.is_allowed_from_server() => DataResult::success(action),
            Some(action) => DataResult::error(format!("Click event type not allowed: {}", action)),
            None => DataResult::error(format!("Unknown element name:{}", name)),
        }),
        Arc::new(|action: &ClickEventAction| {
            if action.is_allowed_from_server() {
                DataResult::success(action.get_serialized_name().to_string())
            } else {
                DataResult::error(format!("Click event type not allowed: {}", action))
            }
        }),
    )
}

/// `ClickEvent.OpenUrl` — `ExtraCodecs.UNTRUSTED_URI.fieldOf("url")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenUrl {
    uri: String,
}

impl OpenUrl {
    /// `OpenUrl.uri()`.
    pub fn uri(&self) -> &str {
        &self.uri
    }
}

/// `ClickEvent.OpenFile` — `Codec.STRING.fieldOf("path")`. `open_file` is
/// marked `allowFromServer=false`, so the dispatch `Action` codec rejects it on
/// decode and encode (`filterForSerialization`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenFile {
    path: String,
}

impl OpenFile {
    /// `OpenFile.path()`.
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// `ClickEvent.RunCommand` — `ExtraCodecs.CHAT_STRING.fieldOf("command")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunCommand {
    command: String,
}

impl RunCommand {
    /// `RunCommand.command()`.
    pub fn command(&self) -> &str {
        &self.command
    }
}

/// `ClickEvent.SuggestCommand` — `ExtraCodecs.CHAT_STRING.fieldOf("command")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestCommand {
    command: String,
}

impl SuggestCommand {
    /// `SuggestCommand.command()`.
    pub fn command(&self) -> &str {
        &self.command
    }
}

/// `ClickEvent.ChangePage` — `ExtraCodecs.POSITIVE_INT.fieldOf("page")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangePage {
    page: i32,
}

impl ChangePage {
    /// `ChangePage.page()`.
    pub fn page(&self) -> i32 {
        self.page
    }
}

/// `ClickEvent.CopyToClipboard` — `Codec.STRING.fieldOf("value")`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyToClipboard {
    value: String,
}

impl CopyToClipboard {
    /// `CopyToClipboard.value()`.
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// Port of `net.minecraft.network.chat.ClickEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClickEvent {
    OpenUrl(OpenUrl),
    OpenFile(OpenFile),
    RunCommand(RunCommand),
    SuggestCommand(SuggestCommand),
    ChangePage(ChangePage),
    CopyToClipboard(CopyToClipboard),
    /// STUB(mc.network.chat): `show_dialog` needs `Holder<Dialog>` (server
    /// crate dep) — the name is recognized but the dispatch lookup errors.
    ShowDialog,
    /// STUB(mc.network.chat): `custom` needs `Identifier` + NBT `Tag`
    /// (registry/nbt deps) — the name is recognized but the dispatch lookup
    /// errors.
    Custom,
}

impl ClickEvent {
    /// `ClickEvent::action()` — `Action.action()`.
    pub fn action(&self) -> ClickEventAction {
        match self {
            ClickEvent::OpenUrl(_) => ClickEventAction::OpenUrl,
            ClickEvent::OpenFile(_) => ClickEventAction::OpenFile,
            ClickEvent::RunCommand(_) => ClickEventAction::RunCommand,
            ClickEvent::SuggestCommand(_) => ClickEventAction::SuggestCommand,
            ClickEvent::ChangePage(_) => ClickEventAction::ChangePage,
            ClickEvent::CopyToClipboard(_) => ClickEventAction::CopyToClipboard,
            ClickEvent::ShowDialog => ClickEventAction::ShowDialog,
            ClickEvent::Custom => ClickEventAction::Custom,
        }
    }

    /// `ClickEvent.CODEC` — `Action.CODEC.dispatch("action", ...)` over the
    /// per-action record `MapCodec`s.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ClickEvent, Ops>> {
        let open_url = open_url_codec();
        let open_file = open_file_codec();
        let run_command = run_command_codec();
        let suggest_command = suggest_command_codec();
        let change_page = change_page_codec();
        let copy_to_clipboard = copy_to_clipboard_codec();
        map_codec::codec_of(key_dispatch_codec::dispatch_map(
            "action",
            action_codec(),
            Arc::new(|event: &ClickEvent| DataResult::success(event.action())),
            Arc::new(move |action: &ClickEventAction| match action {
                ClickEventAction::OpenUrl => DataResult::success(open_url.clone()),
                ClickEventAction::OpenFile => DataResult::success(open_file.clone()),
                ClickEventAction::RunCommand => DataResult::success(run_command.clone()),
                ClickEventAction::SuggestCommand => DataResult::success(suggest_command.clone()),
                ClickEventAction::ChangePage => DataResult::success(change_page.clone()),
                ClickEventAction::CopyToClipboard => DataResult::success(copy_to_clipboard.clone()),
                ClickEventAction::ShowDialog | ClickEventAction::Custom => DataResult::error(
                    format!("ClickEvent action not yet ported: {action} (RivetTodo #85)"),
                ),
            }),
        ))
    }
}

fn open_url_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClickEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(extra_codecs::untrusted_uri(), "url".to_string()),
        Arc::new(|uri: &String| ClickEvent::OpenUrl(OpenUrl { uri: uri.clone() })),
        Arc::new(|event: &ClickEvent| match event {
            ClickEvent::OpenUrl(open) => open.uri.clone(),
            _ => unreachable!("OpenUrl codec only used for OpenUrl events"),
        }),
    )
}

fn open_file_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClickEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(codec::string_codec(), "path".to_string()),
        Arc::new(|path: &String| ClickEvent::OpenFile(OpenFile { path: path.clone() })),
        Arc::new(|event: &ClickEvent| match event {
            ClickEvent::OpenFile(open) => open.path.clone(),
            _ => unreachable!("OpenFile codec only used for OpenFile events"),
        }),
    )
}

fn run_command_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClickEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(extra_codecs::chat_string(), "command".to_string()),
        Arc::new(|command: &String| {
            ClickEvent::RunCommand(RunCommand {
                command: command.clone(),
            })
        }),
        Arc::new(|event: &ClickEvent| match event {
            ClickEvent::RunCommand(run) => run.command.clone(),
            _ => unreachable!("RunCommand codec only used for RunCommand events"),
        }),
    )
}

fn suggest_command_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClickEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(extra_codecs::chat_string(), "command".to_string()),
        Arc::new(|command: &String| {
            ClickEvent::SuggestCommand(SuggestCommand {
                command: command.clone(),
            })
        }),
        Arc::new(|event: &ClickEvent| match event {
            ClickEvent::SuggestCommand(suggest) => suggest.command.clone(),
            _ => unreachable!("SuggestCommand codec only used for SuggestCommand events"),
        }),
    )
}

fn change_page_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClickEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(extra_codecs::positive_int(), "page".to_string()),
        Arc::new(|page: &i32| ClickEvent::ChangePage(ChangePage { page: *page })),
        Arc::new(|event: &ClickEvent| match event {
            ClickEvent::ChangePage(change) => change.page,
            _ => unreachable!("ChangePage codec only used for ChangePage events"),
        }),
    )
}

fn copy_to_clipboard_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<ClickEvent, Ops>> {
    map_codec::xmap(
        codec::field_of(codec::string_codec(), "value".to_string()),
        Arc::new(|value: &String| {
            ClickEvent::CopyToClipboard(CopyToClipboard {
                value: value.clone(),
            })
        }),
        Arc::new(|event: &ClickEvent| match event {
            ClickEvent::CopyToClipboard(copy) => copy.value.clone(),
            _ => unreachable!("CopyToClipboard codec only used for CopyToClipboard events"),
        }),
    )
}

impl std::fmt::Display for ClickEvent {
    /// `ClickEvent.toString()` — the record `toString()` of each action
    /// (`OpenUrl[uri=https://...]`). The deferred `show_dialog`/`custom`
    /// variants have no payload in Rust and are never constructed through the
    /// codec (the dispatch lookup errors); the placeholder keeps the match
    /// exhaustive.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClickEvent::OpenUrl(e) => write!(f, "OpenUrl[uri={}]", e.uri()),
            ClickEvent::OpenFile(e) => write!(f, "OpenFile[path={}]", e.path()),
            ClickEvent::RunCommand(e) => write!(f, "RunCommand[command={}]", e.command()),
            ClickEvent::SuggestCommand(e) => write!(f, "SuggestCommand[command={}]", e.command()),
            ClickEvent::ChangePage(e) => write!(f, "ChangePage[page={}]", e.page()),
            ClickEvent::CopyToClipboard(e) => write!(f, "CopyToClipboard[value={}]", e.value()),
            ClickEvent::ShowDialog => f.write_str("ShowDialog[dialog=<unported>]"),
            ClickEvent::Custom => f.write_str("Custom[id=<unported>]"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_serialization::codec::Codec;
    use rivet_serialization::data_result::DataResult;
    use rivet_serialization::json_ops::JsonOps;

    fn codec() -> Arc<dyn Codec<ClickEvent, JsonOps>> {
        ClickEvent::codec()
    }

    /// Decode a click-event object under non-compressed `JsonOps`.
    fn decode(input: &str) -> DataResult<ClickEvent> {
        let value: serde_json::Value = serde_json::from_str(input).expect("valid JSON");
        codec().parse(&JsonOps::INSTANCE, &value)
    }

    /// The exact error message of a failed decode/encode (Java's
    /// `error().message()`), or a panic on success.
    fn error_message<T: std::fmt::Debug>(result: &DataResult<T>) -> String {
        result
            .error_ref()
            .unwrap_or_else(|| panic!("expected an error, got {:?}", result.result()))
            .message()
            .to_string()
    }

    /// `open_url` accepts a trusted `http`/`https` URI and round-trips the
    /// source string byte-for-byte (the codec's canonical form is the validated
    /// input — `URI.toString()` is identity for every accepted ASCII URI).
    #[test]
    fn open_url_accepts_http_and_https() {
        for url in [
            "https://example.com/path?q=1&r=2",
            "http://example.com",
            "http://127.0.0.1:25565/",
            "http://user:pass@host:8080/path",
        ] {
            let input = format!("{{\"action\":\"open_url\",\"url\":\"{url}\"}}");
            let decoded = decode(&input).result().cloned().expect("must decode");
            let ClickEvent::OpenUrl(open) = &decoded else {
                panic!("expected OpenUrl, got {decoded:?}");
            };
            assert_eq!(
                open.uri(),
                url,
                "OpenUrl must keep the validated source string"
            );

            // Re-encode produces the dispatch map; the record value encodes
            // first and the `action` key last, matching Paper and the golden.
            let encoded = codec()
                .encode_start(&JsonOps::INSTANCE, &decoded)
                .result()
                .cloned()
                .expect("must re-encode");
            assert_eq!(
                encoded,
                serde_json::json!({"url": url, "action": "open_url"}),
                "open_url re-encode must be stable"
            );
        }
    }

    /// A URI with no scheme fails `Util.parseAndValidateUntrustedUri` with the
    /// exact Java message before the dispatch action codec is consulted.
    #[test]
    fn open_url_missing_protocol_carries_exact_message() {
        let err = error_message(&decode("{\"action\":\"open_url\",\"url\":\"example.com\"}"));
        assert_eq!(err, "Missing protocol in URI: example.com");
    }

    /// A URI with a non-http(s) scheme fails with the exact Java message.
    #[test]
    fn open_url_unsupported_protocol_carries_exact_message() {
        let err = error_message(&decode(
            "{\"action\":\"open_url\",\"url\":\"ftp://example.com\"}",
        ));
        assert_eq!(err, "Unsupported protocol in URI: ftp://example.com");
    }

    /// A URI that fails the JDK parser carries the exact `URISyntaxException`
    /// message (verified against the JDK probe, `at index` offsets included).
    #[test]
    fn open_url_malformed_uri_carries_exact_jdk_message() {
        let err = error_message(&decode(
            "{\"action\":\"open_url\",\"url\":\"http://host/p[]\"}",
        ));
        assert_eq!(
            err,
            "Illegal character in path at index 13: http://host/p[]"
        );
    }

    /// `Action.CODEC` decode of an unknown action name errors with the DFU
    /// `stringResolver` no-space message `"Unknown element name:{name}"`.
    #[test]
    fn unknown_action_name_uses_no_space_message() {
        let err = error_message(&decode("{\"action\":\"bogus\",\"url\":\"https://e\"}"));
        assert_eq!(err, "Unknown element name:bogus");
    }

    /// `open_file` is `allowFromServer=false`, so the dispatch `Action` codec
    /// rejects it on decode AND encode with `"Click event type not allowed:
    /// {ACTION}"` (Java `Enum.toString()`).
    #[test]
    fn open_file_decode_and_encode_are_rejected() {
        let err = error_message(&decode(
            "{\"action\":\"open_file\",\"path\":\"/etc/passwd\"}",
        ));
        assert_eq!(err, "Click event type not allowed: OPEN_FILE");

        let encoded = codec().encode_start(
            &JsonOps::INSTANCE,
            &ClickEvent::OpenFile(OpenFile {
                path: "/etc/passwd".to_string(),
            }),
        );
        let err = error_message(&encoded);
        assert_eq!(err, "Click event type not allowed: OPEN_FILE");
    }

    /// `run_command`, `suggest_command`, `change_page`, `copy_to_clipboard`
    /// round-trip through their record codecs.
    #[test]
    fn remaining_actions_round_trip() {
        for (input, expected_action) in [
            (
                "{\"action\":\"run_command\",\"command\":\"/say hi\"}",
                ClickEventAction::RunCommand,
            ),
            (
                "{\"action\":\"suggest_command\",\"command\":\"/help\"}",
                ClickEventAction::SuggestCommand,
            ),
            (
                "{\"action\":\"change_page\",\"page\":3}",
                ClickEventAction::ChangePage,
            ),
            (
                "{\"action\":\"copy_to_clipboard\",\"value\":\"copied\"}",
                ClickEventAction::CopyToClipboard,
            ),
        ] {
            let decoded = decode(input).result().cloned().unwrap_or_else(|| {
                panic!("{input} must decode");
            });
            assert_eq!(decoded.action(), expected_action);
        }
    }

    /// `change_page` requires a positive int (`ExtraCodecs.POSITIVE_INT`):
    /// zero and negatives are rejected with the exact Java message.
    #[test]
    fn change_page_rejects_non_positive_with_exact_message() {
        let err = error_message(&decode("{\"action\":\"change_page\",\"page\":0}"));
        assert_eq!(err, "Value must be positive: 0");
        let err = error_message(&decode("{\"action\":\"change_page\",\"page\":-1}"));
        assert_eq!(err, "Value must be positive: -1");
    }

    /// `run_command` uses `ExtraCodecs.CHAT_STRING` (no `§`, control chars,
    /// DEL); a disallowed character is rejected with the exact Java message.
    #[test]
    fn run_command_rejects_disallowed_chat_character_with_exact_message() {
        let err = error_message(&decode(
            "{\"action\":\"run_command\",\"command\":\"/say \u{7f}\"}",
        ));
        assert_eq!(err, "Disallowed chat character: '\u{7f}'");
    }

    /// The deferred actions (`show_dialog`, `custom`) are recognized by the
    /// action codec but rejected at the dispatch lookup with the explicit
    /// "not yet ported" message, never an ambiguous unknown-name error.
    #[test]
    fn deferred_actions_error_with_not_yet_ported_message() {
        for action in ["show_dialog", "custom"] {
            let input = format!("{{\"action\":\"{action}\"}}");
            let err = error_message(&decode(&input));
            assert!(
                err.contains("not yet ported"),
                "{action}: expected not-yet-ported error, got {err:?}"
            );
        }
    }
}
