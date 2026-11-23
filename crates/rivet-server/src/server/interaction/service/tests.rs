//! Behaviour tests for `WorldInteractionService` + `ServerPlayerGameMode`
//! against a recording `InteractionWorld`. Every expected sequence is read off
//! Paper 26.2 `ServerGamePacketListenerImpl.handlePlayerAction` /
//! `handleUseItemOn` / `handleUseItem` and `ServerPlayerGameMode`
//! (`working/Paper` `0a993450`). The wire layer is proven separately against
//! actual Paper codecs (`tools/rivet-oracle/fixtures/interaction-packets.jsonl`).

use std::collections::{BTreeMap, BTreeSet};

use rivet_protocol::game::serversbound_player_action_packet::Action as PlayerAction;
use rivet_protocol::game::{
    ClientboundBlockChangedAckPacket, ServerboundPlayerActionPacket, ServerboundUseItemOnPacket,
    ServerboundUseItemPacket,
};
use rivet_protocol::protocol::game::clientbound_block_updates::ClientboundBlockUpdatePacket;
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{BlockHitResult, BlockPos, Direction, GameType, InteractionHand, Vec3};
use rivet_registry::generated::blocks::BlockId;
use rivet_world::interaction_result::InteractionResult;

use super::*;
use crate::server::level::server_player_game_mode::{InteractionEffect, InteractionPlayer};

fn state(name: &str) -> BlockState {
    BlockState::of(BlockId::from_name(name).unwrap())
}

fn air() -> BlockState {
    state("minecraft:air")
}

fn stone() -> BlockState {
    state("minecraft:stone")
}

fn dirt() -> BlockState {
    state("minecraft:dirt")
}

const PLAYER_ID: i32 = 7;

/// Recording in-memory `InteractionWorld`.
struct MockWorld {
    loaded_chunks: BTreeSet<(i32, i32)>,
    blocks: BTreeMap<BlockPos, BlockState>,
    min_y: i32,
    max_y: i32,
    spawn_protected: bool,
    may_interact: bool,
    sign_passthrough: bool,
    block_enabled: bool,
    menu_provider: bool,
    open_menu: bool,
    cake: bool,
    destroy_progress: f32,
    allow_destroy: bool,
    hand_empty: [bool; 2],
    hand_enabled: [bool; 2],
    hand_cooldown: [bool; 2],
    hand_places_blocks: [bool; 2],
    adventure_can_break: bool,
    client_predicts: bool,
    block_use_item_on_result: InteractionResult,
    block_use_without_item_result: InteractionResult,
    item_use_on_result: InteractionResult,
    item_use_result: InteractionResult,
    place_on_item_use_on: Option<BlockState>,
    /// Every `level.destroyBlockProgress(id, pos, progress)` call, in order.
    progress_calls: Vec<(i32, BlockPos, i32)>,
    attacks: Vec<BlockPos>,
    destroyed: Vec<BlockPos>,
    item_use_rotation: Option<(f32, f32)>,
    item_use_on_calls: u32,
}

impl MockWorld {
    fn new() -> Self {
        MockWorld {
            loaded_chunks: BTreeSet::from([(0, 0)]),
            blocks: BTreeMap::new(),
            min_y: -64,
            max_y: 319,
            spawn_protected: false,
            may_interact: true,
            sign_passthrough: false,
            block_enabled: true,
            menu_provider: false,
            open_menu: true,
            cake: false,
            destroy_progress: 0.25,
            allow_destroy: true,
            hand_empty: [false, true],
            hand_enabled: [true, true],
            hand_cooldown: [false, false],
            hand_places_blocks: [false, false],
            adventure_can_break: false,
            client_predicts: false,
            block_use_item_on_result: InteractionResult::TRY_WITH_EMPTY_HAND,
            block_use_without_item_result: InteractionResult::PASS,
            item_use_on_result: InteractionResult::PASS,
            item_use_result: InteractionResult::PASS,
            place_on_item_use_on: None,
            progress_calls: Vec::new(),
            attacks: Vec::new(),
            destroyed: Vec::new(),
            item_use_rotation: None,
            item_use_on_calls: 0,
        }
    }

