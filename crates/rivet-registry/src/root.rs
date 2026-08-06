//! The ROOT registry — the `WritableRegistry<AnyRegistry>` registry-of-registries
//! (`BuiltInRegistries.REGISTRY`, key `Registries.ROOT_REGISTRY`).
//!
//! PROVENANCE: `net.minecraft.core.registries.BuiltInRegistries` (the `REGISTRY`
//! static) + the `AnyRegistry` erased boundary. The Java `AnyRegistry` type is
//! Paper's erased registry-interface marker used by `RegistryAccess`/ROOT
//! downcasts (OWNERSHIP.md §Registries: "Heterogeneous registry sets
//! (`RegistryAccess`, the ROOT registry) use `trait AnyRegistry: Any` +
//! `Box<dyn AnyRegistry>`, downcast at those two erased boundaries only").
//!
//! #124 scope (ownership C — access/provider): the erased boundary type and
//! the ROOT construction path. BuiltInRegistries content breadth (every
//! built-in registry) is out of scope; the ROOT starts empty and
//! `RegistryLayer::create_registry_access` seeds the STATIC layer from it.
//! Protocol sync of the ROOT is `rivet-protocol` (#126), never here.

use crate::Identifier;
use crate::ResourceKey;
use crate::builder::RegistryBuilder;
use crate::registry::{Registry, RegistryKey};

use rivet_serialization::lifecycle::Lifecycle;

use std::any::Any;
use std::fmt::Debug;

/// The erased registry value stored at the two erased boundaries
/// (OWNERSHIP.md §Registries). Every frozen `Registry<T>` impls `AnyRegistry`;
/// the `Send + Sync` supertraits make the erased value shareable to worker
/// pools behind `Arc<GameData>`.
pub type AnyBox = Box<dyn AnyRegistry + 'static>;

/// The erased registry boundary — `trait AnyRegistry: Any`.
///
/// Every frozen `Registry<T>` impls this; `RegistryAccess` and the ROOT
/// registry store `Box<dyn AnyRegistry>` and downcast via `as_any` at exactly
/// those two erased boundaries (OWNERSHIP.md). The trait requires `Debug` so
/// the trait object (and therefore any access holding erased registries) is
/// printable, and `Send + Sync` because OWNERSHIP.md §Registries shares
/// `Arc<GameData>` (which owns a `RegistryAccess` of erased entries) to worker
/// pools. Per-registry `Arc<dyn AnyRegistry>` is explicitly forbidden (breaks
/// holder identity / owner checks) — this is the erased storage for the value
/// tables.
pub trait AnyRegistry: Any + Debug + Send + Sync {
    /// `as_any` — the sole downcast seam (Java's erased `Registry<?>` cast).
    fn as_any(&self) -> &dyn Any;

    /// `MappedRegistry.registryLifecycle()` — exposed through the erased
    /// boundary so `RegistryOps`'s `HolderLookupAdapter` can report the real
    /// lifecycle without a downcast. (`Registry<T>: Debug` is hand-written, so
    /// no `T: Debug` bound is needed on the blanket impl.)
    fn registry_lifecycle(&self) -> Lifecycle;
}

impl<T: Send + Sync + 'static> AnyRegistry for Registry<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn registry_lifecycle(&self) -> Lifecycle {
        Registry::registry_lifecycle(self)
    }
}

/// The ROOT registry — `BuiltInRegistries.REGISTRY`, a
/// `WritableRegistry<Box<dyn AnyRegistry>>` registry-of-registries.
#[derive(Debug)]
pub struct RootRegistry {
    /// The frozen registry-of-registries.
    pub inner: Registry<AnyBox>,
}

impl RootRegistry {
    /// `BuiltInRegistries.REGISTRY` — the frozen ROOT registry-of-registries.
    ///
    /// #124 scope: the ROOT carries no built-ins yet (BuiltInRegistries content
    /// breadth is out of scope). The construction path is real so later units
    /// register built-ins here and the STATIC layer seeds from them via
    /// `RegistryAccess::from_registry_of_registries`.
    pub fn root() -> Registry<AnyBox> {
        let root_key: RegistryKey<AnyBox> =
            ResourceKey::create_registry_key(Identifier::with_default_namespace("root"));
        RegistryBuilder::new(&root_key).freeze()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::RegistryAccess;
    use crate::registry::{Registry, RegistryKey};

    #[test]
    fn root_registry_is_an_empty_registry_of_registries() {
        let root = RootRegistry::root();
        assert!(root.entry_set().is_empty());
        // The ROOT-to-access view (erased boundary #2) is empty too.
        let view = RegistryAccess::from_registry_of_registries(&root);
        assert!(view.list_registry_keys().is_empty());
    }

    #[test]
    fn any_registry_downcasts_through_the_trait_object() {
        let key: RegistryKey<()> =
            ResourceKey::create_registry_key(Identifier::with_default_namespace("x"));
        let registry: Registry<()> = RegistryBuilder::new(&key).freeze();
        let boxed: AnyBox = Box::new(registry);
        let erased: &dyn AnyRegistry = boxed.as_ref();
        assert!(erased.as_any().downcast_ref::<Registry<()>>().is_some());
        assert!(erased.as_any().downcast_ref::<Registry<u8>>().is_none());
    }
}
