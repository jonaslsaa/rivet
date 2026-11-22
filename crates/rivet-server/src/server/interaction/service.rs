//! Typed tick-thread `WorldInteractionService` consumed by E9 (`ServerGamePacketListenerImpl`).
//!
//! Mirrors the interaction handlers in Paper 26.2 `ServerGamePacketListenerImpl.java`:
//! - `handlePlayerAction` (start/abort/stop digging)
//! - `handleUseItemOn`
//! - `handleUseItem`
//! - `ackBlockChangesUpTo` + `tick` ack flushing
//!
//! Entity attacks (#23) and container clicks (#22) are intentionally out of scope.

use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_util::mth::wrap_degrees_f32;

use super::interaction_result::{InteractionResult, SwingSource};
use super::packets::{
    ClientboundBlockUpdatePacket, InteractionPacketError, PlayerAction,
    ServerboundPlayerActionPacket, ServerboundUseItemOnPacket, ServerboundUseItemPacket,
    decode_player_action, decode_use_item, decode_use_item_on,
};
use super::server_player_game_mode::{
    CorrectionPacket, InteractionPlayer, InteractionWorld, ServerPlayerGameMode,
};
use super::validation::{
    INTERACTION_RANGE_BUFFER, INVALID_SEQUENCE_DISCONNECT_MESSAGE, SequenceAckTracker,
    validate_hit_location,
};

/// Tick-thread per-player world interaction authority service.
///
/// Owned by the player's tick-thread session state (E9). All methods run synchronously
/// on the tick thread and return ordered [`CorrectionPacket`] values for E9 to transmit.
#[derive(Debug, Clone, Default)]
pub struct WorldInteractionService {
    game_mode: ServerPlayerGameMode,
    sequence_tracker: SequenceAckTracker,
}

impl WorldInteractionService {
    /// Creates a new `WorldInteractionService` with Paper's initial state.
    pub const fn new() -> Self {
        Self {
            game_mode: ServerPlayerGameMode::new(),
            sequence_tracker: SequenceAckTracker::new(),
        }
    }

    pub fn game_mode(&self) -> &ServerPlayerGameMode {
        &self.game_mode
    }

    pub fn sequence_tracker(&self) -> &SequenceAckTracker {
        &self.sequence_tracker
    }