    fn set_block(&mut self, pos: BlockPos, state: BlockState) {
        self.blocks.insert(pos, state);
    }

    fn block(&self, pos: BlockPos) -> BlockState {
        self.blocks.get(&pos).copied().unwrap_or_else(air)
    }
}

fn hand_index(hand: InteractionHand) -> usize {
    hand.ordinal() as usize
}

impl InteractionWorld for MockWorld {
    fn is_chunk_loaded(&self, chunk_x: i32, chunk_z: i32) -> bool {
        self.loaded_chunks.contains(&(chunk_x, chunk_z))
    }
    fn get_min_y(&self) -> i32 {
        self.min_y
    }
    fn get_max_y(&self) -> i32 {
        self.max_y
    }
    fn get_block_state(&self, pos: BlockPos) -> BlockState {
        self.block(pos)
    }
    fn get_block_state_if_loaded(&self, pos: BlockPos) -> Option<BlockState> {
        self.is_chunk_loaded(pos.get_x() >> 4, pos.get_z() >> 4)
            .then(|| self.block(pos))
    }
    fn is_under_spawn_protection(&self, _: &InteractionPlayer, _: BlockPos) -> bool {
        self.spawn_protected
    }
    fn may_interact(&self, _: &InteractionPlayer, _: BlockPos) -> bool {
        // `ServerLevel.mayInteract` = !spawnProtected && inside the border.
        self.may_interact && !self.spawn_protected
    }
    fn is_sign_interaction_passthrough(&self, _: BlockPos) -> bool {
        self.sign_passthrough
    }
    fn destroy_block_progress(&mut self, breaker_id: i32, pos: BlockPos, progress: i32) {
        self.progress_calls.push((breaker_id, pos, progress));
    }
    fn is_block_enabled(&self, _: BlockPos) -> bool {
        self.block_enabled
    }
    fn get_destroy_progress(&self, _: &InteractionPlayer, _: BlockPos) -> f32 {
        self.destroy_progress
    }
    fn attack_block(&mut self, _: &InteractionPlayer, pos: BlockPos) {
        self.attacks.push(pos);
    }
    fn has_menu_provider(&self, _: BlockPos) -> bool {
        self.menu_provider
    }
    fn open_menu(&mut self, _: &InteractionPlayer, _: BlockPos) -> bool {
        self.open_menu
    }
    fn is_cake_block(&self, _: BlockPos) -> bool {
        self.cake
    }
    fn is_jigsaw_structure_or_command_block(&self, _: BlockPos) -> bool {
        false
    }
    fn block_use_item_on(
        &mut self,
        _: &InteractionPlayer,
        _: InteractionHand,
        _: &BlockHitResult,
    ) -> InteractionResult {
        self.block_use_item_on_result.clone()
    }
    fn block_use_without_item(
        &mut self,
        _: &InteractionPlayer,
        _: &BlockHitResult,
    ) -> InteractionResult {
        self.block_use_without_item_result.clone()
    }
    fn destroy_block(&mut self, _: &InteractionPlayer, pos: BlockPos) -> bool {
        if !self.allow_destroy || self.block(pos).is_air() {
            return false;
        }
        self.blocks.insert(pos, air());
        self.destroyed.push(pos);
        true
    }
    fn is_hand_empty(&self, _: &InteractionPlayer, hand: InteractionHand) -> bool {
        self.hand_empty[hand_index(hand)]
    }
    fn is_hand_item_enabled(&self, _: &InteractionPlayer, hand: InteractionHand) -> bool {
        self.hand_enabled[hand_index(hand)]
    }
    fn is_hand_on_cooldown(&self, _: &InteractionPlayer, hand: InteractionHand) -> bool {
        self.hand_cooldown[hand_index(hand)]
    }
    fn is_hand_block_placement_item(&self, _: &InteractionPlayer, hand: InteractionHand) -> bool {
        self.hand_places_blocks[hand_index(hand)]
    }
    fn main_hand_can_break_block_in_adventure_mode(
        &self,
        _: &InteractionPlayer,
        _: BlockPos,
    ) -> bool {
        self.adventure_can_break
    }
    fn handle_debug_stick_left_click(&mut self, _: &InteractionPlayer, _: BlockPos) -> bool {
        false
    }
    fn client_predicts_interaction(
        &self,
        _: &InteractionPlayer,
        _: BlockPos,
        _: InteractionHand,
    ) -> bool {
        self.client_predicts
    }
    fn item_use_on(
        &mut self,
        _: &InteractionPlayer,
        _: InteractionHand,
        hit_result: &BlockHitResult,
    ) -> InteractionResult {
        self.item_use_on_calls += 1;
        if let Some(placed) = self.place_on_item_use_on {
            let target = hit_result
                .get_block_pos()
                .relative(&hit_result.get_direction());
            self.blocks.insert(target, placed);
        }
        self.item_use_on_result.clone()
    }
    fn item_use(&mut self, player: &InteractionPlayer, _: InteractionHand) -> InteractionResult {
        self.item_use_rotation = Some((player.y_rot, player.x_rot));
        self.item_use_result.clone()
    }
}

