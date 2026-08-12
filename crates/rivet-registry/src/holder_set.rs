//! `net.minecraft.core.HolderSet<T>` — the #126 holder-set surface.
//!
//! PROVENANCE: `HolderSet.java` (232 lines, 26.2), a leaf of the `mc.core`
//! manifest unit.
//!
//! `HolderSet<T>` is `Direct(Vec<Holder<T>>)` (an explicit holder list) or
//! `Named { owner, key, contents }` (a bound named tag set). The Java
//! `Named` is a mutable object rebound at tag-reload time; in the Rust
//! value model the frozen `Registry<T>` is immutable, so `RegistryLookup.get(tag)`
//! constructs a fresh, bound `Named` from the registry's frozen member-id list
//! (OWNERSHIP.md §Registries: tags are bound pre-freeze and immutable after).
//!
//! Binding-model deviations (documented, PORTING.md drift checklist):
//! - `Direct.contains` compares holder values. For `Reference` holders that is
//!   the (RegistryId, id) pair — Java's `Set.copyOf(...).contains` uses
//!   `Reference`'s identity `equals` (a `Set` of the *same* holder objects), and
//!   since the id space is unique per registry the pair compare is the faithful
//!   equivalent of same-object membership.
//! - `Named.contains(value)` in Java is `value.is(this.key)` (the holder's bound
//!   tag set); here it is list membership in the bound member list — equivalent,
//!   because the `Named`'s contents ARE the members of `key`.
//! - The transient `emptyNamed` (an unbound `Named` used during construction)
//!   is represented by `contents: None`; `contents()` throws Java's
//!   "Trying to access unbound tag ..." message. No `Named` is ever
//!   constructed unbound by the registry (frozen tags are always bound).

use crate::TagKey;
use crate::holder::{Holder, HolderId, RegistryId};
use crate::holder_lookup::HolderOwner;

use rivet_serialization::either::Either;
use rivet_util::random::RandomSource;
use rivet_util::util::get_random_safe;

/// `net.minecraft.core.HolderSet<T>` — a direct holder list or a bound named tag
/// set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HolderSet<T> {
    /// `HolderSet.Direct<T>` — an explicit, always-bound holder list.
    Direct(Vec<Holder<T>>),
    /// `HolderSet.Named<T>` — a bound named tag set. `contents: None` is the
    /// transient unbound state (`emptyNamed`); `Some` is the frozen member list.
    Named {
        /// The owning registry's `RegistryId` (`HolderSet.Named.owner`).
        owner: RegistryId,
        /// The tag key (`HolderSet.Named.key`).
        key: TagKey<T>,
        /// The bound member holders; `None` = unbound.
        contents: Option<Vec<Holder<T>>>,
    },
}

impl<T> HolderSet<T> {
    /// `HolderSet.empty()`.
    pub fn empty() -> HolderSet<T> {
        HolderSet::Direct(Vec::new())
    }

    /// `HolderSet.direct(List<Holder<T>>)` — Java copies the list; the Rust
    /// value type owns it.
    pub fn direct(values: Vec<Holder<T>>) -> HolderSet<T> {
        HolderSet::Direct(values)
    }

    /// `HolderSet.emptyNamed(HolderOwner, TagKey)` — an unbound named set
    /// (throws on access until bound). The registry never constructs one; kept
    /// for API parity with Java's `@VisibleForTesting` static.
    pub fn empty_named(owner: RegistryId, key: TagKey<T>) -> HolderSet<T> {
        HolderSet::Named {
            owner,
            key,
            contents: None,
        }
    }

    /// The member list (`ListBacked.contents()`); panics on an unbound `Named`
    /// with Java's message.
    fn contents(&self) -> &[Holder<T>] {
        match self {
            HolderSet::Direct(values) => values,
            HolderSet::Named {
                owner,
                key,
                contents,
            } => contents.as_deref().unwrap_or_else(|| {
                panic!(
                    "Trying to access unbound tag '{}' from registry {}",
                    key, owner.0
                )
            }),
        }
    }

    /// `HolderSet.size()`.
    pub fn size(&self) -> usize {
        self.contents().len()
    }

    /// `HolderSet.isBound()`.
    pub fn is_bound(&self) -> bool {
        match self {
            HolderSet::Direct(_) => true,
            HolderSet::Named { contents, .. } => contents.is_some(),
        }
    }

    /// `HolderSet.stream()` — the member holders in order.
    pub fn stream(&self) -> Vec<Holder<T>>
    where
        T: Clone,
    {
        self.contents().to_vec()
    }

