//! Port of `net.minecraft.locale.Language` (the `en_us` default instance) —
//! the key/value table `TranslatableContents.decompose` resolves against.
//!
//! Java's `Language` is an abstract class with a private static default
//! instance built from `/assets/minecraft/lang/en_us.json` plus the
//! `deprecated.json` removal/rename map, and an injectable process-wide
//! instance. The port mirrors the surface `TranslatableContents` needs:
//! `getOrDefault(key)`, `getOrDefault(key, default)`, and `has(key)`.
//! `isDefaultRightToLeft` / `getVisualOrder` are client-rendering and deferred.
//!
//! The default translation data is committed at
//! `crates/rivet-text/assets/en_us.json`, generated once from the pinned Paper
//! source of truth (`working/Paper/.../assets/minecraft/lang/en_us.json` plus
//! `deprecated.json`) with Java's exact pipeline: each value has the
//! `UNSUPPORTED_FORMAT_PATTERN` `%(\d+\$)?[\d.]*[df]` → `%<n>$s` rewrite
//! applied, then `deprecated.json`'s `removed` keys are dropped and `renamed`
//! pairs are moved. [`load_from_json`] is ported and applied at load so the
//! Rust pipeline matches Java's even for a different (raw) source; on the
//! committed asset it is idempotent.

use std::collections::HashMap;
use std::sync::OnceLock;

/// `Language.DEFAULT` — the default locale id.
pub const DEFAULT: &str = "en_us";

/// `Language` — the abstract class's accessor surface. The concrete default
/// instance holds the loaded map; other locales / injected languages implement
/// the same trait.
pub trait Language {
    /// `Language.getOrDefault(String)` — `getOrDefault(elementId, elementId)`;
    /// a missing key resolves to the key itself. The returned reference is
    /// either a stored value or the key, so it lives as long as the shorter of
    /// the two.
    fn get_or_default<'a>(&'a self, key: &'a str) -> &'a str;

    /// `Language.getOrDefault(String, String)`.
    fn get_or_default_with<'a>(&'a self, key: &str, default: &'a str) -> &'a str;

    /// `Language.has(String)`.
    fn has(&self, key: &str) -> bool;
}

/// The concrete default instance — `Language.DEFAULT_INSTANCE`. Holds the
/// immutable `Map.copyOf`-style storage; Java's `Map.copyOf` iteration order is
/// unspecified, so the port keeps a sorted slice for deterministic lookup.
pub struct DefaultLanguage {
    storage: Box<[(String, String)]>,
}

/// `Language.DEFAULT_INSTANCE` — built lazily on first use from the pinned
/// asset (a `static` cannot run the parser, so the storage lives behind a
/// `OnceLock`).
pub static DEFAULT_INSTANCE: OnceLock<DefaultLanguage> = OnceLock::new();

/// `Language.getInstance()` — the process-wide language. The port exposes a
/// single static default instance (Java's `inject`/client resource-pack
/// replacement is deferred; `TranslatableContents.decompose` caches on the
/// language identity exactly like Java, so a future injectable slot fits).
pub fn get_instance() -> &'static dyn Language {
    DEFAULT_INSTANCE.get_or_init(|| DefaultLanguage {
        storage: load_default_storage(),
    })
}

/// `Language.loadFromJson(InputStream, BiConsumer)` — parse the flat
/// `key: "value"` JSON object, applying the unsupported-format rewrite to each
/// value. The port returns the entry list instead of invoking a consumer.
pub fn load_from_json(json: &str) -> Vec<(String, String)> {
    let object: HashMap<String, String> = serde_json::from_str(json).expect("valid language JSON");
    object
        .into_iter()
        .map(|(key, value)| (key, rewrite_unsupported_formats(&value)))
        .collect()
}

