//! Paper AttachedToLeavesDecorator: randomized leaves with exclusion zones.

use super::{TreeDecorator, TreeDecoratorContext, shuffle};
use crate::levelgen::feature::stateproviders::block_state_provider::{
    ErasedBlockStateProvider, block_state_provider_codec, block_state_provider_get_state,
};
use rivet_registry::core::{BlockPos, Direction, direction_codec};
use rivet_registry::registry_ops::RegistryOpsLookup;
use rivet_serialization::{
    codec,
    dynamic_ops::DynamicOps,
    map_codec::{self, MapCodec},
    record_builder::{self, RecordCodecBuilder},
};
use rivet_util::RandomSource;
use std::{collections::HashSet, sync::Arc};

#[derive(Debug, Clone)]
pub struct AttachedToLeavesDecorator {
    pub probability: f32,
    pub exclusion_radius_xz: i32,
    pub exclusion_radius_y: i32,
    pub block_provider: Arc<dyn ErasedBlockStateProvider>,
    pub required_empty_blocks: i32,
    pub directions: Vec<Direction>,
}

impl AttachedToLeavesDecorator {
    pub fn place<R: RandomSource>(&self, context: &mut TreeDecoratorContext<'_, R>) {
        let mut blacklist = HashSet::new();
        let mut leaves = context.leaves.clone();
        shuffle(&mut leaves, context.random);
        for leaf in leaves {
            let direction = self.directions
                [context.random.next_int_bound(self.directions.len() as i32) as usize];
            let pos = leaf.relative(&direction);
            if !blacklist.contains(&pos)
                && context.random.next_float() < self.probability
                && (1..=self.required_empty_blocks)
                    .all(|distance| context.is_air(&leaf.relative_steps(&direction, distance)))
            {
                let corner1 = pos.offset(
                    -self.exclusion_radius_xz,
                    -self.exclusion_radius_y,
                    -self.exclusion_radius_xz,
                );
                let corner2 = pos.offset(
                    self.exclusion_radius_xz,
                    self.exclusion_radius_y,
                    self.exclusion_radius_xz,
                );
                blacklist.extend(BlockPos::between_closed_iter(&corner1, &corner2));
                let state = block_state_provider_get_state(
                    self.block_provider.as_ref(),
                    context.level,
                    context.random,
                    &pos,
                );
                context.set_block(&pos, state);
            }
        }
    }
}

pub(super) fn attached_to_leaves_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn MapCodec<TreeDecorator, Ops>> {
    let inner = record_builder::map_codec(|i| {
        i.group(RecordCodecBuilder::of(
            Arc::new(|d: &AttachedToLeavesDecorator| d.probability),
            codec::field_of(
                codec::float_range::<Ops>(0.0, 1.0),
                "probability".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &AttachedToLeavesDecorator| d.exclusion_radius_xz),
            codec::field_of(
                codec::int_range::<Ops>(0, 16),
                "exclusion_radius_xz".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &AttachedToLeavesDecorator| d.exclusion_radius_y),
            codec::field_of(
                codec::int_range::<Ops>(0, 16),
                "exclusion_radius_y".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &AttachedToLeavesDecorator| d.block_provider.clone()),
            codec::field_of(
                block_state_provider_codec::<Ops>(),
                "block_provider".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &AttachedToLeavesDecorator| d.required_empty_blocks),
            codec::field_of(
                codec::int_range::<Ops>(1, 16),
                "required_empty_blocks".to_string(),
            ),
        ))
        .and(RecordCodecBuilder::of(
            Arc::new(|d: &AttachedToLeavesDecorator| d.directions.clone()),
            codec::field_of(
                codec::list_with_range(direction_codec::<Ops>(), 1, i32::MAX),
                "directions".to_string(),
            ),
        ))
        .apply(
            i,
            Arc::new(
                |probability,
                 exclusion_radius_xz,
                 exclusion_radius_y,
                 block_provider,
                 required_empty_blocks,
                 directions| {
                    AttachedToLeavesDecorator {
                        probability,
                        exclusion_radius_xz,
                        exclusion_radius_y,
                        block_provider,
                        required_empty_blocks,
                        directions,
                    }
                },
            ),
        )
    });
    map_codec::xmap(
        inner,
        Arc::new(|d: &AttachedToLeavesDecorator| TreeDecorator::AttachedToLeaves(d.clone())),
        Arc::new(|d: &TreeDecorator| match d {
            TreeDecorator::AttachedToLeaves(d) => d.clone(),
            _ => unreachable!("attached-to-leaves dispatch"),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rivet_registry::generated::feature_data::CONFIGURED_FEATURE_BY_NAME;
    use rivet_registry::{access::RegistryAccess, registry_ops::RegistryOps};
    use rivet_serialization::json_ops::JsonOps;

    #[test]
    fn actual_mangrove_attachment_config_decodes_and_enforces_source_ranges() {
        let entry = CONFIGURED_FEATURE_BY_NAME
            .get("minecraft:mangrove")
            .expect("real mangrove config");
        let tree: serde_json::Value = serde_json::from_str(entry.json).unwrap();
        let config = tree["config"]["decorators"]
            .as_array()
            .unwrap()
            .iter()
            .find(|config| config["type"] == "minecraft:attached_to_leaves")
            .expect("mangrove's propagule attachment decorator");
        let ops = RegistryOps::create_from_access(&JsonOps::INSTANCE, RegistryAccess::empty());
        let codec = super::super::tree_decorator_codec();
        assert!(matches!(
            codec.parse(&ops, config).result(),
            Some(TreeDecorator::AttachedToLeaves(_))
        ));
        for (field, value) in [
            ("probability", serde_json::json!(1.1)),
            ("exclusion_radius_xz", serde_json::json!(17)),
            ("exclusion_radius_y", serde_json::json!(-1)),
            ("required_empty_blocks", serde_json::json!(0)),
            ("directions", serde_json::json!([])),
        ] {
            let mut invalid = config.clone();
            invalid[field] = value;
            assert!(
                codec.parse(&ops, &invalid).is_error(),
                "invalid {field} must not decode"
            );
        }
    }
}
