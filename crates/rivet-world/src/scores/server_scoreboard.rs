//! Port of `net.minecraft.server.ServerScoreboard` (Paper 26.2) and the
//! authoritative `ScoreboardProvider` trait consumed by future M4 facades
//! (e.g. `CraftScoreboardManager`, #502, #503).

use crate::scores::{
    ClientboundResetScorePacket, ClientboundSetDisplayObjectivePacket,
    ClientboundSetObjectivePacket, ClientboundSetPlayerTeamPacket, ClientboundSetScorePacket,
    CollisionRule, DisplaySlot, NumberFormat, Objective, ObjectiveCriteria, PackedObjective,
    PackedPlayerTeam, PackedScore, PlayerScoreEntry, PlayerTeam, ReadOnlyScoreInfo, RenderType,
    Score, ScoreHolder, Scoreboard, ScoreboardError, ScoreboardListener, ScoreboardPacket,
    TeamColor, TeamPacketAction, Visibility,
};
use indexmap::{IndexMap, IndexSet};
use rivet_text::Component;
use std::collections::BTreeMap;

/// Outbound effects recorded by `ServerScoreboard` during mutations.
///
/// Keeping packet emission as explicit, ordered values preserves tick-thread
/// ownership (`OWNERSHIP.md` — no `Arc<RwLock>`) and lets callers (network
/// broadcast layer, tests, M4 facades) inspect or drain exact packet traces.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ScoreboardEffect {
    /// Broadcast a scoreboard packet to all players observing this scoreboard.
    Broadcast(ScoreboardPacket),
    /// Mark persisted scoreboard state dirty (`ServerScoreboard.setDirty()`).
    MarkDirty,
}

/// Internal listener bridging `Scoreboard` mutation hooks to `ServerScoreboard`
/// tracking, dirty state, and packet emission.
struct ServerListener<'a> {
    tracked_objectives: &'a mut IndexSet<String>,
    effects: &'a mut Vec<ScoreboardEffect>,
    dirty: &'a mut bool,
    enable_player_collisions: bool,
}

impl<'a> ServerListener<'a> {
    fn emit(&mut self, packet: ScoreboardPacket) {
        self.effects.push(ScoreboardEffect::Broadcast(packet));
    }

    fn mark_dirty(&mut self) {
        *self.dirty = true;
        self.effects.push(ScoreboardEffect::MarkDirty);
    }
}

impl<'a> ScoreboardListener for ServerListener<'a> {
    fn on_score_changed(&mut self, holder: &ScoreHolder, objective: &Objective, score: &Score) {
        if self.tracked_objectives.contains(objective.name()) {
            self.emit(ScoreboardPacket::SetScore(ClientboundSetScorePacket {
                owner: holder.scoreboard_name().to_owned(),
                objective_name: objective.name().to_owned(),
                score: score.value(),
                display: score.display().cloned(),
                number_format: score.number_format().cloned(),
            }));
        }
        self.mark_dirty();
    }

    fn on_score_lock_changed(&mut self, _holder: &ScoreHolder, _objective: &Objective) {
        self.mark_dirty();
    }

    fn on_player_removed(&mut self, holder: &ScoreHolder) {
        self.emit(ScoreboardPacket::ResetScore(ClientboundResetScorePacket {
            owner: holder.scoreboard_name().to_owned(),
            objective_name: None,
        }));
        self.mark_dirty();
    }

    fn on_player_score_removed(&mut self, holder: &ScoreHolder, objective: &Objective) {
        if self.tracked_objectives.contains(objective.name()) {
            self.emit(ScoreboardPacket::ResetScore(ClientboundResetScorePacket {
                owner: holder.scoreboard_name().to_owned(),
                objective_name: Some(objective.name().to_owned()),
            }));
        }
        self.mark_dirty();
    }

    fn on_objective_added(&mut self, _objective: &Objective) {
        self.mark_dirty();
    }

