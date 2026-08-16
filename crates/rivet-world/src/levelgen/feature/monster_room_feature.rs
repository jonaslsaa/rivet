//! Port of `net.minecraft.world.level.levelgen.feature.MonsterRoomFeature`
//! (class, 26.2) — the `mc.world.level.levelgen.feature.monsterroom` manifest
//! unit (FeatureId 22; both `minecraft:monster_room` and
//! `minecraft:monster_room_deep` wrap the same configured feature).
//!
//! Java: `Feature<NoneFeatureConfiguration>` that carves a small dungeon
//! cell. `place` scans the `[minX..=maxX] × [-1..=4] × [minZ..=maxZ]`
//! box (radius 2..3 per axis, drawn from the placement random) for a `dy == -1`
//! floor and a `dy == 4` ceiling that are both solid, counting the boundary
//! cells at `dy == 0` whose own and `above()` cells are both empty; the room
//! is placed only when that count is 1..=5. The wall pass then fills the
//! boundary shell (ceiling/wall cells that float above a non-solid base get
//! cave air via a raw `setBlock` — no replaceable gate; the floor cells get
//! mossy/cobblestone via `safeSetBlock` with the `features_cannot_replace`
//! predicate, mossy 3/4 of the time), hollows the interior with the same
//! `safeSetBlock` predicate, and the chest pass tries up to 2×3 random spots
//! at the room's floor row for a chest with exactly one solid horizontal
//! neighbor — `StructurePiece.reorient` orients it, then
//! `RandomizableContainer.setBlockEntityLootTable` stamps the simple-dungeon
//! loot when the resulting block entity is a container. Finally a spawner is
//! placed at the origin and its spawn type is set when its block entity exists.
//!
//! ## RNG draw order (pinned from the Java)
//!
//! 1. `nextInt(2) + 2` → `xr`, then `nextInt(2) + 2` → `zr`.
//! 2. Wall pass, per floor cell that reaches the fill branch:
//!    `nextInt(4) != 0` (mossy-vs-cobble). No draw when the ceiling-gate
//!    writes air first.
//! 3. Chest attempts, up to `2 × 3`, EACH drawing `nextInt(xr*2+1)` then
//!    `nextInt(zr*2+1)` UNCONDITIONALLY — the pair is drawn even when the
//!    spot is not empty, so a filled room still consumes the draws.
//! 4. On a placed chest (empty spot, exactly one solid horizontal neighbor)
//!    whose block entity is a `RandomizableContainer`, `safeSetBlock` writes,
//!    then the loot-table attachment draws the seed —
//!    `RandomizableContainer.setBlockEntityLootTable` does
//!    `random.nextLong()` *inside* the helper. The port's trait is not generic
//!    over `R`, so the feature draws the `next_long()` itself at the same draw
//!    position and passes the seed to the seam. A missing container skips it.
//! 5. Spawner: `safeSetBlock(origin, SPAWNER)` writes no RNG; when the
//!    block-entity lookup succeeds, `randomEntityId(random)` =
//!    `MOBS[nextInt(4)]` (the `nextInt(4)` mob index — `Util.getRandom`).
//!    `SpawnerBlockEntity.setEntityId` draws NOTHING internally (the empty
//!    `SpawnPotentials` `WeightedList.getRandom` short-circuits to
//!    `Optional.empty()` → `SpawnData::new`). A missing spawner entity skips
//!    that mob draw, exactly as Java does.
//!
//! A placed chest draws its `nextLong()` immediately after its block write
//! only when the resulting block entity is a `RandomizableContainer`. The
//! spawner draw, when present, follows the chest-attempt loops. The seam docs
//! on `WorldGenLevel` spell out how the block-entity gates fold in.
//!
//! The interior-hollow `safeSetBlock` uses `Feature.isReplaceable(BlockTags.
//! FEATURES_CANNOT_REPLACE)` — `!state.is_in_tag("minecraft:
//! features_cannot_replace")`; the ceiling/wall `setBlock(wallBlock, AIR,
//! Block.UPDATE_CLIENTS)` is the raw `Feature.setBlock` form (no predicate),
//! exactly as Java.
//!
//! This is a leaf-level port: direct placement and dispatch-id tests exercise
//! the behavior. The generated-world FEATURES decoder now constructs and
//! dispatches the `minecraft:monster_room` entries before stopping at the first
//! unsupported placed-feature value (`minecraft:ore_dirt`); the production
//! FEATURES `WorldGenRegion` materializes the chest/spawner seams those
//! placements query.

