//! Port of `net.minecraft.util.ExtraCodecs` — the slice consumed by the
//! `net.minecraft.network.chat` `ClickEvent`/`HoverEvent` codecs (epic #12).
//!
//! PROVENANCE: `net.minecraft.util.ExtraCodecs` maps to `rivet-util`; the
//! helpers live here because their only consumers are `rivet-text` and the
//! `mc.util` unit has not yet been split into `rivet-util`. RECONCILIATION:
//! move them to `rivet-util` when a fuller `mc.util` port lands.

use crate::uri::parse_uri;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// `ExtraCodecs.POSITIVE_INT` — `Codec.INT` validated to `[1, MAX]` with the
/// Java-exact message `"Value must be positive: {n}"` (Java's
/// `intRangeWithMessage(1, Integer.MAX_VALUE, ...)`).
pub fn positive_int<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    codec::validate(
        codec::int_codec(),
        Arc::new(|value: &i32| {
            if *value >= 1 {
                DataResult::success(*value)
            } else {
                DataResult::error(format!("Value must be positive: {}", value))
            }
        }),
    )
}

/// `StringUtil.isAllowedChatCharacter(char)` — `ch != 167 && ch >= 32 &&
/// ch != 127` over a UTF-16 code unit (Java's `char`).
fn is_allowed_chat_character(ch: u16) -> bool {
    ch != 167 && ch >= 32 && ch != 127
}

/// `ExtraCodecs.CHAT_STRING` — `Codec.STRING.validate(...)` over every UTF-16
/// code unit (Java's `String.charAt`), error
/// `"Disallowed chat character: '{c}'"`.
pub fn chat_string<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<String, Ops>> {
    codec::validate(
        codec::string_codec(),
        Arc::new(|string: &String| {
            for ch in string.encode_utf16() {
                if !is_allowed_chat_character(ch) {
                    // Java renders the `char` itself; the error branch can only
                    // see non-surrogate units (surrogates are always allowed).
                    let c = char::from_u32(ch as u32).unwrap_or(char::REPLACEMENT_CHARACTER);
                    return DataResult::error(format!("Disallowed chat character: '{}'", c));
                }
            }
            DataResult::success(string.clone())
        }),
    )
}

/// `ExtraCodecs.UNTRUSTED_URI` — `Util.parseAndValidateUntrustedUri`: parse the
/// URI with `new URI(string)` (a faithful port of the JDK `Parser`), then
/// require the scheme (lowercased with `Locale.ROOT`) to be `http` or `https`.
///
/// The port keeps the validated source string: Java stores the parsed `URI`
/// and re-encodes via `URI.toString`, which for every ASCII URI that parses is
/// byte-identical to the input (each component is quoted with the mask that
/// validated it — verified empirically against the JDK probe), so identity is
/// the codec's canonical form. A fuller `URI` port would normalize on decode.
pub fn untrusted_uri<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<String, Ops>> {
    codec::comap_flat_map(
        codec::string_codec(),
        Arc::new(validate_untrusted_uri),
        Arc::new(|uri: &String| uri.clone()),
    )
}

fn validate_untrusted_uri(uri: &String) -> DataResult<String> {
    let scheme = match parse_uri(uri) {
        // `URISyntaxException.getMessage()`.
        Err(msg) => return DataResult::error(msg),
        Ok(scheme) => scheme,
    };
    let Some(scheme) = scheme else {
        return DataResult::error(format!("Missing protocol in URI: {}", uri));
    };
    let protocol = scheme.to_ascii_lowercase();
    if protocol != "http" && protocol != "https" {
        return DataResult::error(format!("Unsupported protocol in URI: {}", uri));
    }
    DataResult::success(uri.clone())
}
