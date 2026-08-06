//! Port of `net.minecraft.network.chat.FontDescription`.
//!
//! `FontDescription` is an interface with a `CODEC` that dispatches on the
//! concrete type; the `Resource` record (an `Identifier`) is the only variant
//! reachable in this slice. `AtlasSprite`/`PlayerSprite` require an
//! `Identifier`/`ResolvableProfile` dependency and are deferred with the Object
//! contents.
//!
//! Identifier is owned by `rivet-registry` (which `rivet-text` cannot depend
//! on without a Cargo cycle through `rivet-nbt`), so the `Resource` id is
//! modeled as a namespace:path string pair with `Identifier`-compatible parse/
//! display, and its codec matches `Identifier.CODEC`'s behavior.

/// Port of `net.minecraft.network.chat.FontDescription`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontDescription {
    /// `FontDescription.Resource(Identifier)` — the only reachable variant.
    Resource(ResourceId),
}

impl FontDescription {
    /// `FontDescription.DEFAULT` — `Resource(Identifier.withDefaultNamespace
    /// ("default"))` = `"minecraft:default"`.
    ///
    /// A `const` is impossible (`ResourceId` holds owned `String`s), so this is
    /// a constructor that always yields the same value; `Style.getFont()`
    /// returns the fallback when the style carries no explicit font.
    pub fn default_font() -> FontDescription {
        FontDescription::Resource(ResourceId::with_default_namespace("default"))
    }
}

/// The `Identifier` shape used by `FontDescription.Resource` (namespace:path,
/// `"minecraft"` default namespace) — a stand-in until `rivet-text` can depend
/// on `rivet-registry`'s `Identifier`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceId {
    namespace: String,
    path: String,
}

impl ResourceId {
    /// Parse a `namespace:path` string, defaulting the namespace to
    /// `"minecraft"` when absent (matching `Identifier.bySeparator`).
    pub fn parse(value: &str) -> ResourceId {
        match value.split_once(':') {
            Some((ns, path)) => ResourceId {
                namespace: ns.to_string(),
                path: path.to_string(),
            },
            None => ResourceId {
                namespace: "minecraft".to_string(),
                path: value.to_string(),
            },
        }
    }

    /// `Identifier.withDefaultNamespace(path)`.
    pub fn with_default_namespace(path: &str) -> ResourceId {
        ResourceId {
            namespace: "minecraft".to_string(),
            path: path.to_string(),
        }
    }
}

impl std::fmt::Display for ResourceId {
    /// `Identifier.toString()` = `namespace:path`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

impl std::fmt::Display for FontDescription {
    /// `FontDescription.toString()` — Resource delegates to its Identifier.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FontDescription::Resource(id) => write!(f, "{}", id),
        }
    }
}

/// `FontDescription.CODEC` — `Identifier.CODEC.flatComapMap(Resource::new, ...)`.
/// Encodes a Resource to its identifier string; non-Resource variants would
/// error with `"Unsupported font description type: ..."` (unreachable here).
pub fn font_description_codec<Ops: rivet_serialization::DynamicOps + 'static>()
-> std::sync::Arc<dyn rivet_serialization::Codec<FontDescription, Ops>> {
    use rivet_serialization::codec;
    use rivet_serialization::data_result::DataResult;
    use std::sync::Arc;
    codec::flat_comap_map(
        codec::string_codec(),
        Arc::new(|s: &String| FontDescription::Resource(ResourceId::parse(s))),
        Arc::new(|fd: &FontDescription| match fd {
            FontDescription::Resource(id) => DataResult::success(id.to_string()),
        }),
    )
}