use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::NoneFeatureConfiguration;
use crate::levelgen::feature::is_replaceable;
use rivet_registry::block_state::BlockState;
use rivet_registry::block_state_properties::BlockStateProperties;
use rivet_registry::block_state_property::PropertyValue;
use rivet_registry::core::{BlockPos, Direction};
use rivet_util::RandomSource;

/// `MonsterRoomFeature.AIR` — `Blocks.CAVE_AIR.defaultBlockState()`. Not a
/// `const` (the state table lookup is a runtime `fn`), so it is resolved at
/// the single call site in the wall pass.
const UPDATE_CLIENTS: u32 = 2;

/// `MonsterRoomFeature.MOBS` — `EntityTypes.SKELETON, EntityTypes.ZOMBIE,
/// EntityTypes.ZOMBIE, EntityTypes.SPIDER`.
const MOBS: [&str; 4] = [
    "minecraft:skeleton",
    "minecraft:zombie",
    "minecraft:zombie",
    "minecraft:spider",
];

/// `net.minecraft.world.level.levelgen.feature.MonsterRoomFeature`.
#[derive(Debug)]
pub struct MonsterRoomFeature;

/// `Feature.MONSTER_ROOM` — the registered `minecraft:monster_room` singleton
/// (FeatureId 22).
pub const MONSTER_ROOM: MonsterRoomFeature = MonsterRoomFeature;

/// `Feature.safeSetBlock(WorldGenLevel, BlockPos, BlockState, Predicate<
/// BlockState>)` — write `state` at `pos` (`Block.UPDATE_CLIENTS`) only when
/// `canReplace.test(level.getBlockState(pos))`. `BlockState` is `Copy`, so the
/// by-value predicate (`is_replaceable`'s form) passes the read state directly.
fn safe_set_block(
    level: &mut dyn WorldGenLevel,
    pos: &BlockPos,
    state: BlockState,
    can_replace: impl Fn(BlockState) -> bool,
) {
    if can_replace(level.get_block_state(pos)) {
        level.set_block(pos, state, UPDATE_CLIENTS);
    }
}

/// `MonsterRoomFeature.randomEntityId(RandomSource)` — `Util.getRandom(MOBS,
/// random)` = `MOBS[random.nextInt(MOBS.length)]`.
fn random_entity_id(random: &mut impl RandomSource) -> &'static str {
    MOBS[random.next_int_bound(MOBS.len() as i32) as usize]
}

/// `Direction.Plane.HORIZONTAL` — the horizontal facing values in iteration
/// order (`Direction.getClockWise` chain N→E→S→W).
const HORIZONTAL: [Direction; 4] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
];

/// The `Direction` a `HORIZONTAL_FACING` `PropertyValue` names — the read-back
/// half of `blockState.getValue(HorizontalDirectionalBlock.FACING)`. The chest
/// always carries the property (its default is `north`), so a missing value is
/// unreachable; Java's `getValue` would return the enum directly.
fn direction_from_facing(value: Option<PropertyValue>) -> Direction {
    match value {
        Some(PropertyValue::Enum("north")) => Direction::North,
        Some(PropertyValue::Enum("south")) => Direction::South,
        Some(PropertyValue::Enum("west")) => Direction::West,
        Some(PropertyValue::Enum("east")) => Direction::East,
        _ => panic!("chest state must carry a horizontal facing property"),
    }
}