    fn on_objective_changed(&mut self, objective: &Objective) {
        if self.tracked_objectives.contains(objective.name()) {
            self.emit(ScoreboardPacket::SetObjective(
                ClientboundSetObjectivePacket::from_objective(
                    objective,
                    ClientboundSetObjectivePacket::METHOD_CHANGE,
                ),
            ));
        }
        self.mark_dirty();
    }

    fn on_objective_removed(&mut self, _objective: &Objective) {
        // Note: stop_tracking_objective is handled by ServerScoreboard::remove_objective
        // before/after removing from base scoreboard, matching Paper's order.
        self.mark_dirty();
    }

    fn on_team_added(&mut self, team: &PlayerTeam) {
        self.emit(ScoreboardPacket::SetPlayerTeam(
            ClientboundSetPlayerTeamPacket::create_add_or_modify(
                team,
                true,
                self.enable_player_collisions,
            ),
        ));
        self.mark_dirty();
    }

    fn on_team_changed(&mut self, team: &PlayerTeam) {
        self.emit(ScoreboardPacket::SetPlayerTeam(
            ClientboundSetPlayerTeamPacket::create_add_or_modify(
                team,
                false,
                self.enable_player_collisions,
            ),
        ));
        self.mark_dirty();
    }

    fn on_team_removed(&mut self, team: &PlayerTeam) {
        self.emit(ScoreboardPacket::SetPlayerTeam(
            ClientboundSetPlayerTeamPacket::create_remove(team.name()),
        ));
        self.mark_dirty();
    }

    fn on_player_added_to_team(&mut self, player: &str, team: &PlayerTeam) {
        self.emit(ScoreboardPacket::SetPlayerTeam(
            ClientboundSetPlayerTeamPacket::create_player(
                team.name(),
                player,
                TeamPacketAction::Add,
            ),
        ));
        self.mark_dirty();
    }

    fn on_player_removed_from_team(&mut self, player: &str, team: &PlayerTeam) {
        self.emit(ScoreboardPacket::SetPlayerTeam(
            ClientboundSetPlayerTeamPacket::create_player(
                team.name(),
                player,
                TeamPacketAction::Remove,
            ),
        ));
        self.mark_dirty();
    }
}

/// Port of `net.minecraft.server.ServerScoreboard`.
///
/// Owns the authoritative `Scoreboard`, the set of currently tracked objectives,
/// dirty state for `ScoreboardSaveData`, and the ordered outbound effect queue.
#[derive(Clone, Debug)]
pub struct ServerScoreboard {
    scoreboard: Scoreboard,
    tracked_objectives: IndexSet<String>,
    dirty: bool,
    enable_player_collisions: bool,
    pending_effects: Vec<ScoreboardEffect>,
}

impl Default for ServerScoreboard {
    fn default() -> Self {
        Self::new()
    }
}

impl ServerScoreboard {
    pub fn new() -> Self {
        Self {
            scoreboard: Scoreboard::new(),
            tracked_objectives: IndexSet::new(),
            dirty: false,
            enable_player_collisions: true,
            pending_effects: Vec::new(),
        }
    }

    /// Paper global-config toggle `collisions.enablePlayerCollisions`.
    pub fn with_player_collisions(mut self, enabled: bool) -> Self {
        self.enable_player_collisions = enabled;
        self
    }

    pub fn enable_player_collisions(&self) -> bool {
        self.enable_player_collisions
    }

    pub fn set_enable_player_collisions(&mut self, enabled: bool) {
        if self.enable_player_collisions == enabled {
            return;
        }
        self.enable_player_collisions = enabled;
        for team in self.scoreboard.player_teams() {
            self.pending_effects.push(ScoreboardEffect::Broadcast(
                ScoreboardPacket::SetPlayerTeam(
                    ClientboundSetPlayerTeamPacket::create_add_or_modify(team, false, enabled),
                ),
            ));
        }
    }

