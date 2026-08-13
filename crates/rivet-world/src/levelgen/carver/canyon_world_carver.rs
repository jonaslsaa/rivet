//! Port of `net.minecraft.world.level.levelgen.carver.CanyonWorldCarver` (class,
//! 26.2) — the canyon/ravine carver.
//!
//! Java: `CanyonWorldCarver extends WorldCarver<CanyonCarverConfiguration>`
//! with the `carve`/`isStartChunk` overrides and the private
//! `doCarve`/`initWidthFactors`/`updateVerticalRadius`/`shouldSkip`. The
//! `carve` body dispatches the inherited `carveEllipsoid` (via the base
//! `carveBlock`/`getCarveState`) with a `CarveSkipChecker` lambda capturing
//! `widthFactorPerHeight`.
//!
//! RNG-fidelity notes:
//! - `doCarve` seeds its own `RandomSource.createThreadLocalInstance(tunnelSeed)`
//!   (`random_source_create_thread_local_instance_with_seed`); `initWidthFactors`
//!   consumes `nextInt(widthSmoothness)`/`nextFloat()` from it before the walk.
//! - `widthFactorPerHeight` is indexed `[yIndex - 1]` in `shouldSkip`, where
//!   `yIndex = y - context.getMinGenY()`; the first carve block (`yIndex == 1`)
//!   reads `widthFactorPerHeight[0]` — the closure must see the full array
//!   (`&[f32]`).
//! - `updateVerticalRadius`'s `Mth.randomBetween(random, 0.75F, 1.0F)` is
//!   `mth::random_between(random, 0.75, 1.0)` (max-exclusive).
//! - `maxDistance = (getRange() * 2 - 1) * 16` = 112 for the default range 4.

use crate::chunk::carving_mask::CarvingMask;
use crate::levelgen::carver::canyon_carver_configuration::CanyonCarverConfiguration;
use crate::levelgen::carver::carver_configuration::CarverConfiguration;
use crate::levelgen::carver::carving_context::CarvingContext;
use crate::levelgen::carver::world_carver::{
    CarveChunk, ClosureSkipChecker, WorldCarverBehavior, can_reach,
};
use crate::levelgen::noisegen::aquifer::Aquifer;
use rivet_registry::core::ChunkPos;
use rivet_util::RandomSource;
use rivet_util::mth;
use rivet_util::random::random_source_create_thread_local_instance_with_seed;
use std::fmt::Debug;

/// The `WorldCarver.CANYON` registration id (`register("canyon", …)`, index 2).
pub const CANYON_ID: u32 = 2;

/// `net.minecraft.world.level.levelgen.carver.CanyonWorldCarver` — the canyon
/// carver (id `CANYON`). Zero-sized: all behavior is the trait impl.
#[derive(Debug)]
pub struct CanyonWorldCarver;

impl WorldCarverBehavior<CanyonCarverConfiguration> for CanyonWorldCarver {
    /// `isStartChunk` — `random.nextFloat() <= configuration.probability`.
    fn is_start_chunk<R: RandomSource>(
        &self,
        configuration: &CanyonCarverConfiguration,
        random: &mut R,
    ) -> bool {
        random.next_float() <= configuration.probability()
    }

    /// `carve` — the single canyon walk: pick a start, rotation, thickness and
    /// distance from the config providers, then `doCarve`.
    fn carve<R: RandomSource>(
        &self,
        context: &CarvingContext,
        configuration: &CanyonCarverConfiguration,
        chunk: &mut dyn CarveChunk,
        random: &mut R,
        aquifer: &dyn Aquifer,
        source_chunk_pos: &ChunkPos,
        mask: &mut CarvingMask,
    ) -> bool {
        let max_distance = self
            .get_range()
            .wrapping_mul(2)
            .wrapping_sub(1)
            .wrapping_mul(16);
        let x = source_chunk_pos.get_block_x(random.next_int_bound(16)) as f64;
        let y = configuration.y().sample(random, context.world_context()) as f64;
        let z = source_chunk_pos.get_block_z(random.next_int_bound(16)) as f64;
        let horizontal_rotation = random.next_float() * mth::TWO_PI;
        let vertical_rotation = configuration.vertical_rotation.sample(random);
        let y_scale = configuration.y_scale().sample(random) as f64;
        let thickness = configuration.shape.thickness.sample(random);
        // Java: `(int)(maxDistance * distanceFactor.sample(random))` — the int
        // `maxDistance` widens to float, the product is float math, then
        // truncated.
        let distance =
            (max_distance as f32 * configuration.shape.distance_factor.sample(random)) as i32;
        self.do_carve(
            context,
            configuration,
            chunk,
            random.next_long(),
            aquifer,
            x,
            y,
            z,
            thickness,
            horizontal_rotation,
            vertical_rotation,
            0,
            distance,
            y_scale,
            mask,
        );
        true
    }
}

