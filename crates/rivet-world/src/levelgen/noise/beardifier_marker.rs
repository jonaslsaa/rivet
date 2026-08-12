//! Port of `DensityFunctions.BeardifierMarker` + `BeardifierOrMarker`
//! (enum/interface, 26.2).
//!
//! Java's `BeardifierMarker` is the `beardifier` density-function value — the
//! structural-beard marker used during structure placement. The full
//! `Beardifier` structure runtime (the `BEARD_KERNEL` contributions, `Rigid`,
//! `forStructuresInChunk`) belongs to the `mc.world.level.levelgen.structure`
//! unit and defers — RivetTodo(#177). This module ports the value shell Java's
//! `DensityFunctions` declares: `compute`/`fillArray`/`minValue`/`maxValue`
//! all return `0.0` (the enum's exact bodies). The `BeardifierOrMarker` unit
//! codec (`MapCodec.unit(INSTANCE)`) lives in `density_functions`.

use crate::levelgen::noise::density_function::{DensityFunction, FunctionContext};
use crate::levelgen::noise::density_function_type::{DensityFunctionTypeId, DensityFunctionTypes};
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

/// `DensityFunctions.BeardifierMarker.INSTANCE` — the `beardifier` value
/// shell. The real `Beardifier` structure contributions defer to the structure
/// unit (RivetTodo #177); the shell keeps Java's declared value behavior
/// (everything `0.0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BeardifierMarker;

impl BeardifierMarker {
    /// `BeardifierMarker.INSTANCE`.
    pub fn instance() -> Self {
        BeardifierMarker
    }
}

impl DensityFunction for BeardifierMarker {
    fn compute(&self, _context: &dyn FunctionContext) -> f64 {
        0.0
    }
    fn fill_array(
        &self,
        output: &mut [f64],
        _context_provider: &dyn crate::levelgen::noise::density_function::ContextProvider,
    ) {
        output.fill(0.0);
    }
    fn min_value(&self) -> f64 {
        0.0
    }
    fn max_value(&self) -> f64 {
        0.0
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::BEARDIFIER
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(BeardifierMarker)
    }
}
