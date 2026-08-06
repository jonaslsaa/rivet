//! Port of `net.minecraft.tags.TagKey<T>` (MC 26.2).
//!
//! PROVENANCE: leaf of the `mc.tags` manifest unit. Java source:
//! `net/minecraft/tags/TagKey.java` (53 lines, 26.2).
//!
//! Binding model (OWNERSHIP.md §Registries, #107):
//! - `TagKey<T>` is a **value type**. Java's `Interners.newWeakInterner`
//!   (`VALUES.intern`) makes `==` value equality (a record over
//!   `(ResourceKey, Identifier)`); Rust models it with derived value semantics
//!   over the `registry` + `location` fields. No interning, no interning map.
//! - `Eq`/`Hash`/`PartialEq`/`Clone` are **hand-written with no `T` bound** —
//!   `_marker: PhantomData<fn() -> T>` is the only place `T` appears (see
//!   `resource_key.rs` for why `derive` is wrong here).
//! - `is_for`/`cast`/`toString` mirror Java.
//!
//! Codec boundary: `TagKey.codec`/`hashedCodec` belong here; `streamCodec` is
//! `rivet-protocol` (#126), NOT here.

use crate::Identifier;
use crate::ResourceKey;
use crate::identifier::identifier_codec;
use crate::registry::Registry;

use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;

use std::marker::PhantomData;
use std::sync::Arc;

/// `net.minecraft.tags.TagKey<T>`.
///
/// Java's record fields: `registry` (a `ResourceKey<? extends Registry<T>>`)
/// and `location` (an `Identifier`). Both are `pub(crate)` so `registry()` /
/// `location()` and the sibling ownership modules can read them.
///
/// `Eq`/`Hash`/`PartialEq` are **hand-written with no `T` bound** — the #107
/// binding (OWNERSHIP.md §Registries); see `resource_key.rs` for why
/// `derive(PartialEq)` is wrong here.
#[derive(Debug)]
pub struct TagKey<T> {
    /// The tag's registry key (Java record component `registry`).
    pub(crate) registry: ResourceKey<Registry<T>>,
    /// The tag's identifier (Java record component `location`).
    pub(crate) location: Identifier,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Clone for TagKey<T> {
    fn clone(&self) -> Self {
        TagKey {
            registry: self.registry.clone(),
            location: self.location.clone(),
            _marker: PhantomData,
        }
    }
}

impl<T> PartialEq for TagKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.registry == other.registry && self.location == other.location
    }
}

impl<T> Eq for TagKey<T> {}

impl<T> std::hash::Hash for TagKey<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.registry.hash(state);
        self.location.hash(state);
    }
}

/// Java's `toString()` — `"TagKey[" + registry.identifier() + " / " + location + "]"`.
impl<T> std::fmt::Display for TagKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TagKey[{} / {}]",
            self.registry.identifier(),
            self.location
        )
    }
}

impl<T> TagKey<T> {
    /// `TagKey.create(ResourceKey<? extends Registry<T>>, Identifier)` — the
    /// value-typed construction (Java interns; Rust just builds the value).
    pub fn create(registry: &ResourceKey<Registry<T>>, location: Identifier) -> TagKey<T> {
        TagKey {
            registry: registry.clone(),
            location,
            _marker: PhantomData,
        }
    }

    /// `TagKey.registry()`.
    pub fn registry(&self) -> &ResourceKey<Registry<T>> {
        &self.registry
    }

    /// `TagKey.location()`.
    pub fn location(&self) -> &Identifier {
        &self.location
    }

    /// `TagKey.isFor(ResourceKey<? extends Registry<?>>)`.
    ///
    /// Java compares by reference (`this.registry == registry`) on the
    /// interned `ResourceKey`; Rust's value-typed `ResourceKey` compares by
    /// value. The two registry keys are compared by their identifiers (the
    /// phantom element types differ, so `==` on the full keys would not
    /// compile).
    pub fn is_for<E>(&self, registry: &ResourceKey<Registry<E>>) -> bool {
        self.registry.identifier() == registry.identifier()
    }

    /// `TagKey.cast(ResourceKey<? extends Registry<E>>)` — `Optional`.
    ///
    /// Java's `Optional.of((TagKey<E>)this)` is a type-erased
    /// re-interpretation; Rust re-constructs the value with the same
    /// `registry`/`location` identifiers and a new phantom marker.
    pub fn cast<E>(&self, registry: &ResourceKey<Registry<E>>) -> Option<TagKey<E>> {
        if self.is_for(registry) {
            Some(TagKey {
                registry: ResourceKey::create_registry_key(self.registry.identifier().clone()),
                location: self.location.clone(),
                _marker: PhantomData,
            })
        } else {
            None
        }
    }
}

/// `TagKey.codec(ResourceKey<? extends Registry<T>>)` —
/// `Identifier.CODEC.xmap(name -> create(registryName, name), TagKey::location)`.
pub fn tag_key_codec<T, Ops: DynamicOps + 'static>(
    registry_name: &ResourceKey<Registry<T>>,
) -> Arc<dyn Codec<TagKey<T>, Ops>>
where
    T: 'static,
{
    let registry_name = registry_name.clone();
    codec::xmap::<Identifier, TagKey<T>, Ops>(
        identifier_codec::<Ops>(),
        Arc::new(move |location: &Identifier| TagKey::create(&registry_name, location.clone())),
        Arc::new(|tag: &TagKey<T>| tag.location().clone()),
    )
}

