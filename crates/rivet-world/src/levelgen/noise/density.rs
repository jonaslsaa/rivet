//! Port of `net.minecraft.world.level.levelgen.Density` (class, 26.2).
//!
//! The three surface/density sentinel constants.

/// `net.minecraft.world.level.levelgen.Density`.
pub struct Density;

impl Density {
    /// `Density.SURFACE` — the terrain surface density.
    pub const SURFACE: f64 = 0.0;
    /// `Density.UNRECOVERABLY_DENSE` — a density so high no terrain can reach it.
    pub const UNRECOVERABLY_DENSE: f64 = 64.0;
    /// `Density.UNRECOVERABLY_THIN` — a density so low no terrain can reach it.
    pub const UNRECOVERABLY_THIN: f64 = -64.0;
}

#[cfg(test)]
mod tests {
    use super::Density;

    #[test]
    fn constants_match_java() {
        assert_eq!(Density::SURFACE, 0.0);
        assert_eq!(Density::UNRECOVERABLY_DENSE, 64.0);
        assert_eq!(Density::UNRECOVERABLY_THIN, -64.0);
    }
}
