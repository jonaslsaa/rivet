use super::*;
use crate::server::level::ServerLevel;
use crate::server::level::{
    ServerLevelConfig,
    structure_spawn_events::{
        EventPriority, NativePluginMetadata, TracingStructureEventDiagnostics,
    },
};
use rivet_registry::{
    block_state_properties::BlockStateProperties as P, generated::blocks::BlockId,
};
use std::sync::Mutex;
struct EventAccess(ServerLevel);
impl WorldEventAccess for EventAccess {
    fn world_uuid(&self) -> Option<Uuid> {
        self.0.world_uuid()
    }
    fn block(&mut self, _pos: BlockPos) -> rivet_world::fluid::FluidResult<BlockState> {
        panic!("Paper event-object fixture never reads storage")
    }
    fn set_block(
        &mut self,
        _pos: BlockPos,
        _state: BlockState,
        _flags: u32,
        _limit: i32,
    ) -> rivet_world::fluid::FluidResult<bool> {
        panic!("Paper event-object fixture never writes storage")
    }
}
fn block(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).unwrap())
}
fn alter(event: &mut NativeWorldEvent) -> Result<(), NativeListenerError> {
    match event.kind() {
        WorldEventKind::FluidLevel => {
            event.set_fluid_data(block("minecraft:water").set_value(P::LEVEL, 7).unwrap())
        }
        WorldEventKind::Form => event.set_form_data(block("minecraft:glass")),
        _ => Ok(()),
    }
}
fn push(trace: &Mutex<Vec<String>>, value: &str) {
    trace.lock().unwrap().push(value.into());
}
fn data(kind: &str) -> WorldEventData {
    let pos = BlockPos::new(0, 64, 0);
    match kind {
        "physics" => WorldEventData::Physics {
            position: pos,
            changed: block("minecraft:water"),
            source: pos.west(),
        },
        "fluid" => WorldEventData::FluidLevel {
            position: pos,
            new_data: block("minecraft:water"),
        },
        "from_to" => WorldEventData::FromTo {
            source: pos,
            direction: Direction::East,
        },
        "form" => WorldEventData::Form {
            snapshot: CraftBlockSnapshot {
                position: pos,
                state: block("minecraft:stone"),
                block_entity: None,
            },
        },
        _ => panic!("unknownactualoraclekind"),
    }
}
#[test]
fn native_block_channels_match_actual_paper_dispatch_and_mutability() {
    let mut count = 0;
    for line in
        include_str!("../../../../../tools/rivet-oracle/fixtures/world-events.jsonl").lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let host = NativeWorldEventHost::start_on_current_thread(
            Arc::new(TracingStructureEventDiagnostics),
            Arc::new(Shutdown::new()),
        );
        let registry = host.registry();
        let metadata = |name: &str| NativePluginMetadata {
            display_name: name.into(),
            authors: vec!["author".into()],
        };
        let plugin = registry.enable_plugin(metadata("native")).unwrap();
        let other = registry.enable_plugin(metadata("other")).unwrap();
        let trace = Arc::new(Mutex::new(Vec::<String>::new()));
        let world = Uuid { most: 1, least: 2 };
        let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
        level.install_world_uuid(world);
        let mut level = EventAccess(level);
        let mut event = NativeWorldEvent::new(world, data(row["kind"].as_str().unwrap()));
        let kind = event.kind();
        {
            let trace = trace.clone();
            registry
                .register_world_exception_listener(
                    other,
                    EventPriority::Normal,
                    Arc::new(move |_, event, _| {
                        push(&trace, "exception");
                        event.set_cancelled(true);
                        Ok(())
                    }),
                )
                .unwrap();
        }
        match row["scenario"].as_str().unwrap() {
            "priorities" => {
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            plugin,
                            kind,
                            EventPriority::Lowest,
                            false,
                            Arc::new(move |event, _| {
                                push(&trace, "cancel");
                                event.set_cancelled(true);
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            plugin,
                            kind,
                            EventPriority::Normal,
                            true,
                            Arc::new(move |_, _| {
                                push(&trace, "ignored");
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            plugin,
                            kind,
                            EventPriority::Highest,
                            false,
                            Arc::new(move |event, _| {
                                push(&trace, "uncancel");
                                alter(event)?;
                                event.set_cancelled(false);
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            other,
                            kind,
                            EventPriority::Monitor,
                            false,
                            Arc::new(move |_, _| {
                                push(&trace, "monitor");
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
            }
            "disable" => {
                {
                    let trace = trace.clone();
                    let registry = registry.clone();
                    host.registry()
                        .register_world_listener(
                            plugin,
                            kind,
                            EventPriority::Lowest,
                            false,
                            Arc::new(move |_, _| {
                                push(&trace, "disable");
                                registry.disable_plugin(plugin).unwrap();
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            plugin,
                            kind,
                            EventPriority::Normal,
                            false,
                            Arc::new(move |_, _| {
                                push(&trace, "disabled");
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            other,
                            kind,
                            EventPriority::Monitor,
                            false,
                            Arc::new(move |_, _| {
                                push(&trace, "other");
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
            }
            scenario => {
                let wrong = scenario == "wrong_material";
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            plugin,
                            kind,
                            EventPriority::Lowest,
                            false,
                            Arc::new(move |event, _| {
                                push(&trace, "failure");
                                if wrong {
                                    event.set_fluid_data(block("minecraft:glass"))?;
                                } else {
                                    alter(event)?;
                                }
                                Err(NativeListenerError::Failure(
                                    "fixture listener failure".into(),
                                ))
                            }),
                        )
                        .unwrap();
                }
                {
                    let trace = trace.clone();
                    registry
                        .register_world_listener(
                            other,
                            kind,
                            EventPriority::Monitor,
                            true,
                            Arc::new(move |_, _| {
                                push(&trace, "ignored");
                                Ok(())
                            }),
                        )
                        .unwrap();
                }
            }
        }
        host.finish_loading();
        host.dispatch(&mut level, &mut event).unwrap();
        assert_eq!(
            serde_json::to_value(trace.lock().unwrap().clone()).unwrap(),
            row["trace"],
            "{row}"
        );
        assert_eq!(
            event.is_cancelled(),
            row["cancelled"].as_bool().unwrap(),
            "{row}"
        );
        if let Some(expected) = row["state"].as_u64() {
            let state = match event.data() {
                WorldEventData::FluidLevel { new_data, .. } => new_data,
                WorldEventData::Form { snapshot } => &snapshot.state,
                _ => panic!("actualoraclehasunexpectedstate"),
            };
            assert_eq!(u64::from(state.id().0), expected, "{row}");
        }
        count += 1;
    }
    assert_eq!(count, 13);
}
#[test]
fn real_host_guards_loading_tick_thread_foreign_world_and_actual_shutdown() {
    let shutdown = Arc::new(Shutdown::new());
    let host = Arc::new(NativeWorldEventHost::start_on_current_thread(
        Arc::new(TracingStructureEventDiagnostics),
        shutdown.clone(),
    ));
    let world = Uuid { most: 1, least: 2 };
    let mut level = ServerLevel::new_region_backed(ServerLevelConfig::default());
    level.install_world_uuid(world);
    let mut level = EventAccess(level);
    let mut event = NativeWorldEvent::new(world, data("physics"));
    assert_eq!(
        host.dispatch(&mut level, &mut event),
        Err(StructureSpawnDispatchError::LifecycleLoading)
    );
    host.finish_loading();
    let mut foreign = NativeWorldEvent::new(Uuid { most: 9, least: 9 }, data("physics"));
    assert!(matches!(
        host.dispatch(&mut level, &mut foreign),
        Err(StructureSpawnDispatchError::AuthorityUnavailable(_))
    ));
    let host_worker = host.clone();
    let (mut level, mut event) = std::thread::spawn(move || {
        assert_eq!(
            host_worker.dispatch(&mut level, &mut event),
            Err(StructureSpawnDispatchError::SynchronousThreadRequired)
        );
        (level, event)
    })
    .join()
    .unwrap();
    shutdown.request();
    let requested = host.clone();
    let (mut level, mut event) = std::thread::spawn(move || {
        assert_eq!(
            requested.dispatch(&mut level, &mut event),
            Err(StructureSpawnDispatchError::SynchronousThreadRequired)
        );
        (level, event)
    })
    .join()
    .unwrap();
    shutdown.begin_stopping();
    let worker = host.clone();
    std::thread::spawn(move || {
        worker.dispatch(&mut level, &mut event).unwrap();
        assert_eq!(
            worker.check_block_place_thread(),
            Err(StructureSpawnDispatchError::SynchronousThreadRequired)
        );
    })
    .join()
    .unwrap();
}
