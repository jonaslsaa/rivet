//! Port of `net.minecraft.world.level.levelgen.feature.IcebergFeature`
//! (class, 26.2) — owned by the `mc.world.level.levelgen.feature.iceberg`
//! manifest unit.
//!
//! Java: `Feature<BlockStateConfiguration>` that re-anchors the origin to the
//! chunk generator's sea level, then shapes an iceberg out of `config.state`
//! (with a `SNOW_BLOCK` topper when `snowOnTop`) through three passes: the
//! above-water dome (a horizontal ellipse or circle radius that shrinks with
//! height), a smoothing pass that clips dangling iceberg/snow cells against air
//! and removes cells with three-plus non-iceberg neighbours, and the underwater
//! keel (a steeper radius). A cut-out pass then carves an off-center hole
//! through the iceberg (below water the hole fills with `WATER`), removing any
//! floating snow layer it exposes.
//!
//! The RNG draw order is load-bearing and preserved exactly: the initial
//! `snowOnTop`/`shapeAngle`/`shapeEllipseA`/`shapeEllipseC`/`isEllipse`/height/
//! width draws, then per-cell radius + `signedDistanceCircle`/`compareVal`/
//! `nextDouble > 0.9`/`setIcebergBlock` draws in loop order, the cut-out
//! `nextBoolean`/`nextInt` draws, and the carve pass draws. Writes go through
//! `Feature.setBlock` (`Block.UPDATE_ALL`, 3).

use crate::block::Block;
use crate::block::blocks::Blocks;
use crate::level::WorldGenLevel;
use crate::levelgen::feature::FeatureBehavior;
use crate::levelgen::feature::FeaturePlaceContext;
use crate::levelgen::feature::configurations::BlockStateConfiguration;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::BlockPos;
use rivet_util::{RandomSource, mth};
use std::f64::consts::PI;

/// `Feature.setBlock` — `level.setBlock(pos, state, Block.UPDATE_ALL)`.
const UPDATE_ALL: u32 = 3;

/// `BlockStateBase.is(Blocks.X)` — the block identity check the feature gates
/// its writes on.
#[inline]
fn is_block(state: BlockState, block: Block) -> bool {
    state.block() == block.id()
}

/// `isIcebergState(BlockState)` — `is(PACKED_ICE) || is(SNOW_BLOCK) ||
/// is(BLUE_ICE)`.
fn is_iceberg_state(state: BlockState) -> bool {
    is_block(state, Blocks::PACKED_ICE)
        || is_block(state, Blocks::SNOW_BLOCK)
        || is_block(state, Blocks::BLUE_ICE)
}

/// `Feature.setBlock` — `level.setBlock(pos, state, Block.UPDATE_ALL)`.
fn set_block(level: &mut dyn WorldGenLevel, pos: &BlockPos, state: BlockState) {
    level.set_block(pos, state, UPDATE_ALL);
}

/// `signedDistanceCircle(int, int, BlockPos, int, RandomSource)`.
fn signed_distance_circle<R: RandomSource>(
    xo: i32,
    zo: i32,
    origin: &BlockPos,
    radius: i32,
    random: &mut R,
) -> f64 {
    let off = 10.0f32 * mth::clamp_f32(random.next_float(), 0.2, 0.8) / radius as f32;
    (off as f64)
        + (xo.wrapping_sub(origin.get_x()) as f64).powi(2)
        + (zo.wrapping_sub(origin.get_z()) as f64).powi(2)
        - (radius as f64).powi(2)
}

/// `signedDistanceEllipse(int, int, BlockPos, int, int, double)`.
fn signed_distance_ellipse(xo: i32, zo: i32, origin: &BlockPos, a: i32, c: i32, angle: f64) -> f64 {
    let dx = xo.wrapping_sub(origin.get_x()) as f64;
    let dz = zo.wrapping_sub(origin.get_z()) as f64;
    ((dx * angle.cos() - dz * angle.sin()) / a as f64).powi(2)
        + ((dx * angle.sin() + dz * angle.cos()) / c as f64).powi(2)
        - 1.0
}

/// `heightDependentRadiusRound(RandomSource, int, int, int)`.
fn height_dependent_radius_round<R: RandomSource>(
    random: &mut R,
    y_off: i32,
    height: i32,
    width: i32,
) -> i32 {
    let k = 3.5f32 - random.next_float();
    let mut scale = (1.0f32 - (y_off as f64).powi(2) as f32 / (height as f32 * k)) * width as f32;
    if height > 15 + random.next_int_bound(5) {
        let temp_y_off = if y_off < 3 + random.next_int_bound(6) {
            y_off / 2
        } else {
            y_off
        };
        scale = (1.0f32 - temp_y_off as f32 / (height as f32 * k * 0.4f32)) * width as f32;
    }
    mth::ceil(scale / 2.0f32)
}

