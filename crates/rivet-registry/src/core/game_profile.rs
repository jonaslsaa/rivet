//! Port of the authlib profile value types — `com.mojang.authlib.GameProfile`,
//! `com.mojang.authlib.properties.Property`, `com.mojang.authlib.properties.PropertyMap`
//! (issue #198).
//!
//! Java source: the authlib 9.0.75 sources jar bundled with the pinned Paper
//! (paperweight `minecraft-sources`). These are **not** `net.minecraft.*`
//! classes — they come from the authlib jar — but they are pure value types the
//! login wire codec needs (`ByteBufCodecs.GAME_PROFILE` /
//! `GAME_PROFILE_PROPERTIES` live in `rivet-protocol`). Placement follows the
//! documented `GameType`/`ChunkPos` precedent in OWNERSHIP.md §Registries: pure
//! value types stay in `rivet-registry::core`, with only their `StreamCodec`
//! impls crossing to `rivet-protocol`. There is no authlib-mirror crate below
//! `rivet-protocol`, and `rivet-protocol` cannot depend on one (nothing owns
//! authlib's launcher/session scope — deliberately out of #198), so the profile
//! model lives here, the same class of immutable, registry-independent value
//! type as `UUIDUtil`.
//!
//! Wire behavior preserved:
//! - `GameProfile` is Java's record `(id, name, properties)`; every component
//!   is non-null (the canonical ctor `Objects.requireNonNull`s each). Record
//!   equality is all-three-components, so the Rust `derive`s are faithful.
//! - `Property` is the record `(name, value, signature)` with the 2-arg ctor
//!   `(name, value)` → `signature = null` (Rust `None`). The deprecated
//!   `isSignatureValid(PublicKey)` (SHA1withRSA) is out of #198 scope and not
//!   ported.
//! - `PropertyMap` is a Guava `ForwardingMultimap` over an `ImmutableListMultimap`
//!   (verified from guava 33.6.0: `ImmutableMultimap.builder()` uses a
//!   `LinkedHashMap` + per-key `ImmutableList`, and `ImmutableMultimap.copyOf`
//!   rebuilds from `asMap().entrySet()`, so `values()` **always** iterates
//!   key-grouped: all values of the first-seen key, then the second, etc.,
//!   duplicates preserved within a key — Java can never produce a non-grouped
//!   sequence). The wire codec writes `properties.values()` in that order, so
//!   [`PropertyMap::new`] re-groups its input into key-grouped order exactly as
//!   guava does on build; the map stores a flat `Vec<Property>` in that grouped
//!   order — never a `HashMap`.
//!
//! Equality/hash deliberately match guava's *multimap* semantics rather than a
//! plain `Vec`: `Multimap.equals` is `asMap().equals`, i.e. order-insensitive
//! **across** keys but order-sensitive **within** each key (each key's value
//! collection is an `ImmutableList`). The derived `Vec` equality would diverge
//! when the same key→value pairs are inserted in a different key order — the
//! map view is equal in Java but the flattened vector is not. Wire bytes *do*
//! depend on insertion order; equality deliberately does not, keeping the two
//! separate exactly like Java.

use rivet_util::mth::Uuid;
use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

/// `com.mojang.authlib.properties.Property` — the record `(name, value,
/// signature)`, with `signature` nullable.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Property {
    name: String,
    value: String,
    signature: Option<String>,
}

impl Property {
    /// `new Property(String name, String value)` — `signature = null`.
    pub fn new(name: String, value: String) -> Self {
        Property {
            name,
            value,
            signature: None,
        }
    }

    /// `new Property(String name, String value, @Nullable String signature)`.
    pub fn new_with_signature(name: String, value: String, signature: Option<String>) -> Self {
        Property {
            name,
            value,
            signature,
        }
    }

    /// `Property.name()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `Property.value()`.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// `Property.signature()` — the `@Nullable` signature (Java null ⇄ Rust
    /// `None`).
    pub fn signature(&self) -> Option<&str> {
        self.signature.as_deref()
    }

