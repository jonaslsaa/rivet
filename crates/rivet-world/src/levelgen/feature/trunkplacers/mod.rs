//! `net.minecraft.world.level.levelgen.feature.trunkplacers` — the trunk
//! placer framework of the tree family.
//!
//! The dispatch root is [`trunk_placer`] (`TrunkPlacer` trait +
//! `TrunkPlacer.CODEC` dispatch), with the registry-held [`trunk_placer_type`]
//! ids (`TrunkPlacerType`). The nine concrete placers each implement the trait
//! and export an ops-generic `Xxx_trunk_placer_map_codec::<Ops>()` factory that
//! the dispatch's `codec_for_type` resolves by id:
//!
//! - [`straight_trunk_placer`] — the plain vertical column (below-trunk block +
//!   `placeLog` per height step).
//! - [`forking_trunk_placer`] — the leaning column with an optional second
//!   branch, both steered by `Direction.Plane.HORIZONTAL.getRandomDirection`.
//! - [`giant_trunk_placer`] — the 2x2 trunk (`placeLogIfFreeWithOffset` over the
//!   four offsets, tapering to the single origin column on the top layer).
//! - [`mega_jungle_trunk_placer`] — `GiantTrunkPlacer` plus the five-step
//!   `Mth.cos`/`Mth.sin` branch limbs.
//! - [`dark_oak_trunk_placer`] — the leaning 2x2 trunk (`isAirOrLeaves` gated)
//!   with the random branch ring.
//! - [`fancy_trunk_placer`] — the `Mth`-table limbs, `makeLimb`/`makeBranches`,
//!   and the axis-aligned log placement (`trySetValue` on `RotatedPillarBlock.
//!   AXIS`).
//! - [`bending_trunk_placer`] — the two-phase bent column with the
//!   `min_height_for_leaves`/`bend_length` group.
//! - [`upwards_branching_trunk_placer`] — the per-log branch placement with the
//!   `can_grow_through` holder set and the `validTreePos` override.
//! - [`cherry_trunk_placer`] — the two side branches (plus optional middle) with
//!   the `UniformInt` branch-start validation.

pub mod bending_trunk_placer;
pub mod cherry_trunk_placer;
pub mod dark_oak_trunk_placer;
pub mod fancy_trunk_placer;
pub mod forking_trunk_placer;
pub mod giant_trunk_placer;
pub mod mega_jungle_trunk_placer;
pub mod straight_trunk_placer;
pub mod trunk_placer;
pub mod trunk_placer_type;
pub mod upwards_branching_trunk_placer;
