//! `net.minecraft.world.level.levelgen.feature.foliageplacers` — the foliage
//! placer framework of the tree family.
//!
//! The dispatch root is [`foliage_placer`] (`FoliagePlacer` trait +
//! `FoliagePlacer.CODEC` dispatch), with the registry-held
//! [`foliage_placer_type`] ids (`FoliagePlacerType`). The eleven concrete
//! placers each implement the trait and export an ops-generic
//! `Xxx_foliage_placer_map_codec::<Ops>()` factory that the dispatch's
//! `codec_for_type` resolves by id:
//!
//! - [`blob_foliage_placer`] — the three-field shared record (`blob_parts_map_codec`,
//!   reused by bush/fancy), tapering `max(leafRadius + radiusOffset - 1 - yo/2, 0)`.
//! - [`bush_foliage_placer`] — same record, `currentRadius` dropping by `yo` with
//!   a `random.nextInt(2)` corner skip.
//! - [`fancy_foliage_placer`] — same record, flat rows with the f32
//!   `Mth.square(dx + 0.5F)` quarter-circle skip.
//! - [`spruce_foliage_placer`] — two-level taper cycling `[minRadius, maxRadius)`.
//! - [`pine_foliage_placer`] — crown expanding toward the base.
//! - [`acacia_foliage_placer`] — the flat three-ring disc.
//! - [`mega_jungle_foliage_placer`] — rows widening down with the int circle skip.
//! - [`mega_pine_foliage_placer`] — smooth/jagged radius via `Mth.floor`.
//! - [`dark_oak_foliage_placer`] — double/single trunk crowns; overrides both
//!   skip predicates.
//! - [`random_spread_foliage_placer`] — box-sampled leaf scatter with a nested
//!   two-field `i.group(...)` record.
//! - [`cherry_foliage_placer`] — the hanging-leaves crown with a nested
//!   five-field `i.group(...)` (including the `corner_hole_chance` getter quirk).

pub mod acacia_foliage_placer;
pub mod blob_foliage_placer;
pub mod bush_foliage_placer;
pub mod cherry_foliage_placer;
pub mod dark_oak_foliage_placer;
pub mod fancy_foliage_placer;
pub mod foliage_placer;
pub mod foliage_placer_type;
pub mod mega_jungle_foliage_placer;
pub mod mega_pine_foliage_placer;
pub mod pine_foliage_placer;
pub mod random_spread_foliage_placer;
pub mod spruce_foliage_placer;
