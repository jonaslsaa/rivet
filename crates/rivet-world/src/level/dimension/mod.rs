//! `net.minecraft.world.level.dimension` — the dimension module.
//!
//! Only the `DimensionType` height constants that `VerticalAnchor`'s int-range
//! codecs reference are ported here (issue #388 leaf). The full `DimensionType`
//! record/codec (fields, constructor validation, `MonsterSettings`/`Skybox`,
//! the direct/network codecs) defers with the owning
//! `mc.world.level.dimension` manifest unit; this module is the minimal
//! constants-only shell that later lands with the full record.

pub mod dimension_type;

pub use dimension_type::{
    BITS_FOR_Y, MAX_Y, MIN_HEIGHT, MIN_Y, WAY_ABOVE_MAX_Y, WAY_BELOW_MIN_Y, Y_SIZE,
};
