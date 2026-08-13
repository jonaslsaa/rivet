//! Port of `net.minecraft.world.level.levelgen.carver.CaveWorldCarver` (class,
//! 26.2) — the cave/room/tunnel carver, plus the `NetherWorldCarver` (which
//! extends it) and the shared cave algorithm.
//!
//! Java: `CaveWorldCarver extends WorldCarver<CaveCarverConfiguration>` with
//! the `carve`/`isStartChunk` overrides and the protected
//! `getCaveBound`/`getThickness`/`getYScale`/`createRoom`/`createTunnel`/
//! `shouldSkip`. `NetherWorldCarver extends CaveWorldCarver` overriding
//! `getCaveBound` (10 vs 15), `getThickness` (a `(nextFloat()*2 + nextFloat())
//! *2` double roll vs the 1-in-10 bonus), `getYScale` (5.0 vs 1.0) and
//! `carveBlock` (lava below `getMinGenY()+31`, `CAVE_AIR` above; the base
//! `carveEllipsoid` walk is inherited).
//!
//! Translation notes (Java inheritance → Rust): `NetherWorldCarver extends
//! CaveWorldCarver` means the shared `carve`/`createRoom`/`createTunnel`
//! bodies call the *virtual* `getCaveBound`/`getThickness`/`getYScale`/
//! `carveBlock`. The Rust port models the shared algorithm as a generic
//! [`cave_carve`] over the [`CaveCarverHooks`] hook trait; both carvers are
//! zero-sized structs implementing both `WorldCarverBehavior<CaveCarverConfiguration>`
//! (the non-hook behavior, `carve_block` for the nether override) and
//! `CaveCarverHooks` (the three numbers). Monomorphization produces the two
//! Java instances' distinct behavior.
//!
//! RNG-fidelity notes:
//! - `caveCount = random.nextInt(random.nextInt(random.nextInt(getCaveBound()) + 1) + 1)`
//!   and the `tunnels = 1 + nextInt(4)` room roll match the Java call order.
//! - `createTunnel` seeds its own `RandomSource.createThreadLocalInstance(
//!   tunnelSeed)` (`random_source_create_thread_local_instance_with_seed`) and
//!   the split recursion consumes `nextLong()`/`nextFloat()` in the exact Java
//!   order.
//! - `Mth.sin`/`Mth.cos` take a double and return f32 via the `mth` tables;
//!   `Mth.PI * currentStep / dist` is float math widened to the double
//!   argument; `1.5 + sin(...) * thickness` widens the f32 product to the
//!   double literal.

use crate::chunk::carving_mask::CarvingMask;
use crate::levelgen::carver::carver_configuration::CarverConfiguration;
use crate::levelgen::carver::carving_context::CarvingContext;
use crate::levelgen::carver::cave_carver_configuration::CaveCarverConfiguration;
use crate::levelgen::carver::world_carver::{
    CarveChunk, CarveSkipChecker, ClosureSkipChecker, WorldCarverBehavior, can_reach,
};
use crate::levelgen::noisegen::aquifer::Aquifer;
use rivet_registry::core::SectionPos;
use rivet_registry::core::{ChunkPos, MutableBlockPos};
use rivet_util::RandomSource;
use rivet_util::mth;
use rivet_util::random::random_source_create_thread_local_instance_with_seed;
use std::fmt::Debug;

/// The `WorldCarver.CAVE` registration id (`register("cave", …)`, index 0).
pub const CAVE_ID: u32 = 0;
/// The `WorldCarver.NETHER_CAVE` registration id (`register("nether_cave", …)`,
/// index 1).
pub const NETHER_CAVE_ID: u32 = 1;

/// `net.minecraft.world.level.levelgen.carver.CaveWorldCarver` — the overworld
/// cave carver (id `CAVE`). Zero-sized: all behavior is the trait impls; the
/// shared `carve` algorithm lives in [`cave_carve`].
#[derive(Debug)]
pub struct CaveWorldCarver;

/// The `CaveWorldCarver` protected overridables `NetherWorldCarver` changes —
/// the `getCaveBound`/`getThickness`/`getYScale` hook trio the shared `carve`
/// algorithm calls (Java's virtual calls from the inherited `carve` body).
pub trait CaveCarverHooks: Send + Sync {
    /// `getCaveBound()` — `15` (the `caveCount` roll bound).
    fn get_cave_bound(&self) -> i32 {
        15
    }
    /// `getThickness(RandomSource)` — `nextFloat() * 2.0F + nextFloat()`, with
    /// the 1-in-10 bonus roll.
    fn get_thickness<R: RandomSource>(&self, random: &mut R) -> f32 {
        let mut thickness = random.next_float() * 2.0_f32 + random.next_float();
        if random.next_int_bound(10) == 0 {
            thickness *= random.next_float() * random.next_float() * 3.0_f32 + 1.0_f32;
        }
        thickness
    }
    /// `getYScale()` — `1.0`.
    fn get_y_scale(&self) -> f64 {
        1.0
    }
}