    /// `ServerGamePacketListenerImpl.tick()` + `ServerPlayerGameMode.tick()`.
    ///
    /// Flushes any pending `ClientboundBlockChangedAckPacket` first (matching Paper's
    /// `ServerGamePacketListenerImpl.tick()`), then advances digging progress / delayed
    /// destroy on `ServerPlayerGameMode`.
    pub fn tick(
        &mut self,
        lag_compensation_tick: i32,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<CorrectionPacket> {
        let mut out = Vec::new();
        if let Some(ack) = self.sequence_tracker.flush_ack() {
            out.push(CorrectionPacket::BlockChangedAck(ack));
        }
        self.game_mode
            .tick(lag_compensation_tick, player, world, &mut out);
        out
    }

    /// `ServerGamePacketListenerImpl.handlePlayerAction(ServerboundPlayerActionPacket)`
    /// for block-digging actions (`START_DESTROY_BLOCK`, `ABORT_DESTROY_BLOCK`,
    /// `STOP_DESTROY_BLOCK`).
    ///
    /// Non-digging `PlayerAction` variants are ignored here (they belong to inventory/combat
    /// services).
    pub fn handle_player_action(
        &mut self,
        packet: &ServerboundPlayerActionPacket,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<CorrectionPacket> {
        let mut out = Vec::new();
        if player.is_immobile || !player.has_client_loaded {
            return out;
        }

        match packet.action {
            PlayerAction::StartDestroyBlock
            | PlayerAction::AbortDestroyBlock
            | PlayerAction::StopDestroyBlock => {
                let pos = packet.pos;
                let chunk_x = pos.get_x() >> 4;
                let chunk_z = pos.get_z() >> 4;

                if !world.is_chunk_loaded(chunk_x, chunk_z)
                    || !player.is_within_block_interaction_range(pos, INTERACTION_RANGE_BUFFER)
                {
                    self.record_sequence(packet.sequence, &mut out);
                    return out;
                }

                let max_y = world.max_y();
                self.game_mode.handle_block_break_action(
                    player,
                    world,
                    pos,
                    packet.action,
                    packet.direction,
                    max_y,
                    packet.sequence,
                    &mut out,
                );
                self.record_sequence(packet.sequence, &mut out);
                out
            }
            _ => out,
        }
    }

    /// `ServerGamePacketListenerImpl.handleUseItemOn(ServerboundUseItemOnPacket)`.
    pub fn handle_use_item_on(
        &mut self,
        packet: &ServerboundUseItemOnPacket,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<CorrectionPacket> {
        let mut out = Vec::new();
        if player.is_immobile || !player.has_client_loaded {
            return out;
        }

        // Paper calls ackBlockChangesUpTo(packet.getSequence()) before item/hit checks.
        if !self.record_sequence(packet.sequence, &mut out) {
            return out;
        }

        if !world.is_hand_item_enabled(player, packet.hand) {
            return out;
        }

        let hit = &packet.hit_result;
        let pos = hit.block_pos;

        if validate_hit_location(pos, hit.location).is_err() {
            return out;
        }

        if !player.is_within_block_interaction_range(pos, INTERACTION_RANGE_BUFFER) {
            return out;
        }

        let chunk_x = pos.get_x() >> 4;
        let chunk_z = pos.get_z() >> 4;
        if !world.is_chunk_loaded(chunk_x, chunk_z) {
            return out;
        }

        if pos.get_y() <= world.max_y() && world.may_interact(player, pos) {
            let result = self.game_mode.use_item_on(player, world, packet.hand, hit);
            if matches!(
                result,
                InteractionResult::Success {
                    swing_source: SwingSource::Server,
                    ..
                }
            ) {
                out.push(CorrectionPacket::SwingArm(packet.hand));
            }
        }

        // Paper always sends authoritative ClientboundBlockUpdatePacket for the clicked
        // block and the adjacent block in the clicked face direction.
        if let Some(state) = world.get_block_state_if_loaded(pos) {
            out.push(CorrectionPacket::BlockUpdate(
                ClientboundBlockUpdatePacket { pos, state },
            ));
        }
        let adjacent = pos.relative(&hit.direction);
        if let Some(adj_state) = world.get_block_state_if_loaded(adjacent) {
            out.push(CorrectionPacket::BlockUpdate(
                ClientboundBlockUpdatePacket {
                    pos: adjacent,
                    state: adj_state,
                },
            ));
        }

        out
    }

    /// `ServerGamePacketListenerImpl.handleUseItem(ServerboundUseItemPacket)`.
    pub fn handle_use_item(
        &mut self,
        packet: &ServerboundUseItemPacket,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<CorrectionPacket> {
        let mut out = Vec::new();
        if player.is_immobile || !player.has_client_loaded {
            return out;
        }

        if !self.record_sequence(packet.sequence, &mut out) {
            return out;
        }

        if world.is_hand_empty(player, packet.hand)
            || !world.is_hand_item_enabled(player, packet.hand)
        {
            return out;
        }

        if !packet.y_rot.is_finite() || !packet.x_rot.is_finite() {
            return out;
        }

        let target_y_rot = wrap_degrees_f32(packet.y_rot);
        let target_x_rot = wrap_degrees_f32(packet.x_rot);
        let mut effective_player = *player;
        if target_x_rot != player.pitch || target_y_rot != player.yaw {
            effective_player.yaw = target_y_rot;
            effective_player.pitch = target_x_rot;
            out.push(CorrectionPacket::SnapRotation {
                yaw: target_y_rot,
                pitch: target_x_rot,
            });
        }

        let result = self
            .game_mode
            .use_item(&effective_player, world, packet.hand);
        if matches!(
            result,
            InteractionResult::Success {
                swing_source: SwingSource::Server,
                ..
            }
        ) {
            out.push(CorrectionPacket::SwingArm(packet.hand));
        }

        out
    }

    /// Decodes a raw `ServerboundPlayerActionPacket` frame and processes it safely.
    pub fn decode_and_handle_player_action(
        &mut self,
        input: &mut FriendlyByteBuf,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Result<Vec<CorrectionPacket>, InteractionPacketError> {
        let packet = decode_player_action(input)?;
        Ok(self.handle_player_action(&packet, player, world))
    }

    /// Decodes a raw `ServerboundUseItemOnPacket` frame and processes it safely.
    pub fn decode_and_handle_use_item_on(
        &mut self,
        input: &mut FriendlyByteBuf,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Result<Vec<CorrectionPacket>, InteractionPacketError> {
        let packet = decode_use_item_on(input)?;
        Ok(self.handle_use_item_on(&packet, player, world))
    }

    /// Decodes a raw `ServerboundUseItemPacket` frame and processes it safely.
    pub fn decode_and_handle_use_item(
        &mut self,
        input: &mut FriendlyByteBuf,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Result<Vec<CorrectionPacket>, InteractionPacketError> {
        let packet = decode_use_item(input)?;
        Ok(self.handle_use_item(&packet, player, world))
    }

    /// Records `sequence` via `SequenceAckTracker`; emits a `Disconnect` correction on `< 0`.
    fn record_sequence(&mut self, sequence: i32, out: &mut Vec<CorrectionPacket>) -> bool {
        match self.sequence_tracker.ack_up_to(sequence) {
            Ok(()) => true,
            Err(_) => {
                out.push(CorrectionPacket::Disconnect {
                    reason: INVALID_SEQUENCE_DISCONNECT_MESSAGE.to_owned(),
                });
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use bytes::BytesMut;
    use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
    use rivet_registry::block_state::BlockState;
    use rivet_registry::core::{BlockPos, Direction, GameType, Vec3};
    use rivet_registry::generated::blocks::BlockId;

    use super::*;
    use crate::server::interaction::packets::{
        BlockHitResult, ClientboundBlockChangedAckPacket, ClientboundBlockDestructionPacket,
        InteractionHand,
    };

    fn air() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:air").unwrap())
    }

    fn stone() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:stone").unwrap())
    }

    fn dirt() -> BlockState {
        BlockState::of(BlockId::from_name("minecraft:dirt").unwrap())
    }

    /// Deterministic in-memory `InteractionWorld` for testing Paper 26.2 parity.
    struct MockWorld {
        loaded_chunks: BTreeSet<(i32, i32)>,
        blocks: BTreeMap<BlockPos, BlockState>,
        max_y: i32,
        may_interact: bool,
        destroy_progress: f32,
        allow_destroy: bool,
        attack_log: Vec<BlockPos>,
        hand_empty: [bool; 2],
        hand_enabled: [bool; 2],
        hand_cooldown: [bool; 2],
        block_use_item_on_result: InteractionResult,
        block_use_without_item_result: InteractionResult,
        item_use_on_result: InteractionResult,
        item_use_result: InteractionResult,
        place_on_item_use_on: Option<BlockState>,
        last_item_use_rotation: Option<(f32, f32)>,
    }

    impl MockWorld {
        fn new() -> Self {
            let mut loaded_chunks = BTreeSet::new();
            loaded_chunks.insert((0, 0));
            Self {
                loaded_chunks,
                blocks: BTreeMap::new(),
                max_y: 319,
                may_interact: true,
                destroy_progress: 0.25,
                allow_destroy: true,
                attack_log: Vec::new(),
                hand_empty: [false, true],
                hand_enabled: [true, true],
                hand_cooldown: [false, false],
                block_use_item_on_result: InteractionResult::TryEmptyHandInteraction,
                block_use_without_item_result: InteractionResult::Pass,
                item_use_on_result: InteractionResult::Pass,
                item_use_result: InteractionResult::Pass,
                place_on_item_use_on: None,
                last_item_use_rotation: None,
            }
        }

        fn set_block(&mut self, pos: BlockPos, state: BlockState) {
            self.blocks.insert(pos, state);
        }
    }

    impl InteractionWorld for MockWorld {
        fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
            self.loaded_chunks.contains(&(chunk_x, chunk_z))
        }

        fn max_y(&self) -> i32 {
            self.max_y
        }

        fn get_block_state_if_loaded(&self, pos: BlockPos) -> Option<BlockState> {
            if !self.is_chunk_loaded(pos.get_x() >> 4, pos.get_z() >> 4) {
                return None;
            }
            Some(self.blocks.get(&pos).copied().unwrap_or_else(air))
        }

        fn may_interact(&self, _player: &InteractionPlayer, _pos: BlockPos) -> bool {
            self.may_interact
        }

        fn destroy_progress_per_tick(&self, _player: &InteractionPlayer, _pos: BlockPos) -> f32 {
            self.destroy_progress
        }

        fn destroy_block(&mut self, _player: &InteractionPlayer, pos: BlockPos) -> bool {
            if !self.allow_destroy {
                return false;
            }
            self.blocks.insert(pos, air());
            true
        }

        fn on_block_attack(&mut self, _player: &InteractionPlayer, pos: BlockPos) {
            self.attack_log.push(pos);
        }

        fn is_hand_empty(&self, _player: &InteractionPlayer, hand: InteractionHand) -> bool {
            self.hand_empty[hand.ordinal() as usize]
        }

        fn is_hand_item_enabled(&self, _player: &InteractionPlayer, hand: InteractionHand) -> bool {
            self.hand_enabled[hand.ordinal() as usize]
        }

        fn is_hand_on_cooldown(&self, _player: &InteractionPlayer, hand: InteractionHand) -> bool {
            self.hand_cooldown[hand.ordinal() as usize]
        }

        fn block_use_item_on(
            &mut self,
            _player: &InteractionPlayer,
            _hand: InteractionHand,
            _hit_result: &BlockHitResult,
        ) -> InteractionResult {
            self.block_use_item_on_result
        }

        fn block_use_without_item(
            &mut self,
            _player: &InteractionPlayer,
            _hit_result: &BlockHitResult,
        ) -> InteractionResult {
            self.block_use_without_item_result
        }

        fn item_use_on(
            &mut self,
            _player: &InteractionPlayer,
            _hand: InteractionHand,
            hit_result: &BlockHitResult,
        ) -> InteractionResult {
            if let Some(state) = self.place_on_item_use_on {
                let target = hit_result.block_pos.relative(&hit_result.direction);
                self.blocks.insert(target, state);
            }
            self.item_use_on_result
        }

        fn item_use(
            &mut self,
            player: &InteractionPlayer,
            _hand: InteractionHand,
        ) -> InteractionResult {
            self.last_item_use_rotation = Some((player.yaw, player.pitch));
            self.item_use_result
        }
    }

    fn survival_player() -> InteractionPlayer {
        InteractionPlayer::new(7, Vec3::new(0.5, 65.62, 0.5), GameType::Survival)
    }

    #[test]
    fn survival_dig_start_tick_and_stop_destroys_block_at_paper_threshold() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let pos = BlockPos::new(2, 65, 0);
        world.set_block(pos, stone());
        world.destroy_progress = 0.25; // 3 ticks -> 0.25 * 3 = 0.75 >= 0.70

        let player = survival_player();
        let start = ServerboundPlayerActionPacket::new(
            PlayerAction::StartDestroyBlock,
            pos,
            Direction::Up,
            1,
        );
        let start_out = service.handle_player_action(&start, &player, &mut world);
        assert_eq!(world.attack_log, vec![pos]);
        assert_eq!(
            start_out,
            vec![CorrectionPacket::BlockDestruction(
                ClientboundBlockDestructionPacket::new(7, pos, 2)
            )]
        );
        assert!(service.game_mode().is_destroying_block());

        // Tick 1: flushes seq 1 and advances crack stage to (0.25 * 2 * 10) = 5.
        let t1 = service.tick(1, &player, &mut world);
        assert_eq!(
            t1,
            vec![
                CorrectionPacket::BlockChangedAck(ClientboundBlockChangedAckPacket::new(1)),
                CorrectionPacket::BlockDestruction(ClientboundBlockDestructionPacket::new(
                    7, pos, 5
                )),
            ]
        );

        // Tick 2: advances crack stage to (0.25 * 3 * 10) = 7.
        let t2 = service.tick(2, &player, &mut world);
        assert_eq!(
            t2,
            vec![CorrectionPacket::BlockDestruction(
                ClientboundBlockDestructionPacket::new(7, pos, 7)
            )]
        );

        // Stop digging at tick 2 -> progress = 0.25 * (2 - 0 + 1) = 0.75 >= 0.7 -> destroys.
        let stop = ServerboundPlayerActionPacket::new(
            PlayerAction::StopDestroyBlock,
            pos,
            Direction::Up,
            2,
        );
        let stop_out = service.handle_player_action(&stop, &player, &mut world);
        assert_eq!(
            stop_out,
            vec![CorrectionPacket::BlockDestruction(
                ClientboundBlockDestructionPacket::new(7, pos, -1)
            )]
        );
        assert_eq!(world.get_block_state_if_loaded(pos), Some(air()));
        assert!(!service.game_mode().is_destroying_block());
        assert_eq!(
            service.tick(3, &player, &mut world),
            vec![CorrectionPacket::BlockChangedAck(
                ClientboundBlockChangedAckPacket::new(2)
            )]
        );
    }

    #[test]
    fn early_stop_enters_delayed_destroy_and_finishes_on_later_tick() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let pos = BlockPos::new(1, 65, 0);
        world.set_block(pos, stone());
        world.destroy_progress = 0.34; // tick 0: 0.34 (< 0.7); tick 2: 0.34 * 3 = 1.02 >= 1.0

        let player = survival_player();
        let start = ServerboundPlayerActionPacket::new(
            PlayerAction::StartDestroyBlock,
            pos,
            Direction::Up,
            5,
        );
        let _ = service.handle_player_action(&start, &player, &mut world);

        // Stop immediately on tick 0 -> 0.34 < 0.7 -> enters delayed destroy.
        let stop = ServerboundPlayerActionPacket::new(
            PlayerAction::StopDestroyBlock,
            pos,
            Direction::Up,
            6,
        );
        let stop_out = service.handle_player_action(&stop, &player, &mut world);
        assert!(stop_out.is_empty());
        assert!(!service.game_mode().is_destroying_block());
        assert!(service.game_mode().has_delayed_destroy());
        assert_eq!(world.get_block_state_if_loaded(pos), Some(stone()));

        // Tick 1: progress = 0.68 < 1.0 -> not destroyed yet.
        let _ = service.tick(1, &player, &mut world);
        assert!(service.game_mode().has_delayed_destroy());
        assert_eq!(world.get_block_state_if_loaded(pos), Some(stone()));

        // Tick 2: progress = 1.02 >= 1.0 -> destroyed!
        let _ = service.tick(2, &player, &mut world);
        assert!(!service.game_mode().has_delayed_destroy());
        assert_eq!(world.get_block_state_if_loaded(pos), Some(air()));
    }

    #[test]
    fn abort_digging_clears_stale_and_requested_positions() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let pos_a = BlockPos::new(1, 65, 0);
        let pos_b = BlockPos::new(2, 65, 0);
        world.set_block(pos_a, stone());
        world.set_block(pos_b, dirt());

        let player = survival_player();
        let _ = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                pos_a,
                Direction::Up,
                1,
            ),
            &player,
            &mut world,
        );

        let abort_out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::AbortDestroyBlock,
                pos_b,
                Direction::Up,
                2,
            ),
            &player,
            &mut world,
        );
        assert_eq!(
            abort_out,
            vec![
                CorrectionPacket::BlockDestruction(ClientboundBlockDestructionPacket::new(
                    7, pos_a, -1
                )),
                CorrectionPacket::BlockDestruction(ClientboundBlockDestructionPacket::new(
                    7, pos_b, -1
                )),
            ]
        );
        assert!(!service.game_mode().is_destroying_block());
        assert_eq!(service.game_mode().destroy_pos(), BlockPos::ZERO);
    }

