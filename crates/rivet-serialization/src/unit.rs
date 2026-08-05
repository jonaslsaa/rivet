//! Port of `com.mojang.datafixers.util.Unit` (enum `Unit { INSTANCE }`).

/// `com.mojang.datafixers.util.Unit.INSTANCE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Unit;

impl Unit {
    /// `Unit.INSTANCE`.
    pub const INSTANCE: Unit = Unit;
}

impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Unit")
    }
}
