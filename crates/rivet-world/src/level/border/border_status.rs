//! `net.minecraft.world.level.border.BorderStatus` — the world-border movement
//! status enum with its debug color.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/level/
//! border/BorderStatus.java`.

/// `BorderStatus` — the border extent's movement state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BorderStatus {
    /// `GROWING(4259712)`.
    Growing,
    /// `SHRINKING(16724016)`.
    Shrinking,
    /// `STATIONARY(2138367)`.
    Stationary,
}

impl BorderStatus {
    /// `BorderStatus.getColor()`.
    pub fn get_color(self) -> i32 {
        match self {
            BorderStatus::Growing => 4259712,
            BorderStatus::Shrinking => 16724016,
            BorderStatus::Stationary => 2138367,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_match_java() {
        assert_eq!(BorderStatus::Growing.get_color(), 4259712);
        assert_eq!(BorderStatus::Shrinking.get_color(), 16724016);
        assert_eq!(BorderStatus::Stationary.get_color(), 2138367);
    }
}
