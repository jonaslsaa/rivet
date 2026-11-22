//! Data-driven [`EnchantmentRegistry`] backed by `rivet_registry::Registry<Enchantment>`,
//! populated from the generated MC 26.2 `SYNCHRONIZED_NBT` registry payloads and
//! `ENCHANTMENT_TAG_BY_NAME` tags in `rivet-registry`.

use std::io::Cursor;
use std::sync::{Arc, LazyLock};

use rivet_nbt::nbt_accounter::NbtAccounter;
use rivet_nbt::nbt_io;
use rivet_nbt::tag::Tag;
use rivet_registry::generated::registry_data::SYNCHRONIZED_NBT;
use rivet_registry::generated::tags::ENCHANTMENT_TAG_BY_NAME;
use rivet_registry::{
    HolderId, Identifier, RegistrationInfo, Registry, RegistryBuilder, RegistryId, ResourceKey,
    TagKey,
};
use rivet_util::DataInputStream;

use crate::enchantment::definition::{Enchantment, EnchantmentDecodeError, NamedHolderSet};

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

/// Frozen registry of [`Enchantment`] definitions backed by
/// [`rivet_registry::Registry<Enchantment>`], preserving exact synchronized
/// insertion order (element ID == `HolderId` == insertion index) and bound
/// enchantment tags.
#[derive(Debug)]
pub struct EnchantmentRegistry {
    registry: Registry<Enchantment>,
}

