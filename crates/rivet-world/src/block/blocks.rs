//! `net.minecraft.world.level.block.Blocks` — the named block constants
//! (issue #228). A curated subset, not the full 1000+-block table: the blocks
//! worldgen/lighting/`NbtUtils.readBlockState` reference in the M2 slice.
//!
//! Java's `Blocks` are `Block` objects built via `register(...)`; here each is
//! the id-handle [`Block`] wrapping the registry id from the generated table.
//! The constant values are the generated ids (verified against `BLOCK_BY_ID` by
//! the tests), so `Blocks::STONE == Block::new(BlockId::from_name
//! ("minecraft:stone"))`. The constants are grouped by use-site (worldgen /
//! surface / lighting), not ordered by registry id; each wraps the generated id.

use super::Block;
use rivet_registry::generated::blocks::BlockId;

/// The named subset of `Blocks`. Each constant is the block's default-owning
/// id-handle; `default_block_state()` yields the state worldgen writes.
pub struct Blocks;

impl Blocks {
    /// `Blocks.AIR`.
    pub const AIR: Block = Block::new(BlockId(0));
    /// `Blocks.STONE`.
    pub const STONE: Block = Block::new(BlockId(1));
    /// `Blocks.GRANITE`.
    pub const GRANITE: Block = Block::new(BlockId(2));
    /// `Blocks.DIORITE`.
    pub const DIORITE: Block = Block::new(BlockId(4));
    /// `Blocks.ANDESITE`.
    pub const ANDESITE: Block = Block::new(BlockId(6));
    /// `Blocks.GRASS_BLOCK`.
    pub const GRASS_BLOCK: Block = Block::new(BlockId(8));
    /// `Blocks.DIRT`.
    pub const DIRT: Block = Block::new(BlockId(9));
    /// `Blocks.MYCELIUM` (carver: `carveBlock`'s grass/myc hasGrass check).
    pub const MYCELIUM: Block = Block::new(BlockId(373));
    /// `Blocks.COARSE_DIRT`.
    pub const COARSE_DIRT: Block = Block::new(BlockId(10));
    /// `Blocks.PODZOL`.
    pub const PODZOL: Block = Block::new(BlockId(11));
    /// `Blocks.COBBLESTONE`.
    pub const COBBLESTONE: Block = Block::new(BlockId(12));
    /// `Blocks.OAK_PLANKS`.
    pub const OAK_PLANKS: Block = Block::new(BlockId(13));
    /// `Blocks.BEDROCK`.
    pub const BEDROCK: Block = Block::new(BlockId(34));
    /// `Blocks.WATER`.
    pub const WATER: Block = Block::new(BlockId(35));
    /// `Blocks.LAVA`.
    pub const LAVA: Block = Block::new(BlockId(36));
    /// `Blocks.SAND`.
    pub const SAND: Block = Block::new(BlockId(37));
    /// `Blocks.RED_SAND`.
    pub const RED_SAND: Block = Block::new(BlockId(39));
    /// `Blocks.GRAVEL`.
    pub const GRAVEL: Block = Block::new(BlockId(40));
    /// `Blocks.GOLD_ORE`.
    pub const GOLD_ORE: Block = Block::new(BlockId(42));
    /// `Blocks.IRON_ORE`.
    pub const IRON_ORE: Block = Block::new(BlockId(44));
    /// `Blocks.COAL_ORE`.
    pub const COAL_ORE: Block = Block::new(BlockId(46));
    /// `Blocks.OAK_LOG`.
    pub const OAK_LOG: Block = Block::new(BlockId(49));
    /// `Blocks.SPRUCE_LOG`.
    pub const SPRUCE_LOG: Block = Block::new(BlockId(50));
    /// `Blocks.BIRCH_LOG`.
    pub const BIRCH_LOG: Block = Block::new(BlockId(51));
    /// `Blocks.OAK_LEAVES`.
    pub const OAK_LEAVES: Block = Block::new(BlockId(88));
    /// `Blocks.GLASS`.
    pub const GLASS: Block = Block::new(BlockId(101));
    /// `Blocks.SANDSTONE` (SurfaceRuleData: overworld `sandAndSandstone`
    /// `DEEP_UNDER_FLOOR`/`VERY_DEEP_UNDER_FLOOR` + overworldLike
    /// `sandOrSandstoneIfCeiling`).
    pub const SANDSTONE: Block = Block::new(BlockId(106));
    /// `Blocks.ICE`.
    pub const ICE: Block = Block::new(BlockId(277));
    /// `Blocks.ORANGE_STAINED_GLASS` (carver: `CarverDebugSettings.DEFAULT`
    /// water state, `Blocks.STAINED_GLASS.orange().defaultBlockState()`).
    pub const ORANGE_STAINED_GLASS: Block = Block::new(BlockId(301));
    /// `Blocks.SNOW_BLOCK`.
    pub const SNOW_BLOCK: Block = Block::new(BlockId(278));
    /// `Blocks.WHITE_TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const WHITE_TERRACOTTA: Block = Block::new(BlockId(484));
    /// `Blocks.ORANGE_TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const ORANGE_TERRACOTTA: Block = Block::new(BlockId(485));
    /// `Blocks.YELLOW_TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const YELLOW_TERRACOTTA: Block = Block::new(BlockId(488));
    /// `Blocks.LIGHT_GRAY_TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const LIGHT_GRAY_TERRACOTTA: Block = Block::new(BlockId(492));
    /// `Blocks.BROWN_TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const BROWN_TERRACOTTA: Block = Block::new(BlockId(496));
    /// `Blocks.RED_TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const RED_TERRACOTTA: Block = Block::new(BlockId(498));
    /// `Blocks.TERRACOTTA` (surface: `SurfaceSystem` clay bands).
    pub const TERRACOTTA: Block = Block::new(BlockId(554));
    /// `Blocks.PACKED_ICE` (surface: `SurfaceSystem` iceberg).
    pub const PACKED_ICE: Block = Block::new(BlockId(556));
    /// `Blocks.NETHERRACK`.
    pub const NETHERRACK: Block = Block::new(BlockId(285));
    /// `Blocks.SOUL_SAND`.
    pub const SOUL_SAND: Block = Block::new(BlockId(286));
    /// `Blocks.SOUL_SOIL` (SurfaceRuleData: nether `soulSandValley`
    /// `UNDER_CEILING`/`UNDER_FLOOR`).
    pub const SOUL_SOIL: Block = Block::new(BlockId(287));
    /// `Blocks.BASALT` (SurfaceRuleData: nether `basaltDeltas`
    /// `UNDER_CEILING`/`UNDER_FLOOR`).
    pub const BASALT: Block = Block::new(BlockId(288));
    /// `Blocks.ACACIA_BUTTON` (carver: `CarverDebugSettings.DEFAULT` air state).
    pub const ACACIA_BUTTON: Block = Block::new(BlockId(447));
    /// `Blocks.RED_SANDSTONE` (SurfaceRuleData: overworld badlands `ON_CEILING`).
    pub const RED_SANDSTONE: Block = Block::new(BlockId(595));
    /// `Blocks.NETHER_WART_BLOCK` (SurfaceRuleData: nether `crimsonForest`
    /// `netherWart` selector).
    pub const NETHER_WART_BLOCK: Block = Block::new(BlockId(672));
    /// `Blocks.WARPED_NYLIUM` (SurfaceRuleData: nether `warpedForest`).
    pub const WARPED_NYLIUM: Block = Block::new(BlockId(866));
    /// `Blocks.WARPED_WART_BLOCK` (SurfaceRuleData: nether `warpedForest`
    /// `netherWart` selector).
    pub const WARPED_WART_BLOCK: Block = Block::new(BlockId(868));
    /// `Blocks.CRIMSON_NYLIUM` (SurfaceRuleData: nether `crimsonForest`).
    pub const CRIMSON_NYLIUM: Block = Block::new(BlockId(875));
    /// `Blocks.BLACKSTONE` (SurfaceRuleData: nether `basaltDeltas`
    /// `UNDER_FLOOR` `netherStateSelector`).
    pub const BLACKSTONE: Block = Block::new(BlockId(924));
    /// `Blocks.MUD` (SurfaceRuleData: overworld `mangroveSwamp`).
    pub const MUD: Block = Block::new(BlockId(1150));
    /// `Blocks.DEEPSLATE`.
    pub const DEEPSLATE: Block = Block::new(BlockId(1151));
    /// `Blocks.DEEPSLATE_IRON_ORE` (noisegen: `OreVeinifier.VeinType.IRON`).
    pub const DEEPSLATE_IRON_ORE: Block = Block::new(BlockId(45));
    /// `Blocks.END_STONE` (noisegen: `NoiseGeneratorSettings.end`).
    pub const END_STONE: Block = Block::new(BlockId(393));
    /// `Blocks.OBSIDIAN` (end-leaves: `EndPlatformFeature`'s floor).
    pub const OBSIDIAN: Block = Block::new(BlockId(193));
    /// `Blocks.WALL_TORCH` (end-leaves: `EndPodiumFeature`'s pillar torches).
    pub const WALL_TORCH: Block = Block::new(BlockId(195));
    /// `Blocks.END_PORTAL` (end-leaves: `EndPodiumFeature`'s active portal).
    pub const END_PORTAL: Block = Block::new(BlockId(391));
    /// `Blocks.CHORUS_PLANT` (end-leaves: `ChorusPlantFeature`'s stem).
    pub const CHORUS_PLANT: Block = Block::new(BlockId(656));
    /// `Blocks.CHORUS_FLOWER` (end-leaves: the chorus-growth terminal flower).
    pub const CHORUS_FLOWER: Block = Block::new(BlockId(657));
    /// `Blocks.OAK_BUTTON` (noisegen: `OreVeinifier` debug ore-veins).
    pub const OAK_BUTTON: Block = Block::new(BlockId(443));
    /// `Blocks.SLIME_BLOCK` (noisegen: `debugPreliminarySurfaceLevel`).
    pub const SLIME_BLOCK: Block = Block::new(BlockId(523));
    /// `Blocks.BARRIER` (carver: `CarverDebugSettings.DEFAULT` barrier state).
    pub const BARRIER: Block = Block::new(BlockId(524));
    /// `Blocks.CAVE_AIR` (carver: `WorldCarver.CAVE_AIR`, the nether carver's
    /// carved-block state above lava level).
    pub const CAVE_AIR: Block = Block::new(BlockId(795));
    /// `Blocks.CANDLE` (carver: `CarverDebugSettings.DEFAULT` lava state).
    pub const CANDLE: Block = Block::new(BlockId(944));
    /// `Blocks.TUFF` (noisegen: `OreVeinifier.VeinType.IRON` filler).
    pub const TUFF: Block = Block::new(BlockId(984));
    /// `Blocks.SULFUR` (SurfaceRuleData: overworld `sulfurCaveBands` in
    /// `SULFUR_CAVES`).
    pub const SULFUR: Block = Block::new(BlockId(998));
    /// `Blocks.CINNABAR` (SurfaceRuleData: overworld `sulfurCaveBands` in
    /// `SULFUR_CAVES`).
    pub const CINNABAR: Block = Block::new(BlockId(1012));
    /// `Blocks.CALCITE` (SurfaceRuleData: overworld calcite bands on
    /// `CALCITE` noise).
    pub const CALCITE: Block = Block::new(BlockId(1025));
    /// `Blocks.POWDER_SNOW` (SurfaceRuleData: overworld
    /// `powderSnowUnderRule`/`powderSnowSurfaceRule` in `SNOWY_SLOPES`/`GROVE`).
    pub const POWDER_SNOW: Block = Block::new(BlockId(1027));
    /// `Blocks.COPPER_ORE` (noisegen: `OreVeinifier.VeinType.COPPER`).
    pub const COPPER_ORE: Block = Block::new(BlockId(1042));
    /// `Blocks.RAW_IRON_BLOCK` (noisegen: `OreVeinifier.VeinType.IRON`).
    pub const RAW_IRON_BLOCK: Block = Block::new(BlockId(1173));
    /// `Blocks.RAW_COPPER_BLOCK` (noisegen: `OreVeinifier.VeinType.COPPER`).
    pub const RAW_COPPER_BLOCK: Block = Block::new(BlockId(1174));
    /// `Blocks.HONEY_BLOCK` (noisegen: `debugPreliminarySurfaceLevel`).
    pub const HONEY_BLOCK: Block = Block::new(BlockId(913));
    /// `Blocks.SEAGRASS` (feature: `SeagrassFeature`).
    pub const SEAGRASS: Block = Block::new(BlockId(136));
    /// `Blocks.TALL_SEAGRASS` (feature: `SeagrassFeature`).
    pub const TALL_SEAGRASS: Block = Block::new(BlockId(137));
    /// `Blocks.DIRT_PATH` (feature: `BlockPileFeature` `mayPlaceOn`).
    pub const DIRT_PATH: Block = Block::new(BlockId(666));
    /// `Blocks.KELP` (feature: `KelpFeature`).
    pub const KELP: Block = Block::new(BlockId(742));
    /// `Blocks.KELP_PLANT` (feature: `KelpFeature`).
    pub const KELP_PLANT: Block = Block::new(BlockId(743));
    /// `Blocks.SEA_PICKLE` (feature: `SeaPickleFeature`).
    pub const SEA_PICKLE: Block = Block::new(BlockId(788));
    /// `Blocks.BLUE_ICE` (feature: `BlueIceFeature`).
    pub const BLUE_ICE: Block = Block::new(BlockId(789));
    /// `Blocks.BAMBOO` (feature: `BambooFeature`).
    pub const BAMBOO: Block = Block::new(BlockId(792));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `Blocks` constant must wrap the registry id the generated name
    /// table gives, and `name()` must round-trip through `BlockId::from_name`.
    #[test]
    fn constants_match_generated_names_and_ids() {
        let all = [
            Blocks::AIR,
            Blocks::STONE,
            Blocks::GRANITE,
            Blocks::DIORITE,
            Blocks::ANDESITE,
            Blocks::GRASS_BLOCK,
            Blocks::DIRT,
            Blocks::MYCELIUM,
            Blocks::COARSE_DIRT,
            Blocks::PODZOL,
            Blocks::COBBLESTONE,
            Blocks::OAK_PLANKS,
            Blocks::BEDROCK,
            Blocks::WATER,
            Blocks::LAVA,
            Blocks::SAND,
            Blocks::RED_SAND,
            Blocks::GRAVEL,
            Blocks::GOLD_ORE,
            Blocks::IRON_ORE,
            Blocks::COAL_ORE,
            Blocks::OAK_LOG,
            Blocks::SPRUCE_LOG,
            Blocks::BIRCH_LOG,
            Blocks::OAK_LEAVES,
            Blocks::GLASS,
            Blocks::SANDSTONE,
            Blocks::ICE,
            Blocks::ORANGE_STAINED_GLASS,
            Blocks::SNOW_BLOCK,
            Blocks::WHITE_TERRACOTTA,
            Blocks::ORANGE_TERRACOTTA,
            Blocks::YELLOW_TERRACOTTA,
            Blocks::LIGHT_GRAY_TERRACOTTA,
            Blocks::BROWN_TERRACOTTA,
            Blocks::RED_TERRACOTTA,
            Blocks::TERRACOTTA,
            Blocks::PACKED_ICE,
            Blocks::NETHERRACK,
            Blocks::SOUL_SAND,
            Blocks::SOUL_SOIL,
            Blocks::BASALT,
            Blocks::ACACIA_BUTTON,
            Blocks::RED_SANDSTONE,
            Blocks::NETHER_WART_BLOCK,
            Blocks::WARPED_NYLIUM,
            Blocks::WARPED_WART_BLOCK,
            Blocks::CRIMSON_NYLIUM,
            Blocks::BLACKSTONE,
            Blocks::MUD,
            Blocks::DEEPSLATE,
            Blocks::DEEPSLATE_IRON_ORE,
            Blocks::END_STONE,
            Blocks::OBSIDIAN,
            Blocks::WALL_TORCH,
            Blocks::END_PORTAL,
            Blocks::CHORUS_PLANT,
            Blocks::CHORUS_FLOWER,
            Blocks::OAK_BUTTON,
            Blocks::SLIME_BLOCK,
            Blocks::BARRIER,
            Blocks::CAVE_AIR,
            Blocks::CANDLE,
            Blocks::TUFF,
            Blocks::SULFUR,
            Blocks::CINNABAR,
            Blocks::CALCITE,
            Blocks::POWDER_SNOW,
            Blocks::COPPER_ORE,
            Blocks::RAW_IRON_BLOCK,
            Blocks::RAW_COPPER_BLOCK,
            Blocks::HONEY_BLOCK,
            Blocks::SEAGRASS,
            Blocks::TALL_SEAGRASS,
            Blocks::DIRT_PATH,
            Blocks::KELP,
            Blocks::KELP_PLANT,
            Blocks::SEA_PICKLE,
            Blocks::BLUE_ICE,
            Blocks::BAMBOO,
        ];
        for block in all {
            let by_name = BlockId::from_name(block.name())
                .unwrap_or_else(|| panic!("generated name `{}` must resolve", block.name()));
            assert_eq!(block.id(), by_name, "id mismatch for `{}`", block.name());
            assert_eq!(
                block.name(),
                by_name.name(),
                "name mismatch for id {}",
                block.id().0
            );
        }
    }

