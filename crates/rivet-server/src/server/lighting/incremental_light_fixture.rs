//! Real Paper blocksChangedInChunk on retained sky/block buffers.
use super::generated_block_light_fixture::{air, empty, write};
use super::{LightChunk, SkyLightProvider};
use crate::server::level::level_chunk::StateId;
use rivet_registry::core::{BlockPos, ChunkPos, SectionPos};
use rivet_world::chunk::status::ChunkStatus;
use rivet_world::level::height_accessor::create;
use rivet_world::lighting::star_light_provider::StarLightProvider;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[test]
fn canonical_incremental_light_refreshes_the_served_packet_cache() {
    use super::LiveLightRuntime;
    use crate::server::level::chunk_map::ChunkMap;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::generated::blocks::BlockId;
    let mut chunks = ChunkMap::new(ChunkPos::ZERO, 4);
    let chunk = chunks.get_chunk_mut(ChunkPos::ZERO).unwrap();
    let base = chunk.generation_base_mut();
    let sections = empty(base);
    SkyLightProvider::with_owned_storage(create(-64, 384), true, true, HashMap::new())
        .light_chunk_with(base, &sections)
        .unwrap();
    base.set_persisted_status(ChunkStatus::Full);
    base.set_light_correct(true);
    chunk.refresh_light_data();
    let before = chunk.light_data();
    let torch = BlockState::of(BlockId::from_name("minecraft:torch").unwrap()).id();
    write(chunk.generation_base_mut(), 8, 64, 8, torch);
    let mut runtime = LiveLightRuntime::new(create(-64, 384), true, true);
    runtime.section_change(SectionPos::of(0, 4, 0), false);
    runtime.block_change(BlockPos::new(8, 64, 8));
    assert_eq!(runtime.run_updates(&mut chunks).unwrap(), [ChunkPos::ZERO]);
    assert!(!runtime.has_pending_updates());
    let chunk = chunks.get_chunk(ChunkPos::ZERO).unwrap();
    assert_eq!(
        chunk.generation_base().block_nibbles()[9].get_visible(8, 64, 8),
        14
    );
    assert_eq!(
        chunk.generation_base().block_nibbles()[9].get_visible(9, 64, 8),
        13
    );
    assert_ne!(
        chunk.light_data(),
        before,
        "packet cache still contains initial darkness"
    );
    assert!(runtime.run_updates(&mut chunks).unwrap().is_empty());
}

#[test]
fn incremental_sky_and_block_changes_match_real_paper() {
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/incremental-light.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let coords: Vec<_> = row["chunks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|chunk| {
                (
                    chunk["x"].as_i64().unwrap() as i32,
                    chunk["z"].as_i64().unwrap() as i32,
                )
            })
            .collect();
        let mut chunks: HashMap<_, _> = coords
            .iter()
            .copied()
            .map(|(x, z)| ((x, z), air(ChunkPos::new(x, z))))
            .collect();
        let put = |chunks: &mut HashMap<(i32, i32), LightChunk>, cell: &serde_json::Value| {
            let x = cell[0].as_i64().unwrap() as i32;
            let y = cell[1].as_i64().unwrap() as i32;
            let z = cell[2].as_i64().unwrap() as i32;
            write(
                chunks.get_mut(&(x >> 4, z >> 4)).unwrap(),
                x,
                y,
                z,
                StateId(cell[3].as_u64().unwrap() as u16),
            );
            BlockPos::new(x, y, z)
        };
        for cell in row["initial"].as_array().unwrap() {
            put(&mut chunks, cell);
        }
        // Initial generation establishes the retained buffers. Only the final
        // mutation pass uses the incremental API; it never calls lightChunk.
        for key in &coords {
            let mut chunk = chunks.remove(key).unwrap();
            let mut provider =
                SkyLightProvider::with_owned_storage(create(-64, 384), true, true, chunks);
            let sections = empty(&chunk);
            provider.light_chunk_with(&mut chunk, &sections).unwrap();
            chunk.set_persisted_status(ChunkStatus::Full);
            chunk.set_light_correct(true);
            chunks = provider.into_owned_storage().unwrap();
            chunks.insert(*key, chunk);
        }
        let positions: Vec<_> = row["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| put(&mut chunks, cell))
            .collect();
        let mut provider =
            SkyLightProvider::with_owned_storage(create(-64, 384), true, true, chunks);
        if row["block_changes"].as_bool().unwrap() {
            for pos in positions {
                provider.block_change(pos);
            }
        }
        for (index, value) in row["changed_sections"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            if let Some(empty) = value.as_bool() {
                provider.section_change(SectionPos::of(0, index as i32 - 4, 0), empty);
            }
        }
        let changed = provider.try_run_updates().unwrap();
        let mut expected_notifications: Vec<_> = row["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| {
                (
                    event[1].as_i64().unwrap() as i32,
                    event[3].as_i64().unwrap() as i32,
                )
            })
            .collect();
        expected_notifications.sort_unstable();
        expected_notifications.dedup();
        let mut actual_notifications: Vec<_> =
            changed.iter().map(|pos| (pos.x(), pos.z())).collect();
        actual_notifications.sort_unstable();
        assert_eq!(
            actual_notifications, expected_notifications,
            "source onLightUpdate chunks {}",
            row["name"]
        );
        if row["name"] == "unchanged" {
            assert!(changed.is_empty());
        }
        let actual = provider.into_owned_storage().unwrap();
        for expected in row["chunks"].as_array().unwrap() {
            let key = (
                expected["x"].as_i64().unwrap() as i32,
                expected["z"].as_i64().unwrap() as i32,
            );
            let chunk = &actual[&key];
            assert_eq!(
                serde_json::to_value(chunk.sky_emptiness_map()).unwrap(),
                expected["sky_emptiness"],
                "sky map {}",
                row["name"]
            );
            assert_eq!(
                serde_json::to_value(chunk.block_emptiness_map()).unwrap(),
                expected["block_emptiness"],
                "block map {}",
                row["name"]
            );
            for (channel, nibbles) in [
                ("sky", chunk.sky_nibbles()),
                ("block", chunk.block_nibbles()),
            ] {
                for (index, (nibble, expected)) in nibbles
                    .iter()
                    .zip(expected[channel].as_array().unwrap())
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
                        "{} {key:?} {channel} section {index}",
                        row["name"]
                    );
                    let values: Vec<_> = (0..4096)
                        .map(|i| nibble.get_visible_index(i) as u8)
                        .collect();
                    assert_eq!(
                        format!("{:x}", Sha256::digest(values)),
                        expected["sha256"].as_str().unwrap(),
                        "{} {key:?} {channel} section {index}",
                        row["name"]
                    );
                }
            }
        }
    }
}