impl EnchantmentRegistry {
    /// Returns the `ResourceKey<Registry<Enchantment>>` for `minecraft:enchantment`.
    pub fn registry_key() -> ResourceKey<Registry<Enchantment>> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("enchantment"))
    }

    /// Creates a fresh [`RegistryBuilder<Enchantment>`] keyed by `minecraft:enchantment`.
    pub fn builder() -> RegistryBuilder<Enchantment> {
        RegistryBuilder::new(&Self::registry_key())
    }

    /// Wraps an already-frozen [`Registry<Enchantment>`].
    pub fn from_registry(registry: Registry<Enchantment>) -> Self {
        Self { registry }
    }

    /// Borrows the underlying frozen [`Registry<Enchantment>`].
    pub fn as_registry(&self) -> &Registry<Enchantment> {
        &self.registry
    }

    /// Returns the unique [`RegistryId`] of the underlying frozen registry.
    pub fn registry_id(&self) -> RegistryId {
        self.registry.registry_id()
    }

    /// Builds the vanilla MC 26.2 enchantment registry directly from
    /// `rivet_registry::generated::registry_data::SYNCHRONIZED_NBT` and binds
    /// all generated enchantment tags from `ENCHANTMENT_TAG_BY_NAME`.
    pub fn load_vanilla() -> Result<Self, EnchantmentRegistryError> {
        let (_, elements) = SYNCHRONIZED_NBT
            .iter()
            .find(|(name, _)| *name == "minecraft:enchantment")
            .ok_or(EnchantmentRegistryError::MissingRegistryData)?;

        let reg_key = Self::registry_key();
        let mut builder = RegistryBuilder::new(&reg_key);
        let mut holder_by_id: std::collections::HashMap<String, HolderId> =
            std::collections::HashMap::with_capacity(elements.len());

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
            let resource_key = ResourceKey::create(&reg_key, id);
            let holder_id = builder.register(
                &resource_key,
                Arc::new(enchantment),
                RegistrationInfo::BUILT_IN,
            );
            holder_by_id.insert(raw_id.to_owned(), holder_id);
        }

        let mut bound_tags: Vec<(TagKey<Enchantment>, Vec<HolderId>)> =
            Vec::with_capacity(ENCHANTMENT_TAG_BY_NAME.len());
        for (&tag_name, &members) in ENCHANTMENT_TAG_BY_NAME.entries() {
            let Some(tag_id) = Identifier::try_parse(tag_name) else {
                continue;
            };
            let tag_key = TagKey::create(&reg_key, tag_id);
            let holders = members
                .iter()
                .filter_map(|member| holder_by_id.get(*member).copied())
                .collect();
            bound_tags.push((tag_key, holders));
        }
        builder.bind_tags(bound_tags);

        Ok(Self {
            registry: builder.freeze(),
        })
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

    /// Creates an empty frozen registry (useful for unit tests).
    pub fn empty() -> Self {
        Self {
            registry: Self::builder().freeze(),
        }
    }

    /// Number of registered enchantments.
    pub fn len(&self) -> usize {
        self.registry.size().max(0) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    /// Looks up an enchantment by its [`Identifier`].
    pub fn get(&self, id: &Identifier) -> Option<&Enchantment> {
        self.registry.get_optional(id)
    }

    /// Looks up an enchantment by string ID (e.g. `"minecraft:sharpness"`).
    pub fn get_by_str(&self, id: &str) -> Option<&Enchantment> {
        let parsed = Identifier::try_parse(id)?;
        self.registry.get_optional(&parsed)
    }

    /// Looks up an enchantment by its dense insertion index.
    pub fn get_by_index(&self, index: usize) -> Option<&Enchantment> {
        i32::try_from(index)
            .ok()
            .and_then(|id| self.registry.by_id(id))
    }

    /// Returns the dense insertion index of an enchantment.
    pub fn index_of(&self, id: &Identifier) -> Option<usize> {
        let ench = self.registry.get_optional(id)?;
        let raw = self.registry.get_id(ench);
        usize::try_from(raw).ok()
    }

    /// Iterates all registered enchantments in exact insertion order.
    pub fn iter(&self) -> impl Iterator<Item = &Enchantment> {
        (0..self.registry.size()).filter_map(|id| self.registry.by_id(id))
    }

    /// Resolves a bound enchantment tag (e.g. `"minecraft:in_enchanting_table"`,
    /// `"minecraft:tooltip_order"`, `"minecraft:curse"`, or unqualified
    /// `"in_enchanting_table"` / `"#in_enchanting_table"`) in exact tag order.
    pub fn tag_members(&self, tag_name: &str) -> Vec<&Enchantment> {
        let raw = tag_name.strip_prefix('#').unwrap_or(tag_name);
        let Some(tag_id) = Identifier::try_parse(raw) else {
            return Vec::new();
        };
        let tag_key = TagKey::create(&Self::registry_key(), tag_id);
        self.registry
            .get_tag_or_empty(&tag_key)
            .iter()
            .filter_map(|holder_id| {
                i32::try_from(holder_id.0)
                    .ok()
                    .and_then(|id| self.registry.by_id(id))
            })
            .collect()
    }

    /// Checks whether `id` belongs to the given bound enchantment tag.
    pub fn is_in_tag(&self, id: &Identifier, tag_name: &str) -> bool {
        let Some(target_idx) = self.index_of(id).and_then(|i| u32::try_from(i).ok()) else {
            return false;
        };
        let raw = tag_name.strip_prefix('#').unwrap_or(tag_name);
        let Some(tag_id) = Identifier::try_parse(raw) else {
            return false;
        };
        let tag_key = TagKey::create(&Self::registry_key(), tag_id);
        self.registry
            .get_tag_or_empty(&tag_key)
            .iter()
            .any(|holder_id| holder_id.0 == target_idx)
    }

    /// Checks whether `set` contains enchantment `id`, resolving tag sets against
    /// this registry's bound tags (matching Paper's `HolderSet<Enchantment>.contains`).
    pub fn holder_set_contains_enchantment(&self, set: &NamedHolderSet, id: &Identifier) -> bool {
        match set {
            NamedHolderSet::Empty => false,
            NamedHolderSet::Tag(tag) => self.is_in_tag(id, tag),
            NamedHolderSet::Direct(ids) => ids.contains(id),
        }
    }

    /// `Enchantment.areCompatible` checked via registry lookup, resolving
    /// `exclusive_set` tags against this registry's bound tags.
    pub fn are_compatible(&self, a: &Identifier, b: &Identifier) -> bool {
        if a == b {
            return false;
        }
        match (self.get(a), self.get(b)) {
            (Some(ea), Some(eb)) => {
                !self.holder_set_contains_enchantment(&ea.exclusive_set, b)
                    && !self.holder_set_contains_enchantment(&eb.exclusive_set, a)
            }
            _ => false,
        }
    }
}
