//! `WorldInteractionService` — the #448 slice of
//! `net.minecraft.server.network.ServerGamePacketListenerImpl` (Paper 26.2,
//! `0a993450`): the digging arms of `handlePlayerAction`, `handleUseItemOn`,
//! `handleUseItem`, `ackBlockChangesUpTo`, the ack flush at the head of
//! `tick()`, Spigot's `checkLimit`, and the static `wasBlockPlacementAttempt`.

use rivet_protocol::game::serversbound_player_action_packet::Action as PlayerAction;
use rivet_protocol::game::{
    ClientboundBlockChangedAckPacket, ServerboundPlayerActionPacket, ServerboundUseItemOnPacket,
    ServerboundUseItemPacket,
};
use rivet_registry::block_state::BlockState;
use rivet_registry::core::{Direction, HitResultType, InteractionHand, Vec3};
use rivet_registry::generated::blocks::BlockId;
use rivet_util::mth::{self, clamp_f32, wrap_degrees_f32};
use rivet_world::interaction_result::SwingSource;
use rivet_world::item::item_stack::ItemStack;

use crate::server::level::server_player_game_mode::{
    InteractionEffect, InteractionPlayer, InteractionWorld, ServerPlayerGameMode, send_block_update,
};

/// `ackBlockChangesUpTo`'s disconnect reason for a negative sequence.
pub const INVALID_SEQUENCE_DISCONNECT_MESSAGE: &str = "Expected packet sequence nr >= 0";

/// `GlobalConfiguration.spamLimiter.incomingPacketThreshold` default
/// (`IntOr.Disabled(OptionalInt.of(300))`).
pub const PAPER_DEFAULT_INCOMING_PACKET_THRESHOLD: Option<i32> = Some(300);

/// Tick-thread, per-player world-interaction authority, owned by the player's
/// session state (E9). Every method runs synchronously on the tick thread and
/// returns the ordered [`InteractionEffect`]s for E9 to apply.
#[derive(Debug, Clone)]
pub struct WorldInteractionService {
    game_mode: ServerPlayerGameMode,
    /// `ServerGamePacketListenerImpl.ackBlockChangesUpTo` (`-1` = none pending).
    ack_block_changes_up_to: i32,
    /// Spigot `limitedPackets`.
    limited_packets: i32,
    /// Spigot `lastLimitedPacket`.
    last_limited_packet: i64,
    /// `spamLimiter.incomingPacketThreshold` (`None` = disabled).
    incoming_packet_threshold: Option<i32>,
}

impl WorldInteractionService {
    /// Fresh listener/game-mode state; `incoming_packet_threshold` is the
    /// configured `spamLimiter.incomingPacketThreshold`
    /// ([`PAPER_DEFAULT_INCOMING_PACKET_THRESHOLD`] by default).
    pub const fn new(incoming_packet_threshold: Option<i32>) -> Self {
        WorldInteractionService {
            game_mode: ServerPlayerGameMode::new(),
            ack_block_changes_up_to: -1,
            limited_packets: 0,
            last_limited_packet: -1,
            incoming_packet_threshold,
        }
    }

    pub fn game_mode(&self) -> &ServerPlayerGameMode {
        &self.game_mode
    }

    /// `ackBlockChangesUpTo` when one is pending.
    pub fn pending_ack(&self) -> Option<i32> {
        (self.ack_block_changes_up_to > -1).then_some(self.ack_block_changes_up_to)
    }

    /// `ServerPlayer.tick()` → `gameMode.tick()`. Runs in the level's entity
    /// tick (`MinecraftServer.tickChildren` → `level.tick`), which precedes
    /// [`Self::tick_connection`] within a server tick.
    pub fn tick_game_mode(
        &mut self,
        lag_compensation_tick: i64,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) {
        self.game_mode.tick(lag_compensation_tick, player, world);
    }

    /// The head of `ServerGamePacketListenerImpl.tick()` (run from
    /// `MinecraftServer.tickConnection`, after every level has ticked):
    /// flushes the pending `ClientboundBlockChangedAckPacket`.
    pub fn tick_connection(&mut self) -> Option<ClientboundBlockChangedAckPacket> {
        if self.ack_block_changes_up_to > -1 {
            let packet = ClientboundBlockChangedAckPacket::new(self.ack_block_changes_up_to);
            self.ack_block_changes_up_to = -1;
            Some(packet)
        } else {
            None
        }
    }

    /// `ackBlockChangesUpTo(int packetSequenceNr)`. A negative sequence
    /// disconnects and throws in Java, ending the handler: `false` here.
    fn ack_block_changes_up_to(&mut self, sequence: i32, out: &mut Vec<InteractionEffect>) -> bool {
        if sequence < 0 {
            out.push(InteractionEffect::Disconnect {
                reason: INVALID_SEQUENCE_DISCONNECT_MESSAGE.to_owned(),
            });
            return false;
        }
        self.ack_block_changes_up_to = sequence.max(self.ack_block_changes_up_to);
        true
    }

