//! GrowingPlantBlock/KelpBlock and BambooStalkBlock survival from Paper 26.2.

use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction};
use rivet_registry::generated::blocks::BlockId;
use rivet_world::level::WorldGenLevel;

pub(super) fn aquatic_can_survive(
    level: &dyn WorldGenLevel,
    state: &BlockState,
    pos: &BlockPos,
) -> Option<bool> {
    let kelp = BlockId::from_name("minecraft:kelp").unwrap();
    let plant = BlockId::from_name("minecraft:kelp_plant").unwrap();
    if state.block() == kelp || state.block() == plant {
        let below_pos = pos.below();
        let below = level.get_block_state(&below_pos);
        return Some(
            !below.is_in_tag("minecraft:cannot_support_kelp")
                && (below.block() == kelp
                    || below.block() == plant
                    || level.is_face_sturdy(&below_pos, &below, &Direction::Up)),
        );
    }
    if state.block() == BlockId::from_name("minecraft:bamboo").unwrap()
        || state.block() == BlockId::from_name("minecraft:bamboo_sapling").unwrap()
    {
        return Some(
            level
                .get_block_state(&pos.below())
                .is_in_tag("minecraft:supports_bamboo"),
        );
    }
    if state.block() == BlockId::from_name("minecraft:sea_pickle").unwrap() {
        let below_pos = pos.below();
        let below = level.get_block_state(&below_pos);
        let shape = level
            .shape_query(&below_pos, &below)
            .unwrap_or_else(|error| panic!("SeaPickleBlock.canSurvive: {error}"));
        return Some(
            !shape.is_collision_face_empty(Direction::Up)
                || level.is_face_sturdy(&below_pos, &below, &Direction::Up),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_world::level::block_shape_context::{BlockShapeContext, DetachedShapeContext};
    use rivet_world::level::height_accessor::LevelHeightAccessor;

    struct Support {
        state: BlockState,
        shapes: DetachedShapeContext,
    }

    impl Support {
        fn new(state: BlockState) -> Self {
            Self {
                state,
                shapes: DetachedShapeContext::default(),
            }
        }
    }

    impl LevelHeightAccessor for Support {
        fn get_height(&self) -> i32 {
            384
        }
        fn get_min_y(&self) -> i32 {
            -64
        }
    }

    impl WorldGenLevel for Support {
        fn get_seed(&self) -> i64 {
            42
        }
        fn get_block_state(&self, _: &BlockPos) -> BlockState {
            self.state
        }
        fn shape_context(&self) -> Option<&dyn BlockShapeContext> {
            Some(&self.shapes)
        }
    }

    fn block(name: &str) -> BlockState {
        BlockState::of(BlockId::from_name(name).unwrap())
    }

    #[test]
    fn survival_matches_live_paper_probe() {
        let fixtures =
            include_str!("../../../../../tools/rivet-oracle/fixtures/plant-survival.tsv");
        for line in fixtures
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
        {
            let fields: Vec<_> = line.split('\t').collect();
            assert_eq!(fields.len(), 3);
            let plant = block(&format!("minecraft:{}", fields[0]));
            let support = block(&format!("minecraft:{}", fields[1]));
            assert_eq!(
                aquatic_can_survive(&Support::new(support), &plant, &BlockPos::new(0, 64, 0)),
                Some(fields[2].parse::<bool>().unwrap()),
                "Paper survival: {} on {}",
                fields[0],
                fields[1],
            );
        }
    }

    #[test]
    fn kelp_support_excludes_magma_but_accepts_stalks_and_sturdy_faces() {
        for name in ["minecraft:kelp", "minecraft:kelp_plant"] {
            for (support, survives) in [
                ("minecraft:stone", true),
                ("minecraft:kelp", true),
                ("minecraft:kelp_plant", true),
                ("minecraft:magma_block", false),
                ("minecraft:water", false),
                ("minecraft:air", false),
            ] {
                assert_eq!(
                    aquatic_can_survive(
                        &Support::new(block(support)),
                        &block(name),
                        &BlockPos::new(0, 64, 0)
                    ),
                    Some(survives),
                    "{name} on {support}",
                );
            }
        }
    }

    #[test]
    fn bamboo_requires_its_support_tag() {
        for name in ["minecraft:bamboo", "minecraft:bamboo_sapling"] {
            for (support, survives) in [
                ("minecraft:dirt", true),
                ("minecraft:bamboo", true),
                ("minecraft:sand", true),
                ("minecraft:stone", false),
                ("minecraft:water", false),
            ] {
                assert_eq!(
                    aquatic_can_survive(
                        &Support::new(block(support)),
                        &block(name),
                        &BlockPos::new(0, 64, 0)
                    ),
                    Some(survives),
                    "{name} on {support}",
                );
            }
        }
    }

    #[test]
    fn sea_pickle_accepts_partial_top_collision_faces() {
        for (support, survives) in [
            ("minecraft:stone", true),
            ("minecraft:glass_pane", true),
            ("minecraft:oak_fence", true),
            ("minecraft:oak_slab", false),
            ("minecraft:water", false),
            ("minecraft:air", false),
        ] {
            assert_eq!(
                aquatic_can_survive(
                    &Support::new(block(support)),
                    &block("minecraft:sea_pickle"),
                    &BlockPos::new(0, 64, 0),
                ),
                Some(survives),
                "sea pickle on {support}",
            );
        }
    }
}
