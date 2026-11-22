//! Paper-independent native smoke plugin acceptance & controlled-negative tests (#509).

use std::cell::RefCell;
use std::rc::Rc;

use rivet_api::native::{
    ApiVersion, BatchMode, BlockPos, BlockStateSnapshot, Cancellable, CapabilitySet, ChunkPos,
    CommandDispatchOutcome, CommandSender, EntityCommand, FacadeError, HandleError, LifecycleError,
    NativePluginHost, NegotiationError, PluginState, SmokeBlockPlaceEvent, SmokePlugin,
    SmokeTelemetry, SyntheticWorldState, TickCommand, TickCommandBatch, WorldCommand, WorldId,
    run_native_smoke_acceptance,
};

#[test]
fn native_smoke_acceptance_passes_end_to_end_and_reports_non_terminal_gate() {
    let report = run_native_smoke_acceptance().expect("native smoke acceptance must succeed");
    assert!(
        report.non_terminal_gate.contains("non-terminal"),
        "report must explicitly identify itself as non-terminal"
    );
    assert!(
        report.non_terminal_gate.contains("#251"),
        "report must explicitly distinguish itself from #251 JVM Tier 1 gate"
    );
    assert!(report.lifecycle_passed);
    assert!(report.events_passed);
    assert!(report.commands_passed);
    assert!(report.scheduler_passed);
    assert!(report.world_facade_passed);
    assert!(report.cleanup_passed);
    assert_eq!(
        report.controlled_negatives_passed,
        vec![
            "missing_required_capability_rejected",
            "incompatible_api_version_rejected",
            "stale_entity_handle_rejected",
            "off_thread_access_rejected",
            "missing_m3_provider_unverified",
            "capability_denied_on_mutation",
        ]
    );
    assert!(report.all_passed());
}

#[test]
fn smoke_plugin_lifecycle_handles_events_commands_scheduler_and_auto_cleanup() {
    let caps = SmokePlugin::required_capabilities();
    let mut host = NativePluginHost::new(ApiVersion::CURRENT, caps);
    let telemetry = Rc::new(RefCell::new(SmokeTelemetry::default()));

    let handle = host
        .register(Box::new(SmokePlugin::new(Rc::clone(&telemetry))))
        .expect("register smoke plugin");
    assert_eq!(host.state(handle), Ok(PluginState::Registered));

    host.load(handle).expect("load smoke plugin");
    assert_eq!(host.state(handle), Ok(PluginState::Loaded));

    host.enable(handle).expect("enable smoke plugin");
    assert_eq!(host.state(handle), Ok(PluginState::Enabled));

    // Normal event passes through uncancelled and annotated.
    let world = WorldId::from_raw(7);
    let mut dirt = SmokeBlockPlaceEvent::new(world, BlockPos::new(0, 64, 0), "minecraft:dirt");
    let rep = host.dispatch_event(&mut dirt);
    assert_eq!(rep.invoked, 1);
    assert!(!Cancellable::is_cancelled(&dirt));
    assert_eq!(dirt.annotations, vec!["smoke-inspected".to_owned()]);

    // TNT placement is cancelled by the smoke plugin.
    let mut tnt = SmokeBlockPlaceEvent::new(world, BlockPos::new(1, 64, 0), "minecraft:tnt");
    let rep_tnt = host.dispatch_event(&mut tnt);
    assert_eq!(rep_tnt.invoked, 1);
    assert!(Cancellable::is_cancelled(&tnt));

    // Primary command and alias both execute.
    let console = CommandSender::Console;
    assert!(matches!(
        host.dispatch_command(&console, "smoke-status a b"),
        CommandDispatchOutcome::Handled { .. }
    ));
    assert!(matches!(
        host.dispatch_command(&console, "sstat"),
        CommandDispatchOutcome::Handled { .. }
    ));

    // Advance 3 ticks: delayed runs at tick 2 (once), repeating runs at ticks 1 and 3 (twice).
    for _ in 0..3 {
        let tick = host.tick_scheduler();
        assert!(tick.panics.is_empty());
    }
    {
        let t = telemetry.borrow();
        assert_eq!(t.lifecycle_events, vec!["on_load", "on_enable"]);
        assert_eq!(t.events_seen, 2);
        assert_eq!(t.events_cancelled, 1);
        assert_eq!(t.commands_executed.len(), 2);
        assert_eq!(t.delayed_runs, 1);
        assert_eq!(t.repeating_runs, 2);
    }

    // Disable and unload must clean up every owned listener, command, task, and resource.
    host.disable(handle).expect("disable smoke plugin");
    host.unload(handle).expect("unload smoke plugin");
    assert_eq!(
        telemetry.borrow().lifecycle_events,
        vec!["on_load", "on_enable", "on_disable"]
    );
    assert!(matches!(
        host.state(handle),
        Err(LifecycleError::Handle(HandleError::Stale { .. }))
    ));

    let mut after_unload =
        SmokeBlockPlaceEvent::new(world, BlockPos::new(2, 64, 0), "minecraft:tnt");
    assert_eq!(host.dispatch_event(&mut after_unload).invoked, 0);
    assert!(!Cancellable::is_cancelled(&after_unload));
    assert!(matches!(
        host.dispatch_command(&console, "smoke-status"),
        CommandDispatchOutcome::UnknownCommand
    ));
}

