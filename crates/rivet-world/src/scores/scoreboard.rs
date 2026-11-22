//! Port of `net.minecraft.world.scores.Scoreboard` (Paper 26.2).

use crate::scores::{
    CollisionRule, DisplaySlot, NumberFormat, Objective, ObjectiveCriteria, PackedObjective,
    PackedPlayerTeam, PackedScoreData, PlayerScoreEntry, PlayerScores, PlayerTeam,
    ReadOnlyScoreInfo, RenderType, Score, ScoreHolder, TeamColor, Visibility,
};
use indexmap::IndexMap;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_text::Component;
use std::collections::BTreeMap;
use std::sync::Arc;
use thiserror::Error;

/// Errors returned by authoritative scoreboard mutations (matching Paper exceptions).
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ScoreboardError {
    #[error("An objective with the name '{0}' already exists!")]
    DuplicateObjective(String),
    #[error("Unknown objective '{0}'")]
    UnknownObjective(String),
    #[error("Cannot modify read-only score")]
    ReadOnlyScore,
    #[error("A team with the name '{0}' already exists!")]
    DuplicateTeam(String),
    #[error("Unknown team '{0}'")]
    UnknownTeam(String),
    #[error("Player is either on another team or not on any team. Cannot remove from team '{0}'.")]
    PlayerNotOnTeam(String),
}

/// Port of `net.minecraft.world.scores.Scoreboard.PackedScore`.
#[derive(Clone, Debug, PartialEq)]
pub struct PackedScore {
    pub owner: String,
    pub objective: String,
    pub score: PackedScoreData,
}

impl PackedScore {
    /// `Scoreboard.PackedScore.CODEC`.
    ///
    /// Exact Paper 26.2 layout:
    /// - `"Name"` (required `String`)
    /// - `"Objective"` (required `String`)
    /// - flattened `Score.Packed.MAP_CODEC` (`Score`, `Locked`, `display`, `format`)
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<PackedScore, Ops>> {
        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScore| p.owner.clone()),
                    codec::field_of(codec::string_codec::<Ops>(), "Name".to_owned()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScore| p.objective.clone()),
                    codec::field_of(codec::string_codec::<Ops>(), "Objective".to_owned()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScore| p.score.clone()),
                    PackedScoreData::map_codec::<Ops>(),
                ))
                .apply(
                    instance,
                    Arc::new(|owner, objective, score| PackedScore {
                        owner,
                        objective,
                        score,
                    }),
                )
        })
    }
}

impl rivet_serialization::codec::JavaEquals for PackedScore {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// Callbacks fired by `Scoreboard` mutations, mirroring `Scoreboard.on*` virtual methods
/// overridden by `ServerScoreboard`.
pub trait ScoreboardListener {
    fn on_score_changed(&mut self, _holder: &ScoreHolder, _objective: &Objective, _score: &Score) {}
    fn on_score_lock_changed(&mut self, _holder: &ScoreHolder, _objective: &Objective) {}
    fn on_player_removed(&mut self, _holder: &ScoreHolder) {}
    fn on_player_score_removed(&mut self, _holder: &ScoreHolder, _objective: &Objective) {}
    fn on_objective_added(&mut self, _objective: &Objective) {}
    fn on_objective_changed(&mut self, _objective: &Objective) {}
    fn on_objective_removed(&mut self, _objective: &Objective) {}
    fn on_team_added(&mut self, _team: &PlayerTeam) {}
    fn on_team_changed(&mut self, _team: &PlayerTeam) {}
    fn on_team_removed(&mut self, _team: &PlayerTeam) {}
    fn on_player_added_to_team(&mut self, _player: &str, _team: &PlayerTeam) {}
    fn on_player_removed_from_team(&mut self, _player: &str, _team: &PlayerTeam) {}
    fn on_display_slot_changed(
        &mut self,
        _slot: DisplaySlot,
        _old_objective: Option<&Objective>,
        _new_objective: Option<&Objective>,
    ) {
    }
}

/// No-op listener for standalone `Scoreboard` operations.
#[derive(Default)]
pub struct NoopListener;
impl ScoreboardListener for NoopListener {}

/// Port of `net.minecraft.world.scores.Scoreboard`.
///
/// Uses `IndexMap` / `BTreeMap` everywhere observable iteration occurs so
/// packet order, listing order, and persistence order are deterministic.
#[derive(Clone, Debug, Default)]
pub struct Scoreboard {
    objectives_by_name: IndexMap<String, Objective>,
    objectives_by_criteria: IndexMap<ObjectiveCriteria, Vec<String>>,
    player_scores: IndexMap<String, PlayerScores>,
    display_objectives: BTreeMap<DisplaySlot, String>,
    teams_by_name: IndexMap<String, PlayerTeam>,
    teams_by_player: IndexMap<String, String>,
}

impl Scoreboard {
    pub fn new() -> Self {
        Self::default()
    }