    /// `HolderSet.iter()` — iterate the members.
    pub fn iter(&self) -> std::slice::Iter<'_, Holder<T>> {
        self.contents().iter()
    }

    /// `HolderSet.unwrap()` — `Either<TagKey<T>, List<Holder<T>>>` (borrowed
    /// member list for the encode path).
    pub fn unwrap(&self) -> Either<TagKey<T>, &[Holder<T>]> {
        match self {
            HolderSet::Direct(values) => Either::right(values),
            HolderSet::Named { key, .. } => Either::left(key.clone()),
        }
    }

    /// `HolderSet.unwrapKey()` — `Optional<TagKey<T>>`.
    pub fn unwrap_key(&self) -> Option<TagKey<T>> {
        match self {
            HolderSet::Direct(_) => None,
            HolderSet::Named { key, .. } => Some(key.clone()),
        }
    }

    /// `HolderSet.getRandomElement(RandomSource)` — `Util.getRandomSafe(contents,
    /// random)`.
    pub fn get_random_element(&self, random: &mut impl RandomSource) -> Option<Holder<T>>
    where
        T: Clone,
    {
        get_random_safe(self.contents(), random)
    }

    /// `HolderSet.get(int)`.
    pub fn get(&self, index: usize) -> &Holder<T> {
        &self.contents()[index]
    }

    /// `HolderSet.contains(Holder<T>)`.
    ///
    /// `Direct`: value membership (Java `Set.copyOf(contents).contains`). For
    /// `Reference` holders the (RegistryId, id) value compare is the faithful
    /// equivalent of same-object membership (unique ids per registry).
    /// `Named`: Java `value.is(this.key)` — the holder is a member of the tag —
    /// which list membership in the bound member list reproduces exactly.
    pub fn contains(&self, value: &Holder<T>) -> bool
    where
        T: PartialEq,
    {
        match self {
            HolderSet::Direct(_) => self.contents().contains(value),
            HolderSet::Named { .. } => self.contents().contains(value),
        }
    }

    /// Whether a `Reference` member carries the given element id — the
    /// `BlockState.is(HolderSet)`-style membership check the `matching_blocks`/
    /// `matching_fluids` predicates use (`state.is(set)` compares the state's
    /// block/fluid holder, a `Reference` in the matching registry, against the
    /// set). The set is over the matching registry by construction, so the id
    /// compare is the faithful equivalent of `Reference.equals` on a
    /// same-registry holder.
    pub fn contains_id(&self, id: u32) -> bool {
        self.contents()
            .iter()
            .any(|h| matches!(h, Holder::Reference { id: member, .. } if *member == id))
    }

    /// `HolderSet.canSerializeIn(HolderOwner<T>)` — `Direct` serializes
    /// anywhere; `Named` must belong to the owner (Java `Named.canSerializeIn`
    /// = `owner.canSerializeIn(context)` = the RegistryId O(1) owner check).
    pub fn can_serialize_in(&self, owner: &dyn HolderOwner<T>) -> bool {
        match self {
            HolderSet::Direct(_) => true,
            HolderSet::Named {
                owner: set_owner, ..
            } => *set_owner == owner.registry_id(),
        }
    }

    /// `HolderSet.Direct`-only constructor from an id list — used by the SCC's
    /// registry to build a bound `Named` from its frozen `Vec<HolderId>`.
    pub(crate) fn named_from_ids(
        owner: RegistryId,
        key: TagKey<T>,
        ids: &[HolderId],
    ) -> HolderSet<T> {
        HolderSet::Named {
            owner,
            key,
            contents: Some(
                ids.iter()
                    .map(|id| Holder::Reference {
                        registry: owner,
                        id: id.0,
                    })
                    .collect(),
            ),
        }
    }
}