    /// `Property.hasSignature()`.
    pub fn has_signature(&self) -> bool {
        self.signature.is_some()
    }
}

/// `com.mojang.authlib.properties.PropertyMap` — the ordered
/// `Multimap<String, Property>`.
///
/// Backed by a flat `Vec<Property>` in key-grouped order (see the module doc for
/// why: `new` re-groups like guava's `ImmutableMultimap.copyOf`, so `values()`
/// is always key-grouped exactly as Java's is). Equality/hash use multimap
/// semantics (order-insensitive across keys, order-sensitive within a key).
#[derive(Clone, Debug, Default)]
pub struct PropertyMap {
    properties: Vec<Property>,
}

/// `PropertyMap.EMPTY` — `new PropertyMap(ImmutableMultimap.of())`.
impl PropertyMap {
    /// `PropertyMap.EMPTY`.
    pub fn empty() -> Self {
        PropertyMap::default()
    }

    /// `new PropertyMap(Multimap<String, Property>)` — the ctor copies into an
    /// immutable multimap, which guava *re-groups* by key (`ImmutableMultimap
    /// .copyOf` rebuilds from `asMap().entrySet()`, so `values()` always
    /// iterates all of key-1, then key-2, …). Mirror that: interleaved input
    /// like `[t1(textures), c1(capes), t2(textures)]` is stored as
    /// `[t1, t2, c1]`, so `values()` is the key-grouped sequence Java would
    /// emit — not raw insertion order.
    pub fn new(properties: Vec<Property>) -> Self {
        PropertyMap {
            properties: group_in_key_order(properties),
        }
    }

    /// `PropertyMap.size()` — the total entry count (Java `Multimap.size()`).
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// Whether the multimap is empty.
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// `PropertyMap.values()` — key-grouped insertion order, duplicates
    /// included (the wire codec encodes exactly this sequence).
    pub fn values(&self) -> &[Property] {
        &self.properties
    }

    /// `Multimap.get(key)` — the values for one key in insertion order.
    pub fn get(&self, name: &str) -> Vec<&Property> {
        self.properties.iter().filter(|p| p.name == name).collect()
    }
}

impl PartialEq for PropertyMap {
    fn eq(&self, other: &Self) -> bool {
        // `Multimap.equals` = `asMap().equals`: order-insensitive across keys,
        // but each key's value collection (guava `ImmutableList`) compares in
        // order. Grouping into a `HashMap` (content-equal regardless of
        // iteration order) over per-key `Vec<&Property>` (order-sensitive)
        // reproduces exactly that.
        group_by_key(&self.properties) == group_by_key(&other.properties)
    }
}

impl Eq for PropertyMap {}

impl Hash for PropertyMap {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Java `Multimap.hashCode` = `asMap().hashCode()` = a sum over distinct
        // keys of `key.hash ^ valueCollection.hash` (order-insensitive across
        // keys, order-sensitive within a key). A `BTreeMap` (sorted key order)
        // feeds the same structure deterministically: hashing keys in sorted
        // order makes the result independent of insertion order exactly as the
        // sum does in Java, while each key's `Vec` keeps per-key order.
        let mut groups: BTreeMap<&str, Vec<&Property>> = BTreeMap::new();
        for p in &self.properties {
            groups.entry(p.name()).or_default().push(p);
        }
        for (name, values) in &groups {
            name.hash(state);
            values.len().hash(state);
            for value in values {
                value.hash(state);
            }
        }
    }
}

fn group_by_key(properties: &[Property]) -> HashMap<&str, Vec<&Property>> {
    let mut groups: HashMap<&str, Vec<&Property>> = HashMap::new();
    for p in properties {
        groups.entry(p.name()).or_default().push(p);
    }
    groups
}

