//! Actual region storage plus scheduler/transformer/constructor integration.
//! The controlled stone terrain and Legacy RNG replay the full corridor Java
//! fixture; this is not evidence for a generated FULL chunk or Paper seed42
//! FEATURES terrain. Production uses Xoroshiro, separately wired in the caller.
use super::*;
use crate::server::level::{
    entity_construction::{EntityConstructionRuntime, StructureMinecartFactory},
    entity_id_allocator::EntityIdAllocator,
    structure_generate_events::{StructureGenerateAuthority, StructureGeneratePayload},
    structure_spawn_events::{
        EventPriority, NativePluginMetadata, NativeStructureEventRegistry, StructureWorldIdentity,
        TracingStructureEventDiagnostics,
    },
};
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_registry::{ResourceKey, holder_lookup::HolderGetter, registries::BIOME};
use rivet_util::{mth::Uuid, random_source::LegacyRandomSource};
use rivet_world::{
    biome::fixed_biome_source::FixedBiomeSource,
    levelgen::structure::{
        PiecesContainer, StructurePiece, StructureStart, registry_keys::STRUCTURE,
    },
};
use sha2::{Digest, Sha256};

fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).unwrap())
}

fn controlled_region_inputs(
    generator: &Arc<OverworldGenerator>,
) -> (
    ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    FeatureWorkspace,
) {
    let biomes = generator.registry_access().lookup(&BIOME).unwrap();
    let plains = FixedBiomeSource::new(biomes.get_or_throw(&ResourceKey::create(
        &BIOME,
        Identifier::parse("minecraft:plains"),
    )));
    let workspace = FeatureWorkspace::new();
    let mut center = None;
    for x in -8..=8 {
        for z in -8..=8 {
            let pos = ChunkPos::new(x, z);
            let mut chunk = fresh_worldgen_chunk(pos, generator);
            // Explicit controlled fixture input, not an admission into the live
            // owner and not a claim that terrain generation produced this stone.
            chunk.set_persisted_status(if x.abs().max(z.abs()) <= 1 {
                ChunkStatus::Carvers
            } else {
                ChunkStatus::StructureStarts
            });
            if x.abs().max(z.abs()) <= 1 {
                chunk.fill_biomes_from_noise(
                    &plains,
                    generator.random_state().sampler(),
                    &|holder| WorldgenBiomeId(dense_biome_id(holder)),
                );
                for wx in -2..=4 {
                    for wz in -2..=11 {
                        if SectionPos::block_to_section_coord(wx) == x
                            && SectionPos::block_to_section_coord(wz) == z
                        {
                            for y in -64..=70 {
                                chunk.set_block_state(wx, y, wz, block("minecraft:stone"));
                            }
                        }
                    }
                }
            }
            if pos == ChunkPos::ZERO {
                center = Some(chunk);
            } else {
                workspace.insert(chunk);
            }
        }
    }
    (center.unwrap(), workspace)
}

fn runtime(
    generator: &Arc<OverworldGenerator>,
    registry: Arc<NativeStructureEventRegistry>,
) -> Arc<StructurePlacementRuntime> {
    let players = EntityIdAllocator::new();
    let occupied = Arc::new(HashSet::from([1]));
    let factory = StructureMinecartFactory::new(
        EntityConstructionRuntime::with_seed(players.sequence(), 42),
        Uuid {
            most: 0x1234_5678_9abc_def0,
            least: 0xfedc_ba98_7654_3210_u64 as i64,
        },
        rivet_world::flag::feature_flags::vanilla_set(),
        Arc::new(move |id| occupied.contains(&id)),
    );
    Arc::new(
        StructurePlacementRuntime::new(
            Arc::clone(generator),
            StructureWorldIdentity::new(42, Identifier::parse("minecraft:overworld")),
            StructureGenerateAuthority::registered_native(registry),
            Arc::new(factory),
        )
        .unwrap(),
    )
}

fn install_corridor(
    center: &mut ProtoChunk<BlockState, WorldgenBiomeId, StructureKey>,
    generator: &Arc<OverworldGenerator>,
) -> serde_json::Value {
    let row: serde_json::Value = serde_json::from_str(
        include_str!("../../../../../tools/rivet-oracle/fixtures/mineshaft-chest.jsonl").trim(),
    )
    .unwrap();
    let key = Identifier::parse("minecraft:mineshaft");
    let structure = generator
        .registry_access()
        .lookup(&STRUCTURE)
        .unwrap()
        .get_optional(&key)
        .unwrap();
    let piece =
        StructurePiece::load(&parse_compound_fully(row["input"].as_str().unwrap()).unwrap())
            .unwrap();
    center.set_start_for_structure(
        key.clone(),
        StructureStart::new(
            structure.native_definition(&key).unwrap(),
            ChunkPos::ZERO,
            0,
            PiecesContainer::new(vec![piece]),
        ),
    );
    center.add_reference_for_structure(key, ChunkPos::ZERO.pack() as u64);
    row
}

