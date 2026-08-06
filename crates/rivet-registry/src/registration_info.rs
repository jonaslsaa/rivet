//! Port of `net.minecraft.core.RegistrationInfo` (MC 26.2).
//!
//! PROVENANCE: leaf of the `mc.core` manifest unit. Java source:
//! `net/minecraft/core/RegistrationInfo.java` (9 lines, 26.2).
//!
//! Ownership B — the registry lifecycle owns this small value type because
//! `RegistryBuilder<T>.register` takes it; ownership A's `resources` modules
//! do not depend on it.
//!
//! ```java
//! public record RegistrationInfo(Optional<KnownPack> knownPackInfo, Lifecycle lifecycle) {
//!     public static final RegistrationInfo BUILT_IN =
//!         new RegistrationInfo(Optional.empty(), Lifecycle.stable());
//! }
//! ```
//!
//! `KnownPack` is a `net.minecraft.server.packs.repository` type (owned by the
//! pack unit, not #124). Until that unit lands, the known-pack slot is an
//! opaque `()` placeholder (boundary note); the pack unit widens it to
//! `Option<KnownPack>` when the type exists.

use rivet_serialization::lifecycle::Lifecycle;

/// `net.minecraft.core.RegistrationInfo`.
///
/// `known_pack_info` is an opaque `()` until
/// `net.minecraft.server.packs.repository.KnownPack` is ported; the field's
/// `Option<()>` always holds `None` for `BUILT_IN`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationInfo {
    /// `RegistrationInfo.knownPackInfo()` — `Optional<KnownPack>` (pack unit).
    pub known_pack_info: Option<()>,
    /// `RegistrationInfo.lifecycle()`.
    pub lifecycle: Lifecycle,
}

impl RegistrationInfo {
    /// `RegistrationInfo(Optional<KnownPack>, Lifecycle)`.
    pub fn new(known_pack_info: Option<()>, lifecycle: Lifecycle) -> Self {
        RegistrationInfo {
            known_pack_info,
            lifecycle,
        }
    }

    /// `RegistrationInfo.BUILT_IN` — `(Optional.empty(), Lifecycle.stable())`.
    pub const BUILT_IN: Self = RegistrationInfo {
        known_pack_info: None,
        lifecycle: Lifecycle::Stable,
    };

    /// `RegistrationInfo.knownPackInfo()`.
    pub fn known_pack_info(&self) -> Option<()> {
        self.known_pack_info
    }

    /// `RegistrationInfo.lifecycle()`.
    pub fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }
}
