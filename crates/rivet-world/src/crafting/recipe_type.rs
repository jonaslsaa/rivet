//! Core recipe type, serializer, and recipe-book category identifiers.
//!
//! Mirrors Paper 26.2:
//! - `net.minecraft.world.item.crafting.RecipeType`
//! - `net.minecraft.world.item.crafting.RecipeSerializers`
//! - `net.minecraft.world.item.crafting.CraftingBookCategory`
//! - `net.minecraft.world.item.crafting.CookingBookCategory`
//! - `net.minecraft.world.item.crafting.RecipeBookCategories`
//! - `net.minecraft.world.inventory.RecipeBookType`
//! - `net.minecraft.world.item.crafting.Recipe.CommonInfo`

use rivet_registry::Identifier;

/// `net.minecraft.world.item.crafting.RecipeType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecipeType {
    Crafting,
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
    Stonecutting,
    Smithing,
}

impl RecipeType {
    /// Vanilla registry path for this recipe type.
    pub const fn path(self) -> &'static str {
        match self {
            Self::Crafting => "crafting",
            Self::Smelting => "smelting",
            Self::Blasting => "blasting",
            Self::Smoking => "smoking",
            Self::CampfireCooking => "campfire_cooking",
            Self::Stonecutting => "stonecutting",
            Self::Smithing => "smithing",
        }
    }

    /// Full `minecraft:<path>` identifier.
    pub fn identifier(self) -> Identifier {
        Identifier::with_default_namespace(self.path())
    }

    /// Look up by vanilla path or `minecraft:<path>` identifier string.
    pub fn from_name(name: &str) -> Option<Self> {
        let path = name.strip_prefix("minecraft:").unwrap_or(name);
        match path {
            "crafting" => Some(Self::Crafting),
            "smelting" => Some(Self::Smelting),
            "blasting" => Some(Self::Blasting),
            "smoking" => Some(Self::Smoking),
            "campfire_cooking" => Some(Self::CampfireCooking),
            "stonecutting" => Some(Self::Stonecutting),
            "smithing" => Some(Self::Smithing),
            _ => None,
        }
    }

    /// All 7 vanilla recipe types in registration order (`RecipeType.java`).
    pub const ALL: [Self; 7] = [
        Self::Crafting,
        Self::Smelting,
        Self::Blasting,
        Self::Smoking,
        Self::CampfireCooking,
        Self::Stonecutting,
        Self::Smithing,
    ];
}

/// `net.minecraft.world.item.crafting.RecipeSerializers` — all 21 vanilla
/// recipe serializers in Paper 26.2 registration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecipeSerializerId {
    CraftingShaped,
    CraftingShapeless,
    CraftingDye,
    CraftingImbue,
    CraftingTransmute,
    CraftingDecoratedPot,
    CraftingSpecialBookCloning,
    CraftingSpecialMapExtending,
    CraftingSpecialFireworkRocket,
    CraftingSpecialFireworkStar,
    CraftingSpecialFireworkStarFade,
    CraftingSpecialBannerDuplicate,
    CraftingSpecialShieldDecoration,
    CraftingSpecialRepairItem,
    Smelting,
    Blasting,
    Smoking,
    CampfireCooking,
    Stonecutting,
    SmithingTransform,
    SmithingTrim,
}

