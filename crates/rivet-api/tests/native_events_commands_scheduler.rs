//! Integration tests for #507: native events, commands, and synchronous scheduler.

use std::cell::RefCell;
use std::rc::Rc;

use rivet_api::native::{
    Cancellable, Capability, CapabilitySet, CommandDispatchOutcome, CommandError, CommandRegistry,
    CommandSender, CommandSpec, EventBus, EventPriority, HandleError, LifecycleError, NativeEvent,
    NativePlugin, NativePluginHost, PluginContext, PluginDescriptor, PluginId, ScheduleError,
    SyncScheduler,
};

#[derive(Clone, Debug)]
struct SyntheticChatEvent {
    message: String,
    cancelled: bool,
}

impl Cancellable for SyntheticChatEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn set_cancelled(&mut self, cancelled: bool) {
        self.cancelled = cancelled;
    }
}

impl NativeEvent for SyntheticChatEvent {
    const NAME: &'static str = "SyntheticChatEvent";

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

fn alloc_plugin_ids(count: usize) -> Vec<PluginId> {
    (1..=count as u32).map(PluginId::from_raw).collect()
}

#[test]
fn event_priority_fifo_mutation_cancellation_and_ignore_cancelled() {
    let mut bus = EventBus::new();
    let owner = alloc_plugin_ids(1)[0];
    let trace: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));

    // Register out of priority order to prove sorting by (EventPriority, seq).
    let t = Rc::clone(&trace);
    bus.register::<SyntheticChatEvent, _>(owner, EventPriority::Monitor, false, move |ev| {
        t.borrow_mut().push("monitor");
        assert!(Cancellable::is_cancelled(ev));
        assert_eq!(ev.message, "hello [low-1][low-2]");
    })
    .unwrap();

    let t = Rc::clone(&trace);
    bus.register::<SyntheticChatEvent, _>(owner, EventPriority::High, true, move |_ev| {
        t.borrow_mut().push("high_ignored");
    })
    .unwrap();

    let t = Rc::clone(&trace);
    bus.register::<SyntheticChatEvent, _>(owner, EventPriority::Normal, false, move |ev| {
        t.borrow_mut().push("normal_cancels");
        ev.set_cancelled(true);
    })
    .unwrap();

    let t = Rc::clone(&trace);
    bus.register::<SyntheticChatEvent, _>(owner, EventPriority::Low, false, move |ev| {
        t.borrow_mut().push("low_1");
        ev.message.push_str(" [low-1]");
    })
    .unwrap();

    let t = Rc::clone(&trace);
    bus.register::<SyntheticChatEvent, _>(owner, EventPriority::Low, false, move |ev| {
        t.borrow_mut().push("low_2");
        ev.message.push_str("[low-2]");
    })
    .unwrap();

    let t = Rc::clone(&trace);
    bus.register::<SyntheticChatEvent, _>(owner, EventPriority::Lowest, false, move |_ev| {
        t.borrow_mut().push("lowest");
    })
    .unwrap();

    let mut event = SyntheticChatEvent {
        message: "hello".to_owned(),
        cancelled: false,
    };
    let report = bus.dispatch(&mut event);

    assert_eq!(
        *trace.borrow(),
        vec!["lowest", "low_1", "low_2", "normal_cancels", "monitor"]
    );
    assert_eq!(report.invoked, 5);
    assert_eq!(report.skipped_cancelled, 1);
    assert!(report.panics.is_empty());
}

#[test]
fn event_panic_isolation_preserves_subsequent_listeners() {
    let mut bus = EventBus::new();
    let ids = alloc_plugin_ids(2);
    let (p1, p2) = (ids[0], ids[1]);
    let ran_after = Rc::new(RefCell::new(false));

    bus.register::<SyntheticChatEvent, _>(p1, EventPriority::Low, false, |_ev| {
        panic!("listener boom");
    })
    .unwrap();

    let flag = Rc::clone(&ran_after);
    bus.register::<SyntheticChatEvent, _>(p2, EventPriority::High, false, move |ev| {
        *flag.borrow_mut() = true;
        ev.message.push_str(" survived");
    })
    .unwrap();

    let mut event = SyntheticChatEvent {
        message: "msg".to_owned(),
        cancelled: false,
    };
    let report = bus.dispatch(&mut event);

    assert!(*ran_after.borrow());
    assert_eq!(event.message, "msg survived");
    assert_eq!(report.invoked, 2);
    assert_eq!(report.panics.len(), 1);
    assert_eq!(report.panics[0].owner, p1);
    assert_eq!(report.panics[0].message, "listener boom");
}