impl CaveCarverHooks for CaveWorldCarver {}

impl WorldCarverBehavior<CaveCarverConfiguration> for CaveWorldCarver {
    /// `isStartChunk` — `random.nextFloat() <= configuration.probability`.
    fn is_start_chunk<R: RandomSource>(
        &self,
        configuration: &CaveCarverConfiguration,
        random: &mut R,
    ) -> bool {
        random.next_float() <= configuration.probability()
    }

    /// `carve` — the shared cave algorithm (delegates to [`cave_carve`], which
    /// dispatches the `CaveCarverHooks` + `carve_block` virtuals).
    fn carve<R: RandomSource>(
        &self,
        context: &CarvingContext,
        configuration: &CaveCarverConfiguration,
        chunk: &mut dyn CarveChunk,
        random: &mut R,
        aquifer: &dyn Aquifer,
        source_chunk_pos: &ChunkPos,
        mask: &mut CarvingMask,
    ) -> bool {
        cave_carve(
            self,
            context,
            configuration,
            chunk,
            random,
            aquifer,
            source_chunk_pos,
            mask,
        )
    }
}

/// `net.minecraft.world.level.levelgen.carver.NetherWorldCarver` — the nether
/// cave carver (id `NETHER_CAVE`), extending `CaveWorldCarver`. Overrides the
/// three hook numbers and `carve_block`.
#[derive(Debug)]
pub struct NetherWorldCarver;

impl CaveCarverHooks for NetherWorldCarver {
    /// `getCaveBound()` — `10`.
    fn get_cave_bound(&self) -> i32 {
        10
    }
    /// `getThickness(RandomSource)` — `(nextFloat() * 2.0F + nextFloat()) *
    /// 2.0F` (no bonus roll).
    fn get_thickness<R: RandomSource>(&self, random: &mut R) -> f32 {
        (random.next_float() * 2.0_f32 + random.next_float()) * 2.0_f32
    }
    /// `getYScale()` — `5.0`.
    fn get_y_scale(&self) -> f64 {
        5.0
    }
}

impl WorldCarverBehavior<CaveCarverConfiguration> for NetherWorldCarver {
    /// `isStartChunk` — inherited from `CaveWorldCarver`
    /// (`random.nextFloat() <= configuration.probability`).
    fn is_start_chunk<R: RandomSource>(
        &self,
        configuration: &CaveCarverConfiguration,
        random: &mut R,
    ) -> bool {
        random.next_float() <= configuration.probability()
    }

    /// `carve` — the inherited `CaveWorldCarver.carve` (the shared algorithm
    /// with the nether hook numbers).
    fn carve<R: RandomSource>(
        &self,
        context: &CarvingContext,
        configuration: &CaveCarverConfiguration,
        chunk: &mut dyn CarveChunk,
        random: &mut R,
        aquifer: &dyn Aquifer,
        source_chunk_pos: &ChunkPos,
        mask: &mut CarvingMask,
    ) -> bool {
        cave_carve(
            self,
            context,
            configuration,
            chunk,
            random,
            aquifer,
            source_chunk_pos,
            mask,
        )
    }

    /// `carveBlock` — the override: replaceable blocks become lava below
    /// `getMinGenY() + 31`, `CAVE_AIR` above (no grass/myc surface handling,
    /// no aquifer carve state).
    fn carve_block(
        &self,
        context: &CarvingContext,
        configuration: &CaveCarverConfiguration,
        chunk: &mut dyn CarveChunk,
        block_pos: &mut MutableBlockPos,
        _helper_pos: &mut MutableBlockPos,
        _aquifer: &dyn Aquifer,
        _has_grass: &mut bool,
    ) -> bool {
        if self.can_replace_block(configuration, chunk.get_block_state(&block_pos.immutable())) {
            let state = if block_pos.get_y() <= context.get_min_gen_y().wrapping_add(31) {
                crate::block::blocks::Blocks::LAVA.default_block_state()
            } else {
                crate::block::blocks::Blocks::CAVE_AIR.default_block_state()
            };
            chunk.set_block_state(&block_pos.immutable(), state);
            true
        } else {
            false
        }
    }
}

