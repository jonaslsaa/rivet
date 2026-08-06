//! `net.minecraft.core.Position` — a floating-point position (Java interface).
//!
//! Java:
//! ```java
//! public interface Position { double x(); double y(); double z(); }
//! ```
//! Ported as a trait; implementers provide `x()`, `y()`, `z()`.

/// A floating-point position (`net.minecraft.core.Position`).
pub trait Position {
    /// `Position.x()`.
    fn x(&self) -> f64;

    /// `Position.y()`.
    fn y(&self) -> f64;

    /// `Position.z()`.
    fn z(&self) -> f64;
}