fn survival_player() -> InteractionPlayer {
    InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, 65.62, 0.5), GameType::Survival)
}

fn service() -> WorldInteractionService {
    WorldInteractionService::new(PAPER_DEFAULT_INCOMING_PACKET_THRESHOLD)
}

fn action(action: PlayerAction, pos: BlockPos, sequence: i32) -> ServerboundPlayerActionPacket {
    ServerboundPlayerActionPacket::new(action, pos, Direction::Up, sequence)
}

fn update(pos: BlockPos, state: BlockState) -> InteractionEffect {
    InteractionEffect::BlockUpdate(ClientboundBlockUpdatePacket { pos, state })
}

fn use_item_on(pos: BlockPos, direction: Direction, sequence: i32) -> ServerboundUseItemOnPacket {
    let location = Vec3::new(
        pos.get_x() as f64 + 0.5,
        pos.get_y() as f64 + 1.0,
        pos.get_z() as f64 + 0.5,
    );
    ServerboundUseItemOnPacket::new(
        InteractionHand::MainHand,
        BlockHitResult::new(location, direction, pos, false),
        sequence,
    )
}

const DIG_DONE: [InteractionEffect; 2] = [
    InteractionEffect::ResetLastActionTime,
    InteractionEffect::DetectEquipmentUpdates,
];

#[test]
fn survival_dig_sends_crack_stages_to_the_level_and_acks_on_the_connection_tick() {
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(2, 65, 0);
    world.set_block(pos, stone());
    let player = survival_player();

    service.tick_game_mode(0, &player, &mut world);
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 1),
        &player,
        &mut world,
    );
    // The breaker gets no crack packet: `level.destroyBlockProgress` fans out
    // to the other players only.
    assert_eq!(out, DIG_DONE);
    assert_eq!(world.attacks, vec![pos]);
    assert_eq!(world.progress_calls, vec![(PLAYER_ID, pos, 2)]);
    assert!(service.game_mode().is_destroying_block());

    // Server tick 1: the level tick (gameMode.tick) runs before the
    // connection tick that flushes the ack.
    service.tick_game_mode(1, &player, &mut world);
    assert_eq!(world.progress_calls.last(), Some(&(PLAYER_ID, pos, 5)));
    assert_eq!(
        service.tick_connection(),
        Some(ClientboundBlockChangedAckPacket::new(1))
    );
    assert_eq!(service.tick_connection(), None);

    service.tick_game_mode(2, &player, &mut world);
    assert_eq!(world.progress_calls.last(), Some(&(PLAYER_ID, pos, 7)));
    let out = service.handle_player_action(
        &action(PlayerAction::StopDestroyBlock, pos, 2),
        &player,
        &mut world,
    );
    // 0.25 * (2 + 1) = 0.75 >= 0.7: clear the stage, then destroyAndAck.
    assert_eq!(out, DIG_DONE);
    assert_eq!(world.progress_calls.last(), Some(&(PLAYER_ID, pos, -1)));
    assert_eq!(world.destroyed, vec![pos]);
    assert!(!service.game_mode().is_destroying_block());
    assert_eq!(
        service.tick_connection(),
        Some(ClientboundBlockChangedAckPacket::new(2))
    );
}

