//! Source-controlled native smoke plugin and Paper-independent non-terminal
//! acceptance runner (#509).
//!
//! This module proves that the native Rust plugin boundary (`#506` lifecycle,
//! `#507` events/commands/scheduler, `#508` world/entity/player/item/inventory
//! capability facades) works end-to-end without depending on `rivet-ffi`, the
//! JVM Tier 1 adapter, or `#251`'s JVM corpus gate.
//!
//! ## Ownership and safety contract
//!
//! - No arena pointer, retained caller buffer, Rust reference into game state,
//!   or foreign exception crosses the plugin boundary.
//! - All reads return value-copy snapshots; all mutations flow through typed
//!   tick-thread commands on [`NativeFacadeView`].
//! - All handles (`PluginHandle`, `ResourceHandle`, `EventListenerHandle`,
//!   `CommandHandle`, `TaskHandle`, `EntityHandle`, `PlayerHandle`,
//!   `InventoryHandle`, `MenuHandle`) are generational and reject stale/reused
//!   slots fail-closed.

use std::cell::RefCell;
use std::rc::Rc;

use super::commands::{CommandDispatchOutcome, CommandHandle, CommandSender, CommandSpec};
use super::events::{Cancellable, EventListenerHandle, EventPriority, NativeEvent};
use super::facades::{
    BatchMode, BlockPos, BlockStateSnapshot, ChunkPos, CommandOutcome, EntityCommand, FacadeError,
    GameMode, InventoryCommand, InventoryKind, ItemStackSnapshot, MenuCommand, PlayerCommand,
    SyntheticWorldState, TickCommand, TickCommandBatch, WorldCommand, WorldId,
};
use super::handle::HandleError;
use super::lifecycle::{
    LifecycleError, NativePlugin, NativePluginHost, PluginContext, PluginDescriptor, PluginState,
    ResourceHandle,
};
use super::scheduler::TaskHandle;
use super::version::{ApiVersion, Capability, CapabilitySet, LoadingMode, NegotiationError};

/// Typed native event exercised by [`SmokePlugin`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmokeBlockPlaceEvent {
    pub world: WorldId,
    pub pos: BlockPos,
    pub block_id: String,
    pub cancelled: bool,
    pub annotations: Vec<String>,
}

impl SmokeBlockPlaceEvent {
    #[must_use]
    pub fn new(world: WorldId, pos: BlockPos, block_id: impl Into<String>) -> Self {
        Self {
            world,
            pos,
            block_id: block_id.into(),
            cancelled: false,
            annotations: Vec::new(),
        }
    }
}

impl Cancellable for SmokeBlockPlaceEvent {
    fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    fn set_cancelled(&mut self, cancelled: bool) {
        self.cancelled = cancelled;
    }
}

impl NativeEvent for SmokeBlockPlaceEvent {
    const NAME: &'static str = "SmokeBlockPlaceEvent";

    fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Shared observable telemetry recorded by [`SmokePlugin`].
#[derive(Default, Debug, Clone, Eq, PartialEq)]
pub struct SmokeTelemetry {
    pub lifecycle_events: Vec<String>,
    pub events_seen: u32,
    pub events_cancelled: u32,
    pub commands_executed: Vec<String>,
    pub delayed_runs: u32,
    pub repeating_runs: u32,
}

/// Source-controlled native smoke plugin exercising lifecycle, events, commands,
/// synchronous scheduler tasks, and capability-facade world/entity/player/item
/// state (#509).
pub struct SmokePlugin {
    telemetry: Rc<RefCell<SmokeTelemetry>>,
    resource: Option<ResourceHandle>,
    listener: Option<EventListenerHandle>,
    status_command: Option<CommandHandle>,
    delayed_task: Option<TaskHandle>,
    repeating_task: Option<TaskHandle>,
}

impl SmokePlugin {
    pub const PLUGIN_NAME: &'static str = "rivet-native-smoke";
    pub const PLUGIN_VERSION: &'static str = "0.1.0";

    #[must_use]
    pub fn new(telemetry: Rc<RefCell<SmokeTelemetry>>) -> Self {
        Self {
            telemetry,
            resource: None,
            listener: None,
            status_command: None,
            delayed_task: None,
            repeating_task: None,
        }
    }

