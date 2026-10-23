use super::*;
use crate::server::level::{generated_world::OverworldGenerator, level_chunk::LevelChunk};
use rivet_world::block::Block;
use rivet_world::chunk::status::ChunkStatus;
use serde_json::Value;
use std::sync::Arc;

const POS: BlockPos = BlockPos::new(8, 64, 8);
const PLAYER: Uuid = Uuid { most: 7, least: 9 };

fn chunks(row: &Value) -> ChunkMap {
    let generator = Arc::new(OverworldGenerator::new(42));
    let mut state = generator.create_holder(ChunkPos::ZERO).into_state();
    let proto = state.chunk_mut();
    for y in 58..=64 {
        proto.set_block_state(
            8,
            y,
            8,
            Block::from_name("minecraft:air")
                .unwrap()
                .default_block_state(),
        );
    }
    let distance = row["distance"].as_i64().unwrap() as i32;
    if let Some(id) = row["obstacle"].as_i64().filter(|id| *id >= 0) {
        proto.set_block_state(
            8,
            64 - distance,
            8,
            BlockState::new(rivet_registry::generated::block_states::StateId(id as u16)),
        );
    }
    if row["fire_below"].as_bool().unwrap() {
        proto.set_block_state(
            8,
            63 - distance,
            8,
            Block::from_name("minecraft:campfire")
                .unwrap()
                .default_block_state(),
        );
    }
    proto.set_persisted_status(ChunkStatus::Spawn);
    let chunk = LevelChunk::from_generated_spawn_proto(state.into_loan().0).unwrap();
    let mut chunks = ChunkMap::empty(10);
    chunks.install(ChunkPos::ZERO, chunk);
    chunks
}
fn hive(bees: usize) -> NativeBlockEntity {
    let state = Block::from_name("minecraft:bee_nest")
        .unwrap()
        .default_block_state()
        .set_value(BlockStateProperties::LEVEL_HONEY, 3)
        .unwrap();
    let mut entity = NativeBlockEntity::fresh(POS, state).unwrap();
    let NativeBlockEntityData::Beehive(data) = entity.data_mut() else {
        unreachable!()
    };
    for index in 0..bees {
        data.store_worldgen_bee(7 + index as i32);
    }
    entity
}
fn hex(bytes: Vec<u8>) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[test]
fn actual_paper_registration_hive_payload_smoke_and_packet_bytes() {
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/block-entity-debug.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        match row["mode"].as_str().unwrap() {
            "registry" => assert_eq!(row["names"], serde_json::json!(DebugSubscription::NAMES)),
            "registration" => {
                let block = match row["family"].as_str().unwrap() {
                    "chest" => Block::from_name("minecraft:chest").unwrap(),
                    "spawner" => Block::from_name("minecraft:spawner").unwrap(),
                    "heart" => Block::from_name("minecraft:creaking_heart").unwrap(),
                    "hive" => Block::from_name("minecraft:bee_nest").unwrap(),
                    _ => panic!("family"),
                };
                let entity = NativeBlockEntity::fresh(POS, block.default_block_state()).unwrap();
                let mut sync = LevelDebugSynchronizers::new();
                sync.register_block_entity(&entity).unwrap();
                assert_eq!(
                    sync.registered_hive_count() as u64,
                    row["sleeping_count"].as_u64().unwrap()
                );
                let mut subscribers = ServerDebugSubscribers::default();
                // Awake registration is NOT gated by this hive subscription.
                let unrelated = HashSet::from([DebugSubscription::NeighborUpdates]);
                subscribers.tick([(PLAYER, &unrelated)]);
                assert_eq!(sync.begin_tick(&subscribers), DebugTickTransition::Wake);
                sync.register_block_entity(&entity).unwrap();
                assert_eq!(
                    sync.registered_hive_count() as u64,
                    row["awake_count"].as_u64().unwrap()
                );
                sync.register_block_entity(&entity).unwrap();
                assert_eq!(
                    sync.registered_hive_count() as u64,
                    row["repeat_count"].as_u64().unwrap()
                );
                subscribers.tick([]);
                assert_eq!(sync.begin_tick(&subscribers), DebugTickTransition::Sleeping);
                assert_eq!(
                    sync.is_sleeping(),
                    row["sleep_after_tick"].as_bool().unwrap()
                );
                assert_eq!(
                    sync.registered_hive_count() as u64,
                    row["count_after_sleep_tick"].as_u64().unwrap()
                );
            }
            "hive" => {
                let chunks = chunks(&row);
                let entity = hive(row["bees"].as_u64().unwrap() as usize);
                let value = pack_hive(&entity, &chunks).unwrap();
                assert_eq!(
                    i64::from(value.block_id),
                    row["block"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    i64::from(value.occupant_count),
                    row["occupants"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    i64::from(value.honey_level),
                    row["honey"].as_i64().unwrap(),
                    "{row}"
                );
                assert_eq!(value.sedated, row["sedated"].as_bool().unwrap(), "{row}");
                assert_eq!(
                    hex(HiveDebugUpdate {
                        position: POS,
                        value: Some(value)
                    }
                    .encode_body()),
                    row["packet"].as_str().unwrap()
                );
                assert_eq!(
                    hex(HiveDebugUpdate {
                        position: POS,
                        value: None
                    }
                    .encode_body()),
                    row["clear_packet"].as_str().unwrap()
                );
            }
            _ => panic!("mode"),
        }
        cases += 1;
    }
    assert_eq!(cases, 13);
}

#[derive(Default)]
struct Transport(Vec<(Uuid, HiveDebugUpdate)>);
impl HiveDebugTransport for Transport {
    fn subscriber_order(&self, members: &HashSet<Uuid>) -> Result<Vec<Uuid>> {
        // A single real observer has no identity-order ambiguity.
        assert!(members.len() <= 1);
        Ok(members.iter().copied().collect())
    }
    fn is_chunk_tracked(&self, player: Uuid, chunk: ChunkPos) -> bool {
        player == PLAYER && chunk == ChunkPos::ZERO
    }
    fn send(&mut self, player: Uuid, update: HiveDebugUpdate) -> Result<()> {
        self.0.push((player, update));
        Ok(())
    }
}

#[test]
fn canonical_hive_change_cache_and_drop_send_actual_updates() {
    let row: Value = serde_json::from_str(
        include_str!("../../../../../tools/rivet-oracle/fixtures/block-entity-debug.jsonl")
            .lines()
            .find(|line| line.contains("\"name\":\"air\""))
            .unwrap(),
    )
    .unwrap();
    let mut chunks = chunks(&row);
    let entity = hive(0);
    let mut sync = LevelDebugSynchronizers::new();
    let enabled = HashSet::from([DebugSubscription::BeeHives]);
    let mut subscribers = ServerDebugSubscribers::default();
    subscribers.tick([(PLAYER, &enabled)]);
    assert_eq!(sync.begin_tick(&subscribers), DebugTickTransition::Wake);
    sync.register_block_entity(&entity).unwrap();
    chunks
        .get_chunk_mut(ChunkPos::ZERO)
        .unwrap()
        .generation_base_mut()
        .set_block_entity(entity);
    let mut transport = Transport::default();
    for _ in 0..2 {
        sync.tick_hives(
            &chunks,
            [(PLAYER, &enabled, vec![ChunkPos::ZERO])],
            &mut transport,
        )
        .unwrap();
    }
    assert_eq!(transport.0.len(), 1, "unchanged source is not resent");
    let entity = chunks
        .get_chunk_mut(ChunkPos::ZERO)
        .unwrap()
        .generation_base_mut()
        .get_block_entity_mut(&POS)
        .unwrap();
    let NativeBlockEntityData::Beehive(hive) = entity.data_mut() else {
        unreachable!()
    };
    hive.store_worldgen_bee(12);
    sync.tick_hives(
        &chunks,
        [(PLAYER, &enabled, vec![ChunkPos::ZERO])],
        &mut transport,
    )
    .unwrap();
    assert_eq!(transport.0[1].1.value.unwrap().occupant_count, 1);
    sync.drop_block_entity(POS, &mut transport).unwrap();
    assert_eq!(transport.0[2].1.value, None);
    assert_eq!(sync.registered_hive_count(), 0);
}