/// `ImmutableMultimap.copyOf` / `ImmutableMultimap.Builder.build()` regrouping:
/// all entries of the first-seen key, then the second, …, per-key insertion
/// order preserved. Key-first-seen order and per-key order are exactly guava's
/// `values()` (verified from 33.6.0: `fromMapBuilderEntries` iterates the
/// builder's `LinkedHashMap` in put order).
fn group_in_key_order(properties: Vec<Property>) -> Vec<Property> {
    let mut seen: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Property>> = HashMap::new();
    let capacity = properties.len();
    for p in properties {
        if !groups.contains_key(p.name()) {
            seen.push(p.name().to_string());
        }
        groups.entry(p.name().to_string()).or_default().push(p);
    }
    let mut out = Vec::with_capacity(capacity);
    for name in seen {
        out.extend(groups.remove(&name).unwrap());
    }
    out
}

/// `com.mojang.authlib.GameProfile` — the record `(id, name, properties)`.
///
/// Java's canonical ctor `requireNonNull`s each component (NPE on null), so
/// `Uuid`/`String`/`PropertyMap` are all non-null here. Record equality/hash
/// are all-three-components; the `derive`s match (PropertyMap supplies the
/// multimap semantics above).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GameProfile {
    id: Uuid,
    name: String,
    properties: PropertyMap,
}

impl GameProfile {
    /// The record's canonical constructor.
    pub fn new(id: Uuid, name: String, properties: PropertyMap) -> Self {
        GameProfile {
            id,
            name,
            properties,
        }
    }

    /// `new GameProfile(UUID id, String name)` — `properties = PropertyMap.EMPTY`.
    pub fn new_without_properties(id: Uuid, name: String) -> Self {
        GameProfile::new(id, name, PropertyMap::empty())
    }

    /// `GameProfile.id()`.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// `GameProfile.name()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `GameProfile.properties()`.
    pub fn properties(&self) -> &PropertyMap {
        &self.properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uuid() -> Uuid {
        Uuid {
            most: 0x0a9f_fa92_a706_3e6f,
            // The least half has the high bit set; read as the signed i64.
            least: 0x900c_f12f_869d_37eau64 as i64,
        }
    }

    fn prop(name: &str, value: &str) -> Property {
        Property::new(name.to_string(), value.to_string())
    }

    #[test]
    fn property_signature_optional() {
        let unsigned = Property::new("textures".to_string(), "abc".to_string());
        assert!(!unsigned.has_signature());
        assert_eq!(unsigned.signature(), None);
        assert_eq!(unsigned.name(), "textures");
        assert_eq!(unsigned.value(), "abc");

        let signed = Property::new_with_signature(
            "textures".to_string(),
            "abc".to_string(),
            Some("sig".to_string()),
        );
        assert!(signed.has_signature());
        assert_eq!(signed.signature(), Some("sig"));
    }

    #[test]
    fn property_equality_includes_signature_presence() {
        // Java record equality: `signature = null` vs `Some` differ.
        let a = prop("textures", "abc");
        let b = Property::new_with_signature(
            "textures".to_string(),
            "abc".to_string(),
            Some("".to_string()),
        );
        assert_ne!(a, b);
    }

    #[test]
    fn property_map_new_groups_by_key_in_first_seen_order() {
        // guava `ImmutableMultimap.copyOf` / `Builder.build()` *re-group* by
        // key, so `values()` is always key-grouped (all textures, then capes)
        // — never the raw insertion order of the input. Verified live against
        // guava 33.6.0: input `[t1(textures), c1(capes), t2(textures)]` ->
        // `values()` = `[t1, t2, c1]`.
        let props = vec![
            prop("textures", "t1"),
            prop("capes", "c1"),
            prop("textures", "t2"),
        ];
        let map = PropertyMap::new(props);
        assert_eq!(map.len(), 3);
        assert!(!map.is_empty());
        assert_eq!(
            map.values(),
            &[
                prop("textures", "t1"),
                prop("textures", "t2"),
                prop("capes", "c1"),
            ][..]
        );
        // Multimap.get(key) = the per-key values in insertion order.
        let textures = map.get("textures");
        assert_eq!(textures.len(), 2);
        assert_eq!(textures[0].value(), "t1");
        assert_eq!(textures[1].value(), "t2");
        assert_eq!(map.get("capes")[0].value(), "c1");
        assert!(map.get("missing").is_empty());
    }

    #[test]
    fn property_map_grouping_is_idempotent_and_preserves_per_key_order() {
        // Grouping an already-grouped input is a no-op (decode -> encode stays
        // byte-identical), and the per-key insertion order survives.
        let grouped = vec![
            prop("textures", "t1"),
            prop("textures", "t2"),
            prop("capes", "c1"),
        ];
        let map = PropertyMap::new(grouped.clone());
        assert_eq!(map.values(), grouped.as_slice());
        // Three distinct keys keep first-seen order.
        let map = PropertyMap::new(vec![
            prop("a", "1"),
            prop("b", "1"),
            prop("c", "1"),
            prop("a", "2"),
            prop("b", "2"),
        ]);
        assert_eq!(
            map.values(),
            &[
                prop("a", "1"),
                prop("a", "2"),
                prop("b", "1"),
                prop("b", "2"),
                prop("c", "1"),
            ][..]
        );
    }

    #[test]
    fn property_map_empty() {
        let empty = PropertyMap::empty();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.values(), &[] as &[Property]);
    }