/// `StructurePiece.reorient(BlockGetter, BlockPos, BlockState)` — orient a
/// chest so its front faces away from the enclosing wall. Walks the four
/// horizontal neighbors: a neighbor chest short-circuits to the unchanged
/// state; exactly one solid-render neighbor orients `FACING` to its opposite;
/// otherwise the default facing is stepped (opposite → clockwise → opposite)
/// until the next cell is not solid-render.
fn reorient(level: &dyn WorldGenLevel, pos: &BlockPos, state: BlockState) -> BlockState {
    let mut solid_neighbor: Option<Direction> = None;
    for direction in HORIZONTAL {
        let neighbor_state = level.get_block_state(&pos.relative(&direction));
        if neighbor_state.block() == Blocks::CHEST.id() {
            return state;
        }
        if neighbor_state.solid_render() {
            if solid_neighbor.is_some() {
                solid_neighbor = None;
                break;
            }
            solid_neighbor = Some(direction);
        }
    }

    if let Some(solid_neighbor) = solid_neighbor {
        return state
            .set_value(
                BlockStateProperties::HORIZONTAL_FACING,
                solid_neighbor.get_opposite(),
            )
            .expect("chest carries the horizontal facing property");
    }

    let mut lock_dir =
        direction_from_facing(state.get_value(BlockStateProperties::HORIZONTAL_FACING));
    let mut relative = pos.relative(&lock_dir);
    if level.get_block_state(&relative).solid_render() {
        lock_dir = lock_dir.get_opposite();
        relative = pos.relative(&lock_dir);
    }
    if level.get_block_state(&relative).solid_render() {
        lock_dir = lock_dir.get_clock_wise();
        relative = pos.relative(&lock_dir);
    }
    // The final opposite step's `relativePos` store in Java is dead (never
    // read again) — elided here, matching the pinned `StructurePiece.reorient`.
    if level.get_block_state(&relative).solid_render() {
        lock_dir = lock_dir.get_opposite();
    }

    state
        .set_value(BlockStateProperties::HORIZONTAL_FACING, lock_dir)
        .expect("chest carries the horizontal facing property")
}

impl FeatureBehavior<NoneFeatureConfiguration> for MonsterRoomFeature {
    /// `MonsterRoomFeature.place(FeaturePlaceContext<NoneFeatureConfiguration>)`.
    ///
    /// ```java
    /// Predicate<BlockState> replaceableTag = Feature.isReplaceable(BlockTags.FEATURES_CANNOT_REPLACE);
    /// BlockPos origin = context.origin();
    /// RandomSource random = context.random();
    /// WorldGenLevel level = context.level();
    /// int xr = random.nextInt(2) + 2;
    /// int minX = -xr - 1;
    /// int maxX = xr + 1;
    /// int minY = -1;
    /// int maxY = 4;
    /// int zr = random.nextInt(2) + 2;
    /// int minZ = -zr - 1;
    /// int maxZ = zr + 1;
    /// int holeCount = 0;
    ///
    /// for (int dx = minX; dx <= maxX; dx++) {
    ///     for (int dy = -1; dy <= 4; dy++) {
    ///         for (int dz = minZ; dz <= maxZ; dz++) {
    ///             BlockPos holePos = origin.offset(dx, dy, dz);
    ///             boolean solid = level.getBlockState(holePos).isSolid();
    ///             if (dy == -1 && !solid) return false;
    ///             if (dy == 4 && !solid) return false;
    ///             if ((dx == minX || dx == maxX || dz == minZ || dz == maxZ)
    ///                 && dy == 0
    ///                 && level.isEmptyBlock(holePos)
    ///                 && level.isEmptyBlock(holePos.above())) {
    ///                 holeCount++;
    ///             }
    ///         }
    ///     }
    /// }
    ///
    /// if (holeCount >= 1 && holeCount <= 5) {
    ///     // wall pass ...
    ///     // chest pass ...
    ///     // spawner ...
    ///     return true;
    /// }
    /// return false;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, NoneFeatureConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            random,
            origin,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let origin = **origin;

        let can_replace = is_replaceable("minecraft:features_cannot_replace");

        // `Feature.isReplaceable` — the `features_cannot_replace` tag predicate.
        let xr = random.next_int_bound(2) + 2;
        let min_x = -xr - 1;
        let max_x = xr + 1;
        let zr = random.next_int_bound(2) + 2;
        let min_z = -zr - 1;
        let max_z = zr + 1;
        let mut hole_count = 0;