/// `heightDependentRadiusEllipse(int, int, int)`.
fn height_dependent_radius_ellipse(y_off: i32, height: i32, width: i32) -> i32 {
    let k = 1.0f32;
    let scale = (1.0f32 - (y_off as f64).powi(2) as f32 / (height as f32 * k)) * width as f32;
    mth::ceil(scale / 2.0f32)
}

/// `heightDependentRadiusSteep(RandomSource, int, int, int)`.
fn height_dependent_radius_steep<R: RandomSource>(
    random: &mut R,
    y_off: i32,
    height: i32,
    width: i32,
) -> i32 {
    let k = 1.0f32 + random.next_float() / 2.0f32;
    let scale = (1.0f32 - y_off as f32 / (height as f32 * k)) * width as f32;
    mth::ceil(scale / 2.0f32)
}

/// `getEllipseC(int, int, int)`.
fn get_ellipse_c(y_off: i32, height: i32, shape_ellipse_c: i32) -> i32 {
    let mut c = shape_ellipse_c;
    if y_off > 0 && height.wrapping_sub(y_off) <= 3 {
        c = c.wrapping_sub(4 - height.wrapping_sub(y_off));
    }
    c
}

/// `removeFloatingSnowLayer(LevelAccessor, BlockPos)`.
fn remove_floating_snow_layer(level: &mut dyn WorldGenLevel, pos: &BlockPos) {
    if is_block(level.get_block_state(&pos.above()), Blocks::SNOW) {
        set_block(level, &pos.above(), Blocks::AIR.default_block_state());
    }
}

/// `belowIsAir(BlockGetter, BlockPos)`.
fn below_is_air(level: &dyn WorldGenLevel, pos: &BlockPos) -> bool {
    level.get_block_state(&pos.below()).is_air()
}

/// `setIcebergBlock(BlockPos, LevelAccessor, RandomSource, int, int, boolean,
/// boolean, BlockState)`.
fn set_iceberg_block<R: RandomSource>(
    pos: &BlockPos,
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    h_diff: i32,
    height: i32,
    is_ellipse: bool,
    snow_on_top: bool,
    main_block_state: BlockState,
) {
    let state = level.get_block_state(pos);
    if state.is_air()
        || is_block(state, Blocks::SNOW_BLOCK)
        || is_block(state, Blocks::ICE)
        || is_block(state, Blocks::WATER)
    {
        let randomness = !is_ellipse || random.next_double() > 0.05;
        let divisor = if is_ellipse { 3 } else { 2 };
        if snow_on_top
            && !is_block(state, Blocks::WATER)
            && (h_diff as f64)
                <= random.next_int_bound(1.max(height / divisor)) as f64 + height as f64 * 0.6
            && randomness
        {
            set_block(level, pos, Blocks::SNOW_BLOCK.default_block_state());
        } else {
            set_block(level, pos, main_block_state);
        }
    }
}

/// `generateIcebergBlock(LevelAccessor, RandomSource, BlockPos, int, int, int,
/// int, int, int, boolean, int, double, boolean, BlockState)`.
fn generate_iceberg_block<R: RandomSource>(
    level: &mut dyn WorldGenLevel,
    random: &mut R,
    origin: &BlockPos,
    height: i32,
    xo: i32,
    y_off: i32,
    zo: i32,
    radius: i32,
    a: i32,
    is_ellipse: bool,
    shape_ellipse_c: i32,
    shape_angle: f64,
    snow_on_top: bool,
    main_block_state: BlockState,
) {
    let signed_dist = if is_ellipse {
        signed_distance_ellipse(
            xo,
            zo,
            &BlockPos::ZERO,
            a,
            get_ellipse_c(y_off, height, shape_ellipse_c),
            shape_angle,
        )
    } else {
        signed_distance_circle(xo, zo, &BlockPos::ZERO, radius, random)
    };
    if signed_dist < 0.0 {
        let pos = origin.offset(xo, y_off, zo);
        let compare_val = if is_ellipse {
            -0.5
        } else {
            (-6 - random.next_int_bound(3)) as f64
        };
        if signed_dist > compare_val && random.next_double() > 0.9 {
            return;
        }

        set_iceberg_block(
            &pos,
            level,
            random,
            height.wrapping_sub(y_off),
            height,
            is_ellipse,
            snow_on_top,
            main_block_state,
        );
    }
}

