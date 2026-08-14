//! The overworld's configured world-carver data — `Carvers.bootstrap` for the
//! overworld (26.2), plus the shared `BiomeGenerationSettings` the CARVERS
//! status step drives.
//!
//! Java source: `working/Paper/.../data/worldgen/Carvers.java`. The three
//! overworld configured carvers are registered in `Carvers.bootstrap`:
//!
//! - `CAVE` — `WorldCarver.CAVE.configured(new CaveCarverConfiguration(
//!   0.15F, UniformHeight.of(aboveBottom(8), absolute(180)),
//!   UniformFloat.of(0.1F, 0.9F), aboveBottom(8),
//!   CarverDebugSettings.of(false, CRIMSON_BUTTON),
//!   overworld_carver_replaceables, UniformFloat.of(0.7F, 1.4F),
//!   UniformFloat.of(0.8F, 1.3F), UniformFloat.of(-1.0F, -0.4F)))`
//! - `CAVE_EXTRA_UNDERGROUND` — the same shape with probability 0.07F, the
//!   `aboveBottom(8) → absolute(47)` height band and the `OAK_BUTTON` debug
//!   state.
//! - `CANYON` — `WorldCarver.CANYON.configured(new CanyonCarverConfiguration(
//!   0.01F, UniformHeight.of(absolute(10), absolute(67)),
//!   ConstantFloat.of(3.0F), aboveBottom(8),
//!   CarverDebugSettings.of(false, WARPED_BUTTON), overworld_carver_replaceables,
//!   UniformFloat.of(-0.125F, 0.125F), new CanyonShapeConfiguration(
//!   UniformFloat.of(0.75F, 1.0F), TrapezoidFloat.of(0.0F, 6.0F, 2.0F), 3,
//!   UniformFloat.of(0.75F, 1.0F), 1.0F, 0.0F)))`
//!
//! Every overworld biome shares this carver set: `BiomeDefaultFeatures.
//! addDefaultCarversAndLakes` (`globalOverworldGeneration`) adds exactly
//! `CAVE`, `CAVE_EXTRA_UNDERGROUND`, `CANYON` in that order (BiomeDefaultFeatures.
//! java lines 15-21), and the biome→settings resolution the CARVERS driver uses
//! is a single shared `BiomeGenerationSettings` for any biome holder (the
//! `getBiomeGenerationSettings` panicking path is RivetTodo(#178); see
//! `apply_carvers`).
//!
//! The `overworld_carver_replaceables` `HolderSet<Block>` is built from the
//! generated `BLOCK_TAG_BY_NAME["minecraft:overworld_carver_replaceables"]`
//! member list. The block registry is not populated in the SCC slice, so the
//! set is a `Direct` list of `Reference` holders carrying the raw generated
//! block id against the `registries::BLOCK` registry id (the same identity the
//! production `HolderSet` would carry once the block unit lands; `contains_id`
//! matches `Reference` members only — see holder_set.rs).

use crate::biome::biome_generation_settings::{BiomeGenerationSettings, PlainBuilder};
use crate::block::blocks::Blocks;
use crate::levelgen::carver::canyon_carver_configuration::{
    CanyonCarverConfiguration, CanyonShapeConfiguration,
};
use crate::levelgen::carver::carver_debug_settings::CarverDebugSettings;
use crate::levelgen::carver::cave_carver_configuration::CaveCarverConfiguration;
use crate::levelgen::carver::configured_world_carver::{
    ConfiguredWorldCarver, ConfiguredWorldCarverErased,
};
use crate::levelgen::carver::world_carver::WorldCarverId;
use crate::levelgen::heightproviders::height_provider::HeightProvider;
use crate::levelgen::heightproviders::uniform_height::UniformHeight;
use crate::levelgen::vertical_anchor::VerticalAnchor;
use rivet_registry::block_state::BlockState;
use rivet_registry::builder::RegistryBuilder;
use rivet_registry::generated::blocks::BlockId;
use rivet_registry::generated::tags::BLOCK_TAG_BY_NAME;
use rivet_registry::holder::Holder;
use rivet_registry::holder_set::HolderSet;
use rivet_registry::registries::{self, BlockType};
use rivet_util::valueproviders::constant_float::ConstantFloat;
use rivet_util::valueproviders::float_provider::FloatProvider;
use rivet_util::valueproviders::trapezoid_float::TrapezoidFloat;
use rivet_util::valueproviders::uniform_float::UniformFloat;
use std::sync::LazyLock;

