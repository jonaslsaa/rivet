//! Port of `ca.spottedleaf.dataconverter.util.NamespaceUtil`.

use crate::server::dataconverter::types::map_type::MapType;
use rivet_registry::Identifier;

/// `NamespaceUtil` — normalizes identifier strings (e.g. `"stone"` -> `"minecraft:stone"`).
pub struct NamespaceUtil;

impl NamespaceUtil {
    /// `NamespaceUtil.enforceForPath(MapType, String)`.
    pub fn enforce_for_path(data: Option<&mut dyn MapType>, path: &str) {
        let Some(data) = data else {
            return;
        };

        if let Some(id) = data.get_string(path)
            && let Some(replace) = Self::correct_namespace_or_null(Some(&id))
        {
            data.set_string(path, replace);
        }
    }

    /// `NamespaceUtil.correctNamespace(String)`.
    pub fn correct_namespace(value: Option<&str>) -> Option<String> {
        let value = value?;
        match Identifier::try_parse(value) {
            Some(resource_location) => Some(resource_location.to_string()),
            None => Some(value.to_owned()),
        }
    }

    /// `NamespaceUtil.correctNamespaceOrNull(String)`.
    pub fn correct_namespace_or_null(value: Option<&str>) -> Option<String> {
        let value = value?;
        let correct = Self::correct_namespace(Some(value))?;
        if correct == value {
            None
        } else {
            Some(correct)
        }
    }
}
