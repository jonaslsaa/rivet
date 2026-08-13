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
    /// `Blocks.ICE`.
    pub const ICE: Block = Block::new(BlockId(277));
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
    /// `Blocks.DEEPSLATE`.
    pub const DEEPSLATE: Block = Block::new(BlockId(1151));
    /// `Blocks.DEEPSLATE_IRON_ORE` (noisegen: `OreVeinifier.VeinType.IRON`).
    pub const DEEPSLATE_IRON_ORE: Block = Block::new(BlockId(45));
    /// `Blocks.END_STONE` (noisegen: `NoiseGeneratorSettings.end`).
    pub const END_STONE: Block = Block::new(BlockId(393));
    /// `Blocks.OAK_BUTTON` (noisegen: `OreVeinifier` debug ore-veins).
    pub const OAK_BUTTON: Block = Block::new(BlockId(443));
    /// `Blocks.SLIME_BLOCK` (noisegen: `debugPreliminarySurfaceLevel`).
    pub const SLIME_BLOCK: Block = Block::new(BlockId(523));
    /// `Blocks.TUFF` (noisegen: `OreVeinifier.VeinType.IRON` filler).
    pub const TUFF: Block = Block::new(BlockId(984));
    /// `Blocks.COPPER_ORE` (noisegen: `OreVeinifier.VeinType.COPPER`).
    pub const COPPER_ORE: Block = Block::new(BlockId(1042));
    /// `Blocks.RAW_IRON_BLOCK` (noisegen: `OreVeinifier.VeinType.IRON`).
    pub const RAW_IRON_BLOCK: Block = Block::new(BlockId(1173));
    /// `Blocks.RAW_COPPER_BLOCK` (noisegen: `OreVeinifier.VeinType.COPPER`).
    pub const RAW_COPPER_BLOCK: Block = Block::new(BlockId(1174));
    /// `Blocks.HONEY_BLOCK` (noisegen: `debugPreliminarySurfaceLevel`).
    pub const HONEY_BLOCK: Block = Block::new(BlockId(913));
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
            Blocks::ICE,
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
            Blocks::DEEPSLATE,
            Blocks::DEEPSLATE_IRON_ORE,
            Blocks::END_STONE,
            Blocks::OAK_BUTTON,
            Blocks::SLIME_BLOCK,
            Blocks::TUFF,
            Blocks::COPPER_ORE,
            Blocks::RAW_IRON_BLOCK,
            Blocks::RAW_COPPER_BLOCK,
            Blocks::HONEY_BLOCK,
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