/// The shared `CaveWorldCarver.carve` algorithm, generic over the carver's
/// hooks + behavior (Java's inherited `carve` body dispatching the virtual
/// `getCaveBound`/`getThickness`/`getYScale`/`carveBlock`).
#[allow(clippy::too_many_arguments)]
fn cave_carve<H, R>(
    hooks: &H,
    context: &CarvingContext,
    configuration: &CaveCarverConfiguration,
    chunk: &mut dyn CarveChunk,
    random: &mut R,
    aquifer: &dyn Aquifer,
    source_chunk_pos: &ChunkPos,
    mask: &mut CarvingMask,
) -> bool
where
    H: CaveCarverHooks + WorldCarverBehavior<CaveCarverConfiguration>,
    R: RandomSource,
{
    // `SectionPos.sectionToBlockCoord(getRange() * 2 - 1)` = 112 for the
    // default range 4 (7 sections to blocks).
    let max_distance =
        SectionPos::section_to_block_coord(self_range(hooks).wrapping_mul(2).wrapping_sub(1));
    // `random.nextInt(random.nextInt(random.nextInt(getCaveBound()) + 1) + 1)` —
    // the nested calls evaluate left-to-right, so the nextInt chain must be
    // sequenced (Rust forbids the double mutable borrow a nested call needs).
    let cave_count_inner = random.next_int_bound(hooks.get_cave_bound());
    let cave_count_mid = random.next_int_bound(cave_count_inner.wrapping_add(1));
    let cave_count = random.next_int_bound(cave_count_mid.wrapping_add(1));

    for _cave in 0..cave_count {
        let x = source_chunk_pos.get_block_x(random.next_int_bound(16)) as f64;
        let y = configuration.y().sample(random, context.world_context()) as f64;
        let z = source_chunk_pos.get_block_z(random.next_int_bound(16)) as f64;
        let horizontal_radius_multiplier =
            configuration.horizontal_radius_multiplier.sample(random) as f64;
        let vertical_radius_multiplier =
            configuration.vertical_radius_multiplier.sample(random) as f64;
        let floor_level = configuration.floor_level.sample(random) as f64;
        let skip_checker = ClosureSkipChecker(move |_c, xd, yd, zd, _y| {
            yd <= floor_level || xd * xd + yd * yd + zd * zd >= 1.0
        });
        let mut tunnels: i32 = 1;
        if random.next_int_bound(4) == 0 {
            let y_scale = configuration.y_scale().sample(random) as f64;
            let thickness = 1.0_f32 + random.next_float() * 6.0_f32;
            create_room(
                hooks,
                context,
                configuration,
                chunk,
                aquifer,
                x,
                y,
                z,
                thickness,
                y_scale,
                mask,
                &skip_checker,
            );
            tunnels = tunnels.wrapping_add(random.next_int_bound(4));
        }

        for _ in 0..tunnels {
            let horizontal_rotation = random.next_float() * mth::TWO_PI;
            let vertical_rotation = (random.next_float() - 0.5_f32) / 4.0_f32;
            let thickness = hooks.get_thickness(random);
            let distance = max_distance.wrapping_sub(random.next_int_bound(max_distance / 4));
            create_tunnel(
                hooks,
                context,
                configuration,
                chunk,
                random.next_long(),
                aquifer,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                thickness,
                horizontal_rotation,
                vertical_rotation,
                0,
                distance,
                hooks.get_y_scale(),
                mask,
                &skip_checker,
            );
        }
    }

    true
}

/// `getRange()` for the generic hooks (the trait's default `get_range`).
fn self_range<H: WorldCarverBehavior<CaveCarverConfiguration>>(hooks: &H) -> i32 {
    hooks.get_range()
}

/// `CaveWorldCarver.createRoom` — the single-room ellipsoid (`1.5 +
/// sin(π/2) * thickness` radius at `x + 1`).
#[allow(clippy::too_many_arguments)]
fn create_room<H>(
    hooks: &H,
    context: &CarvingContext,
    configuration: &CaveCarverConfiguration,
    chunk: &mut dyn CarveChunk,
    aquifer: &dyn Aquifer,
    x: f64,
    y: f64,
    z: f64,
    thickness: f32,
    y_scale: f64,
    mask: &mut CarvingMask,
    skip_checker: &dyn CarveSkipChecker,
) where
    H: CaveCarverHooks + WorldCarverBehavior<CaveCarverConfiguration>,
{
    // Java: `1.5 + Mth.sin((float)(Math.PI / 2)) * thickness` — `sin(π/2)` as
    // a float, times the float thickness (f32 product), widened to the double
    // literal.
    let horizontal_radius = 1.5 + (mth::sin(mth::HALF_PI as f64) * thickness) as f64;
    let vertical_radius = horizontal_radius * y_scale;
    hooks.carve_ellipsoid(
        context,
        configuration,
        chunk,
        aquifer,
        x + 1.0,
        y,
        z,
        horizontal_radius,
        vertical_radius,
        mask,
        skip_checker,
    );
}