    /// Required capability set for full smoke plugin operation.
    #[must_use]
    pub fn required_capabilities() -> CapabilitySet {
        CapabilitySet::from_iter([
            Capability::LifecycleHooks,
            Capability::SyntheticResources,
            Capability::EventSubscription,
            Capability::CommandRegistration,
            Capability::TickScheduler,
            Capability::WorldRead,
            Capability::WorldWrite,
            Capability::EntityRead,
            Capability::EntityWrite,
            Capability::PlayerRead,
            Capability::PlayerWrite,
            Capability::InventoryRead,
            Capability::InventoryWrite,
            Capability::MenuRead,
            Capability::MenuWrite,
        ])
    }

    #[must_use]
    pub fn resource_handle(&self) -> Option<ResourceHandle> {
        self.resource
    }

    #[must_use]
    pub fn listener_handle(&self) -> Option<EventListenerHandle> {
        self.listener
    }

    #[must_use]
    pub fn status_command_handle(&self) -> Option<CommandHandle> {
        self.status_command
    }

    #[must_use]
    pub fn delayed_task_handle(&self) -> Option<TaskHandle> {
        self.delayed_task
    }

    #[must_use]
    pub fn repeating_task_handle(&self) -> Option<TaskHandle> {
        self.repeating_task
    }

    /// Exercises representative world, entity, player, item, inventory, and
    /// menu capability facades on the tick thread through value snapshots and
    /// atomic [`TickCommandBatch`] execution.
    pub fn exercise_world_state(
        world_state: &mut SyntheticWorldState,
        granted: &CapabilitySet,
        world_id: WorldId,
    ) -> Result<SmokeWorldOutcome, FacadeError> {
        let villager = world_state.spawn_entity(world_id, "minecraft:villager", 4.5, 65.0, -1.5)?;
        let player =
            world_state.connect_player(world_id, [0x11; 16], "SmokeTester", GameMode::Survival)?;
        let vault = world_state.create_inventory(InventoryKind::Chest, 27)?;

        let mut view = world_state.view(granted);
        let target_pos = BlockPos::new(4, 64, -2);
        let initial_block = view.get_block(world_id, target_pos)?;

        let pickaxe = ItemStackSnapshot::new("minecraft:diamond_pickaxe", 1, 1)?;
        let batch = TickCommandBatch::new(BatchMode::Atomic)
            .with(TickCommand::World(WorldCommand::SetBlock {
                world: world_id,
                pos: target_pos,
                state: BlockStateSnapshot::simple("minecraft:stone_bricks"),
            }))
            .with(TickCommand::Entity(EntityCommand::SetCustomName {
                handle: villager,
                name: Some("SmokeGuide".to_owned()),
            }))
            .with(TickCommand::Player(PlayerCommand::SendMessage {
                handle: player,
                message: "smoke-ready".to_owned(),
            }))
            .with(TickCommand::Inventory(InventoryCommand::SetSlot {
                inventory: vault,
                slot: 0,
                item: Some(pickaxe),
            }))
            .with(TickCommand::Menu(MenuCommand::OpenMenu {
                player,
                inventory: vault,
                title: "Smoke Vault".to_owned(),
            }));

        let receipt = view.execute_batch(batch)?;
        if !receipt.committed || receipt.failed_count() != 0 {
            return Err(FacadeError::InvalidValue {
                field: "batch",
                detail: "atomic smoke setup batch unexpectedly failed".to_owned(),
            });
        }

        let updated_block = view.get_block(world_id, target_pos)?;
        let villager_snap = view.get_entity(villager)?;
        let player_snap = view.get_player(player)?;
        let granted_item = view
            .get_slot(vault, 0)?
            .map(|stack| (stack.item_id, stack.count));

        let open_menu = player_snap.open_menu;
        if let Some(menu) = open_menu {
            view.execute(TickCommand::Menu(MenuCommand::CloseMenu { menu }))?;
        }

        Ok(SmokeWorldOutcome {
            initial_block_id: initial_block.block_id,
            updated_block_id: updated_block.block_id,
            villager_name: villager_snap.custom_name,
            player_name: player_snap.name,
            granted_item,
            opened_and_closed_menu: open_menu.is_some(),
        })
    }
}

/// Summary of the capability-facade world/entity/player/item exercise.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmokeWorldOutcome {
    pub initial_block_id: String,
    pub updated_block_id: String,
    pub villager_name: Option<String>,
    pub player_name: String,
    pub granted_item: Option<(String, i32)>,
    pub opened_and_closed_menu: bool,
}

