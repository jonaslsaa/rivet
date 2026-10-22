use super::*;
use std::sync::Mutex;
#[derive(Default)]
struct Diagnostics(Mutex<Vec<StructureEventDiagnostic>>);
impl StructureEventDiagnostics for Diagnostics {
    fn report(&self, event: StructureEventDiagnostic) {
        self.0.lock().unwrap().push(event);
    }
}
fn registry() -> (Arc<NativeStructureEventRegistry>, Arc<Diagnostics>) {
    let diagnostics = Arc::new(Diagnostics::default());
    (
        Arc::new(NativeStructureEventRegistry::new(
            thread::current().id(),
            diagnostics.clone(),
        )),
        diagnostics,
    )
}
fn payload() -> StructureSpawnPayload {
    static ACCESS: std::sync::LazyLock<RegistryAccess> = std::sync::LazyLock::new(|| {
        rivet_world::levelgen::structure::structure_registry::build_structure_access(
            &rivet_world::data::worldgen::worldgen_bootstraps::build_worldgen_registries(),
        )
        .unwrap()
    });
    StructureSpawnPayload::new(
        StructureWorldIdentity::new(42, Identifier::parse("minecraft:overworld")),
        &ACCESS,
        Identifier::parse("minecraft:buried_treasure"),
        BoundingBox::new([-12, 60, -20, 3, 80, -5]),
        ChunkPos::new(-1, -2),
    )
    .unwrap()
}
fn plugin(registry: &NativeStructureEventRegistry, name: &str) -> NativePluginId {
    registry
        .enable_plugin(NativePluginMetadata {
            display_name: name.into(),
            authors: vec!["actual-author".into()],
        })
        .unwrap()
}

#[test]
fn lifecycle_authority_and_async_guard_precede_empty_listener_shortcut() {
    let (registry, _) = registry();
    let event = payload();
    assert_eq!(
        registry.dispatch_on_current_thread(event.clone()),
        Err(StructureSpawnDispatchError::PrimaryThread)
    );
    let authority = StructureSpawnAuthority::registered_native(Arc::clone(&registry));
    assert_eq!(
        authority.dispatch(event.clone()),
        Err(StructureSpawnDispatchError::LifecycleLoading)
    );
    registry.finish_loading();
    assert_eq!(
        registry.dispatch_on_current_thread(event.clone()),
        Err(StructureSpawnDispatchError::PrimaryThread)
    );
    assert_eq!(
        authority.dispatch(event.clone()),
        Ok(SpawnDecision::Accepted)
    );
    assert!(matches!(
        StructureSpawnAuthority::unavailable("JVM plugin bridge still loading").dispatch(event),
        Err(StructureSpawnDispatchError::AuthorityUnavailable(_))
    ));
}

#[test]
fn priorities_payload_cancellation_and_real_worker_thread_match_source_contract() {
    let (registry, _) = registry();
    let plugin = plugin(&registry, "listeners");
    let trace = Arc::new(Mutex::new(Vec::new()));
    let primary = thread::current().id();
    let expected = payload();
    for (priority, name, ignore, cancel) in [
        (EventPriority::Monitor, "monitor", false, None),
        (EventPriority::Low, "skip-cancelled", true, None),
        (EventPriority::Lowest, "cancel", false, Some(true)),
        (EventPriority::Normal, "uncancel", false, Some(false)),
        (EventPriority::High, "high", true, None),
    ] {
        let trace = Arc::clone(&trace);
        let expected = expected.clone();
        registry
            .register_spawn_listener(
                plugin,
                priority,
                ignore,
                Arc::new(move |event, control| {
                    assert_ne!(thread::current().id(), primary);
                    assert_eq!(event.world(), expected.world());
                    assert_eq!(event.world().seed(), 42);
                    assert_eq!(
                        event.world().dimension(),
                        &Identifier::parse("minecraft:overworld")
                    );
                    assert_eq!(event.structure_key(), expected.structure_key());
                    assert_eq!(event.structure(), expected.structure());
                    assert_eq!(event.bounds(), BoundingBox::new([-12, 60, -20, 3, 80, -5]));
                    assert_eq!(event.chunk(), ChunkPos::new(-1, -2));
                    trace.lock().unwrap().push(name);
                    if let Some(value) = cancel {
                        control.set_cancelled(value);
                    }
                    Ok(())
                }),
            )
            .unwrap();
    }
    registry.finish_loading();
    let authority = StructureSpawnAuthority::registered_native(registry);
    assert_eq!(authority.dispatch(expected), Ok(SpawnDecision::Accepted));
    assert_eq!(
        *trace.lock().unwrap(),
        ["cancel", "uncancel", "high", "monitor"]
    );
}

