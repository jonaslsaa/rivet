//! RootPlacer dispatch root. Vanilla registers exactly MangroveRootPlacer.
use super::MangroveRootPlacer;
use crate::level::WorldGenLevel;
use rivet_registry::{block_state::BlockState, core::BlockPos, registry_ops::RegistryOpsLookup};
use rivet_serialization::{
    codec::{self, Codec},
    data_result::DataResult,
    dynamic_ops::DynamicOps,
    key_dispatch_codec, map_codec,
};
use rivet_util::RandomSource;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum RootPlacer {
    Mangrove(MangroveRootPlacer),
}
impl RootPlacer {
    pub fn get_trunk_origin<R: RandomSource>(&self, origin: &BlockPos, random: &mut R) -> BlockPos {
        match self {
            Self::Mangrove(p) => origin.above_steps(p.trunk_offset_y.sample(random)),
        }
    }
    pub fn place_roots<R: RandomSource>(
        &self,
        level: &dyn WorldGenLevel,
        setter: &mut dyn FnMut(&BlockPos, BlockState),
        random: &mut R,
        origin: &BlockPos,
        trunk_origin: &BlockPos,
    ) -> bool {
        match self {
            Self::Mangrove(p) => p.place_roots(level, setter, random, origin, trunk_origin),
        }
    }
}
pub fn root_placer_codec<Ops: DynamicOps + 'static + RegistryOpsLookup>()
-> Arc<dyn Codec<RootPlacer, Ops>> {
    map_codec::codec_of(key_dispatch_codec::dispatch_map(
        "type",
        codec::string_codec::<Ops>(),
        Arc::new(|_: &RootPlacer| {
            DataResult::success("minecraft:mangrove_root_placer".to_string())
        }),
        Arc::new(|name: &String| {
            if name != "minecraft:mangrove_root_placer" && name != "mangrove_root_placer" {
                return DataResult::error(format!("unknown root placer type {name}"));
            }
            DataResult::success(map_codec::xmap(
                super::mangrove_root_placer::mangrove_root_placer_map_codec::<Ops>(),
                Arc::new(|p: &MangroveRootPlacer| RootPlacer::Mangrove(p.clone())),
                Arc::new(|p: &RootPlacer| match p {
                    RootPlacer::Mangrove(p) => p.clone(),
                }),
            ))
        }),
    ))
}
