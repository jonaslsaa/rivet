//! Paper 26.2 `ServerPlayerGameMode` and `InteractionResult` port.
//!
//! Mirrors `working/Paper` commit `0a993450f129c4942c2a9ed45ba047412b4667cf`:
//! - `net.minecraft.world.InteractionResult`
//! - `net.minecraft.server.level.ServerPlayerGameMode`
//!   (`tick`, `incrementDestroyProgress`, `handleBlockBreakAction`, `destroyAndAck`,
//!   `useItemOn`, `useItem`)
//!
//! All state is tick-thread owned (no `Arc<RwLock>`). Side effects are expressed as
//! ordered [`CorrectionPacket`] values and mutations through [`InteractionWorld`].

use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockPos, Direction, GameType, Vec3};

use super::packets::{
    BlockHitResult, ClientboundBlockChangedAckPacket, ClientboundBlockDestructionPacket,
    ClientboundBlockUpdatePacket, InteractionHand, PlayerAction,
};
use super::validation::{
    DEFAULT_BLOCK_INTERACTION_RANGE, INTERACTION_RANGE_BUFFER, is_within_block_interaction_range,
};

/// `InteractionResult.SwingSource`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SwingSource {
    None,
    Client,
    Server,
}

/// `InteractionResult.PaperSuccessContext`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaperSuccessContext {
    pub placed_pos: Option<BlockPos>,
    pub updated_block_entity: bool,
}

impl PaperSuccessContext {
    pub fn placed_block_at(self, pos: BlockPos) -> Self {
        Self {
            placed_pos: Some(pos),
            updated_block_entity: self.updated_block_entity,
        }
    }

    pub fn updated_block_entity(self, updated_block_entity: bool) -> Self {
        Self {
            placed_pos: self.placed_pos,
            updated_block_entity,
        }
    }
}

/// `net.minecraft.world.InteractionResult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractionResult {
    Success {
        swing_source: SwingSource,
        was_item_interaction: bool,
        paper_context: PaperSuccessContext,
    },
    Fail,
    Pass,
    TryEmptyHandInteraction,
}

impl InteractionResult {
    pub const SUCCESS: Self = Self::Success {
        swing_source: SwingSource::Client,
        was_item_interaction: true,
        paper_context: PaperSuccessContext {
            placed_pos: None,
            updated_block_entity: false,
        },
    };

    pub const SUCCESS_SERVER: Self = Self::Success {
        swing_source: SwingSource::Server,
        was_item_interaction: true,
        paper_context: PaperSuccessContext {
            placed_pos: None,
            updated_block_entity: false,
        },
    };

    pub const CONSUME: Self = Self::Success {
        swing_source: SwingSource::None,
        was_item_interaction: true,
        paper_context: PaperSuccessContext {
            placed_pos: None,
            updated_block_entity: false,
        },
    };

    /// `InteractionResult.consumesAction()`.
    pub fn consumes_action(self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// `Success.swingSource()` when `self` is `Success`.
    pub fn swing_source(self) -> Option<SwingSource> {
        match self {
            Self::Success { swing_source, .. } => Some(swing_source),
            _ => None,
        }
    }
}

/// Tick-thread view of the acting player passed into the interaction service.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionPlayer {
    pub player_id: i32,
    pub eye_position: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub game_type: GameType,
    pub block_interaction_range: f64,
    pub may_build: bool,
    pub is_immobile: bool,
    pub has_client_loaded: bool,
    pub is_secondary_use_active: bool,
}

impl InteractionPlayer {
    /// Creates a standard survival/creative/adventure/spectator player view with Paper's
    /// default `4.5` block interaction range.
    pub fn new(player_id: i32, eye_position: Vec3, game_type: GameType) -> Self {
        Self {
            player_id,
            eye_position,
            yaw: 0.0,
            pitch: 0.0,
            game_type,
            block_interaction_range: DEFAULT_BLOCK_INTERACTION_RANGE,
            may_build: !game_type.is_block_placing_restricted(),
            is_immobile: false,
            has_client_loaded: true,
            is_secondary_use_active: false,
        }
    }

    /// `Player.isWithinBlockInteractionRange(pos, buffer)`.
    pub fn is_within_block_interaction_range(&self, pos: BlockPos, buffer: f64) -> bool {
        is_within_block_interaction_range(
            self.eye_position,
            self.block_interaction_range,
            pos,
            buffer,
        )
    }
}