#[test]
fn early_stop_enters_delayed_destroy_and_finishes_on_a_later_tick() {
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(1, 65, 0);
    world.set_block(pos, stone());
    world.destroy_progress = 0.2;
    let player = survival_player();

    service.tick_game_mode(10, &player, &mut world);
    service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 1),
        &player,
        &mut world,
    );
    // 0.2 * (0 + 1) < 0.7 → delayed destroy.
    service.handle_player_action(
        &action(PlayerAction::StopDestroyBlock, pos, 2),
        &player,
        &mut world,
    );
    assert!(service.game_mode().has_delayed_destroy);
    assert!(!service.game_mode().is_destroying_block());
    assert_eq!(service.game_mode().delayed_destroy_pos(), pos);

    for tick in 11..=13 {
        service.tick_game_mode(tick, &player, &mut world);
        assert!(world.destroyed.is_empty(), "tick {tick}");
    }
    // 0.2 * (14 - 10 + 1) = 1.0 → destroyed.
    service.tick_game_mode(14, &player, &mut world);
    assert_eq!(world.destroyed, vec![pos]);
    assert!(!service.game_mode().has_delayed_destroy);
    assert_eq!(
        world.progress_calls,
        vec![
            (PLAYER_ID, pos, 2),
            (PLAYER_ID, pos, 4),
            (PLAYER_ID, pos, 6),
            (PLAYER_ID, pos, 8),
            (PLAYER_ID, pos, 10),
        ]
    );
}

#[test]
fn abort_clears_the_stale_and_requested_positions() {
    let mut service = service();
    let mut world = MockWorld::new();
    let first = BlockPos::new(1, 65, 0);
    let other = BlockPos::new(2, 65, 0);
    world.set_block(first, stone());
    let player = survival_player();

    service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, first, 1),
        &player,
        &mut world,
    );
    let out = service.handle_player_action(
        &action(PlayerAction::AbortDestroyBlock, other, 2),
        &player,
        &mut world,
    );
    assert_eq!(out, DIG_DONE);
    assert_eq!(
        world.progress_calls,
        vec![
            (PLAYER_ID, first, 2),
            (PLAYER_ID, first, -1),
            (PLAYER_ID, other, -1),
        ]
    );
    assert_eq!(service.game_mode().destroy_pos(), BlockPos::ZERO);
    assert!(!service.game_mode().is_destroying_block());
}

#[test]
fn instabuild_destroys_immediately_and_a_refused_destroy_resyncs_the_block() {
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(1, 65, 0);
    world.set_block(pos, stone());
    let creative =
        InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, 65.62, 0.5), GameType::Creative);
    assert_eq!(
        service.handle_player_action(
            &action(PlayerAction::StartDestroyBlock, pos, 1),
            &creative,
            &mut world
        ),
        DIG_DONE
    );
    assert_eq!(world.destroyed, vec![pos]);
    assert!(world.attacks.is_empty());

    world.set_block(pos, stone());
    world.allow_destroy = false;
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 2),
        &creative,
        &mut world,
    );
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            update(pos, stone()),
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
}