impl NativePlugin for SmokePlugin {
    fn descriptor(&self) -> PluginDescriptor {
        PluginDescriptor {
            name: Self::PLUGIN_NAME.to_owned(),
            version: Self::PLUGIN_VERSION.to_owned(),
            target_api: ApiVersion::CURRENT,
            required_capabilities: Self::required_capabilities(),
            optional_capabilities: CapabilitySet::new(),
            loading_mode: LoadingMode::StaticInProcess,
        }
    }

    fn on_load(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        let handle = ctx
            .alloc_resource("smoke-config:v1")
            .map_err(|e| format!("alloc_resource failed: {e}"))?;
        self.resource = Some(handle);
        ctx.log("loaded smoke plugin");
        self.telemetry
            .borrow_mut()
            .lifecycle_events
            .push("on_load".to_owned());
        Ok(())
    }

    fn on_enable(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        let t_ev = Rc::clone(&self.telemetry);
        let listener = ctx
            .subscribe_event::<SmokeBlockPlaceEvent, _>(
                EventPriority::Normal,
                false,
                move |event| {
                    let mut telem = t_ev.borrow_mut();
                    telem.events_seen += 1;
                    event.annotations.push("smoke-inspected".to_owned());
                    if event.block_id == "minecraft:tnt" {
                        event.set_cancelled(true);
                        telem.events_cancelled += 1;
                    }
                },
            )
            .map_err(|e| format!("subscribe_event failed: {e}"))?;
        self.listener = Some(listener);

        let t_cmd = Rc::clone(&self.telemetry);
        let spec = CommandSpec::new("smoke-status")
            .with_description("Report native smoke plugin status")
            .with_aliases(["sstat"]);
        let cmd = ctx
            .register_command(spec, move |sender, label, args| {
                let mut telem = t_cmd.borrow_mut();
                telem.commands_executed.push(format!(
                    "{}:{label}:{}",
                    sender.name(),
                    args.join(",")
                ));
                Ok(())
            })
            .map_err(|e| format!("register_command failed: {e}"))?;
        self.status_command = Some(cmd);

        let t_delay = Rc::clone(&self.telemetry);
        let delayed = ctx
            .schedule_delayed(2, move |_tctx| {
                t_delay.borrow_mut().delayed_runs += 1;
            })
            .map_err(|e| format!("schedule_delayed failed: {e}"))?;
        self.delayed_task = Some(delayed);

        let t_rep = Rc::clone(&self.telemetry);
        let repeating = ctx
            .schedule_repeating(1, 2, move |_tctx| {
                t_rep.borrow_mut().repeating_runs += 1;
            })
            .map_err(|e| format!("schedule_repeating failed: {e}"))?;
        self.repeating_task = Some(repeating);

        ctx.log("enabled smoke plugin");
        self.telemetry
            .borrow_mut()
            .lifecycle_events
            .push("on_enable".to_owned());
        Ok(())
    }

    fn on_disable(&mut self, ctx: &mut PluginContext<'_>) -> Result<(), String> {
        ctx.log("disabled smoke plugin");
        self.telemetry
            .borrow_mut()
            .lifecycle_events
            .push("on_disable".to_owned());
        Ok(())
    }
}

/// Result of running the Paper-independent native smoke acceptance suite (#509).
///
/// Explicitly reported separately from `#251` (M4 JVM Tier 1 corpus gate).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SmokeAcceptanceReport {
    pub non_terminal_gate: &'static str,
    pub lifecycle_passed: bool,
    pub events_passed: bool,
    pub commands_passed: bool,
    pub scheduler_passed: bool,
    pub world_facade_passed: bool,
    pub cleanup_passed: bool,
    pub controlled_negatives_passed: Vec<&'static str>,
}

impl SmokeAcceptanceReport {
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.lifecycle_passed
            && self.events_passed
            && self.commands_passed
            && self.scheduler_passed
            && self.world_facade_passed
            && self.cleanup_passed
            && self.controlled_negatives_passed.len() == 6
    }
}