        // Hole scan: the floor row `dy == -1` and ceiling row `dy == 4` must
        // both be fully solid, and the `dy == 0` boundary cells must be open
        // two-high for the room to connect.
        for dx in min_x..=max_x {
            for dy in -1..=4 {
                for dz in min_z..=max_z {
                    let hole_pos = origin.offset(dx, dy, dz);
                    let solid = level.get_block_state(&hole_pos).is_solid();
                    if dy == -1 && !solid {
                        return false;
                    }
                    if dy == 4 && !solid {
                        return false;
                    }
                    if (dx == min_x || dx == max_x || dz == min_z || dz == max_z)
                        && dy == 0
                        && level.is_empty_block(&hole_pos)
                        && level.is_empty_block(&hole_pos.above())
                    {
                        hole_count += 1;
                    }
                }
            }
        }

        if !(1..=5).contains(&hole_count) {
            return false;
        }

        // Wall pass (descending `dy` 3..=-1) — the boundary shell becomes
        // cobble/mossy, the interior is hollowed with cave air.
        for dx in min_x..=max_x {
            for dy in (-1..=3).rev() {
                for dz in min_z..=max_z {
                    let wall_block = origin.offset(dx, dy, dz);
                    let wall_state = level.get_block_state(&wall_block);
                    let boundary = dx == min_x
                        || dy == -1
                        || dz == min_z
                        || dx == max_x
                        || dy == 4
                        || dz == max_z;
                    if boundary {
                        // The ceiling gate — `wallBlock.getY() >=
                        // level.getMinY() && !getBlockState(wallBlock.below())
                        // .isSolid()` — writes cave air with the RAW
                        // `Feature.setBlock` (no replaceable predicate), and
                        // consumes NO RNG.
                        if wall_block.get_y() >= level.get_min_y()
                            && !level.get_block_state(&wall_block.below()).is_solid()
                        {
                            // `MonsterRoomFeature.AIR` — `Blocks.CAVE_AIR.
                            // defaultBlockState()`, resolved here (the state
                            // table lookup is a runtime `fn`, not a const).
                            level.set_block(
                                &wall_block,
                                Blocks::CAVE_AIR.default_block_state(),
                                UPDATE_CLIENTS,
                            );
                        } else if wall_state.is_solid() && wall_state.block() != Blocks::CHEST.id()
                        {
                            if dy == -1 && random.next_int_bound(4) != 0 {
                                safe_set_block(
                                    level,
                                    &wall_block,
                                    Blocks::MOSSY_COBBLESTONE.default_block_state(),
                                    &can_replace,
                                );
                            } else {
                                safe_set_block(
                                    level,
                                    &wall_block,
                                    Blocks::COBBLESTONE.default_block_state(),
                                    &can_replace,
                                );
                            }
                        }
                    } else if wall_state.block() != Blocks::CHEST.id()
                        && wall_state.block() != Blocks::SPAWNER.id()
                    {
                        safe_set_block(
                            level,
                            &wall_block,
                            Blocks::CAVE_AIR.default_block_state(),
                            &can_replace,
                        );
                    }
                }
            }
        }

        // Chest pass — up to 2 × 3 random attempts at the floor row. Each
        // attempt draws `nextInt(xr*2+1)` and `nextInt(zr*2+1)`
        // UNCONDITIONALLY, and a placed chest (empty spot, exactly one solid
        // horizontal neighbor) `break`s the inner attempt loop only — the
        // outer `cc` pass may place a second chest.
        for _cc in 0..2 {
            for _i in 0..3 {
                let chest_pos = origin.offset(
                    random.next_int_bound(xr * 2 + 1).wrapping_sub(xr),
                    0,
                    random.next_int_bound(zr * 2 + 1).wrapping_sub(zr),
                );
                if level.is_empty_block(&chest_pos) {
                    let wall_count = HORIZONTAL
                        .iter()
                        .filter(|direction| {
                            level
                                .get_block_state(&chest_pos.relative(direction))
                                .is_solid()
                        })
                        .count();
                    if wall_count == 1 {
                        let chest =
                            reorient(level, &chest_pos, Blocks::CHEST.default_block_state());
                        safe_set_block(level, &chest_pos, chest, &can_replace);
                        // `RandomizableContainer.setBlockEntityLootTable` —
                        // Java draws the loot seed only when the block entity
                        // is a RandomizableContainer. Query first so a rejected
                        // write cannot consume a `nextLong()` that Java skips.
                        if level.is_randomizable_container(&chest_pos) {
                            level.set_block_entity_loot_table(
                                &chest_pos,
                                random.next_long(),
                                "minecraft:chests/simple_dungeon",
                            );
                        }
                        break;
                    }
                }
            }
        }

