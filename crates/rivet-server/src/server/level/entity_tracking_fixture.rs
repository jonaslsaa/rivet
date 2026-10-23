use super::*;
use rivet_util::random_source::LegacyRandomSource;
use rivet_world::entity::{
    chest_minecart::{EntityConstructionContext, NativeChestMinecart},
    farm_variants::{FarmFamily, FarmVariantRegistries},
    generation_creature::{
        NativeEntityIdentity, NativeFarmAnimal, NativeGenerationCreature, NativeSheep,
    },
};
use serde_json::{Value, json};
fn make_entity(row: &Value, registries: &FarmVariantRegistries) -> NativeLoadedEntity {
    let identity = NativeEntityIdentity {
        entity_id: 101,
        uuid: Uuid { most: 1, least: 2 },
        world_uuid: Uuid { most: 3, least: 4 },
    };
    let mut random = LegacyRandomSource::new(42);
    let family = row["family"].as_str().unwrap();
    let input = rivet_nbt::tag_parser::parse_compound_fully(
        row["input"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("{}"),
    )
    .unwrap();
    if family == "chest_minecart" {
        let mut e = NativeChestMinecart::from_identity(
            EntityConstructionContext {
                entity_id: 101,
                world_uuid: identity.world_uuid,
                experimental_minecart_movement: false,
            },
            identity.uuid,
        );
        if !row["input"].as_str().unwrap().is_empty() {
            e.load(&input).unwrap();
        }
        e.snap_to(Vec3::new(8.5, 64., 8.5), 123., 12.);
        NativeLoadedEntity::ChestMinecart(e)
    } else {
        let mut e = if family == "sheep" {
            NativeGenerationCreature::Sheep(NativeSheep::from_identity(identity, &mut random))
        } else {
            let family = match family {
                "cow" => FarmFamily::Cow,
                "pig" => FarmFamily::Pig,
                "chicken" => FarmFamily::Chicken,
                _ => unreachable!(),
            };
            NativeGenerationCreature::Farm(NativeFarmAnimal::from_identity(
                identity,
                registries.family(family),
                &mut random,
            ))
        };
        if !row["input"].as_str().unwrap().is_empty() {
            e.load(&input).unwrap();
        }
        e.snap_to(Vec3::new(8.5, 64., 8.5), 123., 12.);
        NativeLoadedEntity::Creature(e)
    }
}
fn packets(values: Vec<TrackingPacket>) -> Value {
    json!(values.into_iter().map(|packet|json!({"type":format!("minecraft:{}",packet.name),"body":packet.body.iter().map(|b|format!("{b:02x}")).collect::<String>()})).collect::<Vec<_>>())
}

#[derive(Default)]
struct Authority {
    players: Vec<TrackingPlayer>,
    events: Vec<(TrackingWorldEvent, bool, bool, bool)>,
    bundles: Vec<(i32, Vec<TrackingPacket>)>,
    sent: Vec<(i32, TrackingPacket)>,
    closed: Vec<i32>,
}
impl TrackingAuthority for Authority {
    fn observer_order(&mut self, _: &NativeLoadedEntity, members: &[i32]) -> Result<Vec<i32>> {
        Ok(members.iter().rev().copied().collect())
    }
    fn external_uuid_in_use(&self, _: &Uuid) -> bool {
        false
    }
    fn players(&mut self, _: &NativeLoadedEntity) -> Result<Vec<TrackingPlayer>> {
        Ok(self.players.clone())
    }
    fn event(
        &mut self,
        event: TrackingWorldEvent,
        e: &NativeLoadedEntity,
        _: Option<i32>,
    ) -> Result<bool> {
        let origin = e.save().unwrap().contains("Paper.Origin");
        self.events
            .push((event, e.lifecycle().valid, e.lifecycle().in_world, origin));
        Ok(true)
    }
    fn send_bundle(&mut self, id: i32, packets: &[TrackingPacket]) -> Result<()> {
        self.bundles.push((id, packets.to_vec()));
        Ok(())
    }
    fn send(&mut self, id: i32, packet: &TrackingPacket) -> Result<()> {
        self.sent.push((id, packet.clone()));
        Ok(())
    }
    fn debug_tracking(&mut self, _: &NativeLoadedEntity, _: Option<i32>, _: bool) -> Result<()> {
        Ok(())
    }
    fn close_inventory_viewers(&mut self, e: &NativeLoadedEntity) -> Result<()> {
        self.closed.push(e.entity_id());
        Ok(())
    }
}

#[test]
fn lookup_tracking_phases_pair_real_packets_and_preserve_origin_and_in_world_on_end() {
    use crate::server::level::{
        entity_id_allocator::EntityIdOccupancy, entity_lookup::EntityLookup,
        full_chunk_status::FullChunkStatus,
    };
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let entity = make_entity(&json!({"family":"sheep","input":""}), &registries);
    let config = EntityTrackingConfig {
        world_uuid: entity.world_uuid(),
        animal_range_blocks: 48,
        other_range_blocks: 64,
        animal_range_y: None,
        other_range_y: None,
    };
    let mut runtime = EntityTrackingRuntime::new(access, config);
    let mut authority = Authority {
        players: vec![TrackingPlayer {
            id: 9,
            position: Vec3::new(8.5, 64., 8.5),
            view_distance_chunks: 3,
            chunk_tracked: true,
            broadcast: true,
            can_see: true,
        }],
        ..Authority::default()
    };
    let mut lookup = EntityLookup::new(-4, 19, EntityIdOccupancy::default());
    let pos = rivet_registry::core::ChunkPos::ZERO;
    {
        let mut lifecycle = runtime.lifecycle(&mut authority);
        lookup.add_entity(entity, false, &mut lifecycle).unwrap();
        lookup
            .set_chunk_status(pos, FullChunkStatus::EntityTicking, &mut lifecycle)
            .unwrap();
    }
    assert_eq!(runtime.navigating_mobs(), &[101]);
    assert_eq!(runtime.ticking(), &[101]);
    assert_eq!(
        authority.events,
        vec![
            (TrackingWorldEvent::TrackPlayer, true, true, false),
            (TrackingWorldEvent::AddedToWorld, true, true, true)
        ]
    );
    assert_eq!(authority.bundles.len(), 1);
    assert_eq!(authority.bundles[0].0, 9);
    assert_eq!(
        authority.bundles[0]
            .1
            .iter()
            .map(|p| p.name)
            .collect::<Vec<_>>(),
        vec!["add_entity", "set_entity_data"]
    );
    {
        let mut lifecycle = runtime.lifecycle(&mut authority);
        lookup
            .set_chunk_status(pos, FullChunkStatus::Inaccessible, &mut lifecycle)
            .unwrap();
    }
    assert!(runtime.navigating_mobs().is_empty());
    assert!(runtime.ticking().is_empty());
    assert!(!runtime.contains(101));
    assert!(
        authority.closed.is_empty(),
        "animals are not inventory holders"
    );
    assert_eq!(
        authority.events[2..],
        [
            (TrackingWorldEvent::UntrackPlayer, true, true, true),
            (TrackingWorldEvent::RemovedFromWorld, false, true, true)
        ]
    );
    assert_eq!(authority.sent.len(), 1);
    assert_eq!(authority.sent[0].1.name, "remove_entities");
    assert_eq!(authority.sent[0].1.body, vec![1, 101]);
    let native = lookup.get_raw(101).unwrap();
    assert!(!native.lifecycle().valid);
    assert!(native.lifecycle().in_world);
    let saved = native.save().unwrap();
    assert!(saved.contains("Paper.Origin"));
    assert!(saved.contains("Paper.OriginWorld"));
}
#[test]
fn broadcasts_use_canonical_identity_set_order_and_hidden_unviewed_entities_do_not_advance() {
    use crate::server::level::{
        entity_id_allocator::EntityIdOccupancy, entity_lookup::EntityLookup,
        full_chunk_status::FullChunkStatus,
    };
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let entity = make_entity(&json!({"family":"cow","input":""}), &registries);
    let mut runtime = EntityTrackingRuntime::new(
        access,
        EntityTrackingConfig {
            world_uuid: entity.world_uuid(),
            animal_range_blocks: 48,
            other_range_blocks: 64,
            animal_range_y: None,
            other_range_y: None,
        },
    );
    let player = TrackingPlayer {
        id: 9,
        position: Vec3::new(8.5, 64., 8.5),
        view_distance_chunks: 3,
        chunk_tracked: true,
        broadcast: true,
        can_see: true,
    };
    let mut authority = Authority {
        players: vec![player, TrackingPlayer { id: 8, ..player }],
        ..Authority::default()
    };
    let mut lookup = EntityLookup::new(-4, 19, EntityIdOccupancy::default());
    {
        let mut lifecycle = runtime.lifecycle(&mut authority);
        lookup
            .set_chunk_status(
                rivet_registry::core::ChunkPos::ZERO,
                FullChunkStatus::Full,
                &mut lifecycle,
            )
            .unwrap();
        lookup.add_entity(entity, false, &mut lifecycle).unwrap();
    }
    assert_eq!(
        authority.bundles.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![9, 8],
        "initial pairing follows candidate player encounter order"
    );
    runtime
        .tick_entity(lookup.get_raw(101).unwrap(), false, &mut authority)
        .unwrap();
    assert_eq!(
        authority.sent.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![8, 9, 8, 9],
        "each broadcast uses supplied canonical identity-set order, not insertion order"
    );
    authority.players.clear();
    authority.sent.clear();
    let before = runtime.tracked[&101].server.tick_count;
    runtime
        .tick_entity(lookup.get_raw(101).unwrap(), false, &mut authority)
        .unwrap();
    assert_eq!(
        runtime.tracked[&101].server.tick_count, before,
        "unviewed FULL is not ENTITY_TICKING"
    );
    assert_eq!(
        authority.sent.iter().map(|p| p.0).collect::<Vec<_>>(),
        vec![8, 9],
        "purge uses canonical order"
    );
}

#[test]
fn actual_paper_pairing_metadata_and_server_entity_changes_all_native_families() {
    let access = rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries();
    let registries = FarmVariantRegistries::builtin(&access).unwrap();
    let mut cases = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/entity-tracking.jsonl").lines()
    {
        let row: Value = serde_json::from_str(line).unwrap();
        if let Some(types) = row["unknown_types"].as_array() {
            use rivet_protocol::codec::StreamDecoder;
            for value in types {
                let mut buf = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
                let b = buf.inner_mut();
                b.write_var_int(101);
                b.write_uuid(Uuid { most: 1, least: 2 });
                b.write_var_int(value["wire_id"].as_i64().unwrap() as i32);
                b.write_double(0.);
                b.write_double(0.);
                b.write_double(0.);
                b.write_bytes(&[0, 0, 0, 0, 0]);
                let packet = add_entity_codec().decode(&mut buf).unwrap();
                assert_eq!(
                    packet.entity_type_id,
                    value["resolved_id"].as_i64().unwrap() as i32
                );
                assert_eq!(buf.readable_bytes(), 0);
            }
        }
        if let Some(vectors) = row["lp_vectors"].as_array() {
            for vector in vectors {
                let xyz: [f64; 3] = serde_json::from_value(vector["input"].clone()).unwrap();
                let mut buf = FriendlyByteBuf::new(BytesMut::new());
                write_lp_vec3(&mut buf, Vec3::new(xyz[0], xyz[1], xyz[2]));
                assert_eq!(
                    buf.as_slice()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                    vector["body"].as_str().unwrap()
                );
                let decoded =
                    rivet_protocol::game::clientbound_add_entity_packet::read_lp_vec3(&mut buf);
                for (actual, expected) in [decoded.x, decoded.y, decoded.z]
                    .into_iter()
                    .zip(vector["decoded"].as_array().unwrap())
                {
                    assert_eq!(actual.to_bits(), expected.as_f64().unwrap().to_bits());
                }
                assert_eq!(buf.readable_bytes(), 0);
            }
        }
        let mut entity = make_entity(&row, &registries);
        let mut tracker = ServerEntityTracker::new(&entity.tracking_snapshot()).unwrap();
        assert_eq!(
            packets(tracker.pairing(&entity, &access).unwrap()),
            row["pairing"],
            "pairing {}",
            row["family"]
        );
        let mut changes = tracker
            .send_changes(101, &entity.tracking_snapshot())
            .unwrap();
        match &mut entity {
            NativeLoadedEntity::ChestMinecart(e) => e.snap_to(Vec3::new(9., 64., 8.5), 124., 12.),
            NativeLoadedEntity::Creature(e) => e.snap_to(Vec3::new(9., 64., 8.5), 124., 12.),
        }
        for _ in 0..3 {
            changes.extend(
                tracker
                    .send_changes(101, &entity.tracking_snapshot())
                    .unwrap(),
            );
        }
        tracker.on_player_add();
        changes.extend(
            tracker
                .send_changes(101, &entity.tracking_snapshot())
                .unwrap(),
        );
        assert_eq!(
            packets(changes),
            row["changes"],
            "changes {} input{}",
            row["family"],
            row["input"]
        );
        cases += 1;
    }
    assert_eq!(cases, 10);
}