#[test]
fn canonical_feature_region_refuses_duplicate_center_without_transferring_either_owner() {
    let generator = Arc::new(OverworldGenerator::new(42));
    let mut center = fresh_worldgen_chunk(ChunkPos::ZERO, &generator);
    center.set_persisted_status(ChunkStatus::Carvers);
    center.set_block_state(0, 50, 0, block("minecraft:diamond_block"));
    let mut duplicate = fresh_worldgen_chunk(ChunkPos::ZERO, &generator);
    duplicate.set_block_state(0, 50, 0, block("minecraft:gold_block"));
    let workspace = FeatureWorkspace::new();
    workspace.insert(duplicate);
    assert!(matches!(
        compose_feature_region_canonical(&mut center, &generator, &workspace),
        Err(StructurePlacementError::DuplicateCanonicalChunk(
            ChunkPos::ZERO
        ))
    ));
    assert_eq!(
        center.get_block_state(0, 50, 0),
        block("minecraft:diamond_block")
    );
    let recovered = workspace
        .take(ChunkPos::ZERO)
        .expect("rejected duplicate retains its owner");
    assert_eq!(
        recovered.get_block_state(0, 50, 0),
        block("minecraft:gold_block")
    );
    assert_eq!(center.get_persisted_status(), ChunkStatus::Carvers);
}