        // Spawner — placed at the origin, then its spawn type is set only when
        // the block entity lookup succeeds. Java evaluates that lookup before
        // `randomEntityId`, so a blocked/non-spawner origin consumes no mob
        // selection draw. `setEntityId` first calls
        // `getOrCreateNextSpawnData`: an absent `SpawnData` with non-empty
        // potentials consumes one weighted-list draw and clears the potentials
        // after replacing the selected entry's id.
        safe_set_block(
            level,
            &origin,
            Blocks::SPAWNER.default_block_state(),
            &can_replace,
        );
        if level.is_spawner_block_entity(&origin) {
            // Java evaluates the randomEntityId(random) argument before
            // BaseSpawner.setEntityId enters getOrCreateNextSpawnData and
            // selects an existing SpawnPotentials entry.
            let entity_id = random_entity_id(random);
            let potential_roll = level
                .spawner_potential_weight(&origin)
                .map(|total| random.next_int_bound(total));
            level.set_spawner_entity(&origin, entity_id, potential_roll);
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::blocks::Blocks;
    use crate::levelgen::feature::test_support::{
        RngCall, TestBlockEntity, TestGenerator, TestLevel, access,
    };
    use crate::levelgen::feature::{FeatureId, feature_place};
    use rivet_registry::block_state::BlockState;
    use rivet_registry::block_state_property::PropertyValue;
    use rivet_registry::core::BlockPos;
    use rivet_registry::core::Direction;
    use rivet_util::random::LegacyPositionalRandomFactory;

    struct ScriptedRandom {
        bounds: Vec<i32>,
        next_bound: usize,
        loot_seed: i64,
        calls: Vec<RngCall>,
    }

    impl ScriptedRandom {
        fn room(loot_seed: i64) -> Self {
            let mut bounds = vec![0, 0];
            bounds.push(0);
            bounds.extend([1; 48]);
            bounds.extend([0, 2, 2, 2, 2, 2, 2, 2]);
            bounds.push(0);
            Self {
                bounds,
                next_bound: 0,
                loot_seed,
                calls: Vec::new(),
            }
        }

        fn room_with_spawner_potentials(loot_seed: i64) -> Self {
            let mut random = Self::room(loot_seed);
            let mob_roll = random
                .bounds
                .pop()
                .expect("room fixture carries a mob roll");
            random.bounds.push(mob_roll);
            random.bounds.push(0);
            random
        }
    }

    impl RandomSource for ScriptedRandom {
        type Positional = LegacyPositionalRandomFactory;

        fn fork(&mut self) -> Self {
            Self {
                bounds: self.bounds.clone(),
                next_bound: self.next_bound,
                loot_seed: self.loot_seed,
                calls: self.calls.clone(),
            }
        }

        fn fork_positional(&mut self) -> Self::Positional {
            LegacyPositionalRandomFactory::new(0)
        }

        fn set_seed(&mut self, _seed: i64) {}

        fn next_int(&mut self) -> i32 {
            self.calls.push(RngCall::Int);
            0
        }

        fn next_int_bound(&mut self, bound: i32) -> i32 {
            let value = self.bounds[self.next_bound];
            self.next_bound += 1;
            assert!((0..bound).contains(&value));
            self.calls.push(RngCall::IntBound(bound));
            value
        }

        fn next_long(&mut self) -> i64 {
            self.calls.push(RngCall::Long);
            self.loot_seed
        }

        fn next_boolean(&mut self) -> bool {
            self.calls.push(RngCall::Boolean);
            false
        }

        fn next_float(&mut self) -> f32 {
            self.calls.push(RngCall::Float);
            0.0
        }

        fn next_double(&mut self) -> f64 {
            self.calls.push(RngCall::Double);
            0.0
        }

        fn next_gaussian(&mut self) -> f64 {
            0.0
        }
    }

    fn stone() -> BlockState {
        Blocks::STONE.default_block_state()
    }

    fn prepare_room(level: &mut TestLevel, origin: BlockPos) {
        let stone = stone();
        for x in -3..=3 {
            for z in -3..=3 {
                level.states.insert(origin.offset(x, -1, z), stone);
                level.states.insert(origin.offset(x, 4, z), stone);
                level.states.insert(origin.offset(x, -2, z), stone);
            }
        }
        for y in 0..=3 {
            for x in -3..=3 {
                for z in -3..=3 {
                    if x == -3 || x == 3 || z == -3 || z == 3 {
                        level.states.insert(origin.offset(x, y, z), stone);
                    }
                }
            }
        }
        level.states.remove(&origin.offset(-3, 0, 2));
        level.states.remove(&origin.offset(-3, 1, 2));
    }

    fn place<R: RandomSource>(level: &mut TestLevel, origin: BlockPos, random: &mut R) -> bool {
        MONSTER_ROOM.place(&mut FeaturePlaceContext::new(
            None,
            level,
            &TestGenerator,
            random,
            &origin,
            &NoneFeatureConfiguration,
        ))
    }

    #[test]
    fn places_deterministic_room_with_chest_and_spawner() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(10, 20, -4);
        prepare_room(&mut level, origin);
        let loot_seed = 0x1122_3344_5566_7788;
        let mut random = ScriptedRandom::room(loot_seed);

        assert!(place(&mut level, origin, &mut random));
        assert!(!level.chest_loot.is_empty(), "writes: {:?}", level.writes);
        assert_eq!(level.states[&origin].block(), Blocks::SPAWNER.id());
        assert_eq!(
            level.states[&origin.offset(-2, 0, 0)].block(),
            Blocks::CHEST.id()
        );
        assert_eq!(
            level.states[&origin.offset(-2, 0, 0)]
                .get_value(BlockStateProperties::HORIZONTAL_FACING),
            Some(PropertyValue::Enum("east"))
        );
        assert_eq!(
            level.get_block_state(&origin.offset(-3, 0, 2)).block(),
            Blocks::AIR.id()
        );
        assert_eq!(
            level.chest_loot,
            vec![(
                origin.offset(-2, 0, 0),
                loot_seed,
                "minecraft:chests/simple_dungeon".to_string()
            )]
        );
        assert_eq!(
            level.spawner_entities,
            vec![(origin, "minecraft:skeleton".to_string())]
        );
        let floor_mossy = level
            .writes
            .iter()
            .filter(|(pos, state)| {
                pos.get_y() == origin.get_y() - 1 && state.block() == Blocks::MOSSY_COBBLESTONE.id()
            })
            .count();
        let floor_cobble = level
            .writes
            .iter()
            .filter(|(pos, state)| {
                pos.get_y() == origin.get_y() - 1 && state.block() == Blocks::COBBLESTONE.id()
            })
            .count();
        assert_eq!(floor_mossy, 48);
        assert_eq!(floor_cobble, 1);
    }