/// `CaveWorldCarver.createTunnel` — the recursive tunnel walk. Seeds its own
/// `RandomSource` from `tunnelSeed`, walks `[step, dist)` advancing the tunnel
/// tip by the rotation vectors, splitting at `splitPoint` (two child tunnels
/// on perpendicular bearings, `return`-terminating the parent) and carving an
/// ellipsoid at each step unless the `canReach` gate fails.
#[allow(clippy::too_many_arguments)]
fn create_tunnel<H>(
    hooks: &H,
    context: &CarvingContext,
    configuration: &CaveCarverConfiguration,
    chunk: &mut dyn CarveChunk,
    tunnel_seed: i64,
    aquifer: &dyn Aquifer,
    mut x: f64,
    mut y: f64,
    mut z: f64,
    horizontal_radius_multiplier: f64,
    vertical_radius_multiplier: f64,
    thickness: f32,
    mut horizontal_rotation: f32,
    mut vertical_rotation: f32,
    step: i32,
    dist: i32,
    y_scale: f64,
    mask: &mut CarvingMask,
    skip_checker: &dyn CarveSkipChecker,
) where
    H: CaveCarverHooks + WorldCarverBehavior<CaveCarverConfiguration>,
{
    let mut random = random_source_create_thread_local_instance_with_seed(tunnel_seed);
    let split_point = random.next_int_bound(dist / 2).wrapping_add(dist / 4);
    let steep = random.next_int_bound(6) == 0;
    let mut y_rota = 0.0_f32;
    let mut x_rota = 0.0_f32;

    let mut current_step = step;
    while current_step < dist {
        // Java: `Mth.PI * currentStep / dist` — float math widened to the
        // double `Mth.sin` argument; `1.5 + sin * thickness` widens the f32
        // product to the double literal.
        let horizontal_radius = 1.5
            + (mth::sin((mth::PI * current_step as f32 / dist as f32) as f64) * thickness) as f64;
        let vertical_radius = horizontal_radius * y_scale;
        let cos_x = mth::cos(vertical_rotation as f64);
        x += (mth::cos(horizontal_rotation as f64) * cos_x) as f64;
        y += mth::sin(vertical_rotation as f64) as f64;
        z += (mth::sin(horizontal_rotation as f64) * cos_x) as f64;
        vertical_rotation *= if steep { 0.92_f32 } else { 0.7_f32 };
        vertical_rotation += x_rota * 0.1_f32;
        horizontal_rotation += y_rota * 0.1_f32;
        x_rota *= 0.9_f32;
        y_rota *= 0.75_f32;
        x_rota += (random.next_float() - random.next_float()) * random.next_float() * 2.0_f32;
        y_rota += (random.next_float() - random.next_float()) * random.next_float() * 4.0_f32;

        if current_step == split_point && thickness > 1.0_f32 {
            // Java evaluates the recursive args left-to-right: nextLong (seed),
            // nextFloat (child thickness), then the second child's nextLong +
            // nextFloat.
            let left_seed = random.next_long();
            let left_thickness = random.next_float() * 0.5_f32 + 0.5_f32;
            let right_seed = random.next_long();
            let right_thickness = random.next_float() * 0.5_f32 + 0.5_f32;
            create_tunnel::<H>(
                hooks,
                context,
                configuration,
                chunk,
                left_seed,
                aquifer,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                left_thickness,
                horizontal_rotation - mth::HALF_PI,
                vertical_rotation / 3.0_f32,
                current_step,
                dist,
                1.0,
                mask,
                skip_checker,
            );
            create_tunnel::<H>(
                hooks,
                context,
                configuration,
                chunk,
                right_seed,
                aquifer,
                x,
                y,
                z,
                horizontal_radius_multiplier,
                vertical_radius_multiplier,
                right_thickness,
                horizontal_rotation + mth::HALF_PI,
                vertical_rotation / 3.0_f32,
                current_step,
                dist,
                1.0,
                mask,
                skip_checker,
            );
            return;
        }

        if random.next_int_bound(4) != 0 {
            if !can_reach(&chunk.get_pos(), x, z, current_step, dist, thickness) {
                return;
            }
            hooks.carve_ellipsoid(
                context,
                configuration,
                chunk,
                aquifer,
                x,
                y,
                z,
                horizontal_radius * horizontal_radius_multiplier,
                vertical_radius * vertical_radius_multiplier,
                mask,
                skip_checker,
            );
        }
        current_step += 1;
    }
}