#[test]
fn command_normalization_fallback_prefix_collision_and_panic_isolation() {
    let mut registry = CommandRegistry::new();
    let ids = alloc_plugin_ids(2);
    let (p1, p2) = (ids[0], ids[1]);
    let log: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    let log_a = Rc::clone(&log);
    registry
        .register(
            p1,
            "Alpha",
            CommandSpec::new("Ping").with_aliases(["p"]),
            move |sender, label, args| {
                log_a.borrow_mut().push(format!(
                    "alpha:{}:{}:{}",
                    sender.name(),
                    label,
                    args.join(",")
                ));
                Ok(())
            },
        )
        .unwrap();

    // Second plugin registers same primary name: bare "ping" stays with alpha,
    // while "beta:ping" routes to beta (SimpleCommandMap parity).
    let log_b = Rc::clone(&log);
    registry
        .register(
            p2,
            "Beta",
            CommandSpec::new("ping").with_aliases(["p", "bping"]),
            move |sender, label, args| {
                log_b.borrow_mut().push(format!(
                    "beta:{}:{}:{}",
                    sender.name(),
                    label,
                    args.join(",")
                ));
                Ok(())
            },
        )
        .unwrap();

    // Panicking command
    registry
        .register(p2, "Beta", CommandSpec::new("explode"), |_s, _l, _a| {
            panic!("command boom");
        })
        .unwrap();

    let sender = CommandSender::Player {
        uuid: 42,
        name: "Steve".to_owned(),
    };

    // Adjacent spaces + leading slash + mixed case
    assert_eq!(
        registry.dispatch(&sender, "/PING   one   two "),
        CommandDispatchOutcome::Handled {
            resolved_label: "ping".to_owned(),
            owner: p1,
        }
    );
    assert_eq!(
        registry.dispatch(&sender, "beta:ping arg"),
        CommandDispatchOutcome::Handled {
            resolved_label: "beta:ping".to_owned(),
            owner: p2,
        }
    );
    assert_eq!(
        registry.dispatch(&sender, "bping"),
        CommandDispatchOutcome::Handled {
            resolved_label: "bping".to_owned(),
            owner: p2,
        }
    );
    assert_eq!(
        registry.dispatch(&sender, "   "),
        CommandDispatchOutcome::UnknownCommand
    );
    assert_eq!(
        registry.dispatch(&sender, "unknown_cmd"),
        CommandDispatchOutcome::UnknownCommand
    );

    let panic_outcome = registry.dispatch(&CommandSender::Console, "explode");
    assert_eq!(
        panic_outcome,
        CommandDispatchOutcome::Panicked {
            resolved_label: "explode".to_owned(),
            owner: p2,
            message: "command boom".to_owned(),
        }
    );

    assert_eq!(
        *log.borrow(),
        vec![
            "alpha:Steve:ping:one,two",
            "beta:Steve:beta:ping:arg",
            "beta:Steve:bping:",
        ]
    );

    // Invalid command label rejected
    assert_eq!(
        registry.register(p1, "alpha", CommandSpec::new("bad cmd"), |_, _, _| Ok(())),
        Err(CommandError::InvalidLabel("bad cmd".to_owned()))
    );
}

#[test]
fn scheduler_ordering_repeating_reentrancy_cancellation_and_panic_isolation() {
    let mut sched = SyncScheduler::new();
    let ids = alloc_plugin_ids(2);
    let (p1, p2) = (ids[0], ids[1]);
    let trace: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // Zero period rejected
    assert_eq!(
        sched.schedule_repeating(p1, 1, 0, |_| {}),
        Err(ScheduleError::ZeroPeriod)
    );

    // Two tasks due on tick 1: FIFO by registration_seq
    let t = Rc::clone(&trace);
    sched
        .schedule_delayed(p1, 0, move |ctx| {
            t.borrow_mut().push(format!("first@{}", ctx.current_tick()));
            // Reentrant schedule with delay=0 must defer to next tick (tick 2), never same tick!
            let t_inner = Rc::clone(&t);
            ctx.schedule_delayed(0, move |inner_ctx| {
                t_inner
                    .borrow_mut()
                    .push(format!("reentrant@{}", inner_ctx.current_tick()));
            });
        })
        .unwrap();

    let t = Rc::clone(&trace);
    sched
        .schedule_delayed(p1, 1, move |ctx| {
            t.borrow_mut()
                .push(format!("second@{}", ctx.current_tick()));
        })
        .unwrap();

    // Repeating task every 2 ticks starting at tick 2, cancels itself on tick 6
    let t = Rc::clone(&trace);
    let _rep = sched
        .schedule_repeating(p1, 2, 2, move |ctx| {
            let tick = ctx.current_tick();
            t.borrow_mut().push(format!("rep@{tick}"));
            if tick >= 6 {
                ctx.cancel_self();
            }
        })
        .unwrap();

    // Panicking repeating task on p2: runs once on tick 1, panics, is auto-cancelled
    let panic_handle = sched
        .schedule_repeating(p2, 1, 1, |_ctx| {
            panic!("scheduler boom");
        })
        .unwrap();

    // Cross-plugin cancel rejected
    assert!(matches!(
        sched.cancel(p1, panic_handle),
        Err(ScheduleError::Handle(HandleError::UnauthorizedOwner {
            owner,
            caller,
            ..
        })) if owner == p2 && caller == p1
    ));

    let r1 = sched.tick();
    assert_eq!(r1.tick, 1);
    assert_eq!(r1.executed, 3);
    assert_eq!(r1.panics.len(), 1);
    assert_eq!(r1.panics[0].owner, p2);
    assert_eq!(r1.panics[0].message, "scheduler boom");
    assert_eq!(sched.pending_task_count_for(p2), 0);

    for _ in 2..=8 {
        let r = sched.tick();
        assert!(r.panics.is_empty());
    }

    assert_eq!(
        *trace.borrow(),
        vec![
            "first@1",
            "second@1",
            "rep@2",
            "reentrant@2",
            "rep@4",
            "rep@6",
        ]
    );
    assert_eq!(sched.pending_task_count(), 0);
}

