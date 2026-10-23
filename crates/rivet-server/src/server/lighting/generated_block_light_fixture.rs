//! Actual BlockStarLightEngine.light output, including both sides of edges.
use super::{LightChunk, SkyLightProvider};
use crate::server::level::level_chunk::{StateId, container_factory, state_flags};
use rivet_registry::{block_state::BlockState, core::ChunkPos};
use rivet_world::{
    chunk::{chunk_access::ChunkAccess, upgrade_data::UpgradeData},
    level::height_accessor::{LevelHeightAccessor, create},
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub(super) fn air(pos: ChunkPos) -> LightChunk {
    let height = create(-64, 384);
    ChunkAccess::new(
        pos,
        UpgradeData::empty(height.get_sections_count() as usize),
        height,
        &container_factory(),
        0,
        None,
        &|state: &StateId| state_flags(*state),
    )
}
pub(super) fn empty(chunk: &LightChunk) -> Vec<Option<bool>> {
    chunk
        .get_sections()
        .iter()
        .map(|section| Some(section.has_only_air()))
        .collect()
}

pub(super) fn write(chunk: &mut LightChunk, x: i32, y: i32, z: i32, state: StateId) {
    let section = chunk.get_section_index(y) as usize;
    chunk.get_section_mut(section).set_block_state(
        x & 15,
        y & 15,
        z & 15,
        state,
        &|id| BlockState::new(*id).is_air(),
        &|id| BlockState::new(*id).random_ticking(),
        &|id| BlockState::new(*id).fluid_empty(),
        &|_| false,
        &|_| false,
    );
}

#[test]
fn generated_block_light_matches_real_paper_engine() {
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/generated-block-light.jsonl")
            .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let scenario = row["scenario"].as_str().unwrap();
        let mut chunks: HashMap<_, _> = row["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chunk| {
                let x = i32::try_from(chunk["x"].as_i64().unwrap()).unwrap();
                let z = i32::try_from(chunk["z"].as_i64().unwrap()).unwrap();
                ((x, z), air(ChunkPos::new(x, z)))
            })
            .collect();
        for cell in row["blocks"].as_array().unwrap() {
            let number = |i: usize| i32::try_from(cell[i].as_i64().unwrap()).unwrap();
            let (x, y, z) = (number(0), number(1), number(2));
            let chunk = chunks.get_mut(&(x >> 4, z >> 4)).unwrap();
            write(chunk, x, y, z, StateId(u16::try_from(number(3)).unwrap()));
        }
        let center = chunks.remove(&(0, 0)).unwrap();
        if matches!(
            scenario,
            "lit_neighbor" | "low_status_lit_neighbor" | "edge_source"
        ) {
            let mut neighbor = chunks.remove(&(1, 0)).unwrap();
            let mut provider =
                SkyLightProvider::with_owned_storage(create(-64, 384), false, true, chunks);
            let sections = empty(&neighbor);
            provider.light_chunk_with(&mut neighbor, &sections).unwrap();
            neighbor.set_persisted_status(if scenario == "low_status_lit_neighbor" {
                rivet_world::chunk::status::ChunkStatus::Features
            } else {
                rivet_world::chunk::status::ChunkStatus::Light
            });
            neighbor.set_light_correct(true);
            chunks = provider.into_owned_storage().unwrap();
            chunks.insert((1, 0), neighbor);
        }
        let mut provider =
            SkyLightProvider::with_owned_storage(create(-64, 384), false, true, chunks);
        let mut center = center;
        let sections = empty(&center);
        provider.light_chunk_with(&mut center, &sections).unwrap();
        let mut actual = provider.into_owned_storage().unwrap();
        actual.insert((0, 0), center);
        for expected in row["chunks"].as_array().unwrap() {
            let key = (
                expected["x"].as_i64().unwrap() as i32,
                expected["z"].as_i64().unwrap() as i32,
            );
            let chunk = &actual[&key];
            assert_eq!(
                chunk.block_nibbles().len(),
                expected["sections"].as_array().unwrap().len(),
                "section count {scenario} {key:?}"
            );
            assert_eq!(
                serde_json::to_value(chunk.block_emptiness_map()).unwrap(),
                expected["emptiness"],
                "emptiness {scenario} {key:?}"
            );
            for (index, (nibble, expected)) in chunk
                .block_nibbles()
                .iter()
                .zip(expected["sections"].as_array().unwrap())
                .enumerate()
            {
                let state = if nibble.is_null_nibble_visible() {
                    0
                } else if nibble.is_uninitialised_visible() {
                    1
                } else if nibble.is_initialised_visible() {
                    2
                } else {
                    3
                };
                assert_eq!(
                    state,
                    expected["state"].as_i64().unwrap(),
                    "state {scenario} {key:?} section{index}"
                );
                let values: Vec<_> = (0..4096)
                    .map(|i| nibble.get_visible_index(i) as u8)
                    .collect();
                let hash = format!("{:x}", Sha256::digest(values));
                assert_eq!(
                    hash,
                    expected["sha256"].as_str().unwrap(),
                    "values {scenario} {key:?} section{index}"
                );
            }
        }
    }
}

#[test]
fn conditional_block_occlusion_refuses_without_publishing_either_channel_and_can_retry() {
    let state = |name: &str| {
        BlockState::of(rivet_registry::generated::blocks::BlockId::from_name(name).unwrap()).id()
    };
    let mut chunk = air(ChunkPos::ZERO);
    write(&mut chunk, 8, 64, 8, state("minecraft:torch"));
    write(&mut chunk, 9, 64, 8, state("minecraft:stone_slab"));
    let mut provider =
        SkyLightProvider::with_owned_storage(create(-64, 384), true, true, HashMap::new());
    let sections = empty(&chunk);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        provider.light_chunk_with(&mut chunk, &sections)
    }));
    assert!(
        result.is_err(),
        "missing occlusion geometry must not publish approximated light"
    );
    assert!(chunk.sky_emptiness_map().is_none());
    assert!(chunk.block_emptiness_map().is_none());
    assert!(
        chunk
            .sky_nibbles()
            .iter()
            .all(|n| n.is_null_nibble_visible())
    );
    assert!(
        chunk
            .block_nibbles()
            .iter()
            .all(|n| n.is_null_nibble_visible())
    );
    write(&mut chunk, 9, 64, 8, state("minecraft:air"));
    let sections = empty(&chunk);
    provider.light_chunk_with(&mut chunk, &sections).unwrap();
    assert!(chunk.sky_emptiness_map().is_some());
    assert!(chunk.block_emptiness_map().is_some());
}