    /// Spigot `checkLimit(long timestamp)`.
    fn check_limit(&mut self, timestamp: i64) -> bool {
        let Some(threshold) = self.incoming_packet_threshold else {
            return true;
        };
        let threshold = i64::from(threshold);
        if self.last_limited_packet != -1
            && timestamp.wrapping_sub(self.last_limited_packet) < threshold
        {
            // `this.limitedPackets++ >= 8` — evaluated only when the two
            // conditions before it hold.
            let limited = self.limited_packets;
            self.limited_packets = limited.wrapping_add(1);
            if limited >= 8 {
                return false;
            }
        }
        if self.last_limited_packet == -1
            || timestamp.wrapping_sub(self.last_limited_packet) >= threshold
        {
            self.last_limited_packet = timestamp;
            self.limited_packets = 0;
        }
        true
    }

    /// `wasBlockPlacementAttempt(ServerPlayer player, ItemStack itemStack)`.
    fn was_block_placement_attempt(
        player: &InteractionPlayer,
        world: &dyn InteractionWorld,
        hand: InteractionHand,
    ) -> bool {
        if world.is_hand_empty(player, hand) {
            return false;
        }
        world.is_hand_block_placement_item(player, hand) && !world.is_hand_on_cooldown(player, hand)
    }

    /// `handlePlayerAction` — the `START_DESTROY_BLOCK` / `ABORT_DESTROY_BLOCK`
    /// / `STOP_DESTROY_BLOCK` arms (with the shared prefix). The other actions
    /// belong to the inventory and combat owners and yield no effects here.
    pub fn handle_player_action(
        &mut self,
        packet: &ServerboundPlayerActionPacket,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<InteractionEffect> {
        let mut out = Vec::new();
        if !matches!(
            packet.action,
            PlayerAction::StartDestroyBlock
                | PlayerAction::AbortDestroyBlock
                | PlayerAction::StopDestroyBlock
        ) {
            return out;
        }
        if player.is_immobile || !player.has_client_loaded {
            return out;
        }
        let pos = packet.pos;
        out.push(InteractionEffect::ResetLastActionTime);
        // Paper: don't allow digging into unloaded chunks.
        if !world.is_chunk_loaded(pos.get_x() >> 4, pos.get_z() >> 4)
            || !player.is_within_block_interaction_range(pos, 1.0)
        {
            self.ack_block_changes_up_to(packet.sequence, &mut out);
            return out;
        }
        // Paper: send block entities after destroy prediction.
        self.game_mode.captured_block_entity = false;
        self.game_mode.capture_sent_block_entities = true;
        let max_y = world.get_max_y();
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
        if !self.ack_block_changes_up_to(packet.sequence, &mut out) {
            return out;
        }
        self.game_mode.capture_sent_block_entities = false;
        if self.game_mode.captured_block_entity {
            // Manually tick: speed the ack up so the block entity is not overridden.
            out.push(InteractionEffect::BlockChangedAck(
                ClientboundBlockChangedAckPacket::new(self.ack_block_changes_up_to),
            ));
            self.ack_block_changes_up_to = -1;
            self.game_mode.captured_block_entity = false;
            out.push(InteractionEffect::BlockEntityUpdate { pos });
        }
        out.push(InteractionEffect::DetectEquipmentUpdates);
        out
    }

    /// `handleUseItemOn(ServerboundUseItemOnPacket)`.
    pub fn handle_use_item_on(
        &mut self,
        packet: &ServerboundUseItemOnPacket,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<InteractionEffect> {
        let mut out = Vec::new();
        if player.is_immobile {
            return out;
        }
        if !self.check_limit(packet.timestamp) {
            return out;
        }
        if !player.has_client_loaded {
            return out;
        }
        if !self.ack_block_changes_up_to(packet.sequence, &mut out) {
            return out;
        }
        let hand = packet.hand;
        if !world.is_hand_item_enabled(player, hand) {
            return out;
        }
        let block_hit = packet.block_hit;
        let location = block_hit.get_location();
        // Paper: improve distance check — `location.isFinite()`.
        if !(location.x.is_finite() && location.y.is_finite() && location.z.is_finite()) {
            return out;
        }
        let pos = block_hit.get_block_pos();
        if !player.is_within_block_interaction_range(pos, 1.0) {
            return out;
        }
        // `location.subtract(Vec3.atCenterOf(pos))`, each axis `< 1.0000001`.
        let distance_x = location.x - (pos.get_x() as f64 + 0.5);
        let distance_y = location.y - (pos.get_y() as f64 + 0.5);
        let distance_z = location.z - (pos.get_z() as f64 + 0.5);
        if !(distance_x.abs() < 1.0000001
            && distance_y.abs() < 1.0000001
            && distance_z.abs() < 1.0000001)
        {
            return out;
        }

        let direction = block_hit.get_direction();
        out.push(InteractionEffect::ResetLastActionTime);
        let max_y = world.get_max_y();
        let min_y = world.get_min_y();
        if pos.get_y() > max_y {
            out.push(InteractionEffect::SendBuildLimitMessage {
                above: true,
                limit: max_y,
            });
        } else if pos.get_y() < min_y {
            out.push(InteractionEffect::SendBuildLimitMessage {
                above: false,
                limit: min_y,
            });
        } else {
            // Paper: allow using signs inside spawn protection.
            let passthrough_sign_interaction = world.is_sign_interaction_passthrough(pos);
            if !passthrough_sign_interaction && world.is_under_spawn_protection(player, pos) {
                out.push(InteractionEffect::SendSpawnProtectionMessage { pos });
            } else if !player.awaiting_position_from_client
                && (world.may_interact(player, pos) || passthrough_sign_interaction)
            {
                out.push(InteractionEffect::StopUsingItem);
                let interaction_result = self
                    .game_mode
                    .use_item_on(player, world, hand, &block_hit, &mut out);
                let consumes_action = interaction_result.consumes_action();
                // STUB(mc.advancements.triggers) on consumesAction(): CriteriaTriggers.ANY_BLOCK_USE.trigger(player, pos, itemStack).

                if direction == Direction::Up
                    && !consumes_action
                    && pos.get_y() >= max_y
                    && Self::was_block_placement_attempt(player, world, hand)
                {
                    out.push(InteractionEffect::SendBuildLimitMessage {
                        above: true,
                        limit: max_y,
                    });
                } else if interaction_result.is_success_with_swing_source(SwingSource::Server)
                    && !self.game_mode.interact_result
                {
                    out.push(InteractionEffect::Swing(hand));
                }

                if !consumes_action && Self::was_block_placement_attempt(player, world, hand) {
                    if direction == Direction::Up && pos.get_y() >= max_y {
                        out.push(InteractionEffect::SendBuildLimitMessage {
                            above: true,
                            limit: max_y,
                        });
                    } else if direction == Direction::Down && pos.get_y() <= min_y {
                        out.push(InteractionEffect::SendBuildLimitMessage {
                            above: false,
                            limit: min_y,
                        });
                    }
                } else if interaction_result.is_success_with_swing_source(SwingSource::Server) {
                    out.push(InteractionEffect::Swing(hand));
                }
            } else if world.is_under_spawn_protection(player, pos) {
                // Paper: fix inventory desync (MC-99075).
                if world.client_predicts_interaction(player, world.get_block_state(pos), hand) {
                    out.push(InteractionEffect::ContainerSendAllDataToRemote);
                } else {
                    out.push(InteractionEffect::ContainerForceHeldSlot(hand));
                }
            }
            // Paper (MC-306582): no build-limit message in the final else.

            send_block_update(world, pos, &mut out);
            send_block_update(world, pos.relative(&direction), &mut out);
        }
        out.push(InteractionEffect::DetectEquipmentUpdates);
        out
    }

    /// `handleUseItem(ServerboundUseItemPacket)`.
    pub fn handle_use_item(
        &mut self,
        packet: &ServerboundUseItemPacket,
        player: &InteractionPlayer,
        world: &mut dyn InteractionWorld,
    ) -> Vec<InteractionEffect> {
        let mut out = Vec::new();
        if player.is_immobile {
            return out;
        }
        if !self.check_limit(packet.timestamp) {
            return out;
        }
        if !player.has_client_loaded {
            return out;
        }
        if !self.ack_block_changes_up_to(packet.sequence, &mut out) {
            return out;
        }
        let hand = packet.hand;
        out.push(InteractionEffect::ResetLastActionTime);
        if world.is_hand_empty(player, hand) || !world.is_hand_item_enabled(player, hand) {
            return out;
        }
        let target_y_rot = wrap_degrees_f32(packet.y_rot);
        let target_x_rot = wrap_degrees_f32(packet.x_rot);
        let mut player = *player;
        if target_x_rot != player.x_rot || target_y_rot != player.y_rot {
            out.push(InteractionEffect::AbsSnapRotationTo {
                y_rot: target_y_rot,
                x_rot: target_x_rot,
            });
            abs_snap_rotation_to(&mut player, target_y_rot, target_x_rot);
        }

        // CraftBukkit: raytrace to look for 'rogue armswings'.
        let from = player.eye_position;
        let delta = scale(
            calculate_view_vector(player.x_rot, player.y_head_rot),
            player.block_interaction_range,
        );
        let to = Vec3::new(from.x + delta.x, from.y + delta.y, from.z + delta.z);
        let hit_result = world.clip(&player, from, to);

        let cancelled = if hit_result.get_type() != HitResultType::Block {
            // RivetTodo(#501) PlayerInteractEvent RIGHT_CLICK_AIR producer site. With no listener
            // useItemInHand() keeps its DEFAULT, so the use is not cancelled.
            false
        } else {
            let item_stack = world.get_item_in_hand(&player, hand);
            let game_mode = &self.game_mode;
            let cancelled = if game_mode.fired_interact
                && game_mode.interact_position == Some(hit_result.get_block_pos())
                && game_mode.interact_hand == Some(hand)
                && game_mode
                    .interact_item_stack
                    .as_ref()
                    .is_some_and(|interact| {
                        ItemStack::is_same_item_same_components(interact, &item_stack)
                    }) {
                game_mode.interact_result
            } else {
                // RivetTodo(#501) PlayerInteractEvent RIGHT_CLICK_BLOCK producer site. With no
                // listener useItemInHand() keeps its DEFAULT, so the use is not cancelled.
                false
            };
            self.game_mode.fired_interact = false;
            cancelled
        };

        if cancelled {
            // Paper: properly cancel usable items.
            out.push(InteractionEffect::ResyncUsingItem);
            // Paper: fix inventory desync (SPIGOT-2524).
            let air =
                BlockState::of(BlockId::from_name("minecraft:air").expect("air is registered"));
            if world.client_predicts_interaction(&player, air, hand) {
                out.push(InteractionEffect::ContainerSendAllDataToRemote);
            } else {
                out.push(InteractionEffect::ContainerForceHeldSlotAndArmor(hand));
            }
            return out;
        }
        // Re-read in case the event changed the hand item.
        if world.is_hand_empty(&player, hand) {
            return out;
        }

        if self
            .game_mode
            .use_item(&player, world, hand)
            .is_success_with_swing_source(SwingSource::Server)
        {
            out.push(InteractionEffect::Swing(hand));
        }
        out
    }
}

impl Default for WorldInteractionService {
    fn default() -> Self {
        Self::new(PAPER_DEFAULT_INCOMING_PACKET_THRESHOLD)
    }
}

/// The rotation `Entity.absSnapRotationTo(yRot, xRot)` leaves on the player:
/// `setYRot(yRot % 360)` and `setXRot(Mth.clamp(xRot, -90, 90) % 360)`, each
/// discarding a non-finite value (`Entity.setYRot`/`setXRot`), `setXRot`
/// storing `Math.clamp(xRot % 360, -90, 90)`, then Paper's
/// `setYHeadRot(yRot)` (`LivingEntity` stores it unchecked).
fn abs_snap_rotation_to(player: &mut InteractionPlayer, y_rot: f32, x_rot: f32) {
    let head_y_rot = y_rot;
    let y_rot = y_rot % 360.0;
    if y_rot.is_finite() {
        player.y_rot = y_rot;
    }
    let x_rot = clamp_f32(x_rot, -90.0, 90.0) % 360.0;
    if x_rot.is_finite() {
        // `Math.clamp(float, float, float)`; the value is finite here, where
        // `f32::clamp` matches `Math.min(max, Math.max(value, min))` (signed zeros included).
        player.x_rot = (x_rot % 360.0).clamp(-90.0, 90.0);
    }
    player.y_head_rot = head_y_rot;
}

/// `Entity.calculateViewVector(float xRot, float yRot)` — `Mth` table trig in
/// `float`, widened to `double` by `new Vec3(double, double, double)`.
fn calculate_view_vector(x_rot: f32, y_rot: f32) -> Vec3 {
    let real_x_rot = x_rot * mth::DEG_TO_RAD;
    let real_y_rot = -y_rot * mth::DEG_TO_RAD;
    let y_cos = mth::cos(f64::from(real_y_rot));
    let y_sin = mth::sin(f64::from(real_y_rot));
    let x_cos = mth::cos(f64::from(real_x_rot));
    let x_sin = mth::sin(f64::from(real_x_rot));
    Vec3::new(
        f64::from(y_sin * x_cos),
        f64::from(-x_sin),
        f64::from(y_cos * x_cos),
    )
}

/// `Vec3.scale(double scale)`.
fn scale(vec: Vec3, scale: f64) -> Vec3 {
    Vec3::new(vec.x * scale, vec.y * scale, vec.z * scale)
}

#[cfg(test)]
mod tests;
