//! Exact data-component predicate mirroring Paper's `DataComponentExactPredicate`.

use indexmap::IndexMap;
use rivet_nbt::{compound_tag::CompoundTag, tag::Tag};
use rivet_registry::identifier::Identifier;

use super::trade_item_stack::TradeItemStack;

/// Error returned when decoding trading NBT payloads.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TradingCodecError {
    #[error("missing required NBT field `{0}`")]
    MissingField(&'static str),
    #[error("invalid NBT field `{field}`: {reason}")]
    InvalidField { field: &'static str, reason: String },
}

/// Exact data-component predicate mirroring Paper's `DataComponentExactPredicate`.
///
/// Uses `IndexMap` so iteration and serialized key order remain deterministic.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DataComponentExactPredicate {
    pub(crate) expected: IndexMap<Identifier, Tag>,
}

impl DataComponentExactPredicate {
    /// Empty predicate — matches any item stack regardless of its components.
    pub fn empty() -> Self {
        Self {
            expected: IndexMap::new(),
        }
    }

    /// Creates a predicate requiring all `(key, value)` pairs to match exactly.
    pub fn from_entries(entries: impl IntoIterator<Item = (Identifier, Tag)>) -> Self {
        Self {
            expected: entries.into_iter().collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.expected.is_empty()
    }

    pub fn entries(&self) -> &IndexMap<Identifier, Tag> {
        &self.expected
    }

    /// Inserts an expected component value, preserving insertion order.
    pub fn expect(mut self, key: Identifier, value: Tag) -> Self {
        self.expected.insert(key, value);
        self
    }

    /// Returns `true` iff every expected component is present on `stack` with an
    /// equal value (`DataComponentExactPredicate.test`).
    pub fn test(&self, stack: &TradeItemStack) -> bool {
        self.expected
            .iter()
            .all(|(key, expected_value)| stack.components().get(key) == Some(expected_value))
    }

    /// Encodes the predicate as an NBT compound tag (`id -> Tag`).
    pub fn to_nbt(&self) -> CompoundTag {
        let mut tag = CompoundTag::new();
        for (key, value) in &self.expected {
            tag.put(key.to_string(), value.clone());
        }
        tag
    }

    /// Decodes a predicate from an NBT compound tag.
    pub fn from_nbt(tag: &CompoundTag) -> Result<Self, TradingCodecError> {
        let mut expected = IndexMap::with_capacity(tag.size());
        for (key, value) in tag.tags.iter() {
            let id = Identifier::try_parse(key).ok_or_else(|| TradingCodecError::InvalidField {
                field: "components",
                reason: format!("invalid component identifier `{key}`"),
            })?;
            expected.insert(id, value.clone());
        }
        Ok(Self { expected })
    }
}
