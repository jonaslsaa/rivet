//! Structure terrain density from the pinned Paper `Beardifier`. Input views
//! retain their authority's order; absent references or unsupported persisted
//! pieces are errors, never an invented empty structure influence.
use super::{
    density_function::{ContextProvider, DensityFunction, FunctionContext},
    density_function_type::{DensityFunctionTypeId, DensityFunctionTypes},
};
use crate::levelgen::structure::{BoundingBox, TerrainAdjustment};
use rivet_registry::core::{BlockPos, ChunkPos};
use rivet_util::mth;
use std::{
    any::Any,
    sync::{Arc, LazyLock},
};

#[cfg(test)]
#[path = "beardifier_test.rs"]
mod tests;

pub const BEARD_KERNEL_RADIUS: i32 = 12;
const KERNEL_SIZE: usize = 24;
const KERNEL_VOLUME: usize = KERNEL_SIZE * KERNEL_SIZE * KERNEL_SIZE;
static BEARD_KERNEL: LazyLock<[f32; KERNEL_VOLUME]> = LazyLock::new(|| {
    let mut kernel = [0.0; KERNEL_VOLUME];
    for zi in 0..KERNEL_SIZE {
        for xi in 0..KERNEL_SIZE {
            for yi in 0..KERNEL_SIZE {
                let distance_squared = mth::length_squared_xyz(
                    xi as f64 - 12.0,
                    yi as f64 - 12.0 + 0.5,
                    zi as f64 - 12.0,
                );
                // Math.pow(Math.E, ...), not Math.exp; the stored source table
                // rounds to FLOAT before being widened during contribution.
                kernel[zi * KERNEL_SIZE * KERNEL_SIZE + xi * KERNEL_SIZE + yi] =
                    std::f64::consts::E.powf(-distance_squared / 16.0) as f32;
            }
        }
    }
    kernel
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rigid {
    pub bounds: BoundingBox,
    pub terrain_adjustment: TerrainAdjustment,
    pub ground_level_delta: i32,
}

/// The coordinates Beardifier actually reads from a JigsawJunction; this is a
/// terrain-input view, not the complete serialized junction subtype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeardifierJunction {
    pub source_x: i32,
    pub source_ground_y: i32,
    pub source_z: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolProjection {
    Rigid,
    TerrainMatching,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BeardifierPiece {
    Ordinary {
        bounds: BoundingBox,
    },
    Pool {
        bounds: BoundingBox,
        projection: PoolProjection,
        ground_level_delta: i32,
        junctions: Vec<BeardifierJunction>,
    },
}
impl BeardifierPiece {
    fn bounds(&self) -> BoundingBox {
        match self {
            Self::Ordinary { bounds } | Self::Pool { bounds, .. } => *bounds,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BeardifierStart {
    pub terrain_adjustment: TerrainAdjustment,
    pub pieces: Vec<BeardifierPiece>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BeardifierError {
    #[error("structure references unavailable for {0:?}")]
    ReferencesUnavailable(ChunkPos),
    #[error("structure terrain input unavailable: {0}")]
    AuthorityUnavailable(String),
    #[error("deferred structure terrain input: {0}")]
    DeferredInput(String),
    #[error("unsupported structure terrain piece: {0}")]
    UnsupportedPiece(String),
    #[error("source structure iteration order unavailable: {0}")]
    OrderingUnavailable(String),
}
pub trait BeardifierSource {
    /// Resolve actual valid starts referenced by the target chunk, in the
    /// original StructureManager iteration order. Missing/deferred starts or
    /// unknown piece metadata must error. A successful empty Vec asserts that
    /// the authoritative reference traversal found no contributing starts.
    fn starts_for_chunk(&self, chunk: ChunkPos) -> Result<Vec<BeardifierStart>, BeardifierError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beardifier {
    pieces: Vec<Rigid>,
    junctions: Vec<BeardifierJunction>,
    affected_box: Option<BoundingBox>,
}
impl Beardifier {
    pub fn new(
        pieces: Vec<Rigid>,
        junctions: Vec<BeardifierJunction>,
        affected_box: Option<BoundingBox>,
    ) -> Self {
        Self {
            pieces,
            junctions,
            affected_box,
        }
    }
    pub fn empty() -> Self {
        Self::new(Vec::new(), Vec::new(), None)
    }
    pub fn pieces(&self) -> &[Rigid] {
        &self.pieces
    }
    pub fn junctions(&self) -> &[BeardifierJunction] {
        &self.junctions
    }
    pub fn affected_box(&self) -> Option<BoundingBox> {
        self.affected_box
    }

    pub fn for_structures_in_chunk(
        source: &impl BeardifierSource,
        chunk: ChunkPos,
    ) -> Result<Self, BeardifierError> {
        let starts = source.starts_for_chunk(chunk)?;
        Ok(Self::from_ordered_starts(&starts, chunk))
    }

    /// The collector half of forStructuresInChunk. Inputs are already valid
    /// native starts; no deduplication, sorting, or projection inference occurs.
    pub fn from_ordered_starts(starts: &[BeardifierStart], chunk: ChunkPos) -> Self {
        let chunk_x = chunk.get_min_block_x();
        let chunk_z = chunk.get_min_block_z();
        let min_x = chunk_x.wrapping_sub(12);
        let min_z = chunk_z.wrapping_sub(12);
        let max_x = chunk_x.wrapping_add(15).wrapping_add(12);
        let max_z = chunk_z.wrapping_add(15).wrapping_add(12);
        let mut pieces = Vec::new();
        let mut junctions = Vec::new();
        let mut affected: Option<BoundingBox> = None;
        let mut include = |bounds: BoundingBox| match &mut affected {
            Some(box_) => box_.encapsulate(bounds),
            None => affected = Some(bounds),
        };
        for start in starts {
            if start.terrain_adjustment == TerrainAdjustment::None {
                continue;
            }
            for piece in &start.pieces {
                let bounds = piece.bounds();
                // StructurePiece.isCloseToChunk: inclusive X/Z intersection,
                // no Y clipping and no normalization of the query interval.
                if !(bounds.max_x >= min_x
                    && bounds.min_x <= max_x
                    && bounds.max_z >= min_z
                    && bounds.min_z <= max_z)
                {
                    continue;
                }
                match piece {
                    BeardifierPiece::Ordinary { .. } => {
                        pieces.push(Rigid {
                            bounds,
                            terrain_adjustment: start.terrain_adjustment,
                            ground_level_delta: 0,
                        });
                        include(bounds);
                    }
                    BeardifierPiece::Pool {
                        projection,
                        ground_level_delta,
                        junctions: inputs,
                        ..
                    } => {
                        if *projection == PoolProjection::Rigid {
                            pieces.push(Rigid {
                                bounds,
                                terrain_adjustment: start.terrain_adjustment,
                                ground_level_delta: *ground_level_delta,
                            });
                            include(bounds);
                        }
                        for junction in inputs {
                            if junction.source_x > min_x
                                && junction.source_z > min_z
                                && junction.source_x < max_x
                                && junction.source_z < max_z
                            {
                                junctions.push(*junction);
                                include(BoundingBox::at(BlockPos::new(
                                    junction.source_x,
                                    junction.source_ground_y,
                                    junction.source_z,
                                )));
                            }
                        }
                    }
                }
            }
        }
        match affected {
            Some(bounds) => Self::new(pieces, junctions, Some(bounds.inflated_by(24))),
            None => Self::empty(),
        }
    }

    fn bury_contribution(dx: f64, dy: f64, dz: f64) -> f64 {
        mth::clamped_map(mth::length_xyz(dx, dy, dz), 0.0, 6.0, 1.0, 0.0)
    }
    fn beard_contribution(dx: i32, dy: i32, dz: i32, y_to_ground: i32) -> f64 {
        let xi = dx.wrapping_add(12);
        let yi = dy.wrapping_add(12);
        let zi = dz.wrapping_add(12);
        if !(0..24).contains(&xi) || !(0..24).contains(&yi) || !(0..24).contains(&zi) {
            return 0.0;
        }
        let y = f64::from(y_to_ground) + 0.5;
        let squared = mth::length_squared_xyz(f64::from(dx), y, f64::from(dz));
        let value = -y * mth::fast_inv_sqrt(squared / 2.0) / 2.0;
        value * f64::from(BEARD_KERNEL[zi as usize * 24 * 24 + xi as usize * 24 + yi as usize])
    }
}
impl DensityFunction for Beardifier {
    fn compute(&self, context: &dyn FunctionContext) -> f64 {
        let Some(affected) = self.affected_box else {
            return 0.0;
        };
        let x = context.block_x();
        let y = context.block_y();
        let z = context.block_z();
        if !affected.is_inside(&BlockPos::new(x, y, z)) {
            return 0.0;
        }
        let mut value = 0.0;
        for rigid in &self.pieces {
            let bounds = rigid.bounds;
            let dx = 0.max(
                bounds
                    .min_x
                    .wrapping_sub(x)
                    .max(x.wrapping_sub(bounds.max_x)),
            );
            let dz = 0.max(
                bounds
                    .min_z
                    .wrapping_sub(z)
                    .max(z.wrapping_sub(bounds.max_z)),
            );
            let ground = bounds.min_y.wrapping_add(rigid.ground_level_delta);
            let to_ground = y.wrapping_sub(ground);
            let dy = match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0,
                TerrainAdjustment::Bury | TerrainAdjustment::BeardThin => to_ground,
                TerrainAdjustment::BeardBox => {
                    0.max(ground.wrapping_sub(y).max(y.wrapping_sub(bounds.max_y)))
                }
                TerrainAdjustment::Encapsulate => 0.max(
                    bounds
                        .min_y
                        .wrapping_sub(y)
                        .max(y.wrapping_sub(bounds.max_y)),
                ),
            };
            value += match rigid.terrain_adjustment {
                TerrainAdjustment::None => 0.0,
                TerrainAdjustment::Bury => {
                    Self::bury_contribution(f64::from(dx), f64::from(dy) / 2.0, f64::from(dz))
                }
                TerrainAdjustment::BeardThin | TerrainAdjustment::BeardBox => {
                    Self::beard_contribution(dx, dy, dz, to_ground) * 0.8
                }
                TerrainAdjustment::Encapsulate => {
                    Self::bury_contribution(
                        f64::from(dx) / 2.0,
                        f64::from(dy) / 2.0,
                        f64::from(dz) / 2.0,
                    ) * 0.8
                }
            };
        }
        for junction in &self.junctions {
            let dx = x.wrapping_sub(junction.source_x);
            let dy = y.wrapping_sub(junction.source_ground_y);
            let dz = z.wrapping_sub(junction.source_z);
            value += Self::beard_contribution(dx, dy, dz, dy) * 0.4;
        }
        value
    }
    fn fill_array(&self, output: &mut [f64], provider: &dyn ContextProvider) {
        if self.affected_box.is_none() {
            output.fill(0.0);
        } else {
            provider.fill_all_directly(output, self);
        }
    }
    fn min_value(&self) -> f64 {
        f64::NEG_INFINITY
    }
    fn max_value(&self) -> f64 {
        f64::INFINITY
    }
    fn type_id(&self) -> DensityFunctionTypeId {
        DensityFunctionTypes::BEARDIFIER
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn clone_arc(&self) -> Arc<dyn DensityFunction> {
        Arc::new(self.clone())
    }
}
