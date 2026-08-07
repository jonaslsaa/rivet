//! Holder types for `net.minecraft.core` — the #126 holder surface.
//!
//! PROVENANCE: `Holder.java` (278 lines, 26.2), a leaf of the `mc.core`
//! manifest unit.
//!
//! #124 SCC (`RegistryId`, `HolderId`): the registry-internal id space. The
//! frozen `Registry<T>`/`RegistryBuilder<T>` (ownership B) still talk in these
//! Copy ids; **#126 does not edit registry.rs/builder.rs** (their ownership),
//! so `RegistryId`/`HolderId` stay defined here exactly as the SCC left them.
//!
//! #126 holder surface (this file + `holder_set.rs` + `holder_lookup.rs` +
//! `registry_file_codec.rs`): the real `Holder<T>` value type and the
//! `HolderSet<T>`/`HolderGetter<T>`/`HolderLookup<T>`/`HolderOwner<T>` surface.
//!
//! Binding model (OWNERSHIP.md §Registries, decision sub-issues C + E + G):
//! - **`Holder<T>` is an ID, not a value.** `Direct(T)` (unregistered,
//!   decode-only) or `Reference { registry: RegistryId, id: u32 }` (Copy, 8
//!   bytes). No stored `Arc`/`&Registry` in game state (FFI marshal IDs).
//! - **Back-reference rule:** `value()/key()/is()/tags()/unwrap()` resolve
//!   through the owning `&HolderLookup<T>` — the Java holder stores its key/
//!   value/tags inline (its `Reference` is a mutable, rebindable object), the
//!   Rust holder does not, so every data-resolving method takes the lookup.
//!   The pure-ID surface (`kind`, `is_bound`, `can_serialize_in`) is free of a
//!   lookup.
//! - **Holder identity is observable:** repeated `lookup.get(key)` constructs
//!   the same `Reference { registry, id }` value, and `RegistryId + id` is the
//!   identity contract (element id == holder id == network id == insertion
//!   index, OWNERSHIP.md). `HolderSet.Direct.contains` therefore compares
//!   holder values, which for `Reference` is the (registry, id) pair.
//! - **Owner checks are `RegistryId` O(1):** `can_serialize_in` compares the
//!   holder's registry id against the owner's (`context == this` pointer
//!   identity preserved via the per-instance `RegistryId`).
//!
//! - **Panic-message rendering:** both `value()` and `key()` panic with
//!   `"Trying to access unbound value '<render>' from registry <id>"` — Java's
//!   `Reference.value()`/`key()` both throw that literal message (a Mojang
//!   copy-paste quirk), `value()` rendering the holder's *key*, `key()`
//!   rendering its *value*. The Rust `(RegistryId, id)` reference stores
//!   neither, so `render_holder` resolves through the lookup and yields
//!   `"null"` when the id is unresolvable — byte-identical to Java's null
//!   key/value string concatenation.
//!
//! RivetTodo(#201): `components`/`are_components_bound` — `DataComponentMap` is
//! not ported yet (later `mc.core.component` scope); the variants exist without
//! a component payload. `Reference.Type` (STAND_ALONE vs INTRUSIVE) is not
//! ported — the SCC's builder assigns ids at `register` time, so the
//! stand-alone/intrusive distinction is unobservable in the pure-ID model.
//! `is(Holder)` (deprecated) is omitted — its only use is identity/value
//! comparison already covered by the other overloads.

use rivet_serialization::either::Either;

use crate::holder_lookup::{HolderLookup, HolderOwner};
use crate::{Identifier, ResourceKey, TagKey};

/// `RegistryId` — per-instance registry identity (a per-instance u32), distinct
/// from the `ResourceKey<Registry<T>>` key. OWNERSHIP.md §Registries. A
/// `RegistryBuilder` assigns one at construction (see `builder.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RegistryId(pub u32);

/// The minimal registry-held holder reference — Copy, 4 bytes, resolved through
/// the owning registry. Kept for the #124 SCC's `Registry<T>`/`RegistryBuilder<T>`
/// (ownership B, which #126 does not edit). `Holder::Reference` (this file) is
/// the #126 replacement; the SCC's id space (element id == holder id == network
/// id == insertion index) is already the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct HolderId(pub u32);