#[test]
fn spawn_protection_sends_only_the_message_while_border_denial_acks_immediately() {
    let pos = BlockPos::new(1, 65, 0);
    let player = survival_player();

    // `isUnderSpawnProtection` → message, no block update, ack left for the tick.
    let mut service = service();
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    world.spawn_protected = true;
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 4),
        &player,
        &mut world,
    );
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            InteractionEffect::SendSpawnProtectionMessage { pos },
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
    assert_eq!(service.pending_ack(), Some(4));

    // `!mayInteract` (world border) → block update + capturedBlockEntity:
    // immediate ack and block-entity resync, nothing left for the tick.
    let mut service = self::service();
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    world.may_interact = false;
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 5),
        &player,
        &mut world,
    );
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            update(pos, stone()),
            InteractionEffect::BlockChangedAck(ClientboundBlockChangedAckPacket::new(5)),
            InteractionEffect::BlockEntityUpdate { pos },
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
    assert_eq!(service.tick_connection(), None);
    assert!(world.progress_calls.is_empty());
}

#[test]
fn out_of_range_and_unloaded_digs_only_ack() {
    let mut service = service();
    let mut world = MockWorld::new();
    let player = survival_player();
    let far = BlockPos::new(12, 65, 0);
    let unloaded = BlockPos::new(40, 65, 40);
    world.set_block(far, stone());
    for (pos, sequence) in [(far, 3), (unloaded, 9)] {
        let out = service.handle_player_action(
            &action(PlayerAction::StartDestroyBlock, pos, sequence),
            &player,
            &mut world,
        );
        assert_eq!(out, vec![InteractionEffect::ResetLastActionTime]);
    }
    assert!(world.progress_calls.is_empty());
    assert_eq!(
        service.tick_connection(),
        Some(ClientboundBlockChangedAckPacket::new(9))
    );
}

#[test]
fn reordered_sequences_coalesce_and_a_negative_sequence_disconnects() {
    let mut service = service();
    let mut world = MockWorld::new();
    let player = survival_player();
    let pos = BlockPos::new(1, 65, 0);
    for sequence in [5, 3, 4] {
        service.handle_player_action(
            &action(PlayerAction::AbortDestroyBlock, pos, sequence),
            &player,
            &mut world,
        );
    }
    assert_eq!(
        service.tick_connection(),
        Some(ClientboundBlockChangedAckPacket::new(5))
    );

    let out = service.handle_player_action(
        &action(PlayerAction::AbortDestroyBlock, pos, -1),
        &player,
        &mut world,
    );
    assert_eq!(
        out.last(),
        Some(&InteractionEffect::Disconnect {
            reason: INVALID_SEQUENCE_DISCONNECT_MESSAGE.to_owned()
        })
    );
    assert_eq!(service.tick_connection(), None);
}

#[test]
fn adventure_digging_follows_the_main_hand_can_break_predicate() {
    let pos = BlockPos::new(1, 65, 0);
    let adventure =
        InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, 65.62, 0.5), GameType::Adventure);

    let mut service = service();
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 1),
        &adventure,
        &mut world,
    );
    assert_eq!(out[1], update(pos, stone()));
    assert!(world.progress_calls.is_empty());

    // Holding an item whose `can_break` predicate matches: digging proceeds.
    let mut service = self::service();
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    world.adventure_can_break = true;
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 1),
        &adventure,
        &mut world,
    );
    assert_eq!(out, DIG_DONE);
    assert_eq!(world.progress_calls, vec![(PLAYER_ID, pos, 2)]);
}

#[test]
fn starting_on_air_takes_the_block_damage_insta_break_progress() {
    // progress stays 1.0F on air; BlockDamageEvent(instaBreak = true) bumps it
    // to 2.0F, and air is never destroyed, so stage (int) (2.0F * 10) = 20.
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(1, 65, 0);
    let player = survival_player();
    service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, pos, 1),
        &player,
        &mut world,
    );
    assert!(world.attacks.is_empty());
    assert_eq!(world.progress_calls, vec![(PLAYER_ID, pos, 20)]);
    assert!(service.game_mode().is_destroying_block());
}