#[test]
fn native_corridor_places_real_minecart_into_canonical_center_and_matches_paper_body_fixture() {
    let generator = Arc::new(OverworldGenerator::new(42));
    let (mut center, workspace) = controlled_region_inputs(&generator);
    let row = install_corridor(&mut center, &generator);
    let events = Arc::new(NativeStructureEventRegistry::new(
        std::thread::current().id(),
        Arc::new(TracingStructureEventDiagnostics),
    ));
    events.finish_loading();
    let runtime = runtime(&generator, events);
    let mut region = compose_feature_region_canonical(&mut center, &generator, &workspace).unwrap();
    assert!(
        workspace.is_empty(),
        "canonical dependencies are exclusively loaned, not copied"
    );
    let mut random = WorldgenRandom::new(LegacyRandomSource::new(0));
    // Actual source index of MINESHAFT is 1 in UNDERGROUND_STRUCTURES (3).
    // Calibrate the source's setFeatureSeed to the captured Legacy seed 23.
    runtime
        .place_step(&mut region, &mut random, 23 - 1 - 30_000, 3)
        .unwrap();
    let mut changed = Vec::new();
    for x in -2..=4 {
        for y in -64..=70 {
            for z in -2..=11 {
                let state = region.get_block_state(&BlockPos::new(x, y, z));
                if state != block("minecraft:stone") {
                    for n in [x, y, z, i32::from(state.id().0)] {
                        changed.extend_from_slice(&n.to_be_bytes());
                    }
                }
            }
        }
    }
    assert_eq!(
        format!("{:x}", Sha256::digest(changed)),
        row["states_sha256"].as_str().unwrap()
    );
    let saved = region
        .placement_start_entry(ChunkPos::ZERO, &Identifier::parse("minecraft:mineshaft"))
        .unwrap()
        .unwrap()
        .native()
        .unwrap();
    assert_eq!(
        saved.pieces().pieces()[0].create_tag(),
        parse_compound_fully(row["saved"].as_str().unwrap()).unwrap()
    );
    for (_, chunk) in region.into_owned_proto_entries() {
        assert!(workspace.put(chunk).is_ok());
    }
    assert_eq!(workspace.len(), 288);
    let expected: Vec<_> = row["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tag| parse_compound_fully(tag.as_str().unwrap()).unwrap())
        .collect();
    assert_eq!(center.get_entities(), expected);
    assert_eq!(
        center.get_persisted_status(),
        ChunkStatus::Carvers,
        "a structure subpass does not certify FEATURES"
    );
}

#[test]
fn native_feature_failure_restores_all_loans_and_retains_real_neighbor_partial_writes() {
    let generator = Arc::new(OverworldGenerator::new(42));
    let (mut center, workspace) = controlled_region_inputs(&generator);
    install_corridor(&mut center, &generator);
    let registry = Arc::new(NativeStructureEventRegistry::new(
        std::thread::current().id(),
        Arc::new(TracingStructureEventDiagnostics),
    ));
    let plugin = registry
        .enable_plugin(NativePluginMetadata {
            display_name: "failure-fixture".into(),
            authors: vec!["fixture".into()],
        })
        .unwrap();
    let marker = BlockPos::new(16, 55, 0);
    registry
        .register_generate_listener(
            plugin,
            EventPriority::Normal,
            Arc::new(move |_event: &StructureGeneratePayload, maps| {
                maps.set_block(
                    Identifier::parse("fixture:write_then_fail"),
                    Arc::new(move |limited, _, _, _| {
                        limited.set_block_data(marker, block("minecraft:gold_block"))?;
                        Err(
                            rivet_world::levelgen::structure::StructureError::PlacementUnavailable(
                                "injected transformer failure after real neighbor write".into(),
                            ),
                        )
                    }),
                );
                Ok(())
            }),
        )
        .unwrap();
    registry.finish_loading();
    let runtime = runtime(&generator, registry);
    let errors = RefCell::new(None);
    let result = run_biome_decoration_impl(
        &mut center,
        &generator,
        &workspace,
        4,
        Some((&runtime, &errors)),
    );
    assert!(matches!(
        result,
        Err(GenError::StructurePlacementUnavailable { step_index: 3, .. })
    ));
    assert!(
        errors
            .borrow()
            .as_ref()
            .unwrap()
            .to_string()
            .contains("injected transformer failure")
    );
    assert_eq!(
        workspace.len(),
        288,
        "all canonical dependency owners survive the failure"
    );
    workspace.with_chunk(ChunkPos::new(1, 0), |chunk| {
        assert_eq!(
            chunk.unwrap().get_block_state(16, 55, 0),
            block("minecraft:gold_block")
        )
    });
    assert!(
        center
            .get_start_for_structure(&Identifier::parse("minecraft:mineshaft"))
            .unwrap()
            .native()
            .is_ok(),
        "no deferred loan placeholder escapes an error"
    );
    assert_eq!(center.get_persisted_status(), ChunkStatus::Carvers);
}

#[test]
fn limited_region_loads_real_saved_carts_and_commits_the_source_lazy_entity_list() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let rows: Vec<serde_json::Value> =
        include_str!("../../../../../tools/rivet-oracle/fixtures/chest-minecart-load.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
    let inside = rows
        .iter()
        .find(|row| row["name"] == "minimal" && row["experimental"] == false)
        .unwrap();
    let outside = rows
        .iter()
        .find(|row| row["name"] == "persisted_generated" && row["experimental"] == false)
        .unwrap();
    let generator = Arc::new(OverworldGenerator::new(42));
    let (mut center, workspace) = controlled_region_inputs(&generator);
    install_corridor(&mut center, &generator);
    center.add_entity(parse_compound_fully(inside["saved"].as_str().unwrap()).unwrap());
    center.add_entity(parse_compound_fully(outside["saved"].as_str().unwrap()).unwrap());
    let registry = Arc::new(NativeStructureEventRegistry::new(
        std::thread::current().id(),
        Arc::new(TracingStructureEventDiagnostics),
    ));
    let plugin = registry
        .enable_plugin(NativePluginMetadata {
            display_name: "loaded-native-carts".into(),
            authors: vec!["fixture".into()],
        })
        .unwrap();
    let queried = Arc::new(AtomicBool::new(false));
    let visited = queried.clone();
    registry
        .register_generate_listener(
            plugin,
            EventPriority::Normal,
            Arc::new(move |_, maps| {
                let visited = visited.clone();
                maps.set_block(
                    Identifier::parse("fixture:query_existing_carts"),
                    Arc::new(move |limited, _, snapshot, _| {
                        if !visited.swap(true, Ordering::SeqCst) {
                            let entities = limited.entities()?;
                            assert_eq!(entities.len(), 1, "outside entity is retained separately");
                            assert!(entities[0].generation());
                            entities[0].set_loot_table(
                                Identifier::parse("minecraft:chests/buried_treasure"),
                                17,
                            );
                        }
                        Ok(snapshot)
                    }),
                );
                Ok(())
            }),
        )
        .unwrap();
    registry.finish_loading();
    let runtime = runtime(&generator, registry);
    let mut region = compose_feature_region_canonical(&mut center, &generator, &workspace).unwrap();
    runtime
        .place_step(
            &mut region,
            &mut WorldgenRandom::new(LegacyRandomSource::new(0)),
            23 - 1 - 30_000,
            3,
        )
        .unwrap();
    assert!(queried.load(Ordering::SeqCst));
    for (_, chunk) in region.into_owned_proto_entries() {
        assert!(workspace.put(chunk).is_ok());
    }
    let mut expected_inside = parse_compound_fully(inside["saved"].as_str().unwrap()).unwrap();
    expected_inside.put_string("LootTable", "minecraft:chests/buried_treasure");
    expected_inside.put_long("LootTableSeed", 17);
    assert_eq!(
        center.get_entities(),
        [expected_inside],
        "source finish clears the loaded9chunks, including a later direct corridor cart, then writes the retained list"
    );
    let expected_outside = parse_compound_fully(outside["saved"].as_str().unwrap()).unwrap();
    workspace.with_chunk(ChunkPos::new(-2, 2), |chunk| {
        assert_eq!(chunk.unwrap().get_entities(), [expected_outside])
    });
    assert_eq!(workspace.len(), 288);
}
