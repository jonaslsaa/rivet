//! Port of `net.minecraft.server.level.ServerPlayerGameMode` — the
//! block-digging state machine and the `useItemOn` / `useItem` entry points
//! (issue #448).
//!
//! Java: `working/Paper/paper-server/src/minecraft/java/net/minecraft/server/
//! level/ServerPlayerGameMode.java` (Paper 26.2, `0a993450`): `tick`,
//! `incrementDestroyProgress`, `handleBlockBreakAction`, `destroyAndAck`,
//! `useItem`, `useItemOn`, and the Paper/CraftBukkit fields they touch
//! (`captureSentBlockEntities`, `capturedBlockEntity`, `interactResult`).
//!
//! All state is tick-thread owned (no `Arc<RwLock>`). Effects aimed at the
//! acting player or its connection are appended, in Java call order, to an
//! [`InteractionEffect`] list for E9 to apply; level, block, item and
//! `destroyBlock` behaviour is reached through the [`InteractionWorld`] seam.
//!
//! Not ported here (outside this class or not yet available):
//! - `changeGameModeForPlayer` / `setGameModeForPlayer` (game-mode changes);
//!   `gameModeForPlayer` is read from [`InteractionPlayer::game_type`].
//! - `destroyBlock` (block removal, drops, experience): the
//!   [`InteractionWorld::destroy_block`] seam.
//! - The Anti-Xray `chunkPacketBlockController.onPlayerLeftClickBlock` hook at
//!   the end of `handleBlockBreakAction`: with Anti-Xray disabled (Paper's
//!   default) the controller is `NO_OPERATION_INSTANCE` and the call is a no-op.

use rivet_protocol::game::ClientboundBlockChangedAckPacket;
use rivet_protocol::game::serversbound_player_action_packet::Action as PlayerAction;
use rivet_protocol::protocol::game::clientbound_block_updates::ClientboundBlockUpdatePacket;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockHitResult, BlockPos, Direction, GameType, InteractionHand, Vec3};
use rivet_world::interaction_result::InteractionResult;

/// `Attributes.BLOCK_INTERACTION_RANGE` default value (`4.5`).
pub const DEFAULT_BLOCK_INTERACTION_RANGE: f64 = 4.5;

/// Exact `java.lang.Math.max(double, double)`: NaN-propagating, and `+0.0`
/// wins over `-0.0`.
fn java_math_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    if a == 0.0 && b == 0.0 {
        return if a.is_sign_positive() || b.is_sign_positive() {
            0.0
        } else {
            -0.0
        };
    }
    if a >= b { a } else { b }
}

/// `new AABB(pos).distanceToSqr(point)` (`AABB(BlockPos)` spans
/// `pos .. pos + 1`; `Mth.lengthSquared(dx, dy, dz)`).
pub fn block_aabb_distance_to_sqr(pos: BlockPos, point: Vec3) -> f64 {
    let min_x = pos.get_x() as f64;
    let min_y = pos.get_y() as f64;
    let min_z = pos.get_z() as f64;
    let max_x = pos.get_x().wrapping_add(1) as f64;
    let max_y = pos.get_y().wrapping_add(1) as f64;
    let max_z = pos.get_z().wrapping_add(1) as f64;
    let dx = java_math_max(java_math_max(min_x - point.x, point.x - max_x), 0.0);
    let dy = java_math_max(java_math_max(min_y - point.y, point.y - max_y), 0.0);
    let dz = java_math_max(java_math_max(min_z - point.z, point.z - max_z), 0.0);
    dx * dx + dy * dy + dz * dz
}

/// Tick-thread snapshot of the acting `ServerPlayer` (and the listener state
/// the interaction handlers read), supplied by E9 for each call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionPlayer {
    /// `player.getId()`.
    pub id: i32,
    /// `player.getEyePosition()`.
    pub eye_position: Vec3,
    /// `player.getYRot()`.
    pub y_rot: f32,
    /// `player.getXRot()`.
    pub x_rot: f32,
    /// `ServerPlayerGameMode.gameModeForPlayer`.
    pub game_type: GameType,
    /// `player.blockInteractionRange()` (`Attributes.BLOCK_INTERACTION_RANGE`).
    pub block_interaction_range: f64,
    /// `player.mayBuild()` (`getAbilities().mayBuild`).
    pub may_build: bool,
    /// `player.getAbilities().instabuild`.
    pub instabuild: bool,
    /// `player.isSecondaryUseActive()`.
    pub is_secondary_use_active: bool,
    /// `player.isImmobile()`.
    pub is_immobile: bool,
    /// `ServerGamePacketListenerImpl.hasClientLoaded()`.
    pub has_client_loaded: bool,
    /// `ServerGamePacketListenerImpl.awaitingPositionFromClient != null`.
    pub awaiting_position_from_client: bool,
}

