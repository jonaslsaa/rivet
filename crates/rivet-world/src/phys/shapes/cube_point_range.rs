//! `net.minecraft.world.phys.shapes.CubePointRange` — the `parts + 1` evenly
//! spaced points `0, 1/parts, ..., 1`.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/CubePointRange.java`.

use std::any::Any;

use rivet_util::double_list::DoubleList;

/// `CubePointRange extends AbstractDoubleList`.
#[derive(Clone, Debug)]
pub struct CubePointRange {
    parts: i32,
}

impl CubePointRange {
    /// `CubePointRange(int)` — `IllegalArgumentException("Need at least 1
    /// part")` for `parts <= 0`.
    pub fn new(parts: i32) -> CubePointRange {
        if parts <= 0 {
            panic!("Need at least 1 part");
        }
        CubePointRange { parts }
    }
}

impl DoubleList for CubePointRange {
    /// `CubePointRange.getDouble(int)` — `(double) index / this.parts`.
    fn get_double(&self, index: usize) -> f64 {
        index as i32 as f64 / self.parts as f64
    }

    /// `CubePointRange.size()`.
    fn size(&self) -> usize {
        (self.parts + 1) as usize
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