    /// Read-only access to the underlying `Scoreboard`.
    pub fn scoreboard(&self) -> &Scoreboard {
        &self.scoreboard
    }

    /// Currently tracked objective names (objectives shown in >= 1 display slot).
    pub fn tracked_objectives(&self) -> &IndexSet<String> {
        &self.tracked_objectives
    }

    pub fn is_objective_tracked(&self, objective_name: &str) -> bool {
        self.tracked_objectives.contains(objective_name)
    }

    /// Whether the scoreboard has unsaved mutations.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.pending_effects.push(ScoreboardEffect::MarkDirty);
    }

    /// Inspect pending effects without draining.
    pub fn pending_effects(&self) -> &[ScoreboardEffect] {
        &self.pending_effects
    }

    /// Drain all pending effects (broadcast packets + dirty signals) in exact emission order.
    pub fn drain_effects(&mut self) -> Vec<ScoreboardEffect> {
        std::mem::take(&mut self.pending_effects)
    }

    /// Drain only the broadcast packets in exact emission order (discarding `MarkDirty` markers).
    pub fn drain_packets(&mut self) -> Vec<ScoreboardPacket> {
        self.drain_effects()
            .into_iter()
            .filter_map(|e| match e {
                ScoreboardEffect::Broadcast(p) => Some(p),
                ScoreboardEffect::MarkDirty => None,
            })
            .collect()
    }

    // -------------------------------------------------------------------------
    // Tracking packet builders (exact Paper ServerScoreboard methods)
    // -------------------------------------------------------------------------

    /// `ServerScoreboard.getStartTrackingPackets(Objective)`:
    /// 1. `ClientboundSetObjectivePacket(objective, METHOD_ADD)`
    /// 2. `ClientboundSetDisplayObjectivePacket(slot, objective)` for every slot showing it
    /// 3. `ClientboundSetScorePacket(...)` for every score in scoreboard iteration order
    pub fn start_tracking_packets(&self, objective_name: &str) -> Vec<ScoreboardPacket> {
        let Some(objective) = self.scoreboard.get_objective(objective_name) else {
            return Vec::new();
        };
        let mut packets = Vec::new();
        packets.push(ScoreboardPacket::SetObjective(
            ClientboundSetObjectivePacket::from_objective(
                objective,
                ClientboundSetObjectivePacket::METHOD_ADD,
            ),
        ));

        for &slot in &DisplaySlot::ALL {
            if self
                .scoreboard
                .get_display_objective(slot)
                .is_some_and(|o| o.name() == objective_name)
            {
                packets.push(ScoreboardPacket::SetDisplayObjective(
                    ClientboundSetDisplayObjectivePacket::new(slot, Some(objective)),
                ));
            }
        }

        for entry in self.scoreboard.list_player_scores(objective_name) {
            packets.push(ScoreboardPacket::SetScore(ClientboundSetScorePacket {
                owner: entry.owner,
                objective_name: objective.name().to_owned(),
                score: entry.value,
                display: entry.display,
                number_format: entry.number_format_override,
            }));
        }

        packets
    }

    /// `ServerScoreboard.startTrackingObjective(Objective)`.
    pub fn start_tracking_objective(&mut self, objective_name: &str) {
        if self.scoreboard.get_objective(objective_name).is_none() {
            return;
        }
        let packets = self.start_tracking_packets(objective_name);
        for packet in packets {
            self.pending_effects
                .push(ScoreboardEffect::Broadcast(packet));
        }
        self.tracked_objectives.insert(objective_name.to_owned());
    }

    /// `ServerScoreboard.getStopTrackingPackets(Objective)`:
    /// 1. `ClientboundSetObjectivePacket(objective, METHOD_REMOVE)`
    /// 2. `ClientboundSetDisplayObjectivePacket(slot, objective)` for every slot still showing it
    pub fn stop_tracking_packets(&self, objective_name: &str) -> Vec<ScoreboardPacket> {
        let Some(objective) = self.scoreboard.get_objective(objective_name) else {
            return Vec::new();
        };
        let mut packets = Vec::new();
        packets.push(ScoreboardPacket::SetObjective(
            ClientboundSetObjectivePacket::from_objective(
                objective,
                ClientboundSetObjectivePacket::METHOD_REMOVE,
            ),
        ));

        for &slot in &DisplaySlot::ALL {
            if self
                .scoreboard
                .get_display_objective(slot)
                .is_some_and(|o| o.name() == objective_name)
            {
                packets.push(ScoreboardPacket::SetDisplayObjective(
                    ClientboundSetDisplayObjectivePacket::new(slot, Some(objective)),
                ));
            }
        }

        packets
    }

    /// `ServerScoreboard.stopTrackingObjective(Objective)`.
    pub fn stop_tracking_objective(&mut self, objective_name: &str) {
        let packets = self.stop_tracking_packets(objective_name);
        for packet in packets {
            self.pending_effects
                .push(ScoreboardEffect::Broadcast(packet));
        }
        self.tracked_objectives.shift_remove(objective_name);
    }

    /// `ServerScoreboard.getObjectiveDisplaySlotCount(Objective)`.
    pub fn objective_display_slot_count(&self, objective_name: &str) -> usize {
        self.scoreboard.display_slot_count(objective_name)
    }

    /// `PlayerList.updateEntireScoreboard(ServerScoreboard, ServerPlayer)`:
    /// 1. `ClientboundSetPlayerTeamPacket.createAddOrModifyPacket(team, true)` for every team
    /// 2. For each `DisplaySlot` in `DisplaySlot::ALL`: if it has a display objective not yet
    ///    emitted, append `start_tracking_packets(objective)` once.
    pub fn initial_sync_packets_for_player(&self) -> Vec<ScoreboardPacket> {
        let mut packets = Vec::new();
        for team in self.scoreboard.player_teams() {
            packets.push(ScoreboardPacket::SetPlayerTeam(
                ClientboundSetPlayerTeamPacket::create_add_or_modify(
                    team,
                    true,
                    self.enable_player_collisions,
                ),
            ));
        }

        let mut sent_objectives = IndexSet::new();
        for &slot in &DisplaySlot::ALL {
            if let Some(objective) = self.scoreboard.get_display_objective(slot)
                && sent_objectives.insert(objective.name().to_owned())
            {
                packets.extend(self.start_tracking_packets(objective.name()));
            }
        }
        packets
    }

    // -------------------------------------------------------------------------
    // Authoritative mutations (matching ServerScoreboard overrides)
    // -------------------------------------------------------------------------

    /// `ServerScoreboard.addObjective(...)`.
    pub fn add_objective(
        &mut self,
        name: impl Into<String>,
        criteria: ObjectiveCriteria,
        display_name: Component,
        render_type: RenderType,
        display_auto_update: bool,
        number_format: Option<NumberFormat>,
    ) -> Result<&Objective, ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard.add_objective(
            name,
            criteria,
            display_name,
            render_type,
            display_auto_update,
            number_format,
            &mut listener,
        )
    }

    /// `ServerScoreboard.setDisplayObjective(DisplaySlot, @Nullable Objective)`.
    ///
    /// Exact Paper 26.2 order:
    /// 1. Read `old` objective for `slot`.
    /// 2. Update base scoreboard's display map.
    /// 3. If `old != new && old != null`:
    ///    - if `display_slot_count(old) > 0`: emit `SetDisplayObjective(slot, new)`
    ///    - else: `stop_tracking_objective(old)`
    /// 4. If `new != null`:
    ///    - if `tracked_objectives.contains(new)`: emit `SetDisplayObjective(slot, new)`
    ///    - else: `start_tracking_objective(new)`
    /// 5. `setDirty()`.
    pub fn set_display_objective(
        &mut self,
        slot: DisplaySlot,
        objective_name: Option<&str>,
    ) -> Result<(), ScoreboardError> {
        let new_obj = match objective_name {
            Some(name) => Some(
                self.scoreboard
                    .get_objective(name)
                    .ok_or_else(|| ScoreboardError::UnknownObjective(name.to_owned()))?
                    .clone(),
            ),
            None => None,
        };
        let old_obj = self.scoreboard.get_display_objective(slot).cloned();

        let mut noop = crate::scores::NoopListener;
        self.scoreboard
            .set_display_objective(slot, objective_name, &mut noop)?;

        let old_name = old_obj.as_ref().map(Objective::name);
        let new_name = new_obj.as_ref().map(Objective::name);

        if old_name != new_name
            && let Some(old) = &old_obj
        {
            if self.objective_display_slot_count(old.name()) > 0 {
                self.pending_effects.push(ScoreboardEffect::Broadcast(
                    ScoreboardPacket::SetDisplayObjective(
                        ClientboundSetDisplayObjectivePacket::new(slot, new_obj.as_ref()),
                    ),
                ));
            } else {
                self.stop_tracking_objective(old.name());
            }
        }

        if let Some(new) = &new_obj {
            if self.tracked_objectives.contains(new.name()) {
                self.pending_effects.push(ScoreboardEffect::Broadcast(
                    ScoreboardPacket::SetDisplayObjective(
                        ClientboundSetDisplayObjectivePacket::new(slot, Some(new)),
                    ),
                ));
            } else {
                self.start_tracking_objective(new.name());
            }
        }

        self.mark_dirty();
        Ok(())
    }

    /// `ServerScoreboard.removeObjective(Objective)`.
    ///
    /// Clears display slots via `ServerScoreboard::set_display_objective` (matching Paper's
    /// virtual dispatch from `Scoreboard.removeObjective`), removes scores and criteria index,
    /// then stops tracking if still tracked and marks dirty.
    pub fn remove_objective(&mut self, objective_name: &str) -> Option<Objective> {
        self.scoreboard.get_objective(objective_name)?;

        // First clear every display slot pointing at this objective using ServerScoreboard's
        // set_display_objective (which emits the exact Paper display-slot / stop-tracking packets).
        let slots: Vec<DisplaySlot> = DisplaySlot::ALL
            .iter()
            .copied()
            .filter(|&slot| {
                self.scoreboard
                    .get_display_objective(slot)
                    .is_some_and(|o| o.name() == objective_name)
            })
            .collect();
        for slot in slots {
            let _ = self.set_display_objective(slot, None);
        }

        // If still tracked (e.g. manually started tracking without a display slot), stop tracking now.
        if self.tracked_objectives.contains(objective_name) {
            self.stop_tracking_objective(objective_name);
        }

        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .remove_objective(objective_name, &mut listener)
    }

    pub fn set_objective_display_name(
        &mut self,
        objective_name: &str,
        display_name: Component,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_objective_display_name(objective_name, display_name, &mut listener)
    }

    pub fn set_objective_render_type(
        &mut self,
        objective_name: &str,
        render_type: RenderType,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_objective_render_type(objective_name, render_type, &mut listener)
    }

    pub fn set_objective_display_auto_update(
        &mut self,
        objective_name: &str,
        display_auto_update: bool,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard.set_objective_display_auto_update(
            objective_name,
            display_auto_update,
            &mut listener,
        )
    }

    pub fn set_objective_number_format(
        &mut self,
        objective_name: &str,
        number_format: Option<NumberFormat>,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_objective_number_format(objective_name, number_format, &mut listener)
    }

    /// Mutate a single player's score on an objective through `ScoreAccess`.
    pub fn mutate_player_score<R, F>(
        &mut self,
        holder: &ScoreHolder,
        objective_name: &str,
        force_writable: bool,
        f: F,
    ) -> Result<R, ScoreboardError>
    where
        F: FnOnce(&mut crate::scores::ScoreAccess<'_>) -> Result<R, ScoreboardError>,
    {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        let mut access = self.scoreboard.access_player_score(
            holder,
            objective_name,
            force_writable,
            &mut listener,
        )?;
        f(&mut access)
    }

    /// `ServerScoreboard.forAllObjectives(criteria, holder, op)` (always `force_writable = true`).
    pub fn for_all_objectives<F>(
        &mut self,
        criteria: &ObjectiveCriteria,
        holder: &ScoreHolder,
        op: F,
    ) -> Result<(), ScoreboardError>
    where
        F: FnMut(&mut crate::scores::ScoreAccess<'_>) -> Result<(), ScoreboardError>,
    {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .for_all_objectives(criteria, holder, &mut listener, op)
    }

    pub fn reset_all_player_scores(&mut self, holder: &ScoreHolder) -> bool {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .reset_all_player_scores(holder, &mut listener)
    }

    pub fn reset_single_player_score(
        &mut self,
        holder: &ScoreHolder,
        objective_name: &str,
    ) -> bool {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .reset_single_player_score(holder, objective_name, &mut listener)
    }

    // -------------------------------------------------------------------------
    // Teams
    // -------------------------------------------------------------------------

    pub fn add_player_team(
        &mut self,
        name: impl Into<String>,
    ) -> Result<&PlayerTeam, ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard.add_player_team(name, &mut listener)
    }

    pub fn remove_player_team(&mut self, team_name: &str) -> Option<PlayerTeam> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard.remove_player_team(team_name, &mut listener)
    }

    pub fn add_player_to_team(
        &mut self,
        player: &str,
        team_name: &str,
    ) -> Result<bool, ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .add_player_to_team(player, team_name, &mut listener)
    }

    pub fn remove_player_from_any_team(&mut self, player: &str) -> bool {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .remove_player_from_any_team(player, &mut listener)
    }

    pub fn remove_player_from_team(
        &mut self,
        player: &str,
        team_name: &str,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .remove_player_from_team(player, team_name, &mut listener)
    }

    pub fn set_team_display_name(
        &mut self,
        team_name: &str,
        display_name: Component,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_display_name(team_name, display_name, &mut listener)
    }

    pub fn set_team_prefix(
        &mut self,
        team_name: &str,
        prefix: Option<Component>,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_prefix(team_name, prefix, &mut listener)
    }

    pub fn set_team_suffix(
        &mut self,
        team_name: &str,
        suffix: Option<Component>,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_suffix(team_name, suffix, &mut listener)
    }

    pub fn set_team_allow_friendly_fire(
        &mut self,
        team_name: &str,
        allow: bool,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_allow_friendly_fire(team_name, allow, &mut listener)
    }

    pub fn set_team_see_friendly_invisibles(
        &mut self,
        team_name: &str,
        see: bool,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_see_friendly_invisibles(team_name, see, &mut listener)
    }

    pub fn set_team_name_tag_visibility(
        &mut self,
        team_name: &str,
        visibility: Visibility,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_name_tag_visibility(team_name, visibility, &mut listener)
    }

    pub fn set_team_death_message_visibility(
        &mut self,
        team_name: &str,
        visibility: Visibility,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_death_message_visibility(team_name, visibility, &mut listener)
    }

    pub fn set_team_collision_rule(
        &mut self,
        team_name: &str,
        rule: CollisionRule,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_collision_rule(team_name, rule, &mut listener)
    }

    pub fn set_team_color(
        &mut self,
        team_name: &str,
        color: Option<TeamColor>,
    ) -> Result<(), ScoreboardError> {
        let mut listener = ServerListener {
            tracked_objectives: &mut self.tracked_objectives,
            effects: &mut self.pending_effects,
            dirty: &mut self.dirty,
            enable_player_collisions: self.enable_player_collisions,
        };
        self.scoreboard
            .set_team_color(team_name, color, &mut listener)
    }

    // -------------------------------------------------------------------------
    // Persistence load/pack (used by ScoreboardSaveData)
    // -------------------------------------------------------------------------

    /// Load packed scoreboard data in exact Paper 26.2 order:
    /// 1. objectives
    /// 2. player scores
    /// 3. display slots
    /// 4. teams
    ///
    /// Loading from disk does NOT emit clientbound broadcast packets or leave
    /// the scoreboard dirty, while correctly populating `tracked_objectives`
    /// for every objective assigned to a display slot.
    pub fn load_packed(
        &mut self,
        objectives: &[PackedObjective],
        scores: &[PackedScore],
        display_slots: &BTreeMap<DisplaySlot, String>,
        teams: &[PackedPlayerTeam],
    ) -> Result<(), ScoreboardError> {
        let mut noop = crate::scores::NoopListener;
        for packed_obj in objectives {
            self.scoreboard.load_objective(packed_obj, &mut noop)?;
        }
        for packed_score in scores {
            self.scoreboard.load_player_score(packed_score);
        }
        for (&slot, obj_name) in display_slots {
            if self.scoreboard.get_objective(obj_name).is_some() {
                self.scoreboard
                    .set_display_objective(slot, Some(obj_name), &mut noop)?;
                self.tracked_objectives.insert(obj_name.clone());
            }
        }
        for packed_team in teams {
            self.scoreboard.load_player_team(packed_team, &mut noop)?;
        }
        self.dirty = false;
        self.pending_effects.clear();
        Ok(())
    }

    pub fn pack_objectives(&self) -> Vec<PackedObjective> {
        self.scoreboard.pack_objectives()
    }

    pub fn pack_player_scores(&self) -> Vec<PackedScore> {
        self.scoreboard.pack_player_scores()
    }

    pub fn pack_display_slots(&self) -> BTreeMap<DisplaySlot, String> {
        self.scoreboard.pack_display_slots()
    }

    pub fn pack_player_teams(&self) -> Vec<PackedPlayerTeam> {
        self.scoreboard.pack_player_teams()
    }

    // -------------------------------------------------------------------------
    // Convenience read-only delegates
    // -------------------------------------------------------------------------

    pub fn get_objective(&self, name: &str) -> Option<&Objective> {
        self.scoreboard.get_objective(name)
    }

    pub fn get_display_objective(&self, slot: DisplaySlot) -> Option<&Objective> {
        self.scoreboard.get_display_objective(slot)
    }

    pub fn get_player_score_info(
        &self,
        holder: &ScoreHolder,
        objective_name: &str,
    ) -> Option<ReadOnlyScoreInfo> {
        self.scoreboard
            .get_player_score_info(holder, objective_name)
    }

    pub fn list_player_scores(&self, objective_name: &str) -> Vec<PlayerScoreEntry> {
        self.scoreboard.list_player_scores(objective_name)
    }

    pub fn list_scores_for_holder(&self, holder: &ScoreHolder) -> IndexMap<String, i32> {
        self.scoreboard.list_scores_for_holder(holder)
    }

    pub fn get_player_team(&self, name: &str) -> Option<&PlayerTeam> {
        self.scoreboard.get_player_team(name)
    }

    pub fn get_players_team(&self, player_name: &str) -> Option<&PlayerTeam> {
        self.scoreboard.get_players_team(player_name)
    }
}

/// Typed authoritative scoreboard provider implemented by world/server state
/// so future M4 facades (`CraftScoreboardManager`, #502, #503) operate on the
/// single authoritative `ServerScoreboard` instead of duplicating state.
pub trait ScoreboardProvider {
    fn scoreboard(&self) -> &ServerScoreboard;
    fn scoreboard_mut(&mut self) -> &mut ServerScoreboard;
}

impl ScoreboardProvider for ServerScoreboard {
    fn scoreboard(&self) -> &ServerScoreboard {
        self
    }

    fn scoreboard_mut(&mut self) -> &mut ServerScoreboard {
        self
    }
}