#[test]
fn snapshot_unregister_and_disabled_plugin_registration_have_real_lifecycle() {
    let (registry, _) = registry();
    let owner = plugin(&registry, "owner");
    let disabled = plugin(&registry, "disabled");
    let target = Arc::new(Mutex::new(None));
    let hits = Arc::new(Mutex::new(Vec::new()));
    let clone = Arc::downgrade(&registry);
    let id = Arc::clone(&target);
    let trace = Arc::clone(&hits);
    registry
        .register_spawn_listener(
            owner,
            EventPriority::Lowest,
            false,
            Arc::new(move |_, _| {
                let clone = clone.upgrade().expect("dispatch retains registry");
                trace.lock().unwrap().push("first");
                if let Some(id) = *id.lock().unwrap() {
                    clone.unregister_listener(id).unwrap();
                }
                clone.disable_plugin(disabled).unwrap();
                Ok(())
            }),
        )
        .unwrap();
    let trace = Arc::clone(&hits);
    *target.lock().unwrap() = Some(
        registry
            .register_spawn_listener(
                owner,
                EventPriority::Normal,
                false,
                Arc::new(move |_, _| {
                    trace.lock().unwrap().push("snapshotted");
                    Ok(())
                }),
            )
            .unwrap(),
    );
    registry
        .register_spawn_listener(
            disabled,
            EventPriority::High,
            false,
            Arc::new(|_, _| {
                panic!("disabled registration must be skipped even in the baked snapshot")
            }),
        )
        .unwrap();
    registry.finish_loading();
    let authority = StructureSpawnAuthority::registered_native(Arc::clone(&registry));
    assert_eq!(authority.dispatch(payload()), Ok(SpawnDecision::Accepted));
    assert_eq!(*hits.lock().unwrap(), ["first", "snapshotted"]);
    assert!(matches!(
        registry.register_spawn_listener(
            disabled,
            EventPriority::Normal,
            false,
            Arc::new(|_, _| Ok(()))
        ),
        Err(StructureSpawnDispatchError::PluginDisabled(_))
    ));
    assert_eq!(authority.dispatch(payload()), Ok(SpawnDecision::Accepted));
    assert_eq!(*hits.lock().unwrap(), ["first", "snapshotted", "first"]);
}

#[test]
fn nag_once_recoverable_exceptions_and_recursive_exception_failures_do_not_abort_handlers() {
    let (registry, diagnostics) = registry();
    let plugin = plugin(&registry, "broken");
    let exceptions = Arc::new(Mutex::new(Vec::new()));
    for priority in [EventPriority::Lowest, EventPriority::Low] {
        registry
            .register_spawn_listener(
                plugin,
                priority,
                false,
                Arc::new(|_, _| Err(NativeListenerError::AuthorNag("nag".into()))),
            )
            .unwrap();
    }
    registry
        .register_spawn_listener(
            plugin,
            EventPriority::Normal,
            false,
            Arc::new(|_, control| {
                control.set_cancelled(true);
                Err(NativeListenerError::Failure("recoverable".into()))
            }),
        )
        .unwrap();
    let trace = Arc::clone(&exceptions);
    registry
        .register_exception_listener(
            plugin,
            EventPriority::Normal,
            Arc::new(move |exception, control| {
                trace
                    .lock()
                    .unwrap()
                    .push((exception.message.clone(), exception.cancelled_at_failure));
                control.set_cancelled(false);
                Err(NativeListenerError::Failure(
                    "exception listener failed".into(),
                ))
            }),
        )
        .unwrap();
    registry
        .register_spawn_listener(
            plugin,
            EventPriority::Highest,
            false,
            Arc::new(|_, _| panic!("listener panic")),
        )
        .unwrap();
    registry
        .register_spawn_listener(
            plugin,
            EventPriority::Monitor,
            false,
            Arc::new(|_, control| {
                control.set_cancelled(false);
                Ok(())
            }),
        )
        .unwrap();
    registry.finish_loading();
    let authority = StructureSpawnAuthority::registered_native(registry);
    assert_eq!(authority.dispatch(payload()), Ok(SpawnDecision::Accepted));
    assert_eq!(
        exceptions.lock().unwrap().len(),
        2,
        "no recursive exception event from exception handler failure"
    );
    assert_eq!(exceptions.lock().unwrap()[0], ("recoverable".into(), true));
    let reports = diagnostics.0.lock().unwrap();
    assert_eq!(
        reports
            .iter()
            .filter(|event| matches!(event, StructureEventDiagnostic::AuthorNag { .. }))
            .count(),
        1
    );
    assert_eq!(
        reports
            .iter()
            .filter(|event| matches!(event, StructureEventDiagnostic::ListenerFailure { .. }))
            .count(),
        4
    );
}