    #[test]
    fn property_map_equality_is_multimap_not_flattened_vec() {
        // Java `Multimap.equals` = `asMap().equals`: order-insensitive across
        // keys (the map view is an entry set), so interleaving keys still
        // compares equal...
        let a = PropertyMap::new(vec![prop("textures", "t1"), prop("capes", "c1")]);
        let b = PropertyMap::new(vec![prop("capes", "c1"), prop("textures", "t1")]);
        assert_eq!(a, b);
        // ...but order-sensitive within a key (each key's collection is an
        // ImmutableList).
        let c = PropertyMap::new(vec![prop("textures", "t1"), prop("textures", "t2")]);
        let d = PropertyMap::new(vec![prop("textures", "t2"), prop("textures", "t1")]);
        assert_ne!(c, d);
        // Duplicate-count differences matter (multiset within the key).
        let e = PropertyMap::new(vec![prop("textures", "t1"), prop("textures", "t1")]);
        assert_ne!(c, e);
    }

    #[test]
    fn property_map_hash_consistent_with_equality() {
        // The required `a == b ⇒ hash(a) == hash(b)` contract across the
        // multimap equality above.
        use std::hash::{Hash, Hasher};
        fn hash_of(map: &PropertyMap) -> u64 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            map.hash(&mut hasher);
            hasher.finish()
        }
        let a = PropertyMap::new(vec![prop("textures", "t1"), prop("capes", "c1")]);
        let b = PropertyMap::new(vec![prop("capes", "c1"), prop("textures", "t1")]);
        assert_eq!(a, b);
        assert_eq!(hash_of(&a), hash_of(&b));

        let c = PropertyMap::new(vec![prop("textures", "t1"), prop("textures", "t2")]);
        let d = PropertyMap::new(vec![prop("textures", "t2"), prop("textures", "t1")]);
        assert_ne!(c, d);
        assert_ne!(hash_of(&c), hash_of(&d));
    }

    #[test]
    fn game_profile_record_semantics() {
        let id = uuid();
        let profile = GameProfile::new_without_properties(id, "RivetProbe".to_string());
        assert_eq!(profile.id(), id);
        assert_eq!(profile.name(), "RivetProbe");
        assert!(profile.properties().is_empty());

        // The 2-arg ctor uses PropertyMap.EMPTY; equality is all three
        // components (Java record).
        let other = GameProfile::new_without_properties(id, "RivetProbe".to_string());
        assert_eq!(profile, other);
        let renamed = GameProfile::new_without_properties(id, "Other".to_string());
        assert_ne!(profile, renamed);
        let with_props = GameProfile::new(
            id,
            "RivetProbe".to_string(),
            PropertyMap::new(vec![prop("textures", "t1")]),
        );
        assert_ne!(profile, with_props);
    }
}