impl InteractionPlayer {
    /// A player with Paper's default abilities for `game_type`
    /// (`GameType.updatePlayerAbilities`: `instabuild` only in creative,
    /// `mayBuild` unless block placing is restricted) and the default `4.5`
    /// block interaction range.
    pub fn new(id: i32, eye_position: Vec3, game_type: GameType) -> Self {
        InteractionPlayer {
            id,
            eye_position,
            y_rot: 0.0,
            x_rot: 0.0,
            game_type,
            block_interaction_range: DEFAULT_BLOCK_INTERACTION_RANGE,
            may_build: !game_type.is_block_placing_restricted(),
            instabuild: game_type == GameType::Creative,
            is_secondary_use_active: false,
            is_immobile: false,
            has_client_loaded: true,
            awaiting_position_from_client: false,
        }
    }

    /// `Player.isWithinBlockInteractionRange(BlockPos pos, double buffer)`.
    pub fn is_within_block_interaction_range(&self, pos: BlockPos, buffer: f64) -> bool {
        let max_range = self.block_interaction_range + buffer;
        block_aabb_distance_to_sqr(pos, self.eye_position) < max_range * max_range
    }

    /// `Player.blockActionRestricted(Level level, BlockPos pos, GameType gameType)`.
    pub fn block_action_restricted(
        &self,
        world: &dyn InteractionWorld,
        pos: BlockPos,
        game_type: GameType,
    ) -> bool {
        if !game_type.is_block_placing_restricted() {
            return false;
        }
        if game_type == GameType::Spectator {
            return true;
        }
        if self.may_build {
            return false;
        }
        world.is_hand_empty(self, InteractionHand::MainHand)
            || !world.main_hand_can_break_block_in_adventure_mode(self, pos)
    }
}

/// Effects on the acting player / its connection, in Java call order, for E9
/// to apply.
#[derive(Debug, Clone, PartialEq)]
pub enum InteractionEffect {
    /// `connection.send(new ClientboundBlockChangedAckPacket(sequence))`.
    BlockChangedAck(ClientboundBlockChangedAckPacket),
    /// `connection.send(new ClientboundBlockUpdatePacket(pos, state))`.
    BlockUpdate(ClientboundBlockUpdatePacket),
    /// `BlockEntity be = level.getBlockEntity(pos); if (be != null)
    /// connection.send(be.getUpdatePacket())`.
    BlockEntityUpdate { pos: BlockPos },
    /// `disconnect(Component.literal(reason), PlayerKickEvent.Cause.ILLEGAL_ACTION)`.
    Disconnect { reason: String },
    /// `player.swing(hand, true)` — `LivingEntity.swing` decides whether a
    /// `ClientboundAnimatePacket` is (re)broadcast to trackers and self.
    Swing(InteractionHand),
    /// `player.absSnapRotationTo(yRot, xRot)`.
    AbsSnapRotationTo { y_rot: f32, x_rot: f32 },
    /// `player.resetLastActionTime()`.
    ResetLastActionTime,
    /// `player.stopUsingItem()`.
    StopUsingItem,
    /// `player.resyncUsingItem(player)` (Paper).
    ResyncUsingItem,
    /// `player.detectEquipmentUpdates()` (Paper,
    /// `unsupportedSettings.updateEquipmentOnPlayerActions`, default `true`).
    DetectEquipmentUpdates,
    /// `player.sendBuildLimitMessage(boolean above, int limit)`.
    SendBuildLimitMessage { above: bool, limit: i32 },
    /// `player.sendSpawnProtectionMessage(pos)`.
    SendSpawnProtectionMessage { pos: BlockPos },
    /// `player.containerMenu.sendAllDataToRemote()`.
    ContainerSendAllDataToRemote,
    /// `player.containerMenu.forceHeldSlot(hand)`.
    ContainerForceHeldSlot(InteractionHand),
    /// `connection.send(new ClientboundContainerClosePacket(player.containerMenu.containerId))`.
    ContainerClose,
    /// `player.getBukkitEntity().sendHealthUpdate()`.
    SendHealthUpdate,
}