impl RecipeSerializerId {
    pub const fn path(self) -> &'static str {
        match self {
            Self::CraftingShaped => "crafting_shaped",
            Self::CraftingShapeless => "crafting_shapeless",
            Self::CraftingDye => "crafting_dye",
            Self::CraftingImbue => "crafting_imbue",
            Self::CraftingTransmute => "crafting_transmute",
            Self::CraftingDecoratedPot => "crafting_decorated_pot",
            Self::CraftingSpecialBookCloning => "crafting_special_bookcloning",
            Self::CraftingSpecialMapExtending => "crafting_special_mapextending",
            Self::CraftingSpecialFireworkRocket => "crafting_special_firework_rocket",
            Self::CraftingSpecialFireworkStar => "crafting_special_firework_star",
            Self::CraftingSpecialFireworkStarFade => "crafting_special_firework_star_fade",
            Self::CraftingSpecialBannerDuplicate => "crafting_special_bannerduplicate",
            Self::CraftingSpecialShieldDecoration => "crafting_special_shielddecoration",
            Self::CraftingSpecialRepairItem => "crafting_special_repairitem",
            Self::Smelting => "smelting",
            Self::Blasting => "blasting",
            Self::Smoking => "smoking",
            Self::CampfireCooking => "campfire_cooking",
            Self::Stonecutting => "stonecutting",
            Self::SmithingTransform => "smithing_transform",
            Self::SmithingTrim => "smithing_trim",
        }
    }

    pub fn identifier(self) -> Identifier {
        Identifier::with_default_namespace(self.path())
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let path = name.strip_prefix("minecraft:").unwrap_or(name);
        match path {
            "crafting_shaped" => Some(Self::CraftingShaped),
            "crafting_shapeless" => Some(Self::CraftingShapeless),
            "crafting_dye" => Some(Self::CraftingDye),
            "crafting_imbue" => Some(Self::CraftingImbue),
            "crafting_transmute" => Some(Self::CraftingTransmute),
            "crafting_decorated_pot" => Some(Self::CraftingDecoratedPot),
            "crafting_special_bookcloning" => Some(Self::CraftingSpecialBookCloning),
            "crafting_special_mapextending" => Some(Self::CraftingSpecialMapExtending),
            "crafting_special_firework_rocket" => Some(Self::CraftingSpecialFireworkRocket),
            "crafting_special_firework_star" => Some(Self::CraftingSpecialFireworkStar),
            "crafting_special_firework_star_fade" => Some(Self::CraftingSpecialFireworkStarFade),
            "crafting_special_bannerduplicate" => Some(Self::CraftingSpecialBannerDuplicate),
            "crafting_special_shielddecoration" => Some(Self::CraftingSpecialShieldDecoration),
            "crafting_special_repairitem" => Some(Self::CraftingSpecialRepairItem),
            "smelting" => Some(Self::Smelting),
            "blasting" => Some(Self::Blasting),
            "smoking" => Some(Self::Smoking),
            "campfire_cooking" => Some(Self::CampfireCooking),
            "stonecutting" => Some(Self::Stonecutting),
            "smithing_transform" => Some(Self::SmithingTransform),
            "smithing_trim" => Some(Self::SmithingTrim),
            _ => None,
        }
    }
}

/// `net.minecraft.world.item.crafting.CraftingBookCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CraftingBookCategory {
    Building,
    Redstone,
    Equipment,
    #[default]
    Misc,
}

impl CraftingBookCategory {
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Building => "building",
            Self::Redstone => "redstone",
            Self::Equipment => "equipment",
            Self::Misc => "misc",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "building" => Some(Self::Building),
            "redstone" => Some(Self::Redstone),
            "equipment" => Some(Self::Equipment),
            "misc" => Some(Self::Misc),
            _ => None,
        }
    }
}

/// `net.minecraft.world.item.crafting.CookingBookCategory`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CookingBookCategory {
    Food,
    Blocks,
    #[default]
    Misc,
}

impl CookingBookCategory {
    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::Food => "food",
            Self::Blocks => "blocks",
            Self::Misc => "misc",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "food" => Some(Self::Food),
            "blocks" => Some(Self::Blocks),
            "misc" => Some(Self::Misc),
            _ => None,
        }
    }
}

/// `net.minecraft.world.item.crafting.RecipeBookCategories` — all 13 vanilla
/// recipe book categories in Paper 26.2 registration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecipeBookCategory {
    CraftingBuildingBlocks,
    CraftingRedstone,
    CraftingEquipment,
    CraftingMisc,
    FurnaceFood,
    FurnaceBlocks,
    FurnaceMisc,
    BlastFurnaceBlocks,
    BlastFurnaceMisc,
    SmokerFood,
    Stonecutter,
    Smithing,
    Campfire,
}

impl RecipeBookCategory {
    pub const fn path(self) -> &'static str {
        match self {
            Self::CraftingBuildingBlocks => "crafting_building_blocks",
            Self::CraftingRedstone => "crafting_redstone",
            Self::CraftingEquipment => "crafting_equipment",
            Self::CraftingMisc => "crafting_misc",
            Self::FurnaceFood => "furnace_food",
            Self::FurnaceBlocks => "furnace_blocks",
            Self::FurnaceMisc => "furnace_misc",
            Self::BlastFurnaceBlocks => "blast_furnace_blocks",
            Self::BlastFurnaceMisc => "blast_furnace_misc",
            Self::SmokerFood => "smoker_food",
            Self::Stonecutter => "stonecutter",
            Self::Smithing => "smithing",
            Self::Campfire => "campfire",
        }
    }

    pub fn identifier(self) -> Identifier {
        Identifier::with_default_namespace(self.path())
    }
}

/// `net.minecraft.world.inventory.RecipeBookType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RecipeBookType {
    Crafting,
    Furnace,
    BlastFurnace,
    Smoker,
}

/// `net.minecraft.world.item.crafting.Recipe.CommonInfo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CommonInfo {
    pub show_notification: bool,
}

impl Default for CommonInfo {
    fn default() -> Self {
        Self {
            show_notification: true,
        }
    }
}

/// `net.minecraft.world.item.crafting.CraftingRecipe.CraftingBookInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CraftingBookInfo {
    pub category: CraftingBookCategory,
    pub group: String,
}

/// `net.minecraft.world.item.crafting.AbstractCookingRecipe.CookingBookInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct CookingBookInfo {
    pub category: CookingBookCategory,
    pub group: String,
}