#[test]
fn replacement_start_resyncs_the_previous_target() {
    let mut service = service();
    let mut world = MockWorld::new();
    let first = BlockPos::new(1, 65, 0);
    let second = BlockPos::new(2, 65, 0);
    world.set_block(first, stone());
    world.set_block(second, dirt());
    let player = survival_player();
    service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, first, 1),
        &player,
        &mut world,
    );
    let out = service.handle_player_action(
        &action(PlayerAction::StartDestroyBlock, second, 2),
        &player,
        &mut world,
    );
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            update(first, stone()),
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
    assert_eq!(service.game_mode().destroy_pos(), second);
    assert_eq!(world.progress_calls.last(), Some(&(PLAYER_ID, second, 2)));
}

#[test]
fn server_swing_success_swings_twice_and_resyncs_both_blocks() {
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(1, 64, 0);
    world.set_block(pos, stone());
    world.place_on_item_use_on = Some(dirt());
    world.item_use_on_result = InteractionResult::SUCCESS_SERVER;
    let player = survival_player();

    let out = service.handle_use_item_on(&use_item_on(pos, Direction::Up, 3), &player, &mut world);
    let above = pos.relative(&Direction::Up);
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            InteractionEffect::StopUsingItem,
            InteractionEffect::Swing(InteractionHand::MainHand),
            InteractionEffect::Swing(InteractionHand::MainHand),
            update(pos, stone()),
            update(above, dirt()),
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
    assert_eq!(service.pending_ack(), Some(3));
}

#[test]
fn item_on_cooldown_sets_interact_result_and_suppresses_the_first_swing() {
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(1, 64, 0);
    world.set_block(pos, stone());
    world.hand_cooldown = [true, false];
    world.block_use_item_on_result = InteractionResult::SUCCESS_SERVER;
    let player = survival_player();

    let out = service.handle_use_item_on(&use_item_on(pos, Direction::Up, 1), &player, &mut world);
    assert!(service.game_mode().interact_result);
    let swings = out
        .iter()
        .filter(|effect| matches!(effect, InteractionEffect::Swing(_)))
        .count();
    assert_eq!(swings, 1);
    assert_eq!(world.item_use_on_calls, 0);
}

#[test]
fn clicks_outside_the_build_height_only_send_the_build_limit_message() {
    let player = InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, 319.62, 0.5), GameType::Survival);
    let mut service = service();
    let mut world = MockWorld::new();
    world.max_y = 318;
    let pos = BlockPos::new(1, 319, 0);
    let out = service.handle_use_item_on(&use_item_on(pos, Direction::Up, 1), &player, &mut world);
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            InteractionEffect::SendBuildLimitMessage {
                above: true,
                limit: 318
            },
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
    assert_eq!(world.item_use_on_calls, 0);

    let player = InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, -64.0, 0.5), GameType::Survival);
    let pos = BlockPos::new(1, -65, 0);
    let out = service.handle_use_item_on(&use_item_on(pos, Direction::Up, 2), &player, &mut world);
    assert_eq!(
        out[1],
        InteractionEffect::SendBuildLimitMessage {
            above: false,
            limit: -64
        }
    );
    assert_eq!(out.len(), 3);
}

#[test]
fn failed_placement_on_the_top_layer_sends_the_build_limit_message_from_both_blocks() {
    let player = InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, 319.62, 0.5), GameType::Survival);
    let mut service = service();
    let mut world = MockWorld::new();
    let pos = BlockPos::new(1, 319, 0);
    world.set_block(pos, stone());
    world.hand_places_blocks = [true, false];
    let out = service.handle_use_item_on(&use_item_on(pos, Direction::Up, 1), &player, &mut world);
    let message = InteractionEffect::SendBuildLimitMessage {
        above: true,
        limit: 319,
    };
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            InteractionEffect::StopUsingItem,
            message.clone(),
            message,
            update(pos, stone()),
            update(pos.relative(&Direction::Up), air()),
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
}

