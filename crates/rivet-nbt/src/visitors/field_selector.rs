//! Port of `net.minecraft.nbt.visitors.FieldSelector` —
//! `record FieldSelector(List<String> path, TagType<?> type, String name)`.

use crate::tag_type::TagType;

/// `FieldSelector` — a selected field, optionally nested under a parent path.
///
/// The record component `type` is renamed `ty` (`type` is a Rust keyword), per
/// the crate convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSelector {
    pub path: Vec<String>,
    pub ty: TagType,
    pub name: String,
}

impl FieldSelector {
    /// `FieldSelector(TagType, String)` — a top-level field (no parent path).
    pub fn new(ty: TagType, name: String) -> Self {
        FieldSelector {
            path: Vec::new(),
            ty,
            name,
        }
    }

    /// `FieldSelector(String parent, TagType, String)`.
    pub fn with_parent(parent: String, ty: TagType, name: String) -> Self {
        FieldSelector {
            path: vec![parent],
            ty,
            name,
        }
    }

    /// `FieldSelector(String grandparent, String parent, TagType, String)`.
    pub fn with_grandparent(
        grandparent: String,
        parent: String,
        ty: TagType,
        name: String,
    ) -> Self {
        FieldSelector {
            path: vec![grandparent, parent],
            ty,
            name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_has_empty_path() {
        let selector = FieldSelector::new(TagType::Byte, "flag".to_string());
        assert!(selector.path.is_empty());
        assert_eq!(selector.ty, TagType::Byte);
        assert_eq!(selector.name, "flag");
    }

    #[test]
    fn with_parent_has_single_path_component() {
        let selector =
            FieldSelector::with_parent("parent".to_string(), TagType::Int, "x".to_string());
        assert_eq!(selector.path, vec!["parent".to_string()]);
        assert_eq!(selector.ty, TagType::Int);
        assert_eq!(selector.name, "x");
    }

    #[test]
    fn with_grandparent_has_two_path_components() {
        let selector = FieldSelector::with_grandparent(
            "grandparent".to_string(),
            "parent".to_string(),
            TagType::String,
            "y".to_string(),
        );
        assert_eq!(
            selector.path,
            vec!["grandparent".to_string(), "parent".to_string()]
        );
        assert_eq!(selector.ty, TagType::String);
        assert_eq!(selector.name, "y");
    }
}