/// `UNSUPPORTED_FORMAT_PATTERN.matcher(value).replaceAll("%$1s")` — rewrite
/// Java-formatted `%d` / `%f` specifiers (optionally `%<n>$`-indexed, with
/// optional width/precision like `%1.2f`) to `%<n>$s` so the game's `%s`-only
/// formatter accepts them. Implemented as a small scanner (no regex
/// dependency); the pattern is `%(?:\d+\$)?[\d.]*[df]`.
fn rewrite_unsupported_formats(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'%' {
            // bytes[i] is not '%' (a UTF-8 char boundary), so it starts a char.
            let ch = value[i..].chars().next().expect("valid utf-8");
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let mut j = i + 1;
        // Optional `(\d+\$)` index.
        let index_start = j;
        while j < bytes.len() && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let index_end = j;
        let has_index = j < bytes.len() && bytes[j] == b'$';
        if has_index {
            j += 1;
        } else {
            j = index_start;
        }
        // Optional `[\d.]*` width/precision.
        while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'.') {
            j += 1;
        }
        // The conversion char must be `d` or `f` (else `%` stays verbatim).
        if j < bytes.len() && (bytes[j] == b'd' || bytes[j] == b'f') {
            out.push('%');
            if has_index {
                out.push_str(&value[index_start..index_end]); // the index digits
                out.push('$');
            }
            out.push('s');
            i = j + 1;
        } else {
            out.push('%');
            i += 1;
        }
    }
    out
}

/// Build the default instance's storage from the pinned asset. The committed
/// `en_us.json` is already Java-processed, so `load_from_json` is idempotent on
/// it.
fn load_default_storage() -> Box<[(String, String)]> {
    let json: &str = include_str!("../assets/en_us.json");
    let mut entries = load_from_json(json);
    entries.sort_by(|(a, _), (b, _)| a.cmp(b));
    entries.into_boxed_slice()
}

impl Language for DefaultLanguage {
    fn get_or_default<'a>(&'a self, key: &'a str) -> &'a str {
        self.get_or_default_with(key, key)
    }

    fn get_or_default_with<'a>(&'a self, key: &str, default: &'a str) -> &'a str {
        match self.storage.binary_search_by(|(k, _)| k.as_str().cmp(key)) {
            Ok(i) => &self.storage[i].1,
            Err(_) => default,
        }
    }

    fn has(&self, key: &str) -> bool {
        self.storage
            .binary_search_by(|(k, _)| k.as_str().cmp(key))
            .is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_unsupported_formats_like_java() {
        assert_eq!(
            rewrite_unsupported_formats("You have %d items"),
            "You have %s items"
        );
        assert_eq!(
            rewrite_unsupported_formats("%1$d and %2$f"),
            "%1$s and %2$s"
        );
        assert_eq!(rewrite_unsupported_formats("%.2f%% done"), "%s%% done");
        assert_eq!(rewrite_unsupported_formats("100%"), "100%");
        assert_eq!(rewrite_unsupported_formats("%s %d %s"), "%s %s %s");
        assert_eq!(
            rewrite_unsupported_formats("keep %s intact"),
            "keep %s intact"
        );
        assert_eq!(rewrite_unsupported_formats("%2d"), "%s");
        assert_eq!(rewrite_unsupported_formats("%1$2d"), "%1$s");
    }

    #[test]
    fn default_instance_resolves_pinned_values() {
        let lang = get_instance();
        assert_eq!(
            lang.get_or_default("advancements.adventure.adventuring_time.title"),
            "Adventuring Time"
        );
        // Missing key resolves to the key itself (`getOrDefault(id)`).
        assert_eq!(
            lang.get_or_default("no.such.key.exists"),
            "no.such.key.exists"
        );
        assert!(lang.has("menu.disconnect"));
        assert!(!lang.has("upgrade.minecraft.netherite_upgrade"));
        assert!(lang.has("item.minecraft.dune_armor_trim_smithing_template"));
        assert!(!lang.has("item.minecraft.dune_armor_trim_smithing_template.new"));
    }
}