/// Runs the complete Paper-independent native acceptance suite and controlled
/// negatives for `#509`.
pub fn run_native_smoke_acceptance() -> Result<SmokeAcceptanceReport, String> {
    let caps = SmokePlugin::required_capabilities();
    let mut host = NativePluginHost::new(ApiVersion::CURRENT, caps.clone());
    let telemetry = Rc::new(RefCell::new(SmokeTelemetry::default()));
    let plugin = SmokePlugin::new(Rc::clone(&telemetry));

    let handle = host
        .register(Box::new(plugin))
        .map_err(|e| format!("register failed: {e}"))?;
    host.load(handle).map_err(|e| format!("load failed: {e}"))?;
    host.enable(handle)
        .map_err(|e| format!("enable failed: {e}"))?;
    let lifecycle_passed = host.state(handle) == Ok(PluginState::Enabled);

    // 1. Events: normal + cancelled.
    let mut world_state = SyntheticWorldState::new();
    let world_id = world_state.create_world("world", "minecraft:overworld", -64, 384);
    world_state
        .load_chunk(world_id, ChunkPos::new(0, -1))
        .map_err(|e| format!("load_chunk failed: {e}"))?;

    let mut stone_ev =
        SmokeBlockPlaceEvent::new(world_id, BlockPos::new(1, 64, 1), "minecraft:stone");
    let stone_report = host.dispatch_event(&mut stone_ev);
    let mut tnt_ev = SmokeBlockPlaceEvent::new(world_id, BlockPos::new(2, 64, 2), "minecraft:tnt");
    let tnt_report = host.dispatch_event(&mut tnt_ev);
    let events_passed = stone_report.invoked == 1
        && !Cancellable::is_cancelled(&stone_ev)
        && tnt_report.invoked == 1
        && Cancellable::is_cancelled(&tnt_ev)
        && telemetry.borrow().events_seen == 2
        && telemetry.borrow().events_cancelled == 1;

    // 2. Commands: primary label + alias.
    let sender = CommandSender::Console;
    let r1 = host.dispatch_command(&sender, "smoke-status check");
    let r2 = host.dispatch_command(&sender, "sstat");
    let commands_passed = matches!(r1, CommandDispatchOutcome::Handled { .. })
        && matches!(r2, CommandDispatchOutcome::Handled { .. })
        && telemetry.borrow().commands_executed.len() == 2;

    // 3. Synchronous scheduler: 4 ticks -> delayed runs once at tick 2, repeating runs at ticks 1, 3.
    for _ in 0..4 {
        let rep = host.tick_scheduler();
        if !rep.panics.is_empty() {
            return Err("unexpected scheduler panic during smoke acceptance".to_owned());
        }
    }
    let scheduler_passed =
        telemetry.borrow().delayed_runs == 1 && telemetry.borrow().repeating_runs == 2;

    // 4. World/entity/player/item/inventory/menu capability facades.
    let world_outcome = SmokePlugin::exercise_world_state(&mut world_state, &caps, world_id)
        .map_err(|e| format!("exercise_world_state failed: {e}"))?;
    let world_facade_passed = world_outcome.initial_block_id == "minecraft:air"
        && world_outcome.updated_block_id == "minecraft:stone_bricks"
        && world_outcome.villager_name.as_deref() == Some("SmokeGuide")
        && world_outcome.player_name == "SmokeTester"
        && world_outcome.granted_item == Some(("minecraft:diamond_pickaxe".to_owned(), 1))
        && world_outcome.opened_and_closed_menu;

    // 5. Disable & unload -> automatic cleanup of listeners, commands, tasks, resources.
    host.disable(handle)
        .map_err(|e| format!("disable failed: {e}"))?;
    host.unload(handle)
        .map_err(|e| format!("unload failed: {e}"))?;

    let mut post_unload_ev =
        SmokeBlockPlaceEvent::new(world_id, BlockPos::new(3, 64, 3), "minecraft:tnt");
    let post_ev_report = host.dispatch_event(&mut post_unload_ev);
    let post_cmd = host.dispatch_command(&sender, "smoke-status");
    let runs_before = telemetry.borrow().repeating_runs;
    let _ = host.tick_scheduler();
    let _ = host.tick_scheduler();
    let cleanup_passed = post_ev_report.invoked == 0
        && !Cancellable::is_cancelled(&post_unload_ev)
        && matches!(post_cmd, CommandDispatchOutcome::UnknownCommand)
        && telemetry.borrow().repeating_runs == runs_before
        && matches!(
            host.state(handle),
            Err(LifecycleError::Handle(HandleError::Stale { .. }))
        );

    // 6. Controlled negatives.
    let mut controlled_negatives_passed = Vec::new();

    // CN1: Missing required capability rejected at registration.
    let mut restricted_host =
        NativePluginHost::new(ApiVersion::CURRENT, CapabilitySet::synthetic_default());
    let cn1 = restricted_host.register(Box::new(SmokePlugin::new(Rc::new(RefCell::new(
        SmokeTelemetry::default(),
    )))));
    if matches!(
        cn1,
        Err(LifecycleError::Negotiation(
            NegotiationError::MissingCapabilities(_)
        ))
    ) {
        controlled_negatives_passed.push("missing_required_capability_rejected");
    }

    // CN2: Incompatible API major version rejected at registration.
    let mut future_host = NativePluginHost::new(ApiVersion::new(2, 0, 0), caps.clone());
    let cn2 = future_host.register(Box::new(SmokePlugin::new(Rc::new(RefCell::new(
        SmokeTelemetry::default(),
    )))));
    if matches!(
        cn2,
        Err(LifecycleError::Negotiation(
            NegotiationError::IncompatibleApiVersion { .. }
        ))
    ) {
        controlled_negatives_passed.push("incompatible_api_version_rejected");
    }

    // CN3: Stale handle after entity removal & slot reuse fails closed.
    let pig = world_state
        .spawn_entity(world_id, "minecraft:pig", 1.0, 64.0, 1.0)
        .map_err(|e| format!("spawn_entity failed: {e}"))?;
    world_state
        .remove_entity(pig)
        .map_err(|e| format!("remove_entity failed: {e}"))?;
    let _cow = world_state
        .spawn_entity(world_id, "minecraft:cow", 2.0, 64.0, 2.0)
        .map_err(|e| format!("spawn_entity failed: {e}"))?;
    {
        let view = world_state.view(&caps);
        if matches!(
            view.get_entity(pig),
            Err(FacadeError::EntityStale(HandleError::Stale { .. }))
        ) {
            controlled_negatives_passed.push("stale_entity_handle_rejected");
        }
    }

    // CN4: Off-thread facade access fails closed with NotOnTickThread.
    let mut off_thread_state = SyntheticWorldState::new();
    let off_world = off_thread_state.create_world("world", "minecraft:overworld", -64, 384);
    let off_caps = caps.clone();
    let off_result = std::thread::spawn(move || {
        let view = off_thread_state.view(&off_caps);
        view.world_info(off_world).err()
    })
    .join()
    .map_err(|_| "off-thread test panicked".to_owned())?;
    if matches!(off_result, Some(FacadeError::NotOnTickThread)) {
        controlled_negatives_passed.push("off_thread_access_rejected");
    }

    // CN5: Unverified M3 provider fails closed with ProviderUnverified (never silently skipped).
    {
        let view = world_state.view(&caps);
        if matches!(
            view.require_provider("m3.world.level"),
            Err(FacadeError::ProviderUnverified {
                provider: "m3.world.level",
                ..
            })
        ) {
            controlled_negatives_passed.push("missing_m3_provider_unverified");
        }
    }

    // CN6: Capability denied on mutation when plugin lacks WorldWrite.
    let read_only_caps = CapabilitySet::from_iter([Capability::WorldRead]);
    {
        let mut view = world_state.view(&read_only_caps);
        let cn6 = view.execute(TickCommand::World(WorldCommand::SetBlock {
            world: world_id,
            pos: BlockPos::new(4, 64, -2),
            state: BlockStateSnapshot::simple("minecraft:bedrock"),
        }));
        if matches!(
            cn6,
            Err(FacadeError::CapabilityDenied(Capability::WorldWrite))
        ) {
            controlled_negatives_passed.push("capability_denied_on_mutation");
        }
    }

    // Keep CommandOutcome import used in public doc-tested contracts if needed.
    let _ = std::any::type_name::<CommandOutcome>();

    Ok(SmokeAcceptanceReport {
        non_terminal_gate: "native-rust-api-non-terminal (separate from #251 JVM Tier 1 gate)",
        lifecycle_passed,
        events_passed,
        commands_passed,
        scheduler_passed,
        world_facade_passed,
        cleanup_passed,
        controlled_negatives_passed,
    })
}