/// `Blocks.CRIMSON_BUTTON.defaultBlockState()` — the `CAVE` carver's debug air
/// state. The block is not in the curated `Blocks` subset (raw id 897), so the
/// state is resolved per call (`BlockState::of` is a generated-table lookup, not
/// const).
fn crimson_button() -> BlockState {
    BlockState::of(BlockId(897))
}
/// `Blocks.WARPED_BUTTON.defaultBlockState()` — the `CANYON` carver's debug air
/// state. Not in the curated `Blocks` subset (raw id 898).
fn warped_button() -> BlockState {
    BlockState::of(BlockId(898))
}

/// `BlockTags.OVERWORLD_CARVER_REPLACEABLES` as a `HolderSet<Block>` over the
/// `minecraft:block` registry.
///
/// The block registry is unpopulated in the SCC slice, so this is a `Direct`
/// list of `Reference` holders: each member's generated block id against the
/// `registries::BLOCK` registry id. `WorldCarver.canReplaceBlock` tests
/// `set.contains_id(state.block_id())` (the `BlockState.is(HolderSet)`
/// surface), so membership is faithful to the tag.
pub fn overworld_carver_replaceables() -> HolderSet<BlockType> {
    let registry_id = RegistryBuilder::<BlockType>::new(&*registries::BLOCK).registry_id();
    let members = BLOCK_TAG_BY_NAME["minecraft:overworld_carver_replaceables"];
    let holders = members
        .iter()
        .map(|name| {
            let id = BlockId::from_name(name)
                .expect("overworld_carver_replaceables member must resolve in the block table")
                .0 as u32;
            Holder::reference(registry_id, id)
        })
        .collect();
    HolderSet::direct(holders)
}

/// `WorldCarver.CAVE.configured(Carvers.CAVE)` — the `CAVE` configured carver.
pub fn cave_carver() -> ConfiguredWorldCarverErased {
    ConfiguredWorldCarver::new(
        WorldCarverId::CAVE,
        CaveCarverConfiguration::new(
            0.15,
            HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::above_bottom(8),
                VerticalAnchor::absolute(180),
            )),
            FloatProvider::Uniform(UniformFloat::of(0.1, 0.9)),
            VerticalAnchor::above_bottom(8),
            CarverDebugSettings::of_debug_mode_air(false, crimson_button()),
            overworld_carver_replaceables(),
            FloatProvider::Uniform(UniformFloat::of(0.7, 1.4)),
            FloatProvider::Uniform(UniformFloat::of(0.8, 1.3)),
            FloatProvider::Uniform(UniformFloat::of(-1.0, -0.4)),
        ),
    )
    .into_erased()
}

/// `WorldCarver.CAVE.configured(Carvers.CAVE_EXTRA_UNDERGROUND)` — the
/// `CAVE_EXTRA_UNDERGROUND` configured carver (the low-elevation cave band).
pub fn cave_extra_underground_carver() -> ConfiguredWorldCarverErased {
    ConfiguredWorldCarver::new(
        WorldCarverId::CAVE,
        CaveCarverConfiguration::new(
            0.07,
            HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::above_bottom(8),
                VerticalAnchor::absolute(47),
            )),
            FloatProvider::Uniform(UniformFloat::of(0.1, 0.9)),
            VerticalAnchor::above_bottom(8),
            CarverDebugSettings::of_debug_mode_air(false, Blocks::OAK_BUTTON.default_block_state()),
            overworld_carver_replaceables(),
            FloatProvider::Uniform(UniformFloat::of(0.7, 1.4)),
            FloatProvider::Uniform(UniformFloat::of(0.8, 1.3)),
            FloatProvider::Uniform(UniformFloat::of(-1.0, -0.4)),
        ),
    )
    .into_erased()
}

/// `WorldCarver.CANYON.configured(Carvers.CANYON)` — the `CANYON` configured
/// carver.
pub fn canyon_carver() -> ConfiguredWorldCarverErased {
    ConfiguredWorldCarver::new(
        WorldCarverId::CANYON,
        CanyonCarverConfiguration::new(
            0.01,
            HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::absolute(10),
                VerticalAnchor::absolute(67),
            )),
            FloatProvider::Constant(ConstantFloat::of(3.0)),
            VerticalAnchor::above_bottom(8),
            CarverDebugSettings::of_debug_mode_air(false, warped_button()),
            overworld_carver_replaceables(),
            FloatProvider::Uniform(UniformFloat::of(-0.125, 0.125)),
            CanyonShapeConfiguration::new(
                FloatProvider::Uniform(UniformFloat::of(0.75, 1.0)),
                FloatProvider::Trapezoid(TrapezoidFloat::of(0.0, 6.0, 2.0)),
                3,
                FloatProvider::Uniform(UniformFloat::of(0.75, 1.0)),
                1.0,
                0.0,
            ),
        ),
    )
    .into_erased()
}

