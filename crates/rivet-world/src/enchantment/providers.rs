//! Port of `net.minecraft.world.item.enchantment.providers.*` (MC 26.2):
//! `EnchantmentProvider`, `SingleEnchantment`, `EnchantmentsByCost`,
//! `EnchantmentsByCostWithDifficulty`, and `VanillaEnchantmentProviders`.

use rivet_registry::Identifier;
use rivet_util::mth;
use rivet_util::random::RandomSource;
use rivet_util::valueproviders::int_provider::IntProvider;

use crate::enchantment::definition::NamedHolderSet;
use crate::enchantment::helper::select_enchantment;
use crate::enchantment::item_enchantments::{EnchantableItemStack, MutableItemEnchantments};
use crate::enchantment::registry::EnchantmentRegistry;

/// `net.minecraft.world.DifficultyInstance` subset required by
/// [`EnchantmentProvider::enchant`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DifficultyContext {
    /// `DifficultyInstance.getSpecialMultiplier()` — in `[0.0, 1.0]`.
    pub special_multiplier: f32,
}

impl DifficultyContext {
    pub const fn new(special_multiplier: f32) -> Self {
        Self { special_multiplier }
    }
}

/// `net.minecraft.world.item.enchantment.providers.EnchantmentProvider`.
#[derive(Debug, Clone)]
pub enum EnchantmentProvider {
    /// `SingleEnchantment` (`minecraft:single`).
    Single {
        enchantment: Identifier,
        level: IntProvider,
    },
    /// `EnchantmentsByCost` (`minecraft:by_cost`).
    ByCost {
        enchantments: NamedHolderSet,
        cost: IntProvider,
    },
    /// `EnchantmentsByCostWithDifficulty` (`minecraft:by_cost_with_difficulty`).
    ByCostWithDifficulty {
        enchantments: NamedHolderSet,
        min_cost: i32,
        max_cost_span: i32,
    },
}

impl EnchantmentProvider {
    /// Registered codec type name (matches `EnchantmentProviderTypes.bootstrap`).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Single { .. } => "single",
            Self::ByCost { .. } => "by_cost",
            Self::ByCostWithDifficulty { .. } => "by_cost_with_difficulty",
        }
    }

    /// `EnchantmentProvider.enchant(ItemStack, ItemEnchantments.Mutable, RandomSource, DifficultyInstance)`.
    pub fn enchant<R: RandomSource>(
        &self,
        item: &EnchantableItemStack,
        enchantments: &mut MutableItemEnchantments,
        random: &mut R,
        difficulty: DifficultyContext,
        registry: &EnchantmentRegistry,
    ) {
        match self {
            Self::Single { enchantment, level } => {
                let Some(def) = registry.get(enchantment) else {
                    return;
                };
                let sampled = level.sample(random);
                let clamped = mth::clamp(sampled, def.min_level(), def.max_level());
                enchantments.upgrade(enchantment.clone(), clamped);
            }
            Self::ByCost {
                enchantments: source_set,
                cost,
            } => {
                let candidates = resolve_provider_candidates(source_set, registry);
                let sampled_cost = cost.sample(random);
                let selected = select_enchantment(random, item, sampled_cost, &candidates);
                for inst in selected {
                    enchantments.upgrade(inst.enchantment, inst.level);
                }
            }
            Self::ByCostWithDifficulty {
                enchantments: source_set,
                min_cost,
                max_cost_span,
            } => {
                let special = difficulty.special_multiplier;
                let cost = mth::next_int(
                    random,
                    *min_cost,
                    min_cost.wrapping_add((special * (*max_cost_span as f32)) as i32),
                );
                let candidates = resolve_provider_candidates(source_set, registry);
                let selected = select_enchantment(random, item, cost, &candidates);
                for inst in selected {
                    enchantments.upgrade(inst.enchantment, inst.level);
                }
            }
        }
    }
}

fn resolve_provider_candidates<'a>(
    source: &NamedHolderSet,
    registry: &'a EnchantmentRegistry,
) -> Vec<&'a crate::enchantment::Enchantment> {
    match source {
        NamedHolderSet::Empty => Vec::new(),
        NamedHolderSet::Tag(tag) => registry.tag_members(tag),
        NamedHolderSet::Direct(ids) => ids.iter().filter_map(|id| registry.get(id)).collect(),
    }
}

/// Vanilla provider keys from `VanillaEnchantmentProviders.java` (MC 26.2).
pub mod vanilla_providers {
    use super::*;
    use rivet_util::valueproviders::constant_int::ConstantInt;
    use rivet_util::valueproviders::uniform_int::UniformInt;

    pub const MOB_SPAWN_EQUIPMENT: &str = "minecraft:mob_spawn_equipment";
    pub const PILLAGER_SPAWN_CROSSBOW: &str = "minecraft:pillager_spawn_crossbow";
    pub const RAID_PILLAGER_POST_WAVE_3: &str = "minecraft:raid/pillager_post_wave_3";
    pub const RAID_PILLAGER_POST_WAVE_5: &str = "minecraft:raid/pillager_post_wave_5";
    pub const RAID_VINDICATOR: &str = "minecraft:raid/vindicator";
    pub const RAID_VINDICATOR_POST_WAVE_5: &str = "minecraft:raid/vindicator_post_wave_5";
    pub const ENDERMAN_LOOT_DROP: &str = "minecraft:enderman_loot_drop";

    /// Returns the vanilla provider definition for any key in
    /// `VanillaEnchantmentProviders.bootstrap`.
    pub fn get(key: &str) -> Option<EnchantmentProvider> {
        match key {
            MOB_SPAWN_EQUIPMENT => Some(EnchantmentProvider::ByCostWithDifficulty {
                enchantments: NamedHolderSet::Tag("minecraft:on_mob_spawn_equipment".to_owned()),
                min_cost: 5,
                max_cost_span: 17,
            }),
            PILLAGER_SPAWN_CROSSBOW => Some(EnchantmentProvider::Single {
                enchantment: Identifier::with_default_namespace("piercing"),
                level: IntProvider::Constant(ConstantInt::of(1)),
            }),
            RAID_PILLAGER_POST_WAVE_3 => Some(EnchantmentProvider::Single {
                enchantment: Identifier::with_default_namespace("quick_charge"),
                level: IntProvider::Constant(ConstantInt::of(1)),
            }),
            RAID_PILLAGER_POST_WAVE_5 => Some(EnchantmentProvider::Single {
                enchantment: Identifier::with_default_namespace("quick_charge"),
                level: IntProvider::Constant(ConstantInt::of(2)),
            }),
            RAID_VINDICATOR => Some(EnchantmentProvider::Single {
                enchantment: Identifier::with_default_namespace("sharpness"),
                level: IntProvider::Constant(ConstantInt::of(1)),
            }),
            RAID_VINDICATOR_POST_WAVE_5 => Some(EnchantmentProvider::Single {
                enchantment: Identifier::with_default_namespace("sharpness"),
                level: IntProvider::Constant(ConstantInt::of(2)),
            }),
            ENDERMAN_LOOT_DROP => Some(EnchantmentProvider::Single {
                enchantment: Identifier::with_default_namespace("silk_touch"),
                level: IntProvider::Uniform(UniformInt::of(1, 1)),
            }),
            _ => None,
        }
    }
}