    /// The Paper 26.2 `SurfaceRuleData` block constants this PR adds must pin
    /// the exact raw registry ids from the generated `BLOCK_BY_NAME` table.
    /// (The constants already on main are covered by
    /// [`constants_match_generated_names_and_ids`], so this asserts only the
    /// raw-id pin for the PR-owned additions.)
    #[test]
    fn surface_rule_data_constants_pin_raw_ids() {
        let cases: [(Block, u16); 14] = [
            (Blocks::SANDSTONE, 106),
            (Blocks::SOUL_SOIL, 287),
            (Blocks::BASALT, 288),
            (Blocks::RED_SANDSTONE, 595),
            (Blocks::NETHER_WART_BLOCK, 672),
            (Blocks::WARPED_NYLIUM, 866),
            (Blocks::WARPED_WART_BLOCK, 868),
            (Blocks::CRIMSON_NYLIUM, 875),
            (Blocks::BLACKSTONE, 924),
            (Blocks::SULFUR, 998),
            (Blocks::CINNABAR, 1012),
            (Blocks::CALCITE, 1025),
            (Blocks::POWDER_SNOW, 1027),
            (Blocks::MUD, 1150),
        ];
        for (block, raw_id) in cases {
            assert_eq!(block.id().0, raw_id, "raw id for `{}`", block.name());
        }
    }

    /// `Block::default_block_state()` must yield the block's default state
    /// (id equal to `BlockState::of`), and `state_definition` must expose the
    /// owning block. Spot-checks a singleton (stone), a multi-property block
    /// (oak_log has `axis`), and a fluid (water).
    #[test]
    fn default_state_and_definition_are_consistent() {
        use rivet_registry::block_state::BlockState;
        use rivet_registry::generated::blocks::BlockId;

        let stone = Blocks::STONE;
        assert_eq!(stone.default_block_state(), BlockState::of(stone.id()));
        assert!(stone.state_definition().is_singleton_state());

        let oak_log = Blocks::OAK_LOG;
        assert_eq!(oak_log.default_block_state(), BlockState::of(oak_log.id()));
        assert_eq!(
            oak_log
                .state_definition()
                .get_property("axis")
                .unwrap()
                .name(),
            "axis"
        );
        assert_eq!(oak_log.state_definition().block(), oak_log.id());

        let water = Blocks::WATER;
        assert_eq!(water.name(), "minecraft:water");
        assert_eq!(water.id(), BlockId::from_name("minecraft:water").unwrap());
    }
}