/// Authoritative outbound packets and player-state side effects emitted by the service
/// for E9 (`ServerGamePacketListenerImpl`) to dispatch in exact order.
#[derive(Debug, Clone, PartialEq)]
pub enum CorrectionPacket {
    /// `ClientboundBlockChangedAckPacket` flushed on tick.
    BlockChangedAck(ClientboundBlockChangedAckPacket),
    /// `ClientboundBlockUpdatePacket` restoring or confirming a single block state.
    BlockUpdate(ClientboundBlockUpdatePacket),
    /// `ClientboundBlockDestructionPacket` crack-stage broadcast (`-1` clears).
    BlockDestruction(ClientboundBlockDestructionPacket),
    /// Disconnect player due to illegal action (e.g. negative sequence number).
    Disconnect { reason: String },
    /// Server-triggered arm swing (`player.swing(hand, true)`).
    SwingArm(InteractionHand),
    /// Rotation snap triggered by `ServerboundUseItemPacket`.
    SnapRotation { yaw: f32, pitch: f32 },
}

/// World-level callbacks required by `ServerPlayerGameMode` and `WorldInteractionService`.
///
/// This seam keeps `crates/rivet-server/src/server/interaction/` decoupled from
/// `ServerLevel` internals while letting E9 / `ServerLevel` plug in real chunk storage,
/// live block mutation, and item/block behavior.
pub trait InteractionWorld {
    /// `ServerLevel.getChunkIfLoadedImmediately(chunkX, chunkZ) != null`.
    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool;

    /// `Level.getMaxY()` (inclusive maximum buildable Y coordinate, e.g. `319` in Overworld).
    fn max_y(&self) -> i32;

    /// `Level.getBlockStateIfLoaded(pos)`.
    fn get_block_state_if_loaded(&self, pos: BlockPos) -> Option<BlockState>;

    /// `Level.mayInteract(player, pos)` (spawn protection / world border / permissions).
    fn may_interact(&self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// `Player.blockActionRestricted(level, pos, gameType)` (Adventure CanDestroy / Spectator).
    fn block_action_restricted(&self, player: &InteractionPlayer, _pos: BlockPos) -> bool {
        if !player.game_type.is_block_placing_restricted() {
            return false;
        }
        if player.game_type == GameType::Spectator {
            return true;
        }
        !player.may_build
    }

    /// `BlockState.getDestroyProgress(player, level, pos)` — progress added per tick.
    /// Return `>= 1.0` for instant-break blocks (e.g. torches, creative, high efficiency),
    /// `0.0` for unbreakable blocks (e.g. bedrock).
    fn destroy_progress_per_tick(&self, player: &InteractionPlayer, pos: BlockPos) -> f32;

    /// `ServerPlayerGameMode.destroyBlock(pos)` — removes the block in the authoritative world.
    /// Returns `true` if the block was actually destroyed, `false` if cancelled/denied.
    fn destroy_block(&mut self, player: &InteractionPlayer, pos: BlockPos) -> bool;

    /// `BlockState.attack(level, pos, player)` invoked when survival digging starts.
    fn on_block_attack(&mut self, _player: &InteractionPlayer, _pos: BlockPos) {}

    /// Whether the player's held item in `hand` is empty.
    fn is_hand_empty(&self, _player: &InteractionPlayer, _hand: InteractionHand) -> bool {
        false
    }

    /// Whether the player's held item in `hand` is enabled under `level.enabledFeatures()`.
    fn is_hand_item_enabled(&self, _player: &InteractionPlayer, _hand: InteractionHand) -> bool {
        true
    }

    /// `player.getCooldowns().isOnCooldown(itemStack)`.
    fn is_hand_on_cooldown(&self, _player: &InteractionPlayer, _hand: InteractionHand) -> bool {
        false
    }

    /// `BlockState.useItemOn(itemStack, level, player, hand, hitResult)`.
    fn block_use_item_on(
        &mut self,
        _player: &InteractionPlayer,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        InteractionResult::TryEmptyHandInteraction
    }

    /// `BlockState.useWithoutItem(level, player, hitResult)`.
    fn block_use_without_item(
        &mut self,
        _player: &InteractionPlayer,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// `ItemStack.useOn(UseOnContext)` (e.g. placing a block or using a tool on a block).
    fn item_use_on(
        &mut self,
        _player: &InteractionPlayer,
        _hand: InteractionHand,
        _hit_result: &BlockHitResult,
    ) -> InteractionResult {
        InteractionResult::Pass
    }

    /// `ItemStack.use(level, player, hand)` (e.g. eating, throwing, bow draw).
    fn item_use(
        &mut self,
        _player: &InteractionPlayer,
        _hand: InteractionHand,
    ) -> InteractionResult {
        InteractionResult::Pass
    }
}

/// `net.minecraft.server.level.ServerPlayerGameMode` tick-thread digging & interaction state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerPlayerGameMode {
    is_destroying_block: bool,
    destroy_progress_start: i32,
    destroy_pos: BlockPos,
    game_ticks: i32,
    has_delayed_destroy: bool,
    delayed_destroy_pos: BlockPos,
    delayed_tick_start: i32,
    last_sent_state: i32,
}

impl Default for ServerPlayerGameMode {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerPlayerGameMode {
    /// Creates a fresh `ServerPlayerGameMode` with Paper's initial field values.
    pub const fn new() -> Self {
        Self {
            is_destroying_block: false,
            destroy_progress_start: 0,
            destroy_pos: BlockPos::ZERO,
            game_ticks: 0,
            has_delayed_destroy: false,
            delayed_destroy_pos: BlockPos::ZERO,
            delayed_tick_start: 0,
            last_sent_state: -1,
        }
    }

