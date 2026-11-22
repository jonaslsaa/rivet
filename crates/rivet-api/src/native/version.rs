//! API version, capability negotiation, and explicit loading-mode types (#506).

use std::collections::BTreeSet;
use std::fmt;

/// Semantic version of the native plugin API boundary.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub struct ApiVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl ApiVersion {
    pub const CURRENT: Self = Self::new(0, 1, 0);

    #[must_use]
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Returns whether a host running `host` can satisfy a plugin targeting `self`.
    ///
    /// - For `major == 0` (pre-1.0 development): major and minor must match
    ///   exactly, and `self.patch <= host.patch`.
    /// - For `major >= 1`: major must match, and `(self.minor, self.patch) <= (host.minor, host.patch)`.
    #[must_use]
    pub const fn is_compatible_with_host(self, host: Self) -> bool {
        if self.major != host.major {
            return false;
        }
        if self.major == 0 {
            self.minor == host.minor && self.patch <= host.patch
        } else {
            self.minor < host.minor || (self.minor == host.minor && self.patch <= host.patch)
        }
    }
}

impl fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Named host capabilities negotiated when a native plugin registers.
///
/// Capabilities marked `is_m3_gated() == true` require reviewed M3 provider
/// seams before live wiring; synthetic conformance may negotiate only
/// non-gated capabilities (or explicitly configured synthetic host sets).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Ord, PartialOrd, Debug)]
pub enum Capability {
    /// Core register/load/enable/disable lifecycle hooks and host logging.
    LifecycleHooks,
    /// Host-owned opaque generational resource handles.
    SyntheticResources,
    /// M3-gated: event bus subscription seam.
    EventSubscription,
    /// M3-gated: synchronous tick-thread scheduler seam.
    TickScheduler,
    /// M3-gated: read-only world/chunk inspection seam.
    WorldRead,
    /// M3-gated: world mutation seam.
    WorldWrite,
    /// M3-gated: read-only entity inspection seam.
    EntityRead,
    /// M3-gated: entity mutation seam.
    EntityWrite,
    /// M3-gated: read-only player inspection seam.
    PlayerRead,
    /// M3-gated: player mutation seam.
    PlayerWrite,
    /// M3-gated: read-only item/inventory inspection seam.
    InventoryRead,
    /// M3-gated: item/inventory mutation seam.
    InventoryWrite,
    /// M3-gated: read-only container menu inspection seam.
    MenuRead,
    /// M3-gated: container menu open/close mutation seam.
    MenuWrite,
    /// M3-gated: Brigadier command registration seam.
    CommandRegistration,
}

impl Capability {
    #[must_use]
    pub const fn is_m3_gated(self) -> bool {
        !matches!(self, Self::LifecycleHooks | Self::SyntheticResources)
    }
}

/// Deterministic ordered set of negotiated capabilities.
#[derive(Clone, Eq, PartialEq, Default, Debug)]
pub struct CapabilitySet(BTreeSet<Capability>);

impl CapabilitySet {
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    #[must_use]
    pub fn synthetic_default() -> Self {
        Self::from_iter([Capability::LifecycleHooks, Capability::SyntheticResources])
    }

    pub fn insert(&mut self, cap: Capability) -> bool {
        self.0.insert(cap)
    }

    #[must_use]
    pub fn contains(&self, cap: Capability) -> bool {
        self.0.contains(&cap)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.0.iter().copied()
    }

    #[must_use]
    pub fn missing_from(&self, host: &Self) -> Vec<Capability> {
        self.0
            .iter()
            .copied()
            .filter(|cap| !host.contains(*cap))
            .collect()
    }

    #[must_use]
    pub fn intersection(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).copied().collect())
    }
}

impl FromIterator<Capability> for CapabilitySet {
    fn from_iter<T: IntoIterator<Item = Capability>>(iter: T) -> Self {
        Self(iter.into_iter().collect())
    }
}

/// Explicit declaration of how a native plugin is loaded.
///
/// Keeping dynamic loading explicit prevents hidden `dlopen` / ABI-fragile
/// loading paths from sneaking into the safe in-process Rust boundary.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum LoadingMode {
    /// Statically linked / in-process Rust plugin trait object.
    StaticInProcess,
    /// Explicitly declared external C-ABI dynamic library (rejected unless a
    /// dedicated ABI loader is configured).
    DynamicCdylib { abi_version: u32, symbol: String },
}

/// Errors produced during plugin registration and version/capability negotiation.
#[derive(Clone, Eq, PartialEq, Debug)]
pub enum NegotiationError {
    InvalidPluginName(String),
    DuplicatePluginName(String),
    IncompatibleApiVersion {
        host: ApiVersion,
        requested: ApiVersion,
    },
    MissingCapabilities(Vec<Capability>),
    UnsupportedLoadingMode(LoadingMode),
}

impl fmt::Display for NegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPluginName(name) => write!(f, "invalid plugin name: {name:?}"),
            Self::DuplicatePluginName(name) => write!(f, "plugin already registered: {name}"),
            Self::IncompatibleApiVersion { host, requested } => write!(
                f,
                "incompatible native API version: host={host}, plugin requested={requested}"
            ),
            Self::MissingCapabilities(caps) => {
                write!(f, "host is missing required capabilities: {caps:?}")
            }
            Self::UnsupportedLoadingMode(mode) => {
                write!(f, "loading mode not enabled on this host: {mode:?}")
            }
        }
    }
}

impl std::error::Error for NegotiationError {}
