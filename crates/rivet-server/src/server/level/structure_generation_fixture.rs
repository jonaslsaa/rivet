//! Vertical source capture: real seed42 NORMAL placement/biomes/native bodies,
//! explicit worker-dispatched listener decisions, canonical owner and refs.
//! The Java capture stops before events; acceptance/cancellation below is
//! provided by an actual registered native listener, never attributed to it.
use super::super::{
    generated_chunk_owner::{GeneratedChunkOwner, GenerationReadiness},
    structure_spawn_events::{
        EventPriority, NativePluginMetadata, NativeStructureEventRegistry,
        TracingStructureEventDiagnostics,
    },
};
use super::*;
use rivet_nbt::tag_parser::parse_compound_fully;
use rivet_world::levelgen::structure::BoundingBox;
use std::sync::Mutex;

#[test]
fn actual_seed42_spawn_window_starts_events_and_references_match_paper_capture() {
    let generator = Arc::new(OverworldGenerator::new(42));
    let rows: Vec<serde_json::Value> =
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-spawn-trace.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .filter(|row: &serde_json::Value| row["kind"] == "chunk")
            .collect();
    assert_eq!(rows.len(), 289);
    let expected_events: Vec<_> = rows
        .iter()
        .flat_map(|row| {
            let pos = ChunkPos::new(
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            );
            row["attempts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|attempt| attempt["valid"] == true)
                .map(move |attempt| {
                    (
                        Identifier::parse(attempt["structure"].as_str().unwrap()),
                        pos,
                        BoundingBox::new(std::array::from_fn(|i| {
                            attempt["bounds"][i].as_i64().unwrap() as i32
                        })),
                    )
                })
        })
        .collect();
    assert_eq!(expected_events.len(), 1);
    for cancelled in [false, true] {
        let registry = Arc::new(NativeStructureEventRegistry::new(
            std::thread::current().id(),
            Arc::new(TracingStructureEventDiagnostics),
        ));
        let plugin = registry
            .enable_plugin(NativePluginMetadata {
                display_name: "native-generation-fixture".into(),
                authors: vec!["fixture".into()],
            })
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        registry
            .register_spawn_listener(
                plugin,
                EventPriority::Normal,
                false,
                Arc::new(move |event, control| {
                    captured.lock().unwrap().push((
                        event.structure_key().clone(),
                        event.chunk(),
                        event.bounds(),
                    ));
                    control.set_cancelled(cancelled);
                    Ok(())
                }),
            )
            .unwrap();
        registry.finish_loading();
        let runtime = StructureGenerationRuntime::new(
            Arc::clone(&generator),
            StructureWorldIdentity::new(42, Identifier::parse("minecraft:overworld")),
            PaperStructureSeeds::default(),
            StructureSpawnAuthority::registered_native(registry),
        )
        .unwrap();
        let mut owner = GeneratedChunkOwner::new(Arc::clone(&generator));
        assert!(owner.install_structure_runtime(runtime).is_ok());
        let consumer = owner.new_consumer();
        owner.replace_requests(
            consumer,
            [(ChunkPos::ZERO, ChunkStatus::StructureReferences)],
        );
        let report = owner.run_jobs(600);
        assert_eq!(
            report.failed,
            0,
            "cancellation={cancelled}: {:?}",
            owner.readiness(ChunkPos::ZERO, ChunkStatus::StructureReferences)
        );
        assert!(matches!(
            owner.readiness(ChunkPos::ZERO, ChunkStatus::StructureReferences),
            GenerationReadiness::Ready
        ));
        assert_eq!(
            owner.len(),
            289,
            "no invented dependency radius or duplicate chunks"
        );
        assert_eq!(*events.lock().unwrap(), expected_events);
        let mut starts = 0;
        for row in &rows {
            let pos = ChunkPos::new(
                row["x"].as_i64().unwrap() as i32,
                row["z"].as_i64().unwrap() as i32,
            );
            let expected: Vec<_> = row["attempts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|attempt| attempt["valid"] == true && !cancelled)
                .collect();
            assert_eq!(
                owner.start_keys(pos).len(),
                expected.len(),
                "source starts at {pos:?}"
            );
            assert_eq!(
                owner
                    .structure_runtime()
                    .unwrap()
                    .loaded_starts(pos)
                    .unwrap()
                    .len(),
                expected.len()
            );
            for attempt in expected {
                let key = Identifier::parse(attempt["structure"].as_str().unwrap());
                let start = owner.start(pos, &key).unwrap().native().unwrap();
                assert_eq!(
                    start.create_tag(pos),
                    parse_compound_fully(attempt["saved"].as_str().unwrap()).unwrap()
                );
                assert_eq!(
                    start.pieces().pieces().len(),
                    attempt["piece_count"].as_u64().unwrap() as usize
                );
                assert_eq!(
                    start.references(),
                    0,
                    "reference generation does not increment locate counts"
                );
                starts += 1;
            }
        }
        assert_eq!(starts, usize::from(!cancelled));
        let mine = Identifier::parse("minecraft:mineshaft");
        let expected_references = if cancelled {
            Vec::new()
        } else {
            vec![ChunkPos::new(4, 0).pack() as u64]
        };
        assert_eq!(owner.references(ChunkPos::ZERO, &mine), expected_references);
        assert_eq!(
            owner.state(ChunkPos::ZERO).unwrap().status(),
            ChunkStatus::StructureReferences
        );
        owner.release_consumer(consumer);
        assert_eq!(owner.run_jobs(1).attempted, 0);
    }
}
