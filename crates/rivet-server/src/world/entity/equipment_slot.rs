//! Port of `net.minecraft.world.entity.EquipmentSlot` (Paper 26.2, commit `0a993450f129c4942c2a9ed45ba047412b4667cf`).

/// Port of `net.minecraft.world.entity.EquipmentSlot.Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EquipmentSlotType {
    Hand,
    HumanoidArmor,
    AnimalArmor,
    Saddle,
}

/// Port of `net.minecraft.world.entity.EquipmentSlot`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(usize)]
pub enum EquipmentSlot {
    MainHand = 0,
    OffHand = 1,
    Feet = 2,
    Legs = 3,
    Chest = 4,
    Head = 5,
    Body = 6,
    Saddle = 7,
}

impl EquipmentSlot {
    /// Paper `EquipmentSlot.NO_COUNT_LIMIT`.
    pub const NO_COUNT_LIMIT: i32 = 0;

    /// Paper `EquipmentSlot.VALUES` in exact enum declaration order.
    pub const VALUES: [EquipmentSlot; 8] = [
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Feet,
        EquipmentSlot::Legs,
        EquipmentSlot::Chest,
        EquipmentSlot::Head,
        EquipmentSlot::Body,
        EquipmentSlot::Saddle,
    ];

    /// Paper `EquipmentSlot#getType()`.
    #[must_use]
    pub const fn slot_type(self) -> EquipmentSlotType {
        match self {
            Self::MainHand | Self::OffHand => EquipmentSlotType::Hand,
            Self::Feet | Self::Legs | Self::Chest | Self::Head => EquipmentSlotType::HumanoidArmor,
            Self::Body => EquipmentSlotType::AnimalArmor,
            Self::Saddle => EquipmentSlotType::Saddle,
        }
    }

    /// Paper `EquipmentSlot#getIndex()`.
    #[must_use]
    pub const fn index(self) -> i32 {
        match self {
            Self::MainHand | Self::Feet | Self::Body | Self::Saddle => 0,
            Self::OffHand | Self::Legs => 1,
            Self::Chest => 2,
            Self::Head => 3,
        }
    }

    /// Paper `EquipmentSlot#getIndex(int base)`.
    #[must_use]
    pub const fn index_with_base(self, base: i32) -> i32 {
        base.wrapping_add(self.index())
    }

    /// Paper `EquipmentSlot.countLimit`.
    #[must_use]
    pub const fn count_limit(self) -> i32 {
        match self {
            Self::MainHand | Self::OffHand => Self::NO_COUNT_LIMIT,
            Self::Feet | Self::Legs | Self::Chest | Self::Head | Self::Body | Self::Saddle => 1,
        }
    }

    /// Paper `EquipmentSlot#getId()`.
    ///
    /// Note that `id` differs from enum ordinal:
    /// `MainHand=0, Feet=1, Legs=2, Chest=3, Head=4, OffHand=5, Body=6, Saddle=7`.
    #[must_use]
    pub const fn id(self) -> i32 {
        match self {
            Self::MainHand => 0,
            Self::Feet => 1,
            Self::Legs => 2,
            Self::Chest => 3,
            Self::Head => 4,
            Self::OffHand => 5,
            Self::Body => 6,
            Self::Saddle => 7,
        }
    }

    /// Paper `EquipmentSlot#getFilterBit(int offset)`.
    #[must_use]
    pub const fn filter_bit(self, offset: i32) -> i32 {
        self.id().wrapping_add(offset)
    }

    /// Paper `EquipmentSlot#getName()` / `getSerializedName()`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MainHand => "mainhand",
            Self::OffHand => "offhand",
            Self::Feet => "feet",
            Self::Legs => "legs",
            Self::Chest => "chest",
            Self::Head => "head",
            Self::Body => "body",
            Self::Saddle => "saddle",
        }
    }

    /// Paper `EquipmentSlot#isArmor()`.
    #[must_use]
    pub const fn is_armor(self) -> bool {
        matches!(
            self.slot_type(),
            EquipmentSlotType::HumanoidArmor | EquipmentSlotType::AnimalArmor
        )
    }

    /// Paper `EquipmentSlot#canIncreaseExperience()`.
    #[must_use]
    pub const fn can_increase_experience(self) -> bool {
        !matches!(self.slot_type(), EquipmentSlotType::Saddle)
    }

    /// Paper `EquipmentSlot.BY_ID` (`OutOfBoundsStrategy.ZERO` -> `MainHand`).
    #[must_use]
    pub const fn by_id(id: i32) -> Self {
        match id {
            0 => Self::MainHand,
            1 => Self::Feet,
            2 => Self::Legs,
            3 => Self::Chest,
            4 => Self::Head,
            5 => Self::OffHand,
            6 => Self::Body,
            7 => Self::Saddle,
            _ => Self::MainHand,
        }
    }

    /// Paper `EquipmentSlot.byName(String)`.
    #[must_use]
    pub fn by_name(name: &str) -> Option<Self> {
        match name {
            "mainhand" => Some(Self::MainHand),
            "offhand" => Some(Self::OffHand),
            "feet" => Some(Self::Feet),
            "legs" => Some(Self::Legs),
            "chest" => Some(Self::Chest),
            "head" => Some(Self::Head),
            "body" => Some(Self::Body),
            "saddle" => Some(Self::Saddle),
            _ => None,
        }
    }
}
