//! Port of `net.minecraft.nbt.visitors.FieldTree` —
//! `record FieldTree(int depth, Map<String, TagType<?>> selectedFields, Map<String, FieldTree> fieldsToRecurse)`.

use std::collections::HashMap;

use crate::tag_type::TagType;

use super::field_selector::FieldSelector;

/// `FieldTree` — a node in the field-selection tree.
///
/// `depth` is 1-based (`createRoot` is depth 1). `selected_fields` holds the
/// fields to collect at this level; `fields_to_recurse` holds the child frames
/// for compound keys that the selection descends into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldTree {
    pub depth: usize,
    pub selected_fields: HashMap<String, TagType>,
    pub fields_to_recurse: HashMap<String, FieldTree>,
}

impl FieldTree {
    /// The private `FieldTree(int depth)` constructor.
    fn new(depth: usize) -> Self {
        FieldTree {
            depth,
            selected_fields: HashMap::new(),
            fields_to_recurse: HashMap::new(),
        }
    }

    /// `FieldTree.createRoot()`.
    pub fn create_root() -> Self {
        FieldTree::new(1)
    }

    /// `FieldTree.addEntry(FieldSelector)`.
    ///
    /// While the field's path is longer than this node's depth, recurse into a
    /// child frame keyed by `path[depth - 1]`; once the path is exhausted, the
    /// field's name/type lands in `selected_fields`.
    pub fn add_entry(&mut self, field: &FieldSelector) {
        if self.depth <= field.path.len() {
            let key = field.path[self.depth - 1].clone();
            let child_depth = self.depth + 1;
            self.fields_to_recurse
                .entry(key)
                .or_insert_with(|| FieldTree::new(child_depth))
                .add_entry(field);
        } else {
            self.selected_fields.insert(field.name.clone(), field.ty);
        }
    }

    /// `FieldTree.isSelected(TagType, String)` — `type.equals(selectedFields.get(id))`.
    pub fn is_selected(&self, ty: TagType, id: &str) -> bool {
        self.selected_fields.get(id) == Some(&ty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_root_has_depth_one() {
        let root = FieldTree::create_root();
        assert_eq!(root.depth, 1);
        assert!(root.selected_fields.is_empty());
        assert!(root.fields_to_recurse.is_empty());
    }

    #[test]
    fn add_entry_selects_at_root() {
        let mut root = FieldTree::create_root();
        root.add_entry(&FieldSelector::new(TagType::Int, "health".to_string()));
        assert_eq!(root.selected_fields.get("health"), Some(&TagType::Int));
        assert!(root.fields_to_recurse.is_empty());
    }

    #[test]
    fn add_entry_with_parent_creates_child_frame() {
        let mut root = FieldTree::create_root();
        root.add_entry(&FieldSelector::with_parent(
            "a".to_string(),
            TagType::Int,
            "b".to_string(),
        ));
        let child = root.fields_to_recurse.get("a").expect("child frame for a");
        assert_eq!(child.depth, 2);
        assert_eq!(child.selected_fields.get("b"), Some(&TagType::Int));
        assert!(root.selected_fields.is_empty());
    }

    #[test]
    fn add_entry_deep_path_builds_nested_frames() {
        // Dotted path `a.b.c` -> `path = [a, b, c]`, name "d".
        let mut root = FieldTree::create_root();
        root.add_entry(&FieldSelector {
            path: vec!["a".into(), "b".into(), "c".into()],
            ty: TagType::String,
            name: "d".into(),
        });
        let a = root.fields_to_recurse.get("a").expect("frame a");
        assert_eq!(a.depth, 2);
        let b = a.fields_to_recurse.get("b").expect("frame b");
        assert_eq!(b.depth, 3);
        let c = b.fields_to_recurse.get("c").expect("frame c");
        assert_eq!(c.depth, 4);
        assert_eq!(c.selected_fields.get("d"), Some(&TagType::String));
    }

    #[test]
    fn add_entry_merges_under_same_parent() {
        let mut root = FieldTree::create_root();
        root.add_entry(&FieldSelector::with_parent(
            "a".to_string(),
            TagType::Int,
            "b".to_string(),
        ));
        root.add_entry(&FieldSelector::with_parent(
            "a".to_string(),
            TagType::String,
            "c".to_string(),
        ));
        assert_eq!(root.fields_to_recurse.len(), 1);
        let a = root.fields_to_recurse.get("a").expect("frame a");
        assert_eq!(a.selected_fields.get("b"), Some(&TagType::Int));
        assert_eq!(a.selected_fields.get("c"), Some(&TagType::String));
    }

    #[test]
    fn is_selected_requires_type_match() {
        let mut root = FieldTree::create_root();
        root.add_entry(&FieldSelector::new(TagType::Int, "x".to_string()));
        assert!(root.is_selected(TagType::Int, "x"));
        assert!(!root.is_selected(TagType::String, "x"));
        assert!(!root.is_selected(TagType::Int, "y"));
    }
}