#[test]
fn spawn_protected_and_teleporting_clicks_skip_use_but_keep_the_block_resync() {
    let pos = BlockPos::new(1, 64, 0);
    let above = pos.relative(&Direction::Up);

    let mut service = service();
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    world.spawn_protected = true;
    let player = survival_player();
    let out = service.handle_use_item_on(&use_item_on(pos, Direction::Up, 1), &player, &mut world);
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            InteractionEffect::SendSpawnProtectionMessage { pos },
            update(pos, stone()),
            update(above, air()),
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );

    // Pending teleport: no use, no message.
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    let mut teleporting = survival_player();
    teleporting.awaiting_position_from_client = true;
    let out = service.handle_use_item_on(
        &use_item_on(pos, Direction::Up, 2),
        &teleporting,
        &mut world,
    );
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            update(pos, stone()),
            update(above, air()),
            InteractionEffect::DetectEquipmentUpdates,
        ]
    );
    assert_eq!(world.item_use_on_calls, 0);

    // Sign passthrough inside spawn protection while teleporting reaches the
    // MC-99075 inventory resync branch.
    let mut world = MockWorld::new();
    world.set_block(pos, stone());
    world.spawn_protected = true;
    world.sign_passthrough = true;
    let out = service.handle_use_item_on(
        &use_item_on(pos, Direction::Up, 3),
        &teleporting,
        &mut world,
    );
    assert_eq!(
        out[1],
        InteractionEffect::ContainerForceHeldSlot(InteractionHand::MainHand)
    );
}

#[test]
fn invalid_hit_locations_and_disabled_items_only_ack() {
    let mut service = service();
    let mut world = MockWorld::new();
    let player = survival_player();
    let pos = BlockPos::new(1, 64, 0);

    let mut offset = use_item_on(pos, Direction::Up, 1);
    offset.block_hit = BlockHitResult::new(Vec3::new(1.5, 66.0, 0.5), Direction::Up, pos, false);
    let mut non_finite = use_item_on(pos, Direction::Up, 2);
    non_finite.block_hit =
        BlockHitResult::new(Vec3::new(f64::NAN, 64.5, 0.5), Direction::Up, pos, false);
    for packet in [offset, non_finite] {
        assert!(
            service
                .handle_use_item_on(&packet, &player, &mut world)
                .is_empty()
        );
    }

    world.hand_enabled = [false, true];
    assert!(
        service
            .handle_use_item_on(&use_item_on(pos, Direction::Up, 3), &player, &mut world)
            .is_empty()
    );
    assert_eq!(
        service.tick_connection(),
        Some(ClientboundBlockChangedAckPacket::new(3))
    );
}

#[test]
fn spectators_open_menus_or_get_the_denied_interaction_resync() {
    let pos = BlockPos::new(1, 64, 0);
    let spectator =
        InteractionPlayer::new(PLAYER_ID, Vec3::new(0.5, 65.62, 0.5), GameType::Spectator);
    let mut game_mode = ServerPlayerGameMode::new();
    let hit = BlockHitResult::new(Vec3::new(1.5, 65.0, 0.5), Direction::Up, pos, false);

    // No menu provider: useInteractedBlock is pre-set to DENY → SUCCESS + resync.
    let mut world = MockWorld::new();
    world.cake = true;
    let mut out = Vec::new();
    let result = game_mode.use_item_on(
        &spectator,
        &mut world,
        InteractionHand::MainHand,
        &hit,
        &mut out,
    );
    assert!(result.is_success_with_swing_source(SwingSource::Client));
    assert_eq!(
        out,
        vec![
            InteractionEffect::SendHealthUpdate,
            InteractionEffect::ContainerForceHeldSlot(InteractionHand::MainHand),
            InteractionEffect::ResyncUsingItem,
        ]
    );

    // A menu provider that opens → CONSUME; one that does not → PASS.
    let mut world = MockWorld::new();
    world.menu_provider = true;
    let mut out = Vec::new();
    let result = game_mode.use_item_on(
        &spectator,
        &mut world,
        InteractionHand::MainHand,
        &hit,
        &mut out,
    );
    assert!(result.is_success_with_swing_source(SwingSource::None));
    world.open_menu = false;
    let result = game_mode.use_item_on(
        &spectator,
        &mut world,
        InteractionHand::MainHand,
        &hit,
        &mut out,
    );
    assert!(matches!(result, InteractionResult::Pass));
    assert!(out.is_empty());

    // A block disabled by the feature flags fails first.
    world.block_enabled = false;
    let player = survival_player();
    let result = game_mode.use_item_on(
        &player,
        &mut world,
        InteractionHand::MainHand,
        &hit,
        &mut out,
    );
    assert!(matches!(result, InteractionResult::Fail));
}

