//! Full pinned block registry declaring-owner classification. Do not infer
//! LiquidBlockContainer from a name suffix or presence of WATERLOGGED alone.
use super::{FluidError, FluidResult, FluidState, flowing::FluidWorld};
use rivet_registry::{
    block_state::BlockState, block_state_properties::BlockStateProperties as P,
    block_state_property::PropertyValue, core::BlockPos, fluid_id::FluidId,
};
use std::{collections::HashMap, sync::LazyLock};

#[derive(Debug)]
pub struct FluidBlockBehavior {
    pub can_place_owner: &'static str,
    pub place_owner: &'static str,
    pub excluded: bool,
    pub falling_source: bool,
}
impl FluidBlockBehavior {
    pub fn is_container(&self) -> bool {
        self.can_place_owner != "-"
    }
}

pub fn behavior(state: BlockState) -> FluidResult<&'static FluidBlockBehavior> {
    static DATA: LazyLock<HashMap<&'static str, FluidBlockBehavior>> = LazyLock::new(|| {
        include_str!("block_behaviors.tsv")
            .lines()
            .map(|line| {
                let mut row = line.split('\t');
                let name = row.next().expect("blockname");
                let can_place_owner = row.next().expect("canPlaceLiquid owner");
                let place_owner = row.next().expect("placeLiquid owner");
                let excluded = row.next().expect("excluded flag") == "1";
                let falling_source = row.next().expect("falling source flag") == "1";
                (
                    name,
                    FluidBlockBehavior {
                        can_place_owner,
                        place_owner,
                        excluded,
                        falling_source,
                    },
                )
            })
            .collect()
    });
    DATA.get(state.block().name()).ok_or_else(|| {
        FluidError::Unavailable(format!("unclassified fluid block {}", state.block().name()))
    })
}

pub fn can_place(state: BlockState, fluid: FluidId) -> FluidResult<bool> {
    let owner = behavior(state)?.can_place_owner;
    match owner {
        "SimpleWaterloggedBlock" => Ok(fluid == FluidId::WATER),
        "SlabBlock" => Ok(
            state.get_value(P::SLAB_TYPE) != Some(PropertyValue::Enum("double"))
                && fluid == FluidId::WATER,
        ),
        // FlowingFluid passes a null user, never a creative player.
        "BarrierBlock" | "KelpBlock" | "KelpPlantBlock" | "SeagrassBlock" | "TallSeagrassBlock" => {
            Ok(false)
        }
        "-" => Err(FluidError::InvalidState(
            "canPlaceLiquid on non-container".into(),
        )),
        other => Err(FluidError::Unavailable(format!("{other}.canPlaceLiquid"))),
    }
}

/// Genuine complete paths of SimpleWaterloggedBlock, SlabBlock and unlit
/// Candle/Campfire. The effectful lit/dried-ghast overrides need their real
/// sound/game-event/particle/body authority, never an empty successful hook.
pub fn place_standard(
    world: &mut impl FluidWorld,
    pos: BlockPos,
    state: BlockState,
    fluid: FluidState,
) -> FluidResult<bool> {
    let owner = behavior(state)?.place_owner;
    match owner {
        "KelpBlock" | "KelpPlantBlock" | "SeagrassBlock" | "TallSeagrassBlock" => return Ok(false),
        "SlabBlock" if state.get_value(P::SLAB_TYPE) == Some(PropertyValue::Enum("double")) => {
            return Ok(false);
        }
        "SimpleWaterloggedBlock"
        | "SlabBlock"
        | "CandleBlock"
        | "CampfireBlock"
        | "DriedGhastBlock" => {}
        other => return Err(FluidError::Unavailable(format!("{other}.placeLiquid"))),
    }
    if state.get_value(P::WATERLOGGED) != Some(PropertyValue::Bool(false))
        || fluid.id() != FluidId::WATER
    {
        return Ok(false);
    }
    if owner == "DriedGhastBlock"
        || matches!(owner, "CandleBlock" | "CampfireBlock")
            && state.get_value(P::LIT) == Some(PropertyValue::Bool(true))
    {
        return Err(FluidError::Unavailable(format!(
            "{owner}.placeLiquid requires native extinguish/sound/game-event effects"
        )));
    }
    let state = state
        .set_value(P::WATERLOGGED, true)
        .expect("container WATERLOGGED");
    world.set_block(pos, state, 3)?;
    world.schedule(pos, fluid.id(), 5)?;
    Ok(true)
}