struct CallbackPlugin {
    name: String,
    require_callbacks: bool,
    runs: Rc<RefCell<usize>>,
}

impl NativePlugin for CallbackPlugin {
    fn descriptor(&self) -> PluginDescriptor {
        let mut desc = PluginDescriptor::new(&self.name, "0.1.0");
        if self.require_callbacks {
            let mut req = CapabilitySet::synthetic_default();
            req.insert(Capability::EventSubscription);
            req.insert(Capability::CommandRegistration);
            req.insert(Capability::TickScheduler);
            desc.required_capabilities = req;
        }
        desc
    }

    fn on_enable(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        let runs_ev = Rc::clone(&self.runs);
        ctx.subscribe_event::<SyntheticChatEvent, _>(EventPriority::Normal, false, move |ev| {
            *runs_ev.borrow_mut() += 1;
            ev.message.push('!');
        })
        .map_err(|e| e.to_string())?;

        let runs_cmd = Rc::clone(&self.runs);
        ctx.register_command(CommandSpec::new("hello"), move |_s, _l, _a| {
            *runs_cmd.borrow_mut() += 1;
            Ok(())
        })
        .map_err(|e| e.to_string())?;

        let runs_task = Rc::clone(&self.runs);
        ctx.schedule_repeating(1, 1, move |_tctx| {
            *runs_task.borrow_mut() += 1;
        })
        .map_err(|e| e.to_string())?;

        Ok(())
    }
}

#[test]
fn host_capability_gating_plugin_disable_cleanup_and_shutdown() {
    // 1. Plugin without granted callback capabilities is denied when trying to register them.
    let mut minimal_host = NativePluginHost::synthetic_default();
    let runs = Rc::new(RefCell::new(0usize));
    let handle = minimal_host
        .register(Box::new(CallbackPlugin {
            name: "no-caps".to_owned(),
            require_callbacks: false,
            runs: Rc::clone(&runs),
        }))
        .unwrap();
    minimal_host.load(handle).unwrap();
    let err = minimal_host.enable(handle).unwrap_err();
    assert!(matches!(err, LifecycleError::PluginReturnedError { .. }));
    assert_eq!(minimal_host.events().listener_count(), 0);
    assert_eq!(minimal_host.commands().command_count(), 0);
    assert_eq!(minimal_host.scheduler().pending_task_count(), 0);

    // 2. Host with callback capabilities grants them, runs them, and revokes all on disable.
    let mut host = NativePluginHost::with_callback_capabilities();
    let runs = Rc::new(RefCell::new(0usize));
    let p = host
        .register(Box::new(CallbackPlugin {
            name: "full-caps".to_owned(),
            require_callbacks: true,
            runs: Rc::clone(&runs),
        }))
        .unwrap();
    host.load(p).unwrap();
    host.enable(p).unwrap();

    assert_eq!(host.events().listener_count(), 1);
    assert_eq!(host.commands().command_count(), 1);
    assert_eq!(host.scheduler().pending_task_count(), 1);

    let mut ev = SyntheticChatEvent {
        message: "hi".to_owned(),
        cancelled: false,
    };
    host.dispatch_event(&mut ev);
    assert_eq!(ev.message, "hi!");

    assert!(matches!(
        host.dispatch_command(&CommandSender::Console, "hello"),
        CommandDispatchOutcome::Handled { .. }
    ));

    let tick_report = host.tick_scheduler();
    assert_eq!(tick_report.executed, 1);
    assert_eq!(*runs.borrow(), 3);

    // Disabling plugin revokes all its listeners, commands, and tasks immediately.
    host.disable(p).unwrap();
    assert_eq!(host.events().listener_count(), 0);
    assert_eq!(host.commands().command_count(), 0);
    assert_eq!(host.scheduler().pending_task_count(), 0);

    assert_eq!(
        host.dispatch_command(&CommandSender::Console, "hello"),
        CommandDispatchOutcome::UnknownCommand
    );
    assert_eq!(host.tick_scheduler().executed, 0);
    assert_eq!(*runs.borrow(), 3);

    // Re-enable and verify shutdown_callbacks cleans everything cleanly.
    host.enable(p).unwrap();
    assert_eq!(host.events().listener_count(), 1);
    assert_eq!(host.commands().command_count(), 1);
    assert_eq!(host.scheduler().pending_task_count(), 1);
    host.shutdown_callbacks();
    assert_eq!(host.events().listener_count(), 0);
    assert_eq!(host.commands().command_count(), 0);
    assert_eq!(host.scheduler().pending_task_count(), 0);
}