/// The shared overworld `BiomeGenerationSettings` — carvers in the
/// `addDefaultCarversAndLakes` order `CAVE`, `CAVE_EXTRA_UNDERGROUND`,
/// `CANYON`, no feature steps.
///
/// The CARVERS status step resolves every biome holder to this one settings
/// object (RivetTodo(#178) defers the per-biome `getBiomeGenerationSettings`
/// split; all overworld biomes share the same carvers — see the module doc).
/// Built once: the holders and the replaceables set are stateless values, so a
/// single shared instance is the driver's `&'static` reference.
pub fn overworld_carver_settings() -> &'static BiomeGenerationSettings {
    static SETTINGS: LazyLock<BiomeGenerationSettings> = LazyLock::new(|| {
        PlainBuilder::default()
            .add_carver(Holder::direct(cave_carver()))
            .add_carver(Holder::direct(cave_extra_underground_carver()))
            .add_carver(Holder::direct(canyon_carver()))
            .build()
    });
    &SETTINGS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::carver::canyon_carver_configuration::CanyonCarverConfiguration;
    use crate::levelgen::carver::carver_configuration::CarverConfiguration;
    use crate::levelgen::carver::cave_carver_configuration::CaveCarverConfiguration;
    use crate::levelgen::heightproviders::height_provider::HeightProvider;
    use rivet_util::valueproviders::constant_float::ConstantFloat;
    use rivet_util::valueproviders::float_provider::FloatProvider;
    use rivet_util::valueproviders::trapezoid_float::TrapezoidFloat;
    use rivet_util::valueproviders::uniform_float::UniformFloat;

    /// Downcast an erased carver's config to its concrete type.
    fn downcast_cave(config: &dyn CarverConfiguration) -> &CaveCarverConfiguration {
        config
            .as_any()
            .downcast_ref::<CaveCarverConfiguration>()
            .expect("CAVE config must be CaveCarverConfiguration")
    }

    fn downcast_canyon(config: &dyn CarverConfiguration) -> &CanyonCarverConfiguration {
        config
            .as_any()
            .downcast_ref::<CanyonCarverConfiguration>()
            .expect("CANYON config must be CanyonCarverConfiguration")
    }

    #[test]
    fn replaceables_match_the_generated_tag_and_carve_set() {
        let set = overworld_carver_replaceables();
        let members = BLOCK_TAG_BY_NAME["minecraft:overworld_carver_replaceables"];
        // Non-vacuous: the tag is the full 55-member overworld list, and the
        // set carries one Reference per member (the raw generated block id).
        assert_eq!(members.len(), 55);
        // `contains_id` matches Reference members only — the faithful
        // `BlockState.is(HolderSet)` membership for the carver replace test.
        assert!(set.contains_id(Blocks::STONE.id().0 as u32)); // minecraft:stone
        assert!(set.contains_id(Blocks::WATER.id().0 as u32)); // minecraft:water
        assert!(set.contains_id(BlockId::from_name("minecraft:deepslate").unwrap().0 as u32));
        assert!(set.contains_id(BlockId::from_name("minecraft:sulfur").unwrap().0 as u32));
        // Air (0) is not replaceable by the overworld carvers.
        assert!(!set.contains_id(0));
    }

    #[test]
    fn cave_carver_config_matches_paper() {
        let carver = cave_carver();
        assert_eq!(carver.world_carver, WorldCarverId::CAVE);
        let c = downcast_cave(carver.config.as_ref());
        assert_eq!(c.probability(), 0.15);
        assert_eq!(
            c.y(),
            &HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::above_bottom(8),
                VerticalAnchor::absolute(180),
            ))
        );
        assert_eq!(
            c.y_scale(),
            &FloatProvider::Uniform(UniformFloat::of(0.1, 0.9))
        );
        assert_eq!(c.lava_level(), &VerticalAnchor::above_bottom(8));
        // `CarverDebugSettings.of(false, CRIMSON_BUTTON)`: debug off, the raw
        // 897 button state as the air override (water/lava/barrier = DEFAULT).
        let d = c.debug_settings();
        assert!(!d.is_debug_mode());
        assert_eq!(d.air_state(), BlockState::of(BlockId(897)));
        assert_eq!(
            c.horizontal_radius_multiplier,
            FloatProvider::Uniform(UniformFloat::of(0.7, 1.4))
        );
        assert_eq!(
            c.vertical_radius_multiplier,
            FloatProvider::Uniform(UniformFloat::of(0.8, 1.3))
        );
        assert_eq!(
            c.floor_level,
            FloatProvider::Uniform(UniformFloat::of(-1.0, -0.4))
        );
    }

    #[test]
    fn cave_extra_underground_config_matches_paper() {
        let carver = cave_extra_underground_carver();
        assert_eq!(carver.world_carver, WorldCarverId::CAVE);
        let c = downcast_cave(carver.config.as_ref());
        assert_eq!(c.probability(), 0.07);
        assert_eq!(
            c.y(),
            &HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::above_bottom(8),
                VerticalAnchor::absolute(47),
            ))
        );
        assert_eq!(c.lava_level(), &VerticalAnchor::above_bottom(8));
        // `CarverDebugSettings.of(false, OAK_BUTTON)`.
        assert!(!c.debug_settings().is_debug_mode());
        assert_eq!(
            c.debug_settings().air_state(),
            Blocks::OAK_BUTTON.default_block_state()
        );
        assert_eq!(
            c.horizontal_radius_multiplier,
            FloatProvider::Uniform(UniformFloat::of(0.7, 1.4))
        );
        assert_eq!(
            c.vertical_radius_multiplier,
            FloatProvider::Uniform(UniformFloat::of(0.8, 1.3))
        );
        assert_eq!(
            c.floor_level,
            FloatProvider::Uniform(UniformFloat::of(-1.0, -0.4))
        );
    }

    #[test]
    fn canyon_config_matches_paper() {
        let carver = canyon_carver();
        assert_eq!(carver.world_carver, WorldCarverId::CANYON);
        let c = downcast_canyon(carver.config.as_ref());
        assert_eq!(c.probability(), 0.01);
        assert_eq!(
            c.y(),
            &HeightProvider::Uniform(UniformHeight::of(
                VerticalAnchor::absolute(10),
                VerticalAnchor::absolute(67),
            ))
        );
        assert_eq!(
            c.y_scale(),
            &FloatProvider::Constant(ConstantFloat::of(3.0))
        );
        assert_eq!(c.lava_level(), &VerticalAnchor::above_bottom(8));
        // `CarverDebugSettings.of(false, WARPED_BUTTON)` — raw 898.
        assert!(!c.debug_settings().is_debug_mode());
        assert_eq!(c.debug_settings().air_state(), BlockState::of(BlockId(898)));
        assert_eq!(
            c.vertical_rotation,
            FloatProvider::Uniform(UniformFloat::of(-0.125, 0.125))
        );
        let s = &c.shape;
        assert_eq!(
            s.distance_factor,
            FloatProvider::Uniform(UniformFloat::of(0.75, 1.0))
        );
        assert_eq!(
            s.thickness,
            FloatProvider::Trapezoid(TrapezoidFloat::of(0.0, 6.0, 2.0))
        );
        assert_eq!(s.width_smoothness, 3);
        assert_eq!(
            s.horizontal_radius_factor,
            FloatProvider::Uniform(UniformFloat::of(0.75, 1.0))
        );
        assert_eq!(s.vertical_radius_default_factor, 1.0);
        assert_eq!(s.vertical_radius_center_factor, 0.0);
    }

    #[test]
    fn shared_settings_carvers_in_add_default_carvers_and_lakes_order() {
        // The settings must expose the three carvers in the exact
        // `addDefaultCarversAndLakes` order (CAVE, CAVE_EXTRA_UNDERGROUND,
        // CANYON) — the order the CARVERS driver's per-source-chunk draw loop
        // relies on for the `setLargeFeatureSeed(seed + index, ...)` indexing.
        let settings = overworld_carver_settings();
        match settings.get_carvers() {
            HolderSet::Direct(carvers) => {
                assert_eq!(
                    carvers
                        .iter()
                        .map(|h| match h {
                            Holder::Direct(c) => c.world_carver.clone(),
                            Holder::Reference { .. } => panic!("carvers must be Direct holders"),
                        })
                        .collect::<Vec<_>>(),
                    vec![
                        WorldCarverId::CAVE,
                        WorldCarverId::CAVE,
                        WorldCarverId::CANYON,
                    ],
                );
            }
            HolderSet::Named { .. } => panic!("shared carvers must be a Direct set"),
        }
    }

    #[test]
    fn shared_settings_are_a_single_shared_instance() {
        // The driver's biome→settings resolution returns the same `&'static`
        // instance for every biome holder — identity, not value equality.
        assert!(std::ptr::eq(
            overworld_carver_settings(),
            overworld_carver_settings()
        ));
    }
}