/// `Holder.Kind` — `REFERENCE`, `DIRECT` (Java `Holder.Kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HolderKind {
    /// `Holder.Kind.REFERENCE` — a registry-backed reference.
    Reference,
    /// `Holder.Kind.DIRECT` — an unregistered inline value.
    Direct,
}

/// `Holder.Kind.toString()` — the upper-case enum name, embedded in
/// `ExtraCodecs.ensureHomogenous`'s `"Mixed type list: ..."` message.
impl std::fmt::Display for HolderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HolderKind::Reference => write!(f, "REFERENCE"),
            HolderKind::Direct => write!(f, "DIRECT"),
        }
    }
}

/// `net.minecraft.core.Holder<T>` — `Direct(T)` or `Reference { registry, id }`.
///
/// A pure value type per OWNERSHIP.md §Registries: `Reference` is the Copy
/// 8-byte `(RegistryId, u32)` pair, `Direct` carries an owned inline value.
/// Data-resolving methods take a `&HolderLookup<T>` (the back-reference rule);
/// identity/kind/owner-check methods are lookup-free.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder<T> {
    /// `Holder.Direct<T>` — an unregistered, decode-only value.
    Direct(T),
    /// `Holder.Reference<T>` — a registry-backed reference, resolved through the
    /// owning registry by id.
    Reference {
        /// The owning registry's per-instance `RegistryId`.
        registry: RegistryId,
        /// The element id (== holder id == network id == insertion index).
        id: u32,
    },
}

impl<T> Holder<T> {
    /// `Holder.direct(T)` — a direct (unregistered, decode-only) holder.
    pub fn direct(value: T) -> Holder<T> {
        Holder::Direct(value)
    }

    /// The Copy `Reference { registry, id }` constructor.
    pub fn reference(registry: RegistryId, id: u32) -> Holder<T> {
        Holder::Reference { registry, id }
    }

    /// `Holder.kind()`.
    pub fn kind(&self) -> HolderKind {
        match self {
            Holder::Direct(_) => HolderKind::Direct,
            Holder::Reference { .. } => HolderKind::Reference,
        }
    }

    /// `Holder.isBound()`.
    ///
    /// Constant-true stub (like `are_components_bound` reports Java's steady
    /// state). Java `Reference.isBound()` = `key != null && value != null`, a
    /// runtime check on the holder's stored key/value; the pure-ID model stores
    /// neither, so a truthful lookup-free check is impossible. `Direct` is
    /// always bound, and a reference that cannot resolve its key/value is
    /// already an unresolvable/panic state (`value()`/`key()`), so this
    /// reports Java's normal-case `true`. A hand-built out-of-range reference
    /// is not distinguished here (resolving it would need the owning lookup).
    pub fn is_bound(&self) -> bool {
        true
    }

    /// `Holder.areComponentsBound()`.
    ///
    /// RivetTodo(#201): `DataComponentMap` is not ported; the surface
    /// reports Java's steady-state values (`Direct` true, `Reference` false) but
    /// no components are tracked.
    pub fn are_components_bound(&self) -> bool {
        match self {
            Holder::Direct(_) => true,
            Holder::Reference { .. } => false,
        }
    }

    /// `Holder.canSerializeIn(HolderOwner<T>)`.
    ///
    /// `Direct` serializes anywhere. A `Reference` is valid iff the owner is the
    /// registry it references — `RegistryId` O(1) compare (Java `context ==
    /// this` pointer identity on the owning registry, preserved via the
    /// per-instance id).
    pub fn can_serialize_in(&self, owner: &dyn HolderOwner<T>) -> bool {
        match self {
            Holder::Direct(_) => true,
            Holder::Reference { registry, .. } => *registry == owner.registry_id(),
        }
    }