#[test]
fn payload_refuses_unregistered_structure_and_distinguishes_same_seed_worlds() {
    let event = payload();
    assert_ne!(
        event.world(),
        &StructureWorldIdentity::new(42, Identifier::parse("minecraft:overworld"))
    );
    assert!(matches!(
        StructureSpawnPayload::new(
            event.world().clone(),
            &RegistryAccess::empty(),
            event.structure_key().clone(),
            event.bounds(),
            event.chunk()
        ),
        Err(StructureSpawnDispatchError::AuthorityUnavailable(_))
    ));
    assert!(matches!(
        StructureSpawnPayload::new(
            event.world().clone(),
            &event.access,
            Identifier::parse("probe:missing"),
            event.bounds(),
            event.chunk()
        ),
        Err(StructureSpawnDispatchError::AuthorityUnavailable(_))
    ));
    let (first, _) = registry();
    let (second, _) = registry();
    let foreign = plugin(&first, "foreign");
    let _local = plugin(&second, "local");
    assert!(matches!(
        second.register_spawn_listener(
            foreign,
            EventPriority::Normal,
            false,
            Arc::new(|_, _| Ok(()))
        ),
        Err(StructureSpawnDispatchError::UnknownPlugin(_))
    ));
}

#[test]
fn representative_dispatch_trace_matches_actual_paper_event_manager() {
    let row: serde_json::Value = serde_json::from_str(
        include_str!("../../../../../tools/rivet-oracle/fixtures/structure-event-dispatch.jsonl")
            .trim(),
    )
    .unwrap();
    let (registry, diagnostics) = registry();
    let authority = StructureSpawnAuthority::registered_native(Arc::clone(&registry));
    registry.finish_loading();
    assert_eq!(
        registry
            .dispatch_on_current_thread(payload())
            .unwrap_err()
            .to_string(),
        row["empty_primary_error"].as_str().unwrap()
    );
    assert_eq!(
        authority.dispatch(payload()).unwrap() == SpawnDecision::Cancelled,
        row["empty_worker_cancelled"].as_bool().unwrap()
    );
    let inactive = plugin(&registry, "disabled");
    let plugin = plugin(&registry, "registered");
    let trace = Arc::new(Mutex::new(Vec::new()));
    for (priority, name, ignore, action) in [
        (EventPriority::Monitor, "monitor", false, 0),
        (EventPriority::Lowest, "cancel", false, 0),
        (EventPriority::Low, "ignored", true, 1),
        (EventPriority::Normal, "uncancel", false, 1),
        (EventPriority::Highest, "failure", false, 2),
        (EventPriority::Highest, "after-exception", true, 1),
        (EventPriority::Highest, "nag-one", false, 3),
        (EventPriority::Highest, "nag-two", false, 3),
    ] {
        let trace = Arc::clone(&trace);
        registry
            .register_spawn_listener(
                plugin,
                priority,
                ignore,
                Arc::new(move |_, control| {
                    trace.lock().unwrap().push(name);
                    match action {
                        0 => control.set_cancelled(true),
                        1 => control.set_cancelled(false),
                        2 => return Err(NativeListenerError::Failure("recoverable".into())),
                        _ => return Err(NativeListenerError::AuthorNag("author nag".into())),
                    }
                    Ok(())
                }),
            )
            .unwrap();
    }
    registry
        .register_spawn_listener(
            inactive,
            EventPriority::High,
            false,
            Arc::new(|_, _| panic!("disabled handler reached")),
        )
        .unwrap();
    registry.disable_plugin(inactive).unwrap();
    let exceptions = Arc::clone(&trace);
    registry
        .register_exception_listener(
            plugin,
            EventPriority::Normal,
            Arc::new(move |_, control| {
                exceptions.lock().unwrap().push("exception");
                control.set_cancelled(true);
                Err(NativeListenerError::Failure(
                    "exception handler failed".into(),
                ))
            }),
        )
        .unwrap();
    for name in ["first", "second"] {
        let decision = authority.dispatch(payload()).unwrap();
        assert_eq!(
            decision == SpawnDecision::Cancelled,
            row[format!("{name}_cancelled")].as_bool().unwrap()
        );
        assert_eq!(
            serde_json::json!(*trace.lock().unwrap()),
            row[format!("{name}_trace")]
        );
        trace.lock().unwrap().clear();
        let mut diagnostic = diagnostics.0.lock().unwrap();
        let messages: Vec<_> = diagnostic
            .iter()
            .map(|event| match event {
                StructureEventDiagnostic::AuthorNag { .. } => "nag",
                StructureEventDiagnostic::ListenerFailure {
                    event_name: "ServerExceptionEvent",
                    ..
                } => "error:exception",
                StructureEventDiagnostic::ListenerFailure { .. } => "error:spawn",
            })
            .collect();
        assert_eq!(serde_json::json!(messages), row[format!("{name}_logs")]);
        diagnostic.clear();
    }
}