#[test]
fn controlled_negative_missing_capability_and_version_mismatch_fail_closed() {
    let mut host_no_caps =
        NativePluginHost::new(ApiVersion::CURRENT, CapabilitySet::synthetic_default());
    let err = host_no_caps
        .register(Box::new(SmokePlugin::new(Rc::new(RefCell::new(
            SmokeTelemetry::default(),
        )))))
        .expect_err("must reject missing required capabilities");
    assert!(matches!(
        err,
        LifecycleError::Negotiation(NegotiationError::MissingCapabilities(_))
    ));

    let mut host_bad_ver = NativePluginHost::new(
        ApiVersion::new(0, 99, 0),
        SmokePlugin::required_capabilities(),
    );
    let err = host_bad_ver
        .register(Box::new(SmokePlugin::new(Rc::new(RefCell::new(
            SmokeTelemetry::default(),
        )))))
        .expect_err("must reject incompatible pre-1.0 minor version");
    assert!(matches!(
        err,
        LifecycleError::Negotiation(NegotiationError::IncompatibleApiVersion { .. })
    ));
}

#[test]
fn controlled_negative_unverified_m3_provider_and_atomic_rollback_fail_closed() {
    let caps = SmokePlugin::required_capabilities();
    let mut state = SyntheticWorldState::new();
    let world = state.create_world("world", "minecraft:overworld", -64, 384);
    state.load_chunk(world, ChunkPos::new(0, 0)).unwrap();

    // Missing M3 provider fails closed with ProviderUnverified.
    {
        let view = state.view(&caps);
        assert!(matches!(
            view.require_provider("m3.world.interaction"),
            Err(FacadeError::ProviderUnverified {
                provider: "m3.world.interaction",
                ..
            })
        ));
    }

    // Spawn and remove an entity so its handle becomes stale, then verify atomic rollback.
    let sheep = state
        .spawn_entity(world, "minecraft:sheep", 1.5, 65.0, 1.5)
        .expect("spawn sheep");
    state.remove_entity(sheep).expect("remove sheep");

    {
        let mut view = state.view(&caps);
        let atomic = TickCommandBatch::new(BatchMode::Atomic)
            .with(TickCommand::World(WorldCommand::SetBlock {
                world,
                pos: BlockPos::new(1, 64, 1),
                state: BlockStateSnapshot::simple("minecraft:diamond_block"),
            }))
            .with(TickCommand::Entity(EntityCommand::Discard {
                handle: sheep,
            }));
        let receipt = view.execute_batch(atomic).expect("batch returns receipt");
        assert!(!receipt.committed);
        assert_eq!(receipt.failed_count(), 1);
        assert_eq!(
            view.get_block(world, BlockPos::new(1, 64, 1))
                .expect("read block")
                .block_id,
            "minecraft:air",
            "failed atomic batch must roll back earlier block mutation"
        );
    }
}