    // -------------------------------------------------------------------------
    // Objectives
    // -------------------------------------------------------------------------

    /// `Scoreboard.getObjective(@Nullable String)`.
    pub fn get_objective(&self, name: &str) -> Option<&Objective> {
        self.objectives_by_name.get(name)
    }

    /// `Scoreboard.addObjective(...)`.
    #[allow(clippy::too_many_arguments)]
    pub fn add_objective(
        &mut self,
        name: impl Into<String>,
        criteria: ObjectiveCriteria,
        display_name: Component,
        render_type: RenderType,
        display_auto_update: bool,
        number_format: Option<NumberFormat>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<&Objective, ScoreboardError> {
        let name = name.into();
        if self.objectives_by_name.contains_key(&name) {
            return Err(ScoreboardError::DuplicateObjective(name));
        }

        let objective = Objective::new(
            name.clone(),
            criteria.clone(),
            display_name,
            render_type,
            display_auto_update,
            number_format,
        );
        self.objectives_by_criteria
            .entry(criteria)
            .or_default()
            .push(name.clone());
        self.objectives_by_name.insert(name.clone(), objective);
        let obj = self
            .objectives_by_name
            .get(&name)
            .expect("just inserted objective");
        listener.on_objective_added(obj);
        Ok(obj)
    }

    /// Update an objective's display name and fire `on_objective_changed`.
    pub fn set_objective_display_name(
        &mut self,
        objective_name: &str,
        display_name: Component,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let obj = self
            .objectives_by_name
            .get_mut(objective_name)
            .ok_or_else(|| ScoreboardError::UnknownObjective(objective_name.to_owned()))?;
        obj.set_display_name_raw(display_name);
        let snapshot = obj.clone();
        listener.on_objective_changed(&snapshot);
        Ok(())
    }

    /// Update an objective's render type and fire `on_objective_changed`.
    pub fn set_objective_render_type(
        &mut self,
        objective_name: &str,
        render_type: RenderType,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let obj = self
            .objectives_by_name
            .get_mut(objective_name)
            .ok_or_else(|| ScoreboardError::UnknownObjective(objective_name.to_owned()))?;
        obj.set_render_type_raw(render_type);
        let snapshot = obj.clone();
        listener.on_objective_changed(&snapshot);
        Ok(())
    }

    /// Update an objective's `display_auto_update` flag and fire `on_objective_changed`.
    pub fn set_objective_display_auto_update(
        &mut self,
        objective_name: &str,
        display_auto_update: bool,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let obj = self
            .objectives_by_name
            .get_mut(objective_name)
            .ok_or_else(|| ScoreboardError::UnknownObjective(objective_name.to_owned()))?;
        obj.set_display_auto_update_raw(display_auto_update);
        let snapshot = obj.clone();
        listener.on_objective_changed(&snapshot);
        Ok(())
    }

    /// Update an objective's number format and fire `on_objective_changed`.
    pub fn set_objective_number_format(
        &mut self,
        objective_name: &str,
        number_format: Option<NumberFormat>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let obj = self
            .objectives_by_name
            .get_mut(objective_name)
            .ok_or_else(|| ScoreboardError::UnknownObjective(objective_name.to_owned()))?;
        obj.set_number_format_raw(number_format);
        let snapshot = obj.clone();
        listener.on_objective_changed(&snapshot);
        Ok(())
    }

    /// `Scoreboard.removeObjective(Objective)`.
    pub fn remove_objective(
        &mut self,
        objective_name: &str,
        listener: &mut dyn ScoreboardListener,
    ) -> Option<Objective> {
        let objective = self.objectives_by_name.shift_remove(objective_name)?;

        let slots_to_clear: Vec<DisplaySlot> = self
            .display_objectives
            .iter()
            .filter_map(|(&slot, name)| (name == objective_name).then_some(slot))
            .collect();
        for slot in slots_to_clear {
            self.display_objectives.remove(&slot);
            listener.on_display_slot_changed(slot, Some(&objective), None);
        }

        if let Some(list) = self.objectives_by_criteria.get_mut(objective.criteria()) {
            list.retain(|n| n != objective_name);
            if list.is_empty() {
                self.objectives_by_criteria
                    .shift_remove(objective.criteria());
            }
        }

        for scores in self.player_scores.values_mut() {
            scores.remove(objective_name);
        }
        self.player_scores.retain(|_, s| s.has_scores());

        listener.on_objective_removed(&objective);
        Some(objective)
    }

    /// `Scoreboard.getObjectives()`.
    pub fn objectives(&self) -> impl Iterator<Item = &Objective> {
        self.objectives_by_name.values()
    }

    /// `Scoreboard.getObjectiveNames()`.
    pub fn objective_names(&self) -> impl Iterator<Item = &str> {
        self.objectives_by_name.keys().map(String::as_str)
    }

    /// Objectives registered under a given `ObjectiveCriteria`.
    pub fn objectives_by_criteria(&self, criteria: &ObjectiveCriteria) -> Vec<&Objective> {
        self.objectives_by_criteria
            .get(criteria)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| self.objectives_by_name.get(n))
                    .collect()
            })
            .unwrap_or_default()
    }

    // -------------------------------------------------------------------------
    // Display Slots
    // -------------------------------------------------------------------------

    /// `Scoreboard.setDisplayObjective(DisplaySlot, @Nullable Objective)`.
    pub fn set_display_objective(
        &mut self,
        slot: DisplaySlot,
        objective_name: Option<&str>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let new_obj = match objective_name {
            Some(name) => Some(
                self.objectives_by_name
                    .get(name)
                    .ok_or_else(|| ScoreboardError::UnknownObjective(name.to_owned()))?
                    .clone(),
            ),
            None => None,
        };
        let old_obj = self
            .display_objectives
            .get(&slot)
            .and_then(|name| self.objectives_by_name.get(name))
            .cloned();

        match &new_obj {
            Some(obj) => {
                self.display_objectives.insert(slot, obj.name().to_owned());
            }
            None => {
                self.display_objectives.remove(&slot);
            }
        }

        listener.on_display_slot_changed(slot, old_obj.as_ref(), new_obj.as_ref());
        Ok(())
    }

    /// `Scoreboard.getDisplayObjective(DisplaySlot)`.
    pub fn get_display_objective(&self, slot: DisplaySlot) -> Option<&Objective> {
        self.display_objectives
            .get(&slot)
            .and_then(|name| self.objectives_by_name.get(name))
    }

    /// Current display-slot assignments in deterministic `DisplaySlot` order.
    pub fn display_slots(&self) -> &BTreeMap<DisplaySlot, String> {
        &self.display_objectives
    }

    /// Count how many display slots currently show `objective_name`.
    pub fn display_slot_count(&self, objective_name: &str) -> usize {
        self.display_objectives
            .values()
            .filter(|name| name.as_str() == objective_name)
            .count()
    }

    // -------------------------------------------------------------------------
    // Scores
    // -------------------------------------------------------------------------

    /// `Scoreboard.getPlayerScoreInfo(ScoreHolder, Objective)`.
    pub fn get_player_score_info(
        &self,
        holder: &ScoreHolder,
        objective_name: &str,
    ) -> Option<ReadOnlyScoreInfo> {
        self.player_scores
            .get(holder.scoreboard_name())
            .and_then(|ps| ps.get(objective_name))
            .map(Score::as_read_only)
    }

    /// Raw score reference for inspection.
    pub fn get_player_score(&self, holder_name: &str, objective_name: &str) -> Option<&Score> {
        self.player_scores
            .get(holder_name)
            .and_then(|ps| ps.get(objective_name))
    }

    /// `Scoreboard.getOrCreatePlayerScore(ScoreHolder, Objective, boolean forceWritable)`.
    pub fn access_player_score<'a>(
        &'a mut self,
        holder: &'a ScoreHolder,
        objective_name: &str,
        force_writable: bool,
        listener: &'a mut dyn ScoreboardListener,
    ) -> Result<ScoreAccess<'a>, ScoreboardError> {
        let objective = self
            .objectives_by_name
            .get(objective_name)
            .ok_or_else(|| ScoreboardError::UnknownObjective(objective_name.to_owned()))?
            .clone();
        let can_modify = force_writable || !objective.criteria().is_read_only();
        let mut requires_sync = false;
        let player_scores = self
            .player_scores
            .entry(holder.scoreboard_name().to_owned())
            .or_default();
        player_scores.get_or_create(objective.name(), |_| {
            requires_sync = true;
        });
        Ok(ScoreAccess {
            scoreboard: self,
            holder,
            objective,
            can_modify,
            requires_sync,
            listener,
        })
    }

    /// `Scoreboard.forAllObjectives(ObjectiveCriteria, ScoreHolder, Consumer<ScoreAccess>)`.
    ///
    /// Always runs with `force_writable = true`, matching Paper 26.2.
    pub fn for_all_objectives<F>(
        &mut self,
        criteria: &ObjectiveCriteria,
        holder: &ScoreHolder,
        listener: &mut dyn ScoreboardListener,
        mut op: F,
    ) -> Result<(), ScoreboardError>
    where
        F: FnMut(&mut ScoreAccess<'_>) -> Result<(), ScoreboardError>,
    {
        let names = self
            .objectives_by_criteria
            .get(criteria)
            .cloned()
            .unwrap_or_default();
        for name in names {
            let mut access = self.access_player_score(holder, &name, true, listener)?;
            op(&mut access)?;
        }
        Ok(())
    }

    /// `Scoreboard.listPlayerScores(Objective)`.
    pub fn list_player_scores(&self, objective_name: &str) -> Vec<PlayerScoreEntry> {
        let mut result = Vec::new();
        for (owner, scores) in &self.player_scores {
            if let Some(score) = scores.get(objective_name) {
                result.push(PlayerScoreEntry {
                    owner: owner.clone(),
                    value: score.value(),
                    display: score.display().cloned(),
                    number_format_override: score.number_format().cloned(),
                });
            }
        }
        result
    }

    /// `Scoreboard.listPlayerScores(ScoreHolder)` — returns objective name -> score value.
    pub fn list_scores_for_holder(&self, holder: &ScoreHolder) -> IndexMap<String, i32> {
        self.player_scores
            .get(holder.scoreboard_name())
            .map(PlayerScores::list_scores)
            .unwrap_or_default()
    }

    /// `Scoreboard.getTrackedPlayers()`.
    pub fn tracked_players(&self) -> Vec<ScoreHolder> {
        self.player_scores
            .keys()
            .map(|name| ScoreHolder::for_name_only(name.clone()))
            .collect()
    }

    /// `Scoreboard.resetAllPlayerScores(ScoreHolder)`.
    pub fn reset_all_player_scores(
        &mut self,
        holder: &ScoreHolder,
        listener: &mut dyn ScoreboardListener,
    ) -> bool {
        if self
            .player_scores
            .shift_remove(holder.scoreboard_name())
            .is_some()
        {
            listener.on_player_removed(holder);
            true
        } else {
            false
        }
    }

    /// `Scoreboard.resetSinglePlayerScore(ScoreHolder, Objective)`.
    pub fn reset_single_player_score(
        &mut self,
        holder: &ScoreHolder,
        objective_name: &str,
        listener: &mut dyn ScoreboardListener,
    ) -> bool {
        let Some(objective) = self.objectives_by_name.get(objective_name).cloned() else {
            return false;
        };
        let mut removed = false;
        let mut empty_after = false;
        if let Some(scores) = self.player_scores.get_mut(holder.scoreboard_name()) {
            removed = scores.remove(objective_name);
            if removed {
                empty_after = !scores.has_scores();
            }
        }
        if empty_after {
            self.player_scores.shift_remove(holder.scoreboard_name());
        }
        if removed {
            listener.on_player_score_removed(holder, &objective);
        }
        removed
    }

    // -------------------------------------------------------------------------
    // Teams
    // -------------------------------------------------------------------------

    /// `Scoreboard.getPlayerTeam(String)`.
    pub fn get_player_team(&self, name: &str) -> Option<&PlayerTeam> {
        self.teams_by_name.get(name)
    }

    /// `Scoreboard.getPlayersTeam(String)`.
    pub fn get_players_team(&self, player_name: &str) -> Option<&PlayerTeam> {
        self.teams_by_player
            .get(player_name)
            .and_then(|team_name| self.teams_by_name.get(team_name))
    }

    /// `Scoreboard.addPlayerTeam(String)`.
    ///
    /// Matches Paper: if a team with `name` already exists, returns the existing team
    /// without firing `on_team_added` (wait: Paper's `Scoreboard.addPlayerTeam` logs a
    /// warning and returns a new unregistered team? Let's check: actually in `Scoreboard.java`
    /// line 225, if `team != null` it logs a warning and returns `team`, otherwise creates & adds it!).
    pub fn add_player_team(
        &mut self,
        name: impl Into<String>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<&PlayerTeam, ScoreboardError> {
        let name = name.into();
        if self.teams_by_name.contains_key(&name) {
            return Err(ScoreboardError::DuplicateTeam(name));
        }
        let team = PlayerTeam::new(name.clone());
        self.teams_by_name.insert(name.clone(), team);
        let team_ref = self.teams_by_name.get(&name).expect("just inserted team");
        listener.on_team_added(team_ref);
        Ok(team_ref)
    }

    /// `Scoreboard.removePlayerTeam(PlayerTeam)`.
    pub fn remove_player_team(
        &mut self,
        team_name: &str,
        listener: &mut dyn ScoreboardListener,
    ) -> Option<PlayerTeam> {
        let team = self.teams_by_name.shift_remove(team_name)?;
        for player in team.players() {
            self.teams_by_player.shift_remove(player);
        }
        listener.on_team_removed(&team);
        Some(team)
    }

    /// `Scoreboard.addPlayerToTeam(String, PlayerTeam)`.
    ///
    /// If `player` is already on another team, removes them from that team first
    /// (firing `on_player_removed_from_team`), then adds them to `team_name`
    /// (firing `on_player_added_to_team`).
    pub fn add_player_to_team(
        &mut self,
        player: &str,
        team_name: &str,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<bool, ScoreboardError> {
        if !self.teams_by_name.contains_key(team_name) {
            return Err(ScoreboardError::UnknownTeam(team_name.to_owned()));
        }
        if let Some(existing_team_name) = self.teams_by_player.get(player).cloned() {
            self.remove_player_from_team(player, &existing_team_name, listener)?;
        }
        self.teams_by_player
            .insert(player.to_owned(), team_name.to_owned());
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .expect("verified team exists");
        let added = team.add_player_raw(player.to_owned());
        let team_snapshot = team.clone();
        if added {
            listener.on_player_added_to_team(player, &team_snapshot);
        }
        Ok(added)
    }

    /// `Scoreboard.removePlayerFromTeam(String)` — removes player from whichever team they belong to.
    pub fn remove_player_from_any_team(
        &mut self,
        player: &str,
        listener: &mut dyn ScoreboardListener,
    ) -> bool {
        let Some(team_name) = self.teams_by_player.get(player).cloned() else {
            return false;
        };
        self.remove_player_from_team(player, &team_name, listener)
            .is_ok()
    }

    /// `Scoreboard.removePlayerFromTeam(String, PlayerTeam)`.
    ///
    /// Fails with `ScoreboardError::PlayerNotOnTeam` if `player` is not on `team_name`,
    /// matching Paper's `IllegalStateException`.
    pub fn remove_player_from_team(
        &mut self,
        player: &str,
        team_name: &str,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let current = self.teams_by_player.get(player).map(String::as_str);
        if current != Some(team_name) {
            return Err(ScoreboardError::PlayerNotOnTeam(team_name.to_owned()));
        }
        self.teams_by_player.shift_remove(player);
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.remove_player_raw(player);
        let team_snapshot = team.clone();
        listener.on_player_removed_from_team(player, &team_snapshot);
        Ok(())
    }

    /// `Scoreboard.getTeamNames()`.
    pub fn team_names(&self) -> impl Iterator<Item = &str> {
        self.teams_by_name.keys().map(String::as_str)
    }

    /// `Scoreboard.getPlayerTeams()`.
    pub fn player_teams(&self) -> impl Iterator<Item = &PlayerTeam> {
        self.teams_by_name.values()
    }

    /// Mutate team display name and fire `on_team_changed`.
    pub fn set_team_display_name(
        &mut self,
        team_name: &str,
        display_name: Component,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_display_name_raw(display_name);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team prefix and fire `on_team_changed`.
    pub fn set_team_prefix(
        &mut self,
        team_name: &str,
        prefix: Option<Component>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_player_prefix_raw(prefix);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team suffix and fire `on_team_changed`.
    pub fn set_team_suffix(
        &mut self,
        team_name: &str,
        suffix: Option<Component>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_player_suffix_raw(suffix);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team friendly-fire flag and fire `on_team_changed`.
    pub fn set_team_allow_friendly_fire(
        &mut self,
        team_name: &str,
        allow: bool,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_allow_friendly_fire_raw(allow);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team see-friendly-invisibles flag and fire `on_team_changed`.
    pub fn set_team_see_friendly_invisibles(
        &mut self,
        team_name: &str,
        see: bool,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_see_friendly_invisibles_raw(see);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team name-tag visibility and fire `on_team_changed`.
    pub fn set_team_name_tag_visibility(
        &mut self,
        team_name: &str,
        visibility: Visibility,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_name_tag_visibility_raw(visibility);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team death-message visibility and fire `on_team_changed`.
    pub fn set_team_death_message_visibility(
        &mut self,
        team_name: &str,
        visibility: Visibility,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_death_message_visibility_raw(visibility);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team collision rule and fire `on_team_changed`.
    pub fn set_team_collision_rule(
        &mut self,
        team_name: &str,
        rule: CollisionRule,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_collision_rule_raw(rule);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    /// Mutate team color and fire `on_team_changed`.
    pub fn set_team_color(
        &mut self,
        team_name: &str,
        color: Option<TeamColor>,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        let team = self
            .teams_by_name
            .get_mut(team_name)
            .ok_or_else(|| ScoreboardError::UnknownTeam(team_name.to_owned()))?;
        team.set_color_raw(color);
        let snapshot = team.clone();
        listener.on_team_changed(&snapshot);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Persistence helpers (Scoreboard.pack* / Scoreboard.load*)
    // -------------------------------------------------------------------------

    /// `Scoreboard.packObjectives()`.
    pub fn pack_objectives(&self) -> Vec<PackedObjective> {
        self.objectives_by_name
            .values()
            .map(Objective::pack)
            .collect()
    }

    /// `Scoreboard.packPlayerScores()`.
    pub fn pack_player_scores(&self) -> Vec<PackedScore> {
        let mut out = Vec::new();
        for (owner, scores) in &self.player_scores {
            for (objective, score) in scores.raw_scores() {
                out.push(PackedScore {
                    owner: owner.clone(),
                    objective: objective.clone(),
                    score: score.pack(),
                });
            }
        }
        out
    }

    /// `Scoreboard.packDisplaySlots()`.
    pub fn pack_display_slots(&self) -> BTreeMap<DisplaySlot, String> {
        self.display_objectives.clone()
    }

    /// `Scoreboard.packPlayerTeams()`.
    pub fn pack_player_teams(&self) -> Vec<PackedPlayerTeam> {
        self.teams_by_name.values().map(PlayerTeam::pack).collect()
    }

    /// `Scoreboard.loadObjective(Objective.Packed)`.
    pub fn load_objective(
        &mut self,
        packed: &PackedObjective,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        self.add_objective(
            packed.name.clone(),
            packed.criteria.clone(),
            packed.display_name.clone(),
            packed.render_type,
            packed.display_auto_update,
            packed.number_format.clone(),
            listener,
        )?;
        Ok(())
    }

    /// `Scoreboard.loadPlayerScore(Scoreboard.PackedScore)`.
    ///
    /// Silently skips scores referencing unknown objectives, matching Paper 26.2.
    pub fn load_player_score(&mut self, packed: &PackedScore) {
        if !self.objectives_by_name.contains_key(&packed.objective) {
            return;
        }
        let player_scores = self.player_scores.entry(packed.owner.clone()).or_default();
        player_scores.set_score(packed.objective.clone(), Score::from_packed(&packed.score));
    }

    /// `Scoreboard.loadPlayerTeam(PlayerTeam.Packed)`.
    pub fn load_player_team(
        &mut self,
        packed: &PackedPlayerTeam,
        listener: &mut dyn ScoreboardListener,
    ) -> Result<(), ScoreboardError> {
        self.add_player_team(packed.name.clone(), listener)?;
        if let Some(display_name) = &packed.display_name {
            self.set_team_display_name(&packed.name, display_name.clone(), listener)?;
        }
        self.set_team_color(&packed.name, packed.color, listener)?;
        self.set_team_allow_friendly_fire(&packed.name, packed.allow_friendly_fire, listener)?;
        self.set_team_see_friendly_invisibles(
            &packed.name,
            packed.see_friendly_invisibles,
            listener,
        )?;
        self.set_team_prefix(
            &packed.name,
            Some(packed.member_name_prefix.clone()),
            listener,
        )?;
        self.set_team_suffix(
            &packed.name,
            Some(packed.member_name_suffix.clone()),
            listener,
        )?;
        self.set_team_name_tag_visibility(&packed.name, packed.name_tag_visibility, listener)?;
        self.set_team_death_message_visibility(
            &packed.name,
            packed.death_message_visibility,
            listener,
        )?;
        self.set_team_collision_rule(&packed.name, packed.collision_rule, listener)?;
        for player in &packed.players {
            self.add_player_to_team(player, &packed.name, listener)?;
        }
        Ok(())
    }
}

/// Port of `net.minecraft.world.scores.ScoreAccess`.
///
/// Implements Paper's exact `requiresSync`, `canModify`, `displayAutoUpdate`,
/// and lock/unlock notification semantics.
pub struct ScoreAccess<'a> {
    scoreboard: &'a mut Scoreboard,
    holder: &'a ScoreHolder,
    objective: Objective,
    can_modify: bool,
    requires_sync: bool,
    listener: &'a mut dyn ScoreboardListener,
}

impl<'a> ScoreAccess<'a> {
    fn score(&self) -> &Score {
        self.scoreboard
            .player_scores
            .get(self.holder.scoreboard_name())
            .and_then(|ps| ps.get(self.objective.name()))
            .expect("ScoreAccess score exists")
    }

    fn score_mut(&mut self) -> &mut Score {
        self.scoreboard
            .player_scores
            .get_mut(self.holder.scoreboard_name())
            .and_then(|ps| ps.get_mut(self.objective.name()))
            .expect("ScoreAccess score exists")
    }

    fn send_score_to_players(&mut self) {
        let snapshot = self.score().clone();
        self.listener
            .on_score_changed(self.holder, &self.objective, &snapshot);
        self.requires_sync = false;
    }

    /// `ScoreAccess.get()`.
    pub fn get(&self) -> i32 {
        self.score().value()
    }

    /// `ScoreAccess.set(int)`.
    pub fn set(&mut self, value: i32) -> Result<(), ScoreboardError> {
        if !self.can_modify {
            return Err(ScoreboardError::ReadOnlyScore);
        }

        let mut has_changed = self.requires_sync;
        if self.objective.display_auto_update()
            && let Some(new_display) = self.holder.display_name()
            && self.score().display() != Some(new_display)
        {
            let cloned = new_display.clone();
            self.score_mut().set_display(Some(cloned));
            has_changed = true;
        }

        if value != self.score().value() {
            self.score_mut().set_value(value);
            has_changed = true;
        }

        if has_changed {
            self.send_score_to_players();
        }
        Ok(())
    }

    /// `ScoreAccess.add(int)` — wrapping 32-bit Java `int` addition.
    pub fn add(&mut self, amount: i32) -> Result<i32, ScoreboardError> {
        let next = self.get().wrapping_add(amount);
        self.set(next)?;
        Ok(next)
    }

    /// `ScoreAccess.increment()`.
    pub fn increment(&mut self) -> Result<i32, ScoreboardError> {
        self.add(1)
    }

    /// `ScoreAccess.reset()`.
    pub fn reset(&mut self) -> Result<(), ScoreboardError> {
        self.set(0)
    }

    /// `ScoreAccess.display()`.
    pub fn display(&self) -> Option<&Component> {
        self.score().display()
    }

    /// `ScoreAccess.display(@Nullable Component)`.
    pub fn set_display(&mut self, display: Option<Component>) {
        if self.requires_sync || self.score().display() != display.as_ref() {
            self.score_mut().set_display(display);
            self.send_score_to_players();
        }
    }

    /// `ScoreAccess.numberFormatOverride(@Nullable NumberFormat)`.
    pub fn set_number_format_override(&mut self, number_format: Option<NumberFormat>) {
        self.score_mut().set_number_format(number_format);
        self.send_score_to_players();
    }

    /// `ScoreAccess.locked()`.
    pub fn is_locked(&self) -> bool {
        self.score().is_locked()
    }

    /// `ScoreAccess.unlock()`.
    pub fn unlock(&mut self) {
        self.set_locked(false);
    }

    /// `ScoreAccess.lock()`.
    pub fn lock(&mut self) {
        self.set_locked(true);
    }

    fn set_locked(&mut self, locked: bool) {
        self.score_mut().set_locked(locked);
        if self.requires_sync {
            self.send_score_to_players();
        }
        self.listener
            .on_score_lock_changed(self.holder, &self.objective);
    }
}
