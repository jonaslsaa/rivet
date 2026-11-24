//! Port of `net.minecraft.world.entity.Pose` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

/// Port of `net.minecraft.world.entity.Pose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Pose {
    #[default]
    Standing = 0,
    FallFlying = 1,
    Sleeping = 2,
    Swimming = 3,
    SpinAttack = 4,
    Crouching = 5,
    LongJumping = 6,
    Dying = 7,
    Croaking = 8,
    UsingTongue = 9,
    Sitting = 10,
    Roaring = 11,
    Sniffing = 12,
    Emerging = 13,
    Digging = 14,
    Sliding = 15,
    Shooting = 16,
    Inhaling = 17,
}

impl Pose {
    /// All poses in exact Paper enum/id order.
    pub const VALUES: [Pose; 18] = [
        Pose::Standing,
        Pose::FallFlying,
        Pose::Sleeping,
        Pose::Swimming,
        Pose::SpinAttack,
        Pose::Crouching,
        Pose::LongJumping,
        Pose::Dying,
        Pose::Croaking,
        Pose::UsingTongue,
        Pose::Sitting,
        Pose::Roaring,
        Pose::Sniffing,
        Pose::Emerging,
        Pose::Digging,
        Pose::Sliding,
        Pose::Shooting,
        Pose::Inhaling,
    ];

    /// Paper `Pose#id()`.
    #[must_use]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Paper `Pose.BY_ID` (`OutOfBoundsStrategy.ZERO` -> `Standing`).
    #[must_use]
    pub fn by_id(id: i32) -> Self {
        if id < 0 {
            return Self::Standing;
        }
        Self::VALUES
            .get(id as usize)
            .copied()
            .unwrap_or(Self::Standing)
    }

    /// Paper `Pose#getSerializedName()`.
    #[must_use]
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Standing => "standing",
            Self::FallFlying => "fall_flying",
            Self::Sleeping => "sleeping",
            Self::Swimming => "swimming",
            Self::SpinAttack => "spin_attack",
            Self::Crouching => "crouching",
            Self::LongJumping => "long_jumping",
            Self::Dying => "dying",
            Self::Croaking => "croaking",
            Self::UsingTongue => "using_tongue",
            Self::Sitting => "sitting",
            Self::Roaring => "roaring",
            Self::Sniffing => "sniffing",
            Self::Emerging => "emerging",
            Self::Digging => "digging",
            Self::Sliding => "sliding",
            Self::Shooting => "shooting",
            Self::Inhaling => "inhaling",
        }
    }
}
