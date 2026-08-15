//! `net.minecraft.data` — the data-bootstrap package.
//!
//! Currently only the `mc.data.worldgen.prereq` slice is ported
//! (`data::worldgen`), plus the seed-42 FEATURES generated-table read surface
//! (`data::feature_data`); the rest of the package stays pending.

pub mod feature_data;
pub mod worldgen;
