//! CreakingHeartDecorator: replace a shuffled fully log-enclosed trunk cell.
use super::{TreeDecorator, TreeDecoratorContext, shuffle};
use rivet_registry::{
    block_state::BlockState,
    block_state_properties::{BlockStateProperties, CreakingHeartState},
    core::Direction,
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec,
    dynamic_ops::DynamicOps,
    map_codec::{self, MapCodec},
};
use rivet_util::RandomSource;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct CreakingHeartDecorator {
    pub probability: f32,
}
impl CreakingHeartDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        if context.logs.is_empty() || context.random.next_float() >= self.probability {
            return;
        }
        let mut positions = context.logs.clone();
        shuffle(&mut positions, context.random);
        if let Some(pos) = positions.into_iter().find(|pos| {
            Direction::VALUES.iter().all(|direction| {
                context
                    .level
                    .get_block_state(&pos.relative(direction))
                    .is_in_tag("minecraft:logs")
            })
        }) {
            let state = BlockState::of(
                rivet_registry::generated::blocks::BlockId::from_name("minecraft:creaking_heart")
                    .expect("creaking heart registered"),
            )
            .set_value(
                BlockStateProperties::CREAKING_HEART_STATE,
                CreakingHeartState::Dormant,
            )
            .expect("heart state")
            .set_value(BlockStateProperties::NATURAL, true)
            .expect("natural heart");
            context.set_block(&pos, state);
        }
    }
}
pub(super) fn creaking_heart_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<TreeDecorator, Ops>> {
    map_codec::xmap(
        codec::field_of(
            codec::float_range::<Ops>(0.0, 1.0),
            "probability".to_string(),
        ),
        Arc::new(|probability: &f32| {
            TreeDecorator::CreakingHeart(CreakingHeartDecorator {
                probability: *probability,
            })
        }),
        Arc::new(|d: &TreeDecorator| match d {
            TreeDecorator::CreakingHeart(d) => d.probability,
            _ => unreachable!("creaking heart dispatch"),
        }),
    )
}