    /// `Holder.value()` — resolve the value through the owning lookup.
    ///
    /// `Direct` returns the inline value; `Reference` resolves by id. Panics
    /// like `Reference.value()` on an unresolvable reference.
    pub fn value<'a>(&'a self, lookup: &'a dyn HolderLookup<T>) -> &'a T {
        match self {
            Holder::Direct(value) => value,
            Holder::Reference { registry, .. } => lookup.value_of(self).unwrap_or_else(|| {
                panic!(
                    "Trying to access unbound value '{}' from registry {}",
                    render_holder(self, lookup),
                    registry.0
                )
            }),
        }
    }

    /// `Holder.Reference.key()` — resolve the registry key through the lookup.
    /// A `Direct` holder has no key (Java `Holder` has no `key()`) — panics.
    ///
    /// Owned (a clone): `key_of` resolves through the lookup's public by-id
    /// surface, which cannot return a borrow into the registry's key vec (see
    /// `holder_lookup::HolderLookup::key_of`).
    pub fn key(&self, lookup: &dyn HolderLookup<T>) -> ResourceKey<T> {
        match self {
            Holder::Reference { registry, .. } => lookup.key_of(self).unwrap_or_else(|| {
                // Java `Reference.key()` throws "Trying to access unbound value"
                // (the same literal as `value()` — a Mojang copy-paste), rendering
                // the holder's value. Both key and value are unresolvable in the
                // panic state, so `render_holder` yields Java's "null".
                panic!(
                    "Trying to access unbound value '{}' from registry {}",
                    render_holder(self, lookup),
                    registry.0
                )
            }),
            Holder::Direct(_) => panic!("Direct holder has no key"),
        }
    }

    /// `Holder.is(Identifier)` — a `Direct` holder is never equal to a keyed
    /// lookup (Java `Direct.is` returns false).
    pub fn is_identifier(&self, lookup: &dyn HolderLookup<T>, id: &Identifier) -> bool {
        match self {
            Holder::Direct(_) => false,
            Holder::Reference { .. } => self.key(lookup).identifier() == id,
        }
    }

    /// `Holder.is(ResourceKey<T>)`.
    pub fn is_key(&self, lookup: &dyn HolderLookup<T>, key: &ResourceKey<T>) -> bool {
        match self {
            Holder::Direct(_) => false,
            Holder::Reference { .. } => &self.key(lookup) == key,
        }
    }

    /// `Holder.is(TagKey<T>)` — a `Direct` holder is in no named set.
    pub fn is_tag(&self, lookup: &dyn HolderLookup<T>, tag: &TagKey<T>) -> bool {
        match self {
            Holder::Direct(_) => false,
            Holder::Reference { .. } => lookup.tags_of(self).contains(tag),
        }
    }

    /// `Holder.tags()` — the named sets the holder belongs to (Direct: none).
    pub fn tags(&self, lookup: &dyn HolderLookup<T>) -> Vec<TagKey<T>> {
        match self {
            Holder::Direct(_) => Vec::new(),
            Holder::Reference { .. } => lookup.tags_of(self),
        }
    }

    /// `Holder.unwrap()` — `Either<ResourceKey<T>, &T>`.
    ///
    /// A `Reference` is the key (left), a `Direct` is the value (right) — Java
    /// `Reference.unwrap()` = `Either.left(key())`, `Direct.unwrap()` =
    /// `Either.right(value)`.
    pub fn unwrap<'a>(&'a self, lookup: &'a dyn HolderLookup<T>) -> Either<ResourceKey<T>, &'a T> {
        match self {
            Holder::Direct(value) => Either::right(value),
            Holder::Reference { .. } => Either::left(self.key(lookup)),
        }
    }

    /// `Holder.unwrapKey()` — `Optional<ResourceKey<T>>` (Direct: `None`).
    pub fn unwrap_key(&self, lookup: &dyn HolderLookup<T>) -> Option<ResourceKey<T>> {
        match self {
            Holder::Direct(_) => None,
            Holder::Reference { .. } => Some(self.key(lookup)),
        }
    }

    /// `Holder.getRegisteredName()`.
    pub fn get_registered_name(&self, lookup: &dyn HolderLookup<T>) -> String {
        self.unwrap_key(lookup)
            .map(|key| key.identifier().to_string())
            .unwrap_or_else(|| "[unregistered]".to_string())
    }