/// Render a holder list like Java's `List.toString()` — `[e1, e2, ...]` with
/// each member's `Holder.toString()`.
fn render_members<T: std::fmt::Debug>(members: &[Holder<T>]) -> String {
    let inner = members
        .iter()
        .map(|member| member.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// `HolderSet.toString()` — `"DirectSet[...]"` / `"NamedSet(key)[contents]"`.
///
/// Java's `Named.toString()` is `"NamedSet(" + key + ")[" + contents + "]"`;
/// the unbound state's null contents render as `"null"`.
impl<T: std::fmt::Debug> std::fmt::Display for HolderSet<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HolderSet::Direct(values) => write!(f, "DirectSet[{}]", render_members(values)),
            HolderSet::Named { key, contents, .. } => match contents {
                Some(members) => write!(f, "NamedSet({})[{}]", key, render_members(members)),
                None => write!(f, "NamedSet({})[null]", key),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::RegistryKey;
    use crate::{Identifier, ResourceKey};
    use rivet_util::random::LegacyRandomSource;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestElement(u8);

    fn registry_key() -> RegistryKey<TestElement> {
        ResourceKey::create_registry_key(Identifier::with_default_namespace("test"))
    }

    fn tag_key(id: &str) -> TagKey<TestElement> {
        TagKey::create(&registry_key(), Identifier::with_default_namespace(id))
    }

    fn holder(id: u32) -> Holder<TestElement> {
        Holder::reference(RegistryId(1), id)
    }

    /// A minimal `HolderOwner` standing in for the codec owner view — the owner
    /// check is the `RegistryId` compare.
    #[derive(Clone, Copy)]
    struct TestOwner(RegistryId);

    impl HolderOwner<TestElement> for TestOwner {
        fn registry_id(&self) -> RegistryId {
            self.0
        }
    }

    #[test]
    fn direct_set_is_bound_and_lists_members() {
        let empty = HolderSet::<TestElement>::empty();
        assert!(empty.is_bound());
        assert_eq!(empty.size(), 0);
        assert_eq!(empty.iter().count(), 0);
        assert_eq!(empty.unwrap_key(), None);

        let set = HolderSet::direct(vec![holder(0), holder(1)]);
        assert!(set.is_bound());
        assert_eq!(set.size(), 2);
        assert_eq!(set.stream(), vec![holder(0), holder(1)]);
        // Direct.unwrap() = Either.right(member list).
        assert!(matches!(set.unwrap(), Either::Right(_)));
        // get(int) and contains.
        assert_eq!(set.get(1), &holder(1));
        assert!(set.contains(&holder(0)));
        assert!(!set.contains(&holder(9)));
    }

    #[test]
    fn empty_named_is_unbound_and_contents_panics_with_java_message() {
        let set = HolderSet::empty_named(RegistryId(3), tag_key("group"));
        assert!(!set.is_bound());
        // Access through size() (Java `contents()` throws on the unbound set).
        let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| set.size()));
        let msg = err.unwrap_err().downcast_ref::<String>().cloned().unwrap();
        assert_eq!(
            msg,
            format!(
                "Trying to access unbound tag '{}' from registry {}",
                tag_key("group"),
                3
            )
        );
    }

    #[test]
    fn named_from_ids_binds_a_named_set_to_the_member_ids() {
        let set =
            HolderSet::named_from_ids(RegistryId(1), tag_key("group"), &[HolderId(0), HolderId(2)]);
        assert!(set.is_bound());
        assert_eq!(set.size(), 2);
        assert_eq!(set.unwrap_key(), Some(tag_key("group")));
        assert!(matches!(set.unwrap(), Either::Left(_)));
        let members: Vec<_> = set.iter().cloned().collect();
        // Each member is a Reference (element id == holder id == insertion index).
        assert_eq!(members, vec![holder(0), holder(2)]);
        assert!(set.contains(&holder(0)));
        assert!(!set.contains(&holder(1)));
    }

    #[test]
    fn unwrap_and_unwrap_key_distinguish_direct_from_named() {
        let direct = HolderSet::direct(vec![holder(0)]);
        assert_eq!(direct.unwrap_key(), None);
        assert!(matches!(direct.unwrap(), Either::Right(_)));

        let named = HolderSet::named_from_ids(RegistryId(1), tag_key("group"), &[HolderId(0)]);
        assert_eq!(named.unwrap_key(), Some(tag_key("group")));
        assert_eq!(named.unwrap(), Either::Left(tag_key("group")));
    }

    #[test]
    fn get_random_element_returns_a_member() {
        let mut random = LegacyRandomSource::new(42);
        // Single-member set: deterministic.
        let set = HolderSet::direct(vec![holder(0)]);
        assert_eq!(set.get_random_element(&mut random), Some(holder(0)));
        // Multi-member set: the draw is always a member.
        let set = HolderSet::direct(vec![holder(0), holder(1), holder(2)]);
        for _ in 0..16 {
            let drawn = set.get_random_element(&mut random).unwrap();
            assert!(set.contains(&drawn));
        }
        // Empty set: None.
        assert_eq!(
            HolderSet::<TestElement>::empty().get_random_element(&mut random),
            None
        );
    }

    #[test]
    fn can_serialize_in_depends_on_the_named_owner() {
        // Direct serializes in any context (Java `Direct.canSerializeIn` = true).
        let direct = HolderSet::direct(vec![holder(0)]);
        assert!(direct.can_serialize_in(&TestOwner(RegistryId(1))));
        assert!(direct.can_serialize_in(&TestOwner(RegistryId(9))));
        // Named serializes only in its owning registry.
        let named = HolderSet::named_from_ids(RegistryId(1), tag_key("group"), &[HolderId(0)]);
        assert!(named.can_serialize_in(&TestOwner(RegistryId(1))));
        assert!(!named.can_serialize_in(&TestOwner(RegistryId(9))));
    }

    #[test]
    fn display_formats_match_java_tostring() {
        // "DirectSet[members]".
        assert_eq!(
            HolderSet::<TestElement>::empty().to_string(),
            "DirectSet[[]]".to_string()
        );
        assert_eq!(
            HolderSet::direct(vec![holder(0), holder(1)]).to_string(),
            "DirectSet[[Reference{1=0}, Reference{1=1}]]".to_string()
        );
        // "NamedSet(key)[contents]" — bound lists the members, unbound `null`.
        let named = HolderSet::named_from_ids(RegistryId(1), tag_key("group"), &[HolderId(0)]);
        assert_eq!(
            named.to_string(),
            format!("NamedSet({})[[Reference{{1=0}}]]", tag_key("group"))
        );
        let unbound = HolderSet::empty_named(RegistryId(1), tag_key("group"));
        assert_eq!(
            unbound.to_string(),
            format!("NamedSet({})[null]", tag_key("group"))
        );
    }
}