    #[test]
    fn creative_break_destroys_instantly_and_denied_break_sends_correction() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let pos = BlockPos::new(1, 65, 0);
        world.set_block(pos, stone());
        let creative = InteractionPlayer::new(7, Vec3::new(0.5, 65.62, 0.5), GameType::Creative);

        // Denied destroy sends authoritative ClientboundBlockUpdatePacket.
        world.allow_destroy = false;
        let denied_out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                pos,
                Direction::Up,
                10,
            ),
            &creative,
            &mut world,
        );
        assert_eq!(
            denied_out,
            vec![CorrectionPacket::BlockUpdate(
                ClientboundBlockUpdatePacket {
                    pos,
                    state: stone(),
                }
            )]
        );
        assert_eq!(world.get_block_state_if_loaded(pos), Some(stone()));

        // Allowed creative destroy removes block without crack-stage packets.
        world.allow_destroy = true;
        let ok_out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                pos,
                Direction::Up,
                11,
            ),
            &creative,
            &mut world,
        );
        assert!(ok_out.is_empty());
        assert_eq!(world.get_block_state_if_loaded(pos), Some(air()));
    }

    #[test]
    fn out_of_range_and_unloaded_chunk_still_ack_sequence_without_mutating() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let player = survival_player();

        // Out of reach (range 4.5 + buffer 1.0 = 5.5; block at x=10 is 9.5 away).
        let far_pos = BlockPos::new(10, 65, 0);
        world.set_block(far_pos, stone());
        let far_out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                far_pos,
                Direction::Up,
                12,
            ),
            &player,
            &mut world,
        );
        assert!(far_out.is_empty());
        assert!(!service.game_mode().is_destroying_block());

        // Unloaded chunk at (100, 65, 100).
        let unloaded = BlockPos::new(100, 65, 100);
        let unloaded_out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                unloaded,
                Direction::Up,
                15,
            ),
            &player,
            &mut world,
        );
        assert!(unloaded_out.is_empty());

        // Sequence high-water mark was still recorded and flushes as 15.
        assert_eq!(
            service.tick(1, &player, &mut world),
            vec![CorrectionPacket::BlockChangedAck(
                ClientboundBlockChangedAckPacket::new(15)
            )]
        );
    }

    #[test]
    fn reordered_sequences_coalesce_to_max_and_negative_sequence_disconnects() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let pos = BlockPos::new(1, 65, 0);
        world.set_block(pos, stone());
        let player = survival_player();

        // Reordered arrival: 9 then 4 then 7 -> single ack of 9 on tick.
        for seq in [9, 4, 7] {
            let pkt = ServerboundUseItemPacket::new(InteractionHand::MainHand, seq, 0.0, 0.0);
            let _ = service.handle_use_item(&pkt, &player, &mut world);
        }
        assert_eq!(
            service.tick(1, &player, &mut world),
            vec![CorrectionPacket::BlockChangedAck(
                ClientboundBlockChangedAckPacket::new(9)
            )]
        );

        // Controlled negative: negative sequence triggers Paper's exact disconnect message.
        let neg = ServerboundUseItemPacket::new(InteractionHand::MainHand, -1, 0.0, 0.0);
        let neg_out = service.handle_use_item(&neg, &player, &mut world);
        assert_eq!(
            neg_out,
            vec![CorrectionPacket::Disconnect {
                reason: INVALID_SEQUENCE_DISCONNECT_MESSAGE.to_owned(),
            }]
        );
    }

    #[test]
    fn use_item_on_places_block_and_emits_authoritative_pair_of_block_updates() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let clicked = BlockPos::new(2, 65, 0);
        let adjacent = clicked.relative(&Direction::Up);
        world.set_block(clicked, stone());
        world.place_on_item_use_on = Some(dirt());
        world.item_use_on_result = InteractionResult::SUCCESS_SERVER;

        let player = survival_player();
        let hit = BlockHitResult::new(
            Vec3::new(2.5, 66.0, 0.5),
            Direction::Up,
            clicked,
            false,
            false,
        );
        let pkt = ServerboundUseItemOnPacket::new(InteractionHand::MainHand, hit, 3);
        let out = service.handle_use_item_on(&pkt, &player, &mut world);
        assert_eq!(
            out,
            vec![
                CorrectionPacket::SwingArm(InteractionHand::MainHand),
                CorrectionPacket::BlockUpdate(ClientboundBlockUpdatePacket {
                    pos: clicked,
                    state: stone(),
                }),
                CorrectionPacket::BlockUpdate(ClientboundBlockUpdatePacket {
                    pos: adjacent,
                    state: dirt(),
                }),
            ]
        );
    }

    #[test]
    fn use_item_on_rejects_non_finite_or_offset_hit_location_but_still_acks_sequence() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let clicked = BlockPos::new(2, 65, 0);
        world.set_block(clicked, stone());
        world.place_on_item_use_on = Some(dirt());
        let player = survival_player();

        // NaN location
        let nan_hit = BlockHitResult::new(
            Vec3::new(f64::NAN, 65.5, 0.5),
            Direction::Up,
            clicked,
            false,
            false,
        );
        let out_nan = service.handle_use_item_on(
            &ServerboundUseItemOnPacket::new(InteractionHand::MainHand, nan_hit, 20),
            &player,
            &mut world,
        );
        assert!(out_nan.is_empty());

        // Hit location > 1.0000001 away from block center (2.5, 65.5, 0.5)
        let bad_offset_hit = BlockHitResult::new(
            Vec3::new(3.6, 65.5, 0.5),
            Direction::Up,
            clicked,
            false,
            false,
        );
        let out_offset = service.handle_use_item_on(
            &ServerboundUseItemOnPacket::new(InteractionHand::MainHand, bad_offset_hit, 21),
            &player,
            &mut world,
        );
        assert!(out_offset.is_empty());
        assert_eq!(
            world.get_block_state_if_loaded(clicked.relative(&Direction::Up)),
            Some(air())
        );

        // Sequence 21 was acknowledged before the hit-location rejection.
        assert_eq!(
            service.tick(1, &player, &mut world),
            vec![CorrectionPacket::BlockChangedAck(
                ClientboundBlockChangedAckPacket::new(21)
            )]
        );
    }

    #[test]
    fn use_item_wraps_rotation_snaps_player_and_swings_on_server_success() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        world.item_use_result = InteractionResult::SUCCESS_SERVER;
        let player = survival_player();

        // 450.0 wraps to 90.0 via Mth.wrapDegrees.
        let pkt = ServerboundUseItemPacket::new(InteractionHand::MainHand, 4, 450.0, -370.0);
        let out = service.handle_use_item(&pkt, &player, &mut world);
        assert_eq!(
            out,
            vec![
                CorrectionPacket::SnapRotation {
                    yaw: 90.0,
                    pitch: -10.0,
                },
                CorrectionPacket::SwingArm(InteractionHand::MainHand),
            ]
        );
        assert_eq!(world.last_item_use_rotation, Some((90.0, -10.0)));
    }

    #[test]
    fn wire_codecs_roundtrip_and_reject_malformed_frames_without_panicking() {
        let mut buf = FriendlyByteBuf::new(BytesMut::new());
        let hit = BlockHitResult::new(
            Vec3::new(10.25, 64.5, -3.75),
            Direction::North,
            BlockPos::new(10, 64, -4),
            false,
            true,
        );
        let pkt = ServerboundUseItemOnPacket::new(InteractionHand::OffHand, hit, 42);
        pkt.encode(&mut buf);
        let decoded = decode_use_item_on(&mut buf).expect("valid use_item_on frame");
        assert_eq!(decoded, pkt);

        // Malformed action ordinal (99) must return Err, never panic.
        let mut bad_action = FriendlyByteBuf::new(BytesMut::new());
        bad_action.write_var_int(99);
        bad_action.write_long(BlockPos::ZERO.as_long());
        bad_action.write_byte(0);
        bad_action.write_var_int(1);
        assert_eq!(
            decode_player_action(&mut bad_action),
            Err(InteractionPacketError::InvalidActionOrdinal(99))
        );

        // Truncated use_item frame returns UnexpectedEof.
        let mut truncated = FriendlyByteBuf::new(BytesMut::new());
        truncated.write_var_int(0);
        assert!(matches!(
            decode_use_item(&mut truncated),
            Err(InteractionPacketError::UnexpectedEof { .. })
        ));
    }

    #[test]
    fn adventure_mode_restriction_blocks_dig_and_sends_block_update_correction() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let pos = BlockPos::new(1, 65, 0);
        world.set_block(pos, stone());
        let adventure = InteractionPlayer::new(7, Vec3::new(0.5, 65.62, 0.5), GameType::Adventure);

        let out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                pos,
                Direction::Up,
                1,
            ),
            &adventure,
            &mut world,
        );
        assert_eq!(
            out,
            vec![CorrectionPacket::BlockUpdate(
                ClientboundBlockUpdatePacket {
                    pos,
                    state: stone(),
                }
            )]
        );
        assert!(!service.game_mode().is_destroying_block());
    }

    #[test]
    fn use_item_on_denies_protected_block_when_may_interact_is_false() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        let clicked = BlockPos::new(2, 64, 0);
        let placed = BlockPos::new(2, 65, 0);
        world.set_block(clicked, stone());
        world.may_interact = false;
        world.place_on_item_use_on = Some(dirt());
        world.item_use_on_result = InteractionResult::SUCCESS_SERVER;

        let player = survival_player();
        let hit = BlockHitResult::new(
            Vec3::new(2.5, 65.0, 0.5),
            Direction::Up,
            clicked,
            false,
            false,
        );
        let out = service.handle_use_item_on(
            &ServerboundUseItemOnPacket::new(InteractionHand::MainHand, hit, 9),
            &player,
            &mut world,
        );

        // Block must not be placed, no SwingArm emitted, and authoritative updates still sent.
        assert_eq!(world.get_block_state_if_loaded(placed), Some(air()));
        assert_eq!(
            out,
            vec![
                CorrectionPacket::BlockUpdate(ClientboundBlockUpdatePacket {
                    pos: clicked,
                    state: stone(),
                }),
                CorrectionPacket::BlockUpdate(ClientboundBlockUpdatePacket {
                    pos: placed,
                    state: air(),
                }),
            ]
        );
    }

    #[test]
    fn replacement_start_destroy_resyncs_old_block_and_starts_new_target() {
        let mut service = WorldInteractionService::new();
        let mut world = MockWorld::new();
        world.destroy_progress = 0.4;
        let first = BlockPos::new(1, 65, 0);
        let second = BlockPos::new(2, 65, 0);
        world.set_block(first, stone());
        world.set_block(second, dirt());
        let player = survival_player();

        let _ = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                first,
                Direction::Up,
                1,
            ),
            &player,
            &mut world,
        );
        assert!(service.game_mode().is_destroying_block());
        assert_eq!(service.game_mode().destroy_pos(), first);

        let out = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StartDestroyBlock,
                second,
                Direction::Up,
                2,
            ),
            &player,
            &mut world,
        );
        assert_eq!(
            out,
            vec![
                CorrectionPacket::BlockUpdate(ClientboundBlockUpdatePacket {
                    pos: first,
                    state: stone(),
                }),
                CorrectionPacket::BlockDestruction(ClientboundBlockDestructionPacket::new(
                    player.player_id,
                    second,
                    4,
                )),
            ]
        );
        assert!(service.game_mode().is_destroying_block());
        assert_eq!(service.game_mode().destroy_pos(), second);

        // One tick + StopDestroyBlock reaches 0.4 * 2 = 0.8 >= 0.7 and destroys `second`.
        let _ = service.tick(1, &player, &mut world);
        let _ = service.handle_player_action(
            &ServerboundPlayerActionPacket::new(
                PlayerAction::StopDestroyBlock,
                second,
                Direction::Up,
                3,
            ),
            &player,
            &mut world,
        );
        assert_eq!(world.get_block_state_if_loaded(first), Some(stone()));
        assert_eq!(world.get_block_state_if_loaded(second), Some(air()));
    }
}