/// `carve(int, int, BlockPos, LevelAccessor, boolean, double, BlockPos, int,
/// int)`.
fn carve(
    level: &mut dyn WorldGenLevel,
    radius: i32,
    y_off: i32,
    global_origin: &BlockPos,
    under_water: bool,
    angle: f64,
    local_origin: &BlockPos,
    shape_ellipse_a: i32,
    shape_ellipse_c: i32,
) {
    let a = radius.wrapping_add(1).wrapping_add(shape_ellipse_a / 3);
    let c = radius
        .wrapping_sub(3)
        .min(3)
        .wrapping_add(shape_ellipse_c / 2)
        .wrapping_sub(1);

    for xo in -a..a {
        for zo in -a..a {
            let signed_dist = signed_distance_ellipse(xo, zo, local_origin, a, c, angle);
            if signed_dist < 0.0 {
                let pos = global_origin.offset(xo, y_off, zo);
                let state = level.get_block_state(&pos);
                if is_iceberg_state(state) || is_block(state, Blocks::SNOW_BLOCK) {
                    if under_water {
                        set_block(level, &pos, Blocks::WATER.default_block_state());
                    } else {
                        set_block(level, &pos, Blocks::AIR.default_block_state());
                        remove_floating_snow_layer(level, &pos);
                    }
                }
            }
        }
    }
}

/// `net.minecraft.world.level.levelgen.feature.IcebergFeature`.
#[derive(Debug)]
pub struct IcebergFeature;

/// `Feature.ICEBERG` — the registered `minecraft:iceberg` singleton.
pub const ICEBERG: IcebergFeature = IcebergFeature;

impl IcebergFeature {
    /// `smooth(LevelAccessor, BlockPos, int, int, boolean, int)`.
    fn smooth(
        &self,
        level: &mut dyn WorldGenLevel,
        origin: &BlockPos,
        width: i32,
        height: i32,
        is_ellipse: bool,
        shape_ellipse_a: i32,
    ) {
        let a = if is_ellipse {
            shape_ellipse_a
        } else {
            width / 2
        };

        for x in -a..=a {
            for z in -a..=a {
                for y_off in 0..=height {
                    let pos = origin.offset(x, y_off, z);
                    let state = level.get_block_state(&pos);
                    if is_iceberg_state(state) || is_block(state, Blocks::SNOW) {
                        if below_is_air(level, &pos) {
                            set_block(level, &pos, Blocks::AIR.default_block_state());
                            set_block(level, &pos.above(), Blocks::AIR.default_block_state());
                        } else if is_iceberg_state(state) {
                            let sides = [
                                level.get_block_state(&pos.west()),
                                level.get_block_state(&pos.east()),
                                level.get_block_state(&pos.north()),
                                level.get_block_state(&pos.south()),
                            ];
                            let mut counter = 0;
                            for side in sides {
                                if !is_iceberg_state(side) {
                                    counter += 1;
                                }
                            }
                            if counter >= 3 {
                                set_block(level, &pos, Blocks::AIR.default_block_state());
                            }
                        }
                    }
                }
            }
        }
    }

    /// `generateCutOut(RandomSource, LevelAccessor, int, int, BlockPos,
    /// boolean, int, double, int)`.
    fn generate_cut_out<R: RandomSource>(
        &self,
        random: &mut R,
        level: &mut dyn WorldGenLevel,
        width: i32,
        height: i32,
        global_origin: &BlockPos,
        is_ellipse: bool,
        shape_ellipse_a: i32,
        shape_angle: f64,
        shape_ellipse_c: i32,
    ) {
        let random_sign_x: i32 = if random.next_boolean() { -1 } else { 1 };
        let random_sign_z: i32 = if random.next_boolean() { -1 } else { 1 };
        let mut x_off = random.next_int_bound((width / 2 - 2).max(1));
        if random.next_boolean() {
            x_off = width / 2 + 1 - random.next_int_bound((width - width / 2 - 1).max(1));
        }

        let mut z_off = random.next_int_bound((width / 2 - 2).max(1));
        if random.next_boolean() {
            z_off = width / 2 + 1 - random.next_int_bound((width - width / 2 - 1).max(1));
        }

        if is_ellipse {
            let both = random.next_int_bound((shape_ellipse_a - 5).max(1));
            x_off = both;
            z_off = both;
        }

        let local_origin = BlockPos::new(
            random_sign_x.wrapping_mul(x_off),
            0,
            random_sign_z.wrapping_mul(z_off),
        );
        let angle = if is_ellipse {
            shape_angle + PI / 2.0
        } else {
            random.next_double() * 2.0 * PI
        };

        for y_off in 0..height.wrapping_sub(3) {
            let radius = height_dependent_radius_round(random, y_off, height, width);
            carve(
                level,
                radius,
                y_off,
                global_origin,
                false,
                angle,
                &local_origin,
                shape_ellipse_a,
                shape_ellipse_c,
            );
        }

        // `-height + random.nextInt(5)` is drawn once, at the loop's first
        // condition evaluation; the loop then runs `yOff` down to that bound.
        let bound = height.wrapping_neg().wrapping_add(random.next_int_bound(5));
        for y_off in (bound.wrapping_add(1)..=-1).rev() {
            let radius = height_dependent_radius_steep(random, -y_off, height, width);
            carve(
                level,
                radius,
                y_off,
                global_origin,
                true,
                angle,
                &local_origin,
                shape_ellipse_a,
                shape_ellipse_c,
            );
        }
    }
}