/// The level, block, item, and `destroyBlock` behaviour `ServerPlayerGameMode`
/// and the world-interaction handlers call into. Every method names the Java
/// expression it stands for. "The hand item" is the `ItemStack` the handler
/// captured from `player.getItemInHand(hand)` for the current packet.
pub trait InteractionWorld {
    /// `level.getChunkIfLoadedImmediately(chunkX, chunkZ) != null`.
    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// `level.getMinY()`.
    fn get_min_y(&self) -> i32;

    /// `level.getMaxY()`.
    fn get_max_y(&self) -> i32;

    /// `level.getBlockState(pos)`.
    fn get_block_state(&self, pos: BlockPos) -> BlockState;

    /// `level.getBlockStateIfLoaded(pos)` (Paper).
    fn get_block_state_if_loaded(&self, pos: BlockPos) -> Option<BlockState>;

    /// `server.isUnderSpawnProtection(level, pos, player)`.
    fn is_under_spawn_protection(&self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// `level.mayInteract(player, pos)`.
    fn may_interact(&self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// `level.paperConfig().spawn.allowUsingSignsInsideSpawnProtection &&
    /// level.getBlockState(pos).getBlock() instanceof SignBlock`.
    fn is_sign_interaction_passthrough(&self, pos: BlockPos) -> bool;

    /// `level.destroyBlockProgress(breakerId, pos, progress)` — fans the
    /// `ClientboundBlockDestructionPacket` out to the *other* players of the
    /// level within 32 blocks who can see the breaker (`ServerLevel.java`
    /// 1719-1749), never to the breaker itself.
    fn destroy_block_progress(&mut self, breaker_id: i32, pos: BlockPos, progress: i32);

    /// `state.getBlock().isEnabled(level.enabledFeatures())` for the state at `pos`.
    fn is_block_enabled(&self, pos: BlockPos) -> bool;

    /// `state.getDestroyProgress(player, level, pos)` for the state at `pos`.
    fn get_destroy_progress(&self, player: &InteractionPlayer, pos: BlockPos) -> f32;

    /// `EnchantmentHelper.onHitBlock(level, mainHand, player, player,
    /// MAINHAND, Vec3.atCenterOf(pos), state, ...)` then
    /// `state.attack(level, pos, player)`.
    fn attack_block(&mut self, player: &InteractionPlayer, pos: BlockPos);

    /// `state.getMenuProvider(level, pos) != null`.
    fn has_menu_provider(&self, pos: BlockPos) -> bool;

    /// `player.openMenu(state.getMenuProvider(level, pos)).isPresent()`.
    fn open_menu(&mut self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// `state.getBlock() instanceof CakeBlock`.
    fn is_cake_block(&self, pos: BlockPos) -> bool;

    /// `state.is(Blocks.JIGSAW) || state.is(Blocks.STRUCTURE_BLOCK) ||
    /// state.getBlock() instanceof CommandBlock`.
    fn is_jigsaw_structure_or_command_block(&self, pos: BlockPos) -> bool;

    /// `state.useItemOn(player.getItemInHand(hand), level, player, hand, hitResult)`.
    fn block_use_item_on(
        &mut self,
        player: &InteractionPlayer,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
    ) -> InteractionResult;

    /// `state.useWithoutItem(level, player, hitResult)`.
    fn block_use_without_item(
        &mut self,
        player: &InteractionPlayer,
        hit_result: &BlockHitResult,
    ) -> InteractionResult;

    /// `ServerPlayerGameMode.destroyBlock(pos)`.
    fn destroy_block(&mut self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// The hand item's `isEmpty()`.
    fn is_hand_empty(&self, player: &InteractionPlayer, hand: InteractionHand) -> bool;

    /// The hand item's `isItemEnabled(level.enabledFeatures())`.
    fn is_hand_item_enabled(&self, player: &InteractionPlayer, hand: InteractionHand) -> bool;

    /// `player.getCooldowns().isOnCooldown(<hand item>)`.
    fn is_hand_on_cooldown(&self, player: &InteractionPlayer, hand: InteractionHand) -> bool;

    /// `item instanceof BlockItem || item instanceof BucketItem bucket &&
    /// bucket.getContent() != Fluids.EMPTY` for the hand item's `getItem()`.
    fn is_hand_block_placement_item(
        &self,
        player: &InteractionPlayer,
        hand: InteractionHand,
    ) -> bool;

    /// `player.getMainHandItem().canBreakBlockInAdventureMode(new
    /// BlockInWorld(level, pos, false))`.
    fn main_hand_can_break_block_in_adventure_mode(
        &self,
        player: &InteractionPlayer,
        pos: BlockPos,
    ) -> bool;

    /// Spigot: `player.getMainHandItem().is(Items.DEBUG_STICK) &&
    /// DEBUG_STICK.handleInteraction(player, level.getBlockState(pos), level,
    /// pos, false, player.getMainHandItem())`.
    fn handle_debug_stick_left_click(&mut self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// `MCUtil.clientPredictsInteraction(player, level.getBlockState(pos), <hand item>)`.
    fn client_predicts_interaction(
        &self,
        player: &InteractionPlayer,
        pos: BlockPos,
        hand: InteractionHand,
    ) -> bool;

    /// The `itemStack.useOn(new UseOnContext(player, hand, hitResult))` arm of
    /// `useItemOn`, including the `player.hasInfiniteMaterials()` count restore.
    fn item_use_on(
        &mut self,
        player: &InteractionPlayer,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
    ) -> InteractionResult;

    /// The tail of `useItem` after the spectator and cooldown checks:
    /// `itemStack.use(level, player, hand)` and the held-stack bookkeeping that
    /// follows it.
    fn item_use(&mut self, player: &InteractionPlayer, hand: InteractionHand) -> InteractionResult;
}

/// `net.minecraft.server.level.ServerPlayerGameMode` digging and interaction state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPlayerGameMode {
    is_destroying_block: bool,
    destroy_progress_start: i32,
    destroy_pos: BlockPos,
    game_ticks: i32,
    /// `public boolean hasDelayedDestroy`.
    pub has_delayed_destroy: bool,
    delayed_destroy_pos: BlockPos,
    delayed_tick_start: i32,
    last_sent_state: i32,
    /// Paper `captureSentBlockEntities`.
    pub capture_sent_block_entities: bool,
    /// Paper `capturedBlockEntity`.
    pub captured_block_entity: bool,
    /// CraftBukkit `interactResult` — `PlayerInteractEvent.useItemInHand() == DENY`
    /// of the last `useItemOn`.
    pub interact_result: bool,
}

impl Default for ServerPlayerGameMode {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerPlayerGameMode {
    /// `new ServerPlayerGameMode(player)` field initialisers.
    pub const fn new() -> Self {
        ServerPlayerGameMode {
            is_destroying_block: false,
            destroy_progress_start: 0,
            destroy_pos: BlockPos::ZERO,
            game_ticks: 0,
            has_delayed_destroy: false,
            delayed_destroy_pos: BlockPos::ZERO,
            delayed_tick_start: 0,
            last_sent_state: -1,
            capture_sent_block_entities: false,
            captured_block_entity: false,
            interact_result: false,
        }
    }

    pub fn is_destroying_block(&self) -> bool {
        self.is_destroying_block
    }

    pub fn destroy_pos(&self) -> BlockPos {
        self.destroy_pos
    }

    pub fn delayed_destroy_pos(&self) -> BlockPos {
        self.delayed_destroy_pos
    }

    pub fn last_sent_state(&self) -> i32 {
        self.last_sent_state
    }

    /// `tick()` — run from `ServerPlayer.tick()` during the level's entity tick.
    /// `lag_compensation_tick` is `level.getLagCompensationTick()`.
    pub fn tick(
        &mut self,
        lag_compensation_tick: i64,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) {
        // `(int) this.level.getLagCompensationTick()` — narrowing truncation.
        self.game_ticks = lag_compensation_tick as i32;
        if self.has_delayed_destroy {
            match world.get_block_state_if_loaded(self.delayed_destroy_pos) {
                Some(state) if !state.is_air() => {
                    let progress = self.increment_destroy_progress(
                        player,
                        world,
                        self.delayed_destroy_pos,
                        self.delayed_tick_start,
                    );
                    if progress >= 1.0_f32 {
                        self.has_delayed_destroy = false;
                        world.destroy_block(player, self.delayed_destroy_pos);
                    }
                }
                _ => self.has_delayed_destroy = false,
            }
        } else if self.is_destroying_block {
            let Some(state) = world.get_block_state_if_loaded(self.destroy_pos) else {
                self.is_destroying_block = false;
                return;
            };
            if state.is_air() {
                world.destroy_block_progress(player.id, self.destroy_pos, -1);
                self.last_sent_state = -1;
                self.is_destroying_block = false;
            } else {
                self.increment_destroy_progress(
                    player,
                    world,
                    self.destroy_pos,
                    self.destroy_progress_start,
                );
            }
        }
    }

    /// `incrementDestroyProgress(BlockState, BlockPos, int destroyStartTick)`.
    fn increment_destroy_progress(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        pos: BlockPos,
        destroy_start_tick: i32,
    ) -> f32 {
        let ticks_spent_destroying = self.game_ticks.wrapping_sub(destroy_start_tick);
        let destroy_progress =
            world.get_destroy_progress(player, pos) * ticks_spent_destroying.wrapping_add(1) as f32;
        let state = (destroy_progress * 10.0_f32) as i32;
        if state != self.last_sent_state {
            world.destroy_block_progress(player.id, pos, state);
            self.last_sent_state = state;
        }
        destroy_progress
    }

    /// `handleBlockBreakAction(BlockPos pos, Action action, Direction direction,
    /// int maxY, int sequence)`.
    #[allow(clippy::too_many_arguments)]
    pub fn handle_block_break_action(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        pos: BlockPos,
        action: PlayerAction,
        _direction: Direction,
        max_y: i32,
        _sequence: i32,
        out: &mut Vec<InteractionEffect>,
    ) {
        if !player.is_within_block_interaction_range(pos, 1.0) {
            // Paper: `if (true) return;` — don't notify if unreasonably far away.
            return;
        }
        if pos.get_y() > max_y {
            send_block_update(world, pos, out);
            return;
        }
        match action {
            PlayerAction::StartDestroyBlock => self.start_destroy_block(player, world, pos, out),
            PlayerAction::StopDestroyBlock => {
                if pos == self.destroy_pos {
                    let ticks_spent_destroying =
                        self.game_ticks.wrapping_sub(self.destroy_progress_start);
                    let state = world.get_block_state(pos);
                    if !state.is_air() {
                        let destroy_progress = world.get_destroy_progress(player, pos)
                            * ticks_spent_destroying.wrapping_add(1) as f32;
                        if destroy_progress >= 0.7_f32 {
                            self.is_destroying_block = false;
                            world.destroy_block_progress(player.id, pos, -1);
                            self.destroy_and_ack(player, world, pos, out);
                            return;
                        }
                        if !self.has_delayed_destroy {
                            self.is_destroying_block = false;
                            self.has_delayed_destroy = true;
                            self.delayed_destroy_pos = pos;
                            self.delayed_tick_start = self.destroy_progress_start;
                        }
                    }
                }
            }
            PlayerAction::AbortDestroyBlock => {
                self.is_destroying_block = false;
                if self.destroy_pos != pos && self.destroy_pos != BlockPos::ZERO {
                    // Paper: don't load unloaded chunks for stale records.
                    if world.get_block_state_if_loaded(self.destroy_pos).is_some() {
                        world.destroy_block_progress(player.id, self.destroy_pos, -1);
                    }
                    self.destroy_pos = BlockPos::ZERO;
                }
                world.destroy_block_progress(player.id, pos, -1);
                // RivetTodo(#501) CraftEventFactory.callBlockDamageAbortEvent producer site.
            }
            // Java only dispatches the three digging actions here.
            _ => {}
        }
    }

    /// The `START_DESTROY_BLOCK` arm of `handleBlockBreakAction`.
    fn start_destroy_block(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        pos: BlockPos,
        out: &mut Vec<InteractionEffect>,
    ) {
        if world.is_under_spawn_protection(player, pos) {
            out.push(InteractionEffect::SendSpawnProtectionMessage { pos });
            return;
        }
        if !world.may_interact(player, pos) {
            // RivetTodo(#501) PlayerInteractEvent LEFT_CLICK_BLOCK producer site (result unused here).
            send_block_update(world, pos, out);
            self.captured_block_entity = true;
            return;
        }
        // RivetTodo(#501) PlayerInteractEvent LEFT_CLICK_BLOCK producer site. With no listener the event is
        // not cancelled, `useInteractedBlock()` is ALLOW (a block was clicked) and `useItemInHand()` is
        // DEFAULT, so none of its early returns fire.
        if player.instabuild {
            self.destroy_and_ack(player, world, pos, out);
            return;
        }
        if world.handle_debug_stick_left_click(player, pos) {
            return;
        }
        if player.block_action_restricted(world, pos, player.game_type) {
            send_block_update(world, pos, out);
            return;
        }

        self.destroy_progress_start = self.game_ticks;
        let mut progress = 1.0_f32;
        let state = world.get_block_state(pos);
        if !state.is_air() {
            world.attack_block(player, pos);
            progress = world.get_destroy_progress(player, pos);
        }

        // RivetTodo(#501) BlockDamageEvent producer site. With no listener the event is not cancelled and
        // `getInstaBreak()` returns the constructor's `progress >= 1.0F`, which bumps progress to 2.0F.
        if progress >= 1.0_f32 {
            progress = 2.0_f32;
        }

        if !state.is_air() && progress >= 1.0_f32 {
            self.destroy_and_ack(player, world, pos, out);
        } else {
            if self.is_destroying_block {
                send_block_update(world, self.destroy_pos, out);
            }
            self.is_destroying_block = true;
            self.destroy_pos = pos;
            let stage = (progress * 10.0_f32) as i32;
            world.destroy_block_progress(player.id, pos, stage);
            self.last_sent_state = stage;
        }
    }

    /// `destroyAndAck(BlockPos pos, int sequence, String exitId)`.
    pub fn destroy_and_ack(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        pos: BlockPos,
        out: &mut Vec<InteractionEffect>,
    ) {
        if !world.destroy_block(player, pos) {
            send_block_update(world, pos, out);
        }
    }

    /// `useItem(ServerPlayer player, Level level, ItemStack itemStack, InteractionHand hand)`.
    pub fn use_item(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        hand: InteractionHand,
    ) -> InteractionResult {
        if player.game_type == GameType::Spectator {
            return InteractionResult::PASS;
        }
        if world.is_hand_on_cooldown(player, hand) {
            return InteractionResult::PASS;
        }
        world.item_use(player, hand)
    }

    /// `useItemOn(ServerPlayer player, Level level, ItemStack itemStack,
    /// InteractionHand hand, BlockHitResult hitResult)` (CraftBukkit "whole
    /// method" form).
    pub fn use_item_on(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
        out: &mut Vec<InteractionEffect>,
    ) -> InteractionResult {
        let pos = hit_result.get_block_pos();
        if !world.is_block_enabled(pos) {
            return InteractionResult::FAIL;
        }
        let mut cancelled_block = false;
        if player.game_type == GameType::Spectator {
            cancelled_block = !world.has_menu_provider(pos);
        }
        let cancelled_item = world.is_hand_on_cooldown(player, hand);

        // RivetTodo(#501) PlayerInteractEvent RIGHT_CLICK_BLOCK producer site (and the
        // firedInteract/interactPosition/interactHand/interactItemStack record that handleUseItem reads).
        // With no listener the event keeps its constructor defaults plus the two pre-sets:
        // useInteractedBlock() is DENY iff cancelledBlock, useItemInHand() is DENY iff cancelledItem.
        let use_interacted_block_denied = cancelled_block;
        let use_item_in_hand_denied = cancelled_item;
        self.interact_result = use_item_in_hand_denied;

        if use_interacted_block_denied {
            if world.is_cake_block(pos) {
                out.push(InteractionEffect::SendHealthUpdate);
            } else if world.is_jigsaw_structure_or_command_block(pos) {
                out.push(InteractionEffect::ContainerClose);
            }
            if world.client_predicts_interaction(player, pos, hand) {
                out.push(InteractionEffect::ContainerSendAllDataToRemote);
            } else {
                out.push(InteractionEffect::ContainerForceHeldSlot(hand));
            }
            out.push(InteractionEffect::ResyncUsingItem);
            // `useItemInHand() != ALLOW` — DEFAULT or DENY without a listener.
            return InteractionResult::SUCCESS;
        } else if player.game_type == GameType::Spectator {
            if world.has_menu_provider(pos) && world.open_menu(player, pos) {
                return InteractionResult::CONSUME;
            }
            return InteractionResult::PASS;
        }

        let have_something_in_our_hands = !world.is_hand_empty(player, InteractionHand::MainHand)
            || !world.is_hand_empty(player, InteractionHand::OffHand);
        let suppress_using_block = player.is_secondary_use_active && have_something_in_our_hands;
        if !suppress_using_block {
            let item_use = world.block_use_item_on(player, hand, hit_result);
            if item_use.consumes_action() {
                // STUB(mc.advancements.triggers) CriteriaTriggers.ITEM_USED_ON_BLOCK.trigger(player, pos, usedItemStack).
                return item_use;
            }
            if matches!(item_use, InteractionResult::TryEmptyHandInteraction)
                && hand == InteractionHand::MainHand
            {
                let use_result = world.block_use_without_item(player, hit_result);
                if use_result.consumes_action() {
                    // STUB(mc.advancements.triggers) CriteriaTriggers.DEFAULT_BLOCK_USE.trigger(player, pos).
                    return use_result;
                }
            }
        }

        if !world.is_hand_empty(player, hand) && !self.interact_result {
            // STUB(mc.advancements.triggers) CriteriaTriggers.ITEM_USED_ON_BLOCK.trigger on a consuming useOn.
            world.item_use_on(player, hand, hit_result)
        } else {
            // Paper: `interactResult && interactResult != cancelledItem` — never true without a listener.
            if self.interact_result && self.interact_result != cancelled_item {
                out.push(InteractionEffect::ResyncUsingItem);
            }
            InteractionResult::PASS
        }
    }
}

/// `connection.send(new ClientboundBlockUpdatePacket(pos, level.getBlockState(pos)))`.
pub(crate) fn send_block_update(
    world: &dyn InteractionWorld,
    pos: BlockPos,
    out: &mut Vec<InteractionEffect>,
) {
    out.push(InteractionEffect::BlockUpdate(
        ClientboundBlockUpdatePacket {
            pos,
            state: world.get_block_state(pos),
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_aabb_distance_follows_java_math_max() {
        let pos = BlockPos::new(1, 2, 3);
        assert!(block_aabb_distance_to_sqr(pos, Vec3::new(f64::NAN, 2.5, 3.5)).is_nan());
        assert!(block_aabb_distance_to_sqr(pos, Vec3::new(1.5, f64::NAN, 3.5)).is_nan());
        assert!(block_aabb_distance_to_sqr(pos, Vec3::new(1.5, 2.5, f64::NAN)).is_nan());
        assert_eq!(
            block_aabb_distance_to_sqr(pos, Vec3::new(1.5, 2.5, 3.5)),
            0.0
        );
        assert_eq!(
            block_aabb_distance_to_sqr(pos, Vec3::new(4.0, 2.5, 3.5)),
            4.0
        );
    }

    #[test]
    fn reach_check_squares_a_negative_range_like_java() {
        // `maxRange * maxRange` — Java does not reject a negative range.
        let mut player = InteractionPlayer::new(1, Vec3::new(0.5, 0.5, 0.5), GameType::Survival);
        player.block_interaction_range = -10.0;
        assert!(player.is_within_block_interaction_range(BlockPos::new(5, 0, 0), 1.0));
        player.eye_position = Vec3::new(f64::NAN, 0.5, 0.5);
        assert!(!player.is_within_block_interaction_range(BlockPos::ZERO, 1.0));
    }
}
