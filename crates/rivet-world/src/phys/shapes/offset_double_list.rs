//! `net.minecraft.world.phys.shapes.OffsetDoubleList` — a list view with a
//! constant added to every element.
//!
//! Java source:
//! `working/Paper/paper-server/src/minecraft/java/net/minecraft/world/phys/shapes/OffsetDoubleList.java`
//! (Paper: public `delegate`/`offset`).

use std::any::Any;
use std::sync::Arc;

use rivet_util::double_list::DoubleList;

/// `OffsetDoubleList extends AbstractDoubleList`.
#[derive(Clone, Debug)]
pub struct OffsetDoubleList {
    pub delegate: Arc<dyn DoubleList>,
    pub offset: f64,
}

impl OffsetDoubleList {
    /// `OffsetDoubleList(DoubleList, double)`.
    pub fn new(delegate: Arc<dyn DoubleList>, offset: f64) -> OffsetDoubleList {
        OffsetDoubleList { delegate, offset }
    }
}

impl DoubleList for OffsetDoubleList {
    /// `OffsetDoubleList.getDouble(int)`.
    fn get_double(&self, index: usize) -> f64 {
        self.delegate.get_double(index) + self.offset
    }

    /// `OffsetDoubleList.size()`.
    fn size(&self) -> usize {
        self.delegate.size()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
