//! Port of `net.minecraft.world.entity.HumanoidArm` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

/// Port of `net.minecraft.world.entity.HumanoidArm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HumanoidArm {
    Left,
    #[default]
    Right,
}

impl HumanoidArm {
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}
