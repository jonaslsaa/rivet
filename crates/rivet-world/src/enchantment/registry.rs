//! Data-driven [`EnchantmentRegistry`] backed by the generated MC 26.2
//! `SYNCHRONIZED_NBT` registry payloads and `ENCHANTMENT_TAG_BY_NAME` tags in
//! `rivet-registry`.

use std::io::Cursor;
use std::sync::LazyLock;

use indexmap::IndexMap;
use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_registry::Identifier;
use rivet_registry::generated::registry_data::SYNCHRONIZED_NBT;
use rivet_registry::generated::tags::ENCHANTMENT_TAG_BY_NAME;
use rivet_util::DataInputStream;

use crate::enchantment::definition::{Enchantment, EnchantmentDecodeError};

/// Errors returned when loading the vanilla [`EnchantmentRegistry`].
#[derive(Debug, thiserror::Error)]
pub enum EnchantmentRegistryError {
    #[error("missing 'minecraft:enchantment' entry in SYNCHRONIZED_NBT")]
    MissingRegistryData,
    #[error("NBT decode error for enchantment '{id}': {source}")]
    NbtDecode {
        id: String,
        #[source]
        source: std::io::Error,
    },
    #[error("expected compound root tag for enchantment '{0}'")]
    ExpectedCompound(String),
    #[error("invalid enchantment identifier '{0}'")]
    InvalidIdentifier(String),
    #[error("enchantment '{id}' decode error: {source}")]
    Decode {
        id: String,
        #[source]
        source: EnchantmentDecodeError,
    },
}

/// Immutable registry of [`Enchantment`] definitions preserving exact
/// synchronized insertion order (element ID == insertion index).
#[derive(Debug, Clone)]
pub struct EnchantmentRegistry {
    entries: IndexMap<Identifier, Enchantment>,
}

impl EnchantmentRegistry {
    /// Builds the vanilla MC 26.2 enchantment registry directly from
    /// `rivet_registry::generated::registry_data::SYNCHRONIZED_NBT`.
    pub fn load_vanilla() -> Result<Self, EnchantmentRegistryError> {
        let (_, elements) = SYNCHRONIZED_NBT
            .iter()
            .find(|(name, _)| *name == "minecraft:enchantment")
            .ok_or(EnchantmentRegistryError::MissingRegistryData)?;

        let mut entries = IndexMap::with_capacity(elements.len());
        for &(raw_id, bytes) in *elements {
            let id = Identifier::try_parse(raw_id)
                .ok_or_else(|| EnchantmentRegistryError::InvalidIdentifier(raw_id.to_owned()))?;
            let mut input = DataInputStream::new(Cursor::new(bytes));
            let tag = nbt_io::read_any_tag(&mut input, &mut NbtAccounter::unlimited_heap())
                .map_err(|source| EnchantmentRegistryError::NbtDecode {
                    id: raw_id.to_owned(),
                    source,
                })?;
            let Tag::Compound(compound) = tag else {
                return Err(EnchantmentRegistryError::ExpectedCompound(
                    raw_id.to_owned(),
                ));
            };
            let enchantment = Enchantment::from_nbt(id.clone(), &compound).map_err(|source| {
                EnchantmentRegistryError::Decode {
                    id: raw_id.to_owned(),
                    source,
                }
            })?;
            entries.insert(id, enchantment);
        }

        Ok(Self { entries })
    }

    /// Returns a shared reference to the lazily-loaded vanilla MC 26.2
    /// [`EnchantmentRegistry`].
    pub fn vanilla() -> &'static Self {
        static VANILLA: LazyLock<EnchantmentRegistry> = LazyLock::new(|| {
            EnchantmentRegistry::load_vanilla()
                .expect("generated vanilla minecraft:enchantment registry must decode cleanly")
        });
        &VANILLA
    }

    /// Creates an empty registry (useful for unit tests).
    pub fn empty() -> Self {
        Self {
            entries: IndexMap::new(),
        }
    }

    /// Inserts an enchantment into a custom registry.
    pub fn insert(&mut self, enchantment: Enchantment) {
        self.entries.insert(enchantment.id.clone(), enchantment);
    }

    /// Number of registered enchantments.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Looks up an enchantment by its [`Identifier`].
    pub fn get(&self, id: &Identifier) -> Option<&Enchantment> {
        self.entries.get(id)
    }

    /// Looks up an enchantment by string ID (e.g. `"minecraft:sharpness"`).
    pub fn get_by_str(&self, id: &str) -> Option<&Enchantment> {
        let parsed = Identifier::try_parse(id)?;
        self.entries.get(&parsed)
    }

    /// Looks up an enchantment by its dense insertion index.
    pub fn get_by_index(&self, index: usize) -> Option<&Enchantment> {
        self.entries.get_index(index).map(|(_, e)| e)
    }

    /// Returns the dense insertion index of an enchantment.
    pub fn index_of(&self, id: &Identifier) -> Option<usize> {
        self.entries.get_index_of(id)
    }

    /// Iterates all registered enchantments in exact insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Enchantment> {
        self.entries.values()
    }

    /// Resolves a vanilla enchantment tag (e.g. `"minecraft:in_enchanting_table"`,
    /// `"minecraft:tooltip_order"`, `"minecraft:curse"`) in exact tag order.
    pub fn tag_members(&self, tag_name: &str) -> Vec<&Enchantment> {
        let normalized = tag_name.strip_prefix('#').unwrap_or(tag_name);
        let Some(names) = ENCHANTMENT_TAG_BY_NAME.get(normalized) else {
            return Vec::new();
        };
        names
            .iter()
            .filter_map(|name| self.get_by_str(name))
            .collect()
    }

    /// Checks whether `id` belongs to the given vanilla enchantment tag.
    pub fn is_in_tag(&self, id: &Identifier, tag_name: &str) -> bool {
        let normalized = tag_name.strip_prefix('#').unwrap_or(tag_name);
        let id_str = id.to_string();
        ENCHANTMENT_TAG_BY_NAME
            .get(normalized)
            .is_some_and(|members| members.contains(&id_str.as_str()))
    }

    /// `Enchantment.areCompatible` checked via registry lookup.
    pub fn are_compatible(&self, a: &Identifier, b: &Identifier) -> bool {
        match (self.get(a), self.get(b)) {
            (Some(ea), Some(eb)) => Enchantment::are_compatible(ea, eb),
            _ => false,
        }
    }
}