    /// The `Reference`'s `RegistryId` (Direct: `None`).
    pub fn registry_id(&self) -> Option<RegistryId> {
        match self {
            Holder::Direct(_) => None,
            Holder::Reference { registry, .. } => Some(*registry),
        }
    }
}

/// `Holder.toString()` — `"Direct{value}"` / `"Reference{key=value}"`.
///
/// The value part needs the lookup; the trait-level `Display` uses the id-only
/// fallback (see the module doc for why a lookup-free `Display` cannot render
/// the Java value part).
impl<T: std::fmt::Debug> std::fmt::Display for Holder<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Holder::Direct(value) => write!(f, "Direct{{{:?}}}", value),
            Holder::Reference { registry, id } => write!(f, "Reference{{{}={}}}", registry.0, id),
        }
    }
}

/// Render a holder for a panic message, resolving the key through the lookup
/// when possible. Only ever reached with a `Reference` (a `Direct` holder's
/// value/key never panic), so the `Direct` arm needs no `T: Debug`.
///
/// An unresolvable reference renders `"null"` — Java's `Reference.value()`/
/// `key()` string-concatenate the null key/value into the message, so the
/// byte-identical panic text is `"... unbound value 'null' from registry <id>"`.
fn render_holder<T>(holder: &Holder<T>, lookup: &dyn HolderLookup<T>) -> String {
    match holder {
        Holder::Direct(_) => "Direct".to_string(),
        Holder::Reference { .. } => lookup
            .key_of(holder)
            .map(|key| key.to_string())
            .unwrap_or_else(|| "null".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::holder_lookup::HolderGetter;
    use crate::registry::RegistryKey;
    use crate::{Identifier, ResourceKey};

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestElement(u8);

    fn registry_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    /// A minimal owner view for the `canSerializeIn` RegistryId O(1) check.
    #[derive(Clone, Copy)]
    struct TestOwner(RegistryId);

    impl HolderOwner<TestElement> for TestOwner {
        fn registry_id(&self) -> RegistryId {
            self.0
        }
    }

    /// A lookup that resolves nothing — exercises the unbound-reference panic
    /// paths (`Reference.value()/key()` when the owning registry cannot resolve
    /// the id). `Direct` holders never resolve through the lookup.
    struct NullLookup;

    impl HolderGetter<TestElement> for NullLookup {
        fn get(&self, _key: &ResourceKey<TestElement>) -> Option<Holder<TestElement>> {
            None
        }
        fn get_tag(&self, _tag: &TagKey<TestElement>) -> Option<crate::HolderSet<TestElement>> {
            None
        }
    }

    impl HolderLookup<TestElement> for NullLookup {
        fn list_elements(&self) -> Vec<Holder<TestElement>> {
            Vec::new()
        }
        fn list_tags(&self) -> Vec<crate::HolderSet<TestElement>> {
            Vec::new()
        }
        fn value_of<'a>(&'a self, _holder: &'a Holder<TestElement>) -> Option<&'a TestElement> {
            None
        }
        fn key_of(&self, _holder: &Holder<TestElement>) -> Option<ResourceKey<TestElement>> {
            None
        }
        fn tags_of(&self, _holder: &Holder<TestElement>) -> Vec<TagKey<TestElement>> {
            Vec::new()
        }
    }

    #[test]
    fn construction_and_kind() {
        // `Holder.direct(T)` → Kind.DIRECT.
        let direct = Holder::direct(TestElement(5));
        assert_eq!(direct.kind(), HolderKind::Direct);
        assert!(matches!(direct, Holder::Direct(_)));
        assert_eq!(direct.registry_id(), None);
        // `Holder.reference(registry, id)` → Kind.REFERENCE (the Copy id pair).
        let reference: Holder<TestElement> = Holder::reference(RegistryId(7), 3);
        assert_eq!(reference.kind(), HolderKind::Reference);
        match &reference {
            Holder::Reference { registry, id } => {
                assert_eq!(*registry, RegistryId(7));
                assert_eq!(*id, 3);
            }
            Holder::Direct(_) => panic!("reference must be Reference"),
        }
        assert_eq!(reference.registry_id(), Some(RegistryId(7)));
    }

    #[test]
    fn binding_model_flags() {
        // `isBound()` — constant-true stub (Java's runtime key/value null-check
        // is unrepresentable in the pure-ID model; a reference that cannot
        // resolve is already an unresolvable/panic state, see `value()`).
        let direct = Holder::direct(TestElement(5));
        let reference: Holder<TestElement> = Holder::reference(RegistryId(1), 0);
        assert!(direct.is_bound());
        assert!(reference.is_bound());
        // `areComponentsBound()` — Java steady state: Direct true, Reference false.
        assert!(direct.are_components_bound());
        assert!(!reference.are_components_bound());
    }

    #[test]
    fn can_serialize_in_is_a_registry_id_compare() {
        let owner = TestOwner(RegistryId(1));
        // Direct serializes in any context.
        assert!(Holder::direct(TestElement(5)).can_serialize_in(&owner));
        // Reference serializes only in its owning registry (Java `context == this`).
        assert!(Holder::reference(RegistryId(1), 0).can_serialize_in(&owner));
        assert!(!Holder::<TestElement>::reference(RegistryId(2), 0).can_serialize_in(&owner));
    }

    #[test]
    fn direct_value_unwrap_and_registered_name() {
        let lookup = NullLookup;
        let direct = Holder::direct(TestElement(5));
        // `value()` returns the inline value without touching the lookup.
        assert_eq!(direct.value(&lookup), &TestElement(5));
        // `unwrap()` = Either.right(value).
        assert_eq!(direct.unwrap(&lookup), Either::Right(&TestElement(5)));
        // `unwrapKey()` = Optional.empty → `getRegisteredName()` = "[unregistered]".
        assert_eq!(direct.unwrap_key(&lookup), None);
        assert_eq!(direct.get_registered_name(&lookup), "[unregistered]");
    }

    #[test]
    fn direct_is_identifier_key_and_tag_are_false() {
        // Java `Direct.is(...)` always false (never equal to a keyed lookup).
        let lookup = NullLookup;
        let direct = Holder::direct(TestElement(5));
        let key = ResourceKey::create(&registry_key(), Identifier::with_default_namespace("x"));
        let tag = TagKey::create(&registry_key(), Identifier::with_default_namespace("t"));
        assert!(!direct.is_identifier(&lookup, &Identifier::with_default_namespace("x")));
        assert!(!direct.is_key(&lookup, &key));
        assert!(!direct.is_tag(&lookup, &tag));
        assert!(direct.tags(&lookup).is_empty());
    }

    #[test]
    fn reference_value_panics_on_unresolvable_id_with_java_message() {
        // Java `Reference.value()` throws "Trying to access unbound value '<key>'
        // from registry <owner>"; the unresolvable reference's key is null, which
        // string-concatenates as "null" — byte-identical here.
        let lookup = NullLookup;
        let reference: Holder<TestElement> = Holder::reference(RegistryId(9), 4);
        let err =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reference.value(&lookup)));
        let msg = err.unwrap_err().downcast_ref::<String>().cloned().unwrap();
        assert_eq!(
            msg,
            "Trying to access unbound value 'null' from registry 9".to_string()
        );
    }

    #[test]
    fn reference_key_panics_on_unresolvable_id_with_java_message() {
        // Java `Reference.key()` throws the same literal "Trying to access
        // unbound value '<value>'" message as `value()` (a Mojang copy-paste),
        // rendering the null value as "null".
        let lookup = NullLookup;
        let reference: Holder<TestElement> = Holder::reference(RegistryId(9), 4);
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reference.key(&lookup)));
        let msg = err.unwrap_err().downcast_ref::<String>().cloned().unwrap();
        assert_eq!(
            msg,
            "Trying to access unbound value 'null' from registry 9".to_string()
        );
    }

    #[test]
    fn display_formats_match_java_tostring() {
        assert_eq!(
            Holder::direct(TestElement(5)).to_string(),
            "Direct{TestElement(5)}".to_string()
        );
        let reference: Holder<TestElement> = Holder::reference(RegistryId(7), 3);
        assert_eq!(reference.to_string(), "Reference{7=3}".to_string());
    }
}