/// `TagKey.hashedCodec(ResourceKey<? extends Registry<T>>)` —
/// `Codec.STRING.comapFlatMap(..., e -> "#" + e.location)`.
///
/// Encodes as `"#<location>"`; decodes by stripping the leading `#` and
/// reading the location. Java errors with `"Not a tag id"` when the string
/// does not start with `#`, and delegates the rest to `Identifier.read`.
pub fn tag_key_hashed_codec<T, Ops: DynamicOps + 'static>(
    registry_name: &ResourceKey<Registry<T>>,
) -> Arc<dyn Codec<TagKey<T>, Ops>>
where
    T: 'static,
{
    let registry_name = registry_name.clone();
    codec::comap_flat_map::<String, TagKey<T>, Ops>(
        codec::string_codec::<Ops>(),
        Arc::new(move |input: &String| {
            if let Some(rest) = input.strip_prefix('#') {
                Identifier::read(rest).map(|id| TagKey::create(&registry_name, id.clone()))
            } else {
                DataResult::error("Not a tag id")
            }
        }),
        Arc::new(|tag: &TagKey<T>| format!("#{}", tag.location())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use rivet_serialization::json_ops::JsonOps;
    use std::collections::HashSet;

    #[derive(Debug)]
    struct TestElement;
    #[derive(Debug)]
    struct OtherElement;

    fn registry_key(path: &str) -> ResourceKey<Registry<TestElement>> {
        ResourceKey::create_registry_key(Identifier::parse(path))
    }

    fn tag(path: &str) -> TagKey<TestElement> {
        let registry = registry_key("minecraft:block");
        TagKey::create(&registry, Identifier::parse(path))
    }

    #[test]
    fn create_sets_registry_and_location() {
        let registry = registry_key("minecraft:block");
        let t = TagKey::create(&registry, Identifier::parse("minecraft:stone"));
        assert_eq!(t.registry(), &registry);
        assert_eq!(t.location(), &Identifier::parse("minecraft:stone"));
    }

    #[test]
    fn value_equality_no_t_bound() {
        let a = tag("minecraft:stone");
        let b = tag("minecraft:stone");
        assert_eq!(a, b);
        assert_ne!(a, tag("minecraft:dirt"));
        assert_ne!(a, tag("foo:stone"));

        // A value-equal tag of a different element type compares equal — no
        // `T: PartialEq` bound. `assert_eq!` needs same-typed operands, so
        // compare the erased fields instead.
        let other: TagKey<OtherElement> = TagKey::create(
            &ResourceKey::create_registry_key(Identifier::parse("minecraft:block")),
            Identifier::parse("minecraft:stone"),
        );
        assert_eq!(a.location(), other.location());
        assert_eq!(a.registry().identifier(), other.registry().identifier());
    }

    #[test]
    fn hash_consistent_with_eq() {
        let a = tag("minecraft:stone");
        let b = tag("minecraft:stone");
        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&tag("minecraft:dirt")));
    }

    #[test]
    fn display_matches_java() {
        let t = tag("minecraft:stone");
        assert_eq!(t.to_string(), "TagKey[minecraft:block / minecraft:stone]");
    }

    #[test]
    fn is_for_compares_registry_value() {
        let block = tag("minecraft:stone");
        let block_registry: ResourceKey<Registry<TestElement>> =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:block"));
        let dirt_registry: ResourceKey<Registry<TestElement>> =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:dirt"));
        assert!(block.is_for(&block_registry));
        assert!(!block.is_for(&dirt_registry));
    }

    #[test]
    fn cast_reinterprets_when_is_for() {
        let block_registry: ResourceKey<Registry<OtherElement>> =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:block"));
        let stone = tag("minecraft:stone");
        let cast: TagKey<OtherElement> = stone.cast(&block_registry).unwrap();
        assert_eq!(cast.location(), &Identifier::parse("minecraft:stone"));
        let wrong: ResourceKey<Registry<OtherElement>> =
            ResourceKey::create_registry_key(Identifier::parse("minecraft:dirt"));
        assert!(stone.cast::<OtherElement>(&wrong).is_none());
    }

    #[test]
    fn tag_key_codec_roundtrips() {
        let ops = JsonOps::INSTANCE;
        let registry = registry_key("minecraft:block");
        let codec = tag_key_codec::<TestElement, JsonOps>(&registry);
        let t = TagKey::create(&registry, Identifier::parse("minecraft:stone"));
        let encoded = codec.encode_start(&ops, &t).get_or_throw("encode").clone();
        assert_eq!(encoded, ops.create_string("minecraft:stone".to_string()));
        let input = ops.create_string("minecraft:stone".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0.location(), &Identifier::parse("minecraft:stone"));
    }

    #[test]
    fn tag_key_hashed_codec_roundtrips() {
        let ops = JsonOps::INSTANCE;
        let registry = registry_key("minecraft:block");
        let codec = tag_key_hashed_codec::<TestElement, JsonOps>(&registry);
        let t = TagKey::create(&registry, Identifier::parse("minecraft:stone"));
        let encoded = codec.encode_start(&ops, &t).get_or_throw("encode").clone();
        assert_eq!(encoded, ops.create_string("#minecraft:stone".to_string()));
        let input = ops.create_string("#minecraft:stone".to_string());
        let decoded = codec.decode(&ops, &input).get_or_throw("decode").clone();
        assert_eq!(decoded.0.location(), &Identifier::parse("minecraft:stone"));
    }

    #[test]
    fn tag_key_hashed_codec_rejects_missing_hash() {
        let ops = JsonOps::INSTANCE;
        let registry = registry_key("minecraft:block");
        let codec = tag_key_hashed_codec::<TestElement, JsonOps>(&registry);
        let input = ops.create_string("minecraft:stone".to_string());
        let result = codec.decode(&ops, &input);
        assert!(result.result().is_none());
    }
}