#[test]
fn spam_limiter_drops_the_tenth_packet_inside_the_threshold() {
    let mut service = service();
    let mut world = MockWorld::new();
    let player = survival_player();
    let pos = BlockPos::new(1, 64, 0);
    for sequence in 0..9 {
        let mut packet = use_item_on(pos, Direction::Up, sequence);
        packet.timestamp = 1_000;
        assert!(
            !service
                .handle_use_item_on(&packet, &player, &mut world)
                .is_empty()
        );
    }
    let mut dropped = ServerboundUseItemPacket::new(InteractionHand::MainHand, 50, 0.0, 0.0);
    dropped.timestamp = 1_299;
    assert!(
        service
            .handle_use_item(&dropped, &player, &mut world)
            .is_empty()
    );
    assert_eq!(service.pending_ack(), Some(8));

    let mut later = ServerboundUseItemPacket::new(InteractionHand::MainHand, 51, 0.0, 0.0);
    later.timestamp = 1_300;
    assert!(
        !service
            .handle_use_item(&later, &player, &mut world)
            .is_empty()
    );
    assert_eq!(service.pending_ack(), Some(51));

    // Disabled limiter never drops.
    let mut unlimited = WorldInteractionService::new(None);
    for sequence in 0..20 {
        let packet = ServerboundUseItemPacket::new(InteractionHand::MainHand, sequence, 0.0, 0.0);
        assert!(
            !unlimited
                .handle_use_item(&packet, &player, &mut world)
                .is_empty()
        );
    }
}

#[test]
fn use_item_snaps_the_wrapped_rotation_before_using_and_swings_on_server_success() {
    let mut service = service();
    let mut world = MockWorld::new();
    world.item_use_result = InteractionResult::SUCCESS_SERVER;
    let player = survival_player();
    let packet = ServerboundUseItemPacket::new(InteractionHand::MainHand, 4, 370.0, -100.0);
    let out = service.handle_use_item(&packet, &player, &mut world);
    assert_eq!(
        out,
        vec![
            InteractionEffect::ResetLastActionTime,
            InteractionEffect::AbsSnapRotationTo {
                y_rot: 10.0,
                x_rot: -100.0
            },
            InteractionEffect::Swing(InteractionHand::MainHand),
        ]
    );
    // setXRot stores Mth.clamp(-100, -90, 90) % 360 = -90.
    assert_eq!(world.item_use_rotation, Some((10.0, -90.0)));
}

#[test]
fn non_finite_use_item_rotations_are_discarded_but_the_item_is_still_used() {
    let mut service = service();
    let mut world = MockWorld::new();
    let mut player = survival_player();
    player.y_rot = 45.0;
    player.x_rot = 12.0;
    let packet =
        ServerboundUseItemPacket::new(InteractionHand::MainHand, 1, f32::NAN, f32::INFINITY);
    let out = service.handle_use_item(&packet, &player, &mut world);
    assert!(matches!(
        out[1],
        InteractionEffect::AbsSnapRotationTo { y_rot, x_rot } if y_rot.is_nan() && x_rot.is_nan()
    ));
    assert_eq!(world.item_use_rotation, Some((45.0, 12.0)));
}

#[test]
fn use_item_requires_a_non_empty_enabled_item() {
    let mut service = service();
    let mut world = MockWorld::new();
    let player = survival_player();
    let packet = ServerboundUseItemPacket::new(InteractionHand::OffHand, 1, 0.0, 0.0);
    assert_eq!(
        service.handle_use_item(&packet, &player, &mut world),
        vec![InteractionEffect::ResetLastActionTime]
    );
    assert_eq!(world.item_use_rotation, None);
}