    #[test]
    fn existing_spawner_potentials_consume_weighted_draw_and_clear_state() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(10, 20, -4);
        prepare_room(&mut level, origin);
        level.set_spawner_state(origin, None, vec![("minecraft:zombie".to_string(), 1)]);
        let mut random = ScriptedRandom::room_with_spawner_potentials(7);

        assert!(place(&mut level, origin, &mut random));
        assert_eq!(random.calls[60], RngCall::IntBound(4));
        assert_eq!(random.calls[61], RngCall::IntBound(1));
        assert_eq!(
            level.block_entities.get(&origin),
            Some(&TestBlockEntity::Spawner {
                next_spawn: Some("minecraft:skeleton".to_string()),
                spawn_potentials: Vec::new(),
            })
        );
    }

    #[test]
    fn existing_spawner_data_skips_potential_rng_draw() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(10, 20, -4);
        prepare_room(&mut level, origin);
        level.set_spawner_state(
            origin,
            Some("minecraft:spider".to_string()),
            vec![("minecraft:zombie".to_string(), 1)],
        );
        let mut random = ScriptedRandom::room(7);

        assert!(place(&mut level, origin, &mut random));
        assert_eq!(random.next_bound, 60);
        assert_eq!(random.calls[60], RngCall::IntBound(4));
        assert_eq!(
            level.block_entities.get(&origin),
            Some(&TestBlockEntity::Spawner {
                next_spawn: Some("minecraft:skeleton".to_string()),
                spawn_potentials: Vec::new(),
            })
        );
    }

    #[test]
    fn rejects_hostile_room_without_writes_or_block_entities() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 20, 0);
        let mut random = ScriptedRandom::room(7);

        assert!(!place(&mut level, origin, &mut random));
        assert!(level.writes.is_empty());
        assert!(level.chest_loot.is_empty());
        assert!(level.spawner_entities.is_empty());
        assert_eq!(
            random.calls,
            vec![RngCall::IntBound(2), RngCall::IntBound(2)]
        );
    }

    #[test]
    fn rejects_room_with_more_than_five_openings_without_writes() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 20, 0);
        prepare_room(&mut level, origin);
        for z in -3..=1 {
            level.states.remove(&origin.offset(-3, 0, z));
            level.states.remove(&origin.offset(-3, 1, z));
        }
        let mut random = ScriptedRandom::room(7);

        assert!(!place(&mut level, origin, &mut random));
        assert!(level.writes.is_empty());
        assert!(level.chest_loot.is_empty());
        assert!(level.spawner_entities.is_empty());
        assert_eq!(random.calls, [RngCall::IntBound(2), RngCall::IntBound(2)]);
    }

    #[test]
    fn blocked_origin_skips_spawner_entity_and_mob_rng() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 20, 0);
        prepare_room(&mut level, origin);
        level
            .states
            .insert(origin, Blocks::CHEST.default_block_state());
        let mut random = ScriptedRandom::room(7);

        assert!(place(&mut level, origin, &mut random));
        assert_eq!(level.get_block_state(&origin).block(), Blocks::CHEST.id());
        assert!(level.spawner_entities.is_empty());
        assert_eq!(random.next_bound, 59);
        assert!(!matches!(random.calls.last(), Some(RngCall::IntBound(4))));
    }

    #[test]
    fn ensure_can_write_rejects_before_any_feature_rng_or_world_access() {
        let mut level = TestLevel::over(access());
        level.can_write = false;
        let origin = BlockPos::new(0, 20, 0);
        let mut random = ScriptedRandom::room(7);

        assert!(!feature_place(
            FeatureId::new(22),
            &NoneFeatureConfiguration,
            &mut level,
            &TestGenerator,
            &mut random,
            &origin,
        ));
        assert!(random.calls.is_empty());
        assert!(level.writes.is_empty());
        assert!(level.chest_loot.is_empty());
        assert!(level.spawner_entities.is_empty());
    }

    #[test]
    fn preserves_paper_rng_order_through_chest_and_spawner() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 20, 0);
        prepare_room(&mut level, origin);
        let mut random = ScriptedRandom::room(7);

        assert!(place(&mut level, origin, &mut random));
        assert_eq!(random.next_bound, 60);
        assert_eq!(
            random.calls[0..2],
            [RngCall::IntBound(2), RngCall::IntBound(2)]
        );
        assert_eq!(random.calls[2..51], [RngCall::IntBound(4); 49]);
        assert_eq!(random.calls[51..53], [RngCall::IntBound(5); 2]);
        assert_eq!(random.calls[53], RngCall::Long);
        assert_eq!(random.calls[54..60], [RngCall::IntBound(5); 6]);
        assert_eq!(random.calls[60..], [RngCall::IntBound(4)]);
    }

    #[test]
    fn dispatch_arm_22_places_the_none_configured_feature() {
        let mut level = TestLevel::over(access());
        let origin = BlockPos::new(0, 20, 0);
        prepare_room(&mut level, origin);
        let mut random = ScriptedRandom::room(9);

        assert!(feature_place(
            FeatureId::new(22),
            &NoneFeatureConfiguration,
            &mut level,
            &TestGenerator,
            &mut random,
            &origin,
        ));
        assert_eq!(level.spawner_entities.len(), 1);
    }

    #[test]
    fn reorient_uses_the_single_solid_neighbor() {
        let mut level = TestLevel::over(access());
        let pos = BlockPos::new(0, 0, 0);
        level.states.insert(pos.relative(&Direction::West), stone());
        let state = reorient(&level, &pos, Blocks::CHEST.default_block_state());
        assert_eq!(
            state.get_value(BlockStateProperties::HORIZONTAL_FACING),
            Some(PropertyValue::Enum("east"))
        );
    }
}
