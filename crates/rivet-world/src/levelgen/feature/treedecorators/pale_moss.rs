//! PaleMossDecorator including the actual nested pale-moss patch branch.
use super::{TreeDecorator, TreeDecoratorContext, shuffle};
use rivet_registry::{
    block_state::BlockState, block_state_properties::BlockStateProperties, core::BlockPos,
    registry_ops::RegistryOpsLookup,
};
use rivet_serialization::{
    codec,
    dynamic_ops::DynamicOps,
    map_codec::{self, MapCodec},
    record_builder::{self, RecordCodecBuilder},
};
use rivet_util::RandomSource;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct PaleMossDecorator {
    pub leaves_probability: f32,
    pub trunk_probability: f32,
    pub ground_probability: f32,
}
impl PaleMossDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        let mut logs = context.logs.clone();
        shuffle(&mut logs, context.random);
        let Some(mut origin) = logs.first().copied() else {
            return;
        };
        // Collections.min retains the first shuffled position on equal Y.
        for pos in logs.iter().skip(1) {
            if pos.get_y() < origin.get_y() {
                origin = *pos;
            }
        }
        if context.random.next_float() < self.ground_probability {
            context.place_configured_feature("minecraft:pale_moss_patch", &origin.above());
        }
        // The shuffled copy only selects the ground origin. Hanging placements
        // use the original context lists, in Java's stable Y-sorted order.
        for pos in context.logs.clone() {
            if context.random.next_float() < self.trunk_probability {
                let below = pos.below();
                if context.is_air(&below) {
                    add_moss_hanger(&below, context);
                }
            }
        }
        for pos in context.leaves.clone() {
            if context.random.next_float() < self.leaves_probability {
                let below = pos.below();
                if context.is_air(&below) {
                    add_moss_hanger(&below, context);
                }
            }
        }
    }
}
fn add_moss_hanger<R: RandomSource>(origin: &BlockPos, context: &mut TreeDecoratorContext<'_, R>) {
    let state = BlockState::of(
        rivet_registry::generated::blocks::BlockId::from_name("minecraft:pale_hanging_moss")
            .expect("hanging moss registered"),
    );
    let mut pos = *origin;
    while context.is_air(&pos.below()) && context.random.next_float() >= 0.5 {
        context.set_block(
            &pos,
            state
                .set_value(BlockStateProperties::TIP, false)
                .expect("hanging moss tip"),
        );
        pos = pos.below();
    }
    context.set_block(
        &pos,
        state
            .set_value(BlockStateProperties::TIP, true)
            .expect("hanging moss tip"),
    );
}
pub(super) fn pale_moss_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<TreeDecorator, Ops>> {
    let inner = record_builder::map_codec(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|d: &PaleMossDecorator| d.leaves_probability),
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "leaves_probability".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &PaleMossDecorator| d.trunk_probability),
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "trunk_probability".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &PaleMossDecorator| d.ground_probability),
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "ground_probability".to_string(),
            ),
        ))
        .apply(
            i,
            Arc::new(
                |leaves_probability, trunk_probability, ground_probability| PaleMossDecorator {
                    leaves_probability,
                    trunk_probability,
                    ground_probability,
                },
            ),
        )
    });
    map_codec::xmap(
        inner,
        Arc::new(|d: &PaleMossDecorator| TreeDecorator::PaleMoss(*d)),
        Arc::new(|d: &TreeDecorator| match d {
            TreeDecorator::PaleMoss(d) => *d,
            _ => unreachable!("pale moss dispatch"),
        }),
    )
}
