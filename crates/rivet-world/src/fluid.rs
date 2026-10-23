//! Native scheduled water/lava simulation, separate from their registry IDs.
pub mod containers;
pub mod flowing;
#[cfg(test)]
pub(crate) mod paper_fixture;

use rivet_registry::{
    block_state::BlockState, block_state_properties::BlockStateProperties as P,
    block_state_property::PropertyValue, fluid_id::FluidId, generated::blocks::BlockId,
};

#[derive(Debug, Clone, thiserror::Error)]
pub enum FluidError {
    #[error("fluid runtime authority unavailable: {0}")]
    Unavailable(String),
    #[error("invalid native fluid state: {0}")]
    InvalidState(String),
}
pub type FluidResult<T> = Result<T, FluidError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidKind {
    Water,
    Lava,
}
impl FluidKind {
    pub fn source_id(self) -> FluidId {
        match self {
            Self::Water => FluidId::WATER,
            Self::Lava => FluidId::LAVA,
        }
    }
    pub fn flowing_id(self) -> FluidId {
        FluidId(self.source_id().0 - 1)
    }
    pub fn tick_delay(self, fast_lava: bool) -> i32 {
        match self {
            Self::Water => 5,
            Self::Lava if fast_lava => 10,
            Self::Lava => 30,
        }
    }
    pub fn drop_off(self, fast_lava: bool) -> i32 {
        if self == Self::Lava && !fast_lava {
            2
        } else {
            1
        }
    }
    pub fn slope_distance(self, fast_lava: bool) -> i32 {
        if self == Self::Lava && !fast_lava {
            2
        } else {
            4
        }
    }
}

/// Empty, source and flowing are different fluid types, not merely an amount.
/// Flowing amount eight is still non-source and has FLOWING_* registry ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FluidState {
    Empty,
    Source {
        kind: FluidKind,
        falling: bool,
    },
    Flowing {
        kind: FluidKind,
        amount: std::num::NonZeroU8,
        falling: bool,
    },
}
impl FluidState {
    pub fn source(kind: FluidKind, falling: bool) -> Self {
        Self::Source { kind, falling }
    }
    pub fn flowing(kind: FluidKind, amount: u8, falling: bool) -> FluidResult<Self> {
        if !(1..=8).contains(&amount) {
            return Err(FluidError::InvalidState(format!("flowing amount {amount}")));
        }
        Ok(Self::Flowing {
            kind,
            amount: std::num::NonZeroU8::new(amount).unwrap(),
            falling,
        })
    }
    pub fn kind(self) -> Option<FluidKind> {
        match self {
            Self::Empty => None,
            Self::Source { kind, .. } | Self::Flowing { kind, .. } => Some(kind),
        }
    }
    pub fn amount(self) -> u8 {
        match self {
            Self::Empty => 0,
            Self::Source { .. } => 8,
            Self::Flowing { amount, .. } => amount.get(),
        }
    }
    pub fn is_source(self) -> bool {
        matches!(self, Self::Source { .. })
    }
    pub fn is_empty(self) -> bool {
        self == Self::Empty
    }
    pub fn falling(self) -> bool {
        match self {
            Self::Empty => false,
            Self::Source { falling, .. } | Self::Flowing { falling, .. } => falling,
        }
    }
    pub fn id(self) -> FluidId {
        match self {
            Self::Empty => FluidId::EMPTY,
            Self::Source { kind, .. } => kind.source_id(),
            Self::Flowing { kind, .. } => kind.flowing_id(),
        }
    }
    pub fn own_height(self) -> f32 {
        f32::from(self.amount()) / 9.0
    }
    pub fn from_block(state: BlockState) -> FluidResult<Self> {
        let (kind, source) = match state.fluid_id() {
            0 => return Ok(Self::Empty),
            1 => (FluidKind::Water, false),
            2 => (FluidKind::Water, true),
            3 => (FluidKind::Lava, false),
            4 => (FluidKind::Lava, true),
            other => {
                return Err(FluidError::InvalidState(format!(
                    "unknown fluid id {other}"
                )));
            }
        };
        if source {
            // WaterloggedTransparentBlock.getFluidState deliberately asks
            // WATER.getSource(true); source does not imply FALLING=false.
            return Ok(Self::source(
                kind,
                containers::behavior(state)?.falling_source,
            ));
        }
        let Some(PropertyValue::Int(level)) = state.get_value(P::LEVEL) else {
            return Err(FluidError::InvalidState(format!(
                "flowing block lacks liquid level: {}",
                state.block().name()
            )));
        };
        if level >= 8 {
            Self::flowing(kind, 8, true)
        } else {
            Self::flowing(kind, (8 - level) as u8, false)
        }
    }
    pub fn legacy_block(self) -> BlockState {
        let Some(kind) = self.kind() else {
            return BlockState::of(BlockId::from_name("minecraft:air").unwrap());
        };
        let name = match kind {
            FluidKind::Water => "minecraft:water",
            FluidKind::Lava => "minecraft:lava",
        };
        let level = if self.is_source() {
            0
        } else {
            8 - i32::from(self.amount().min(8)) + if self.falling() { 8 } else { 0 }
        };
        BlockState::of(BlockId::from_name(name).unwrap())
            .set_value(P::LEVEL, level)
            .expect("native liquid level")
    }
}