    pub fn is_destroying_block(&self) -> bool {
        self.is_destroying_block
    }

    pub fn has_delayed_destroy(&self) -> bool {
        self.has_delayed_destroy
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

    /// `ServerPlayerGameMode.tick()`.
    pub fn tick(
        &mut self,
        lag_compensation_tick: i32,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        out: &mut Vec<CorrectionPacket>,
    ) {
        self.game_ticks = lag_compensation_tick;
        if self.has_delayed_destroy {
            match world.get_block_state_if_loaded(self.delayed_destroy_pos) {
                None => {
                    self.has_delayed_destroy = false;
                }
                Some(state) if state.is_air() => {
                    self.has_delayed_destroy = false;
                }
                Some(_) => {
                    let progress = self.increment_destroy_progress(
                        player,
                        world,
                        self.delayed_destroy_pos,
                        self.delayed_tick_start,
                        out,
                    );
                    if progress >= 1.0_f32 {
                        self.has_delayed_destroy = false;
                        let target = self.delayed_destroy_pos;
                        let _ = world.destroy_block(player, target);
                    }
                }
            }
        } else if self.is_destroying_block {
            match world.get_block_state_if_loaded(self.destroy_pos) {
                None => {
                    self.is_destroying_block = false;
                }
                Some(state) if state.is_air() => {
                    out.push(CorrectionPacket::BlockDestruction(
                        ClientboundBlockDestructionPacket::new(
                            player.player_id,
                            self.destroy_pos,
                            -1,
                        ),
                    ));
                    self.last_sent_state = -1;
                    self.is_destroying_block = false;
                }
                Some(_) => {
                    let target = self.destroy_pos;
                    let start = self.destroy_progress_start;
                    let _ = self.increment_destroy_progress(player, world, target, start, out);
                }
            }
        }
    }

    /// `ServerPlayerGameMode.incrementDestroyProgress(BlockState, BlockPos, int)`.
    fn increment_destroy_progress(
        &mut self,
        player: &InteractionPlayer,
        world: &dyn InteractionWorld,
        pos: BlockPos,
        destroy_start_tick: i32,
        out: &mut Vec<CorrectionPacket>,
    ) -> f32 {
        let ticks_spent = self.game_ticks.wrapping_sub(destroy_start_tick);
        let destroy_progress =
            world.destroy_progress_per_tick(player, pos) * (ticks_spent.wrapping_add(1) as f32);
        let stage = (destroy_progress * 10.0_f32) as i32;
        if stage != self.last_sent_state {
            out.push(CorrectionPacket::BlockDestruction(
                ClientboundBlockDestructionPacket::new(player.player_id, pos, stage),
            ));
            self.last_sent_state = stage;
        }
        destroy_progress
    }

    /// `ServerPlayerGameMode.handleBlockBreakAction(pos, action, direction, maxY, sequence)`.
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
        out: &mut Vec<CorrectionPacket>,
    ) {
        if !player.is_within_block_interaction_range(pos, INTERACTION_RANGE_BUFFER) {
            return;
        }
        if pos.get_y() > max_y {
            if let Some(state) = world.get_block_state_if_loaded(pos) {
                out.push(CorrectionPacket::BlockUpdate(
                    ClientboundBlockUpdatePacket { pos, state },
                ));
            }
            return;
        }

        match action {
            PlayerAction::StartDestroyBlock => {
                if !world.may_interact(player, pos) {
                    if let Some(state) = world.get_block_state_if_loaded(pos) {
                        out.push(CorrectionPacket::BlockUpdate(
                            ClientboundBlockUpdatePacket { pos, state },
                        ));
                    }
                    return;
                }

                if player.game_type.is_creative() {
                    self.destroy_and_ack(player, world, pos, out);
                    return;
                }

                if world.block_action_restricted(player, pos) {
                    if let Some(state) = world.get_block_state_if_loaded(pos) {
                        out.push(CorrectionPacket::BlockUpdate(
                            ClientboundBlockUpdatePacket { pos, state },
                        ));
                    }
                    return;
                }

                self.destroy_progress_start = self.game_ticks;
                let mut progress = 1.0_f32;
                let state = world.get_block_state_if_loaded(pos);
                if let Some(block_state) = state
                    && !block_state.is_air()
                {
                    world.on_block_attack(player, pos);
                    progress = world.destroy_progress_per_tick(player, pos);
                }

                if state.is_some_and(|s| !s.is_air()) && progress >= 1.0_f32 {
                    self.destroy_and_ack(player, world, pos, out);
                } else if self.is_destroying_block {
                    if let Some(prev_state) = world.get_block_state_if_loaded(self.destroy_pos) {
                        out.push(CorrectionPacket::BlockUpdate(
                            ClientboundBlockUpdatePacket {
                                pos: self.destroy_pos,
                                state: prev_state,
                            },
                        ));
                    }
                    self.is_destroying_block = false;
                } else {
                    self.is_destroying_block = true;
                    self.destroy_pos = pos;
                    let stage = (progress * 10.0_f32) as i32;
                    out.push(CorrectionPacket::BlockDestruction(
                        ClientboundBlockDestructionPacket::new(player.player_id, pos, stage),
                    ));
                    self.last_sent_state = stage;
                }
            }
            PlayerAction::StopDestroyBlock => {
                if pos == self.destroy_pos {
                    let ticks_spent = self.game_ticks.wrapping_sub(self.destroy_progress_start);
                    if let Some(state) = world.get_block_state_if_loaded(pos)
                        && !state.is_air()
                    {
                        let destroy_progress = world.destroy_progress_per_tick(player, pos)
                            * (ticks_spent.wrapping_add(1) as f32);
                        if destroy_progress >= 0.7_f32 {
                            self.is_destroying_block = false;
                            out.push(CorrectionPacket::BlockDestruction(
                                ClientboundBlockDestructionPacket::new(player.player_id, pos, -1),
                            ));
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
                    if world.get_block_state_if_loaded(self.destroy_pos).is_some() {
                        out.push(CorrectionPacket::BlockDestruction(
                            ClientboundBlockDestructionPacket::new(
                                player.player_id,
                                self.destroy_pos,
                                -1,
                            ),
                        ));
                    }
                    self.destroy_pos = BlockPos::ZERO;
                }
                out.push(CorrectionPacket::BlockDestruction(
                    ClientboundBlockDestructionPacket::new(player.player_id, pos, -1),
                ));
            }
            _ => {}
        }
    }

    /// `ServerPlayerGameMode.destroyAndAck(BlockPos, int, String)`.
    fn destroy_and_ack(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        pos: BlockPos,
        out: &mut Vec<CorrectionPacket>,
    ) {
        if !world.destroy_block(player, pos)
            && let Some(state) = world.get_block_state_if_loaded(pos)
        {
            out.push(CorrectionPacket::BlockUpdate(
                ClientboundBlockUpdatePacket { pos, state },
            ));
        }
    }

    /// `ServerPlayerGameMode.useItemOn(player, level, itemStack, hand, hitResult)`.
    pub fn use_item_on(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        hand: InteractionHand,
        hit_result: &BlockHitResult,
    ) -> InteractionResult {
        let pos = hit_result.block_pos;
        if world.get_block_state_if_loaded(pos).is_none() {
            return InteractionResult::Pass;
        }
        if player.game_type == GameType::Spectator {
            return InteractionResult::Pass;
        }

        let main_empty = world.is_hand_empty(player, InteractionHand::MainHand);
        let off_empty = world.is_hand_empty(player, InteractionHand::OffHand);
        let secondary_bypass = player.is_secondary_use_active && !(main_empty && off_empty);

        if !secondary_bypass {
            let mut block_result = world.block_use_item_on(player, hand, hit_result);
            if block_result == InteractionResult::TryEmptyHandInteraction
                && hand == InteractionHand::MainHand
            {
                block_result = world.block_use_without_item(player, hit_result);
            }
            if block_result.consumes_action() {
                return block_result;
            }
        }

        if world.is_hand_empty(player, hand) || world.is_hand_on_cooldown(player, hand) {
            return InteractionResult::Pass;
        }

        world.item_use_on(player, hand, hit_result)
    }

    /// `ServerPlayerGameMode.useItem(player, level, itemStack, hand)`.
    pub fn use_item(
        &mut self,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
        hand: InteractionHand,
    ) -> InteractionResult {
        if player.game_type == GameType::Spectator {
            return InteractionResult::Pass;
        }
        if world.is_hand_empty(player, hand) || world.is_hand_on_cooldown(player, hand) {
            return InteractionResult::Pass;
        }
        world.item_use(player, hand)
    }
}