impl FeatureBehavior<BlockStateConfiguration> for IcebergFeature {
    /// `IcebergFeature.place(FeaturePlaceContext<BlockStateConfiguration>)`.
    ///
    /// ```java
    /// BlockPos origin = context.origin();
    /// WorldGenLevel level = context.level();
    /// origin = new BlockPos(origin.getX(), context.chunkGenerator().getSeaLevel(), origin.getZ());
    /// RandomSource random = context.random();
    /// boolean snowOnTop = random.nextDouble() > 0.7;
    /// BlockState mainBlockState = context.config().state;
    /// double shapeAngle = random.nextDouble() * 2.0 * Math.PI;
    /// int shapeEllipseA = 11 - random.nextInt(5);
    /// int shapeEllipseC = 3 + random.nextInt(3);
    /// boolean isEllipse = random.nextDouble() > 0.7;
    /// int overWaterHeight = isEllipse ? random.nextInt(6) + 6 : random.nextInt(15) + 3;
    /// if (!isEllipse && random.nextDouble() > 0.9) {
    ///     overWaterHeight += random.nextInt(19) + 7;
    /// }
    /// int underWaterHeight = Math.min(overWaterHeight + random.nextInt(11), 18);
    /// int width = Math.min(overWaterHeight + random.nextInt(7) - random.nextInt(5), 11);
    /// int a = isEllipse ? shapeEllipseA : 11;
    /// // above-water dome, then smooth, then the underwater keel, then the cut-out.
    /// return true;
    /// ```
    fn place<R: RandomSource>(
        &self,
        context: &mut FeaturePlaceContext<'_, BlockStateConfiguration, R>,
    ) -> bool {
        let FeaturePlaceContext {
            level,
            chunk_generator,
            random,
            origin,
            config,
            ..
        } = context;
        let level: &mut dyn WorldGenLevel = &mut **level;
        let random: &mut R = random;
        let config = *config;
        let origin = BlockPos::new(
            origin.get_x(),
            chunk_generator.get_sea_level(),
            origin.get_z(),
        );
        let snow_on_top = random.next_double() > 0.7;
        let main_block_state = config.state;
        let shape_angle = random.next_double() * 2.0 * PI;
        let shape_ellipse_a = 11 - random.next_int_bound(5);
        let shape_ellipse_c = 3 + random.next_int_bound(3);
        let is_ellipse = random.next_double() > 0.7;
        let mut over_water_height = if is_ellipse {
            random.next_int_bound(6) + 6
        } else {
            random.next_int_bound(15) + 3
        };
        if !is_ellipse && random.next_double() > 0.9 {
            over_water_height = over_water_height.wrapping_add(random.next_int_bound(19) + 7);
        }

        let under_water_height = over_water_height
            .wrapping_add(random.next_int_bound(11))
            .min(18);
        let width = over_water_height
            .wrapping_add(random.next_int_bound(7))
            .wrapping_sub(random.next_int_bound(5))
            .min(11);
        let a = if is_ellipse { shape_ellipse_a } else { 11 };

        for xo in -a..a {
            for zo in -a..a {
                for y_off in 0..over_water_height {
                    let radius = if is_ellipse {
                        height_dependent_radius_ellipse(y_off, over_water_height, width)
                    } else {
                        height_dependent_radius_round(random, y_off, over_water_height, width)
                    };
                    if is_ellipse || xo < radius {
                        generate_iceberg_block(
                            level,
                            random,
                            &origin,
                            over_water_height,
                            xo,
                            y_off,
                            zo,
                            radius,
                            a,
                            is_ellipse,
                            shape_ellipse_c,
                            shape_angle,
                            snow_on_top,
                            main_block_state,
                        );
                    }
                }
            }
        }

        self.smooth(
            level,
            &origin,
            width,
            over_water_height,
            is_ellipse,
            shape_ellipse_a,
        );

        for xo in -a..a {
            for zo in -a..a {
                for y_off in (-under_water_height + 1..0).rev() {
                    let new_a = if is_ellipse {
                        mth::ceil(
                            (a as f32)
                                * (1.0f32
                                    - (y_off as f64).powi(2) as f32
                                        / (under_water_height as f32 * 8.0f32)),
                        )
                    } else {
                        a
                    };
                    let radius =
                        height_dependent_radius_steep(random, -y_off, under_water_height, width);
                    if xo < radius {
                        generate_iceberg_block(
                            level,
                            random,
                            &origin,
                            under_water_height,
                            xo,
                            y_off,
                            zo,
                            radius,
                            new_a,
                            is_ellipse,
                            shape_ellipse_c,
                            shape_angle,
                            snow_on_top,
                            main_block_state,
                        );
                    }
                }
            }
        }

        let do_cut_out = if is_ellipse {
            random.next_double() > 0.1
        } else {
            random.next_double() > 0.7
        };
        if do_cut_out {
            self.generate_cut_out(
                random,
                level,
                width,
                over_water_height,
                &origin,
                is_ellipse,
                shape_ellipse_a,
                shape_angle,
                shape_ellipse_c,
            );
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levelgen::feature::test_support::{
        RecordingRandom, RngCall, SeaLevelGenerator, TestLevel, access,
    };
    use rivet_registry::core::BlockPos;
    use rivet_registry::generated::blocks::BlockId;

    fn packed_ice() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:packed_ice").unwrap())
    }

    fn place(
        level: &mut TestLevel,
        generator: &SeaLevelGenerator,
        random: &mut RecordingRandom,
    ) -> bool {
        let config = BlockStateConfiguration::new(packed_ice());
        let origin = BlockPos::new(0, 0, 0);
        ICEBERG.place(&mut FeaturePlaceContext::new(
            None, level, generator, random, &origin, &config,
        ))
    }

    /// On an all-air level the feature re-anchors the origin to the sea level
    /// and builds the iceberg out of the config state: `place` returns `true`,
    /// writes happen both at and below the sea level, and the iceberg material
    /// survives (the underwater keel writes `packed_ice` after `smooth`).
    #[test]
    fn returns_true_and_writes_iceberg_material_at_sea_level() {
        let mut level = TestLevel::over(access());
        let generator = SeaLevelGenerator { sea_level: 63 };
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &generator, &mut random));
        assert!(!level.writes.is_empty());
        // At least one underwater (y < 63) write anchors the sea-level rebase.
        assert!(level.writes.iter().any(|(pos, _)| pos.get_y() < 63));
        // The iceberg material (packed_ice or the snow topper) is actually placed.
        assert!(level.writes.iter().any(|(_, state)| {
            is_block(*state, Blocks::PACKED_ICE) || is_block(*state, Blocks::SNOW_BLOCK)
        }));
        // Every write is one of the states this feature produces.
        for (_, state) in &level.writes {
            assert!(
                is_block(*state, Blocks::PACKED_ICE)
                    || is_block(*state, Blocks::SNOW_BLOCK)
                    || is_block(*state, Blocks::AIR)
                    || is_block(*state, Blocks::WATER),
                "unexpected write block: {:?}",
                state.block()
            );
        }
    }

    /// The initial draw sequence is unconditional (before the `isEllipse`
    /// branch): `snowOnTop` `nextDouble`, `shapeAngle` `nextDouble`,
    /// `shapeEllipseA` `nextInt(5)`, `shapeEllipseC` `nextInt(3)`, `isEllipse`
    /// `nextDouble`. The next draw is the over-water height bound of the
    /// branch.
    #[test]
    fn pins_the_unconditional_draw_prefix() {
        let mut level = TestLevel::over(access());
        let generator = SeaLevelGenerator { sea_level: 63 };
        let mut random = RecordingRandom::new(1);
        assert!(place(&mut level, &generator, &mut random));
        assert_eq!(random.calls[0], RngCall::Double);
        assert_eq!(random.calls[1], RngCall::Double);
        assert_eq!(random.calls[2], RngCall::IntBound(5));
        assert_eq!(random.calls[3], RngCall::IntBound(3));
        assert_eq!(random.calls[4], RngCall::Double);
        assert!(
            random.calls[5] == RngCall::IntBound(6) || random.calls[5] == RngCall::IntBound(15),
            "the sixth draw is the over-water height bound, got {:?}",
            random.calls[5]
        );
    }
}
