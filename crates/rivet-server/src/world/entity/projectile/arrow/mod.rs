//! Port of `net.minecraft.world.entity.projectile.arrow` (Paper 26.2).

pub mod abstract_arrow;
#[allow(clippy::module_inception)]
pub mod arrow;
pub mod spectral_arrow;
pub mod thrown_trident;

pub use abstract_arrow::{AbstractArrow, Pickup};
pub use arrow::{Arrow, PotionContents, PotionEffect};
pub use spectral_arrow::SpectralArrow;
pub use thrown_trident::ThrownTrident;
