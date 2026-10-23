//! Paper's shared EntityType.loadEntitiesRecursive / chunk save limit rule.
use rivet_registry::Identifier;
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
#[error("unregistered entity save-limit key {0}")]
pub struct EntitySaveLimitError(pub Identifier);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EntitySaveLimits(HashMap<Identifier, i32>);
impl EntitySaveLimits {
    /// Source treats every negative value, and missing keys, as unlimited.
    /// Preserve raw bound settings while comparing their executable meaning.
    pub fn equivalent(&self, other: &Self) -> bool {
        self.0
            .iter()
            .filter(|(_, limit)| **limit >= 0)
            .all(|(id, limit)| other.0.get(id) == Some(limit))
            && other
                .0
                .iter()
                .filter(|(_, limit)| **limit >= 0)
                .all(|(id, limit)| self.0.get(id) == Some(limit))
    }
    /// Bind the actual world's validated Paper settings. Missing keys mean
    /// source getOrDefault(type,-1); this does not load global default config.
    pub fn from_entries(
        entries: impl IntoIterator<Item = (Identifier, i32)>,
    ) -> Result<Self, EntitySaveLimitError> {
        let mut limits = HashMap::new();
        for (key, value) in entries {
            if !rivet_registry::generated::registries::ENTITY_TYPE_BY_NAME
                .contains_key(key.to_string().as_str())
            {
                return Err(EntitySaveLimitError(key));
            }
            limits.insert(key, value);
        }
        Ok(Self(limits))
    }
    /// Invoke AFTER constructor/load on reading, BEFORE save on writing. A
    /// rejected native value was genuinely constructed and is not discarded.
    pub fn accepts(&self, id: &'static str, counts: &mut HashMap<&'static str, i32>) -> bool {
        let limit = self.0.get(&Identifier::parse(id)).copied().unwrap_or(-1);
        if limit < 0 {
            return true;
        }
        let count = counts.entry(id).or_default();
        if *count >= limit {
            return false;
        }
        *count = count.wrapping_add(1);
        true
    }
}