impl CanyonWorldCarver {
    /// `CanyonWorldCarver.doCarve` — the canyon walk: the width-factor table,
    /// the per-step rotation updates and the ellipsoid carve gated by
    /// `canReach` + the width-profile `shouldSkip`.
    #[allow(clippy::too_many_arguments)]
    fn do_carve(
        &self,
        context: &CarvingContext,
        configuration: &CanyonCarverConfiguration,
        chunk: &mut dyn CarveChunk,
        tunnel_seed: i64,
        aquifer: &dyn Aquifer,
        mut x: f64,
        mut y: f64,
        mut z: f64,
        thickness: f32,
        mut horizontal_rotation: f32,
        mut vertical_rotation: f32,
        step: i32,
        distance: i32,
        y_scale: f64,
        mask: &mut CarvingMask,
    ) {
        let mut random = random_source_create_thread_local_instance_with_seed(tunnel_seed);
        let width_factor_per_height = self.init_width_factors(context, configuration, &mut random);
        let mut y_rota = 0.0_f32;
        let mut x_rota = 0.0_f32;

        let mut current_step = step;
        while current_step < distance {
            // Java: `Mth.sin(currentStep * Mth.PI / distance)` — int * float
            // (`currentStep * Mth.PI`) is float math, then / distance (int,
            // widened), widened to the double `Mth.sin` argument; `1.5 +
            // sin * thickness` widens the f32 product to the double literal.
            let horizontal_radius = 1.5
                + (mth::sin((current_step as f32 * mth::PI / distance as f32) as f64) * thickness)
                    as f64;
            let mut vertical_radius = horizontal_radius * y_scale;
            let horizontal_radius = horizontal_radius
                * configuration
                    .shape
                    .horizontal_radius_factor
                    .sample(&mut random) as f64;
            vertical_radius = self.update_vertical_radius(
                configuration,
                &mut random,
                vertical_radius,
                distance as f32,
                current_step as f32,
            );
            let xc = mth::cos(vertical_rotation as f64);
            let xs = mth::sin(vertical_rotation as f64);
            x += (mth::cos(horizontal_rotation as f64) * xc) as f64;
            y += xs as f64;
            z += (mth::sin(horizontal_rotation as f64) * xc) as f64;
            vertical_rotation *= 0.7_f32;
            vertical_rotation += x_rota * 0.05_f32;
            horizontal_rotation += y_rota * 0.05_f32;
            x_rota *= 0.8_f32;
            y_rota *= 0.5_f32;
            x_rota += (random.next_float() - random.next_float()) * random.next_float() * 2.0_f32;
            y_rota += (random.next_float() - random.next_float()) * random.next_float() * 4.0_f32;

            if random.next_int_bound(4) != 0 {
                if !can_reach(&chunk.get_pos(), x, z, current_step, distance, thickness) {
                    return;
                }
                let skip = ClosureSkipChecker(|c: &CarvingContext, xd, yd, zd, y1: i32| {
                    let y_index = y1.wrapping_sub(c.get_min_gen_y());
                    // Java widens the float width factor to the double product.
                    (xd * xd + zd * zd)
                        * width_factor_per_height[y_index.wrapping_sub(1) as usize] as f64
                        + yd * yd / 6.0
                        >= 1.0
                });
                self.carve_ellipsoid(
                    context,
                    configuration,
                    chunk,
                    aquifer,
                    x,
                    y,
                    z,
                    horizontal_radius,
                    vertical_radius,
                    mask,
                    &skip,
                );
            }
            current_step += 1;
        }
    }

    /// `CanyonWorldCarver.initWidthFactors` — the per-height width-factor table
    /// (`float[depth]`): `widthFactor = 1.0F + nextFloat() * nextFloat()` at
    /// `yIndex == 0` or on a `nextInt(widthSmoothness) == 0` roll, storing
    /// `widthFactor * widthFactor`.
    fn init_width_factors<R: RandomSource>(
        &self,
        context: &CarvingContext,
        configuration: &CanyonCarverConfiguration,
        random: &mut R,
    ) -> Vec<f32> {
        let depth = context.get_gen_depth();
        let mut width_factor_per_height = vec![0.0_f32; depth as usize];
        let mut width_factor = 1.0_f32;

        for y_index in 0..depth {
            if y_index == 0 || random.next_int_bound(configuration.shape.width_smoothness) == 0 {
                width_factor = 1.0_f32 + random.next_float() * random.next_float();
            }
            width_factor_per_height[y_index as usize] = width_factor * width_factor;
        }

        width_factor_per_height
    }

    /// `CanyonWorldCarver.updateVerticalRadius` — the vertical-radius
    /// multiplier peaked at the canyon's vertical center
    /// (`1.0F - abs(0.5F - currentStep/distance) * 2.0F`), weighted by the
    /// default/center factors and jittered by
    /// `Mth.randomBetween(random, 0.75F, 1.0F)`.
    fn update_vertical_radius<R: RandomSource>(
        &self,
        configuration: &CanyonCarverConfiguration,
        random: &mut R,
        vertical_radius: f64,
        distance: f32,
        current_step: f32,
    ) -> f64 {
        // Java: `0.5F - currentStep / distance` — int/float division is float
        // (`currentStep` widened), then `abs(...) * 2.0F` float math.
        let vertical_multiplier = 1.0_f32 - mth::abs(0.5_f32 - current_step / distance) * 2.0_f32;
        let factor = configuration.shape.vertical_radius_default_factor
            + configuration.shape.vertical_radius_center_factor * vertical_multiplier;
        (factor as f64 * vertical_radius) * mth::random_between(random, 0.75, 1.0) as f64
    }
}
