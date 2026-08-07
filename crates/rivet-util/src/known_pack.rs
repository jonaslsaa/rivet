//! Port of `net.minecraft.server.packs.repository.KnownPack` (MC 26.2).
//!
//! Java: `KnownPack.java` in `working/Paper`. A `(namespace, id, version)` pack
//! identity — the client/server `select_known_packs` handshake value.
//!
//! Lives in `rivet-util` (not its package-mirror server crate) because both
//! `rivet-protocol` (the `select_known_packs` packet bodies, issue #109) and
//! `rivet-server` (the configuration listener, #96) need it, and `rivet-util` is
//! the lowest existing dependency the two share. `RegistrationInfo`'s known-pack
//! slot stays an opaque `()` placeholder — that field is owned by the pack unit,
//! and this issue does not require widening it.
//!
//! The wire codec (`STRING_UTF8` x3) is a protocol concern and lives in
//! `rivet-protocol` (`crate::protocol::stream_codecs::known_pack_stream_codec`),
//! per the same OWNERSHIP line that keeps `Identifier.STREAM_CODEC` in
//! `rivet-protocol`.

use std::fmt;

/// `net.minecraft.server.packs.repository.KnownPack`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KnownPack {
    namespace: String,
    id: String,
    version: String,
}

impl KnownPack {
    /// `new KnownPack(String namespace, String id, String version)`.
    pub fn new(namespace: String, id: String, version: String) -> Self {
        KnownPack {
            namespace,
            id,
            version,
        }
    }

    /// `KnownPack.namespace()`.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// `KnownPack.id()`.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// `KnownPack.version()`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// `KnownPack.VANILLA_NAMESPACE`.
    pub const VANILLA_NAMESPACE: &'static str = "minecraft";

    /// `KnownPack.vanilla(String id)` — `(VANILLA_NAMESPACE, id,
    /// SharedConstants.getCurrentVersion().id())`. The version string is the
    /// data-version protocol id (`26.2` for protocol 776); `SharedConstants` is
    /// not ported, so the version is a parameter here.
    pub fn vanilla(id: String, version: String) -> Self {
        KnownPack {
            namespace: Self::VANILLA_NAMESPACE.to_string(),
            id,
            version,
        }
    }

    /// `KnownPack.isVanilla()` — `this.namespace.equals("minecraft")`.
    pub fn is_vanilla(&self) -> bool {
        self.namespace == Self::VANILLA_NAMESPACE
    }
}

/// Java `toString()` — `this.namespace + ":" + this.id + ":" + this.version`.
impl fmt::Display for KnownPack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.namespace, self.id, self.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_constructor_and_is_vanilla() {
        let pack = KnownPack::vanilla("core".to_string(), "26.2".to_string());
        assert_eq!(pack.namespace(), "minecraft");
        assert_eq!(pack.id(), "core");
        assert_eq!(pack.version(), "26.2");
        assert!(pack.is_vanilla());
        let non_vanilla = KnownPack::new("paper".to_string(), "x".to_string(), "1".to_string());
        assert!(!non_vanilla.is_vanilla());
    }

    #[test]
    fn display_is_colon_joined() {
        let pack = KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            "26.2".to_string(),
        );
        assert_eq!(pack.to_string(), "minecraft:core:26.2");
    }

    #[test]
    fn value_equality() {
        let a = KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            "26.2".to_string(),
        );
        let b = KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            "26.2".to_string(),
        );
        let c = KnownPack::new(
            "minecraft".to_string(),
            "core".to_string(),
            "26.3".to_string(),
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
