//! Port of `net.minecraft.world.scores.ScoreboardSaveData` (Paper 26.2).
//!
//! Current-version persistence only (historical DataVersion migration belongs to #378).

use crate::level::saveddata::SavedData;
use crate::scores::{
    DisplaySlot, PackedObjective, PackedPlayerTeam, PackedScore, ScoreboardError, ServerScoreboard,
    data_result_to_result,
};
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

/// Vanilla saved-data file identifier (`minecraft:scoreboard` / `data/scoreboard.dat`).
pub const SCOREBOARD_SAVED_DATA_ID: &str = "scoreboard";

/// Port of `net.minecraft.world.scores.ScoreboardSaveData.Packed`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PackedScoreboardData {
    pub objectives: Vec<PackedObjective>,
    pub scores: Vec<PackedScore>,
    pub display_slots: BTreeMap<DisplaySlot, String>,
    pub teams: Vec<PackedPlayerTeam>,
}

impl PackedScoreboardData {
    pub fn empty() -> Self {
        Self::default()
    }

    /// `ScoreboardSaveData.Packed.CODEC`.
    ///
    /// Exact Paper 26.2 field names and defaults:
    /// - `"Objectives"` (default `[]`)
    /// - `"PlayerScores"` (default `[]`)
    /// - `"DisplaySlots"` (default `{}`)
    /// - `"Teams"` (default `[]`)
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<PackedScoreboardData, Ops>> {
        let display_slots_map_codec = codec::xmap(
            codec::unbounded_map(DisplaySlot::codec::<Ops>(), codec::string_codec::<Ops>()),
            Arc::new(|m: &HashMap<DisplaySlot, String>| {
                m.iter()
                    .map(|(k, v)| (*k, v.clone()))
                    .collect::<BTreeMap<_, _>>()
            }),
            Arc::new(|m: &BTreeMap<DisplaySlot, String>| {
                m.iter()
                    .map(|(k, v)| (*k, v.clone()))
                    .collect::<HashMap<_, _>>()
            }),
        );
        let display_slots_field = map_codec::xmap(
            codec::optional_field("DisplaySlots".to_owned(), display_slots_map_codec, false),
            Arc::new(|opt: &Option<BTreeMap<DisplaySlot, String>>| opt.clone().unwrap_or_default()),
            Arc::new(
                |m: &BTreeMap<DisplaySlot, String>| {
                    if m.is_empty() { None } else { Some(m.clone()) }
                },
            ),
        );

        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreboardData| p.objectives.clone()),
                    codec::optional_field_of(
                        "Objectives",
                        codec::list(PackedObjective::codec::<Ops>()),
                        Vec::new(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreboardData| p.scores.clone()),
                    codec::optional_field_of(
                        "PlayerScores",
                        codec::list(PackedScore::codec::<Ops>()),
                        Vec::new(),
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreboardData| p.display_slots.clone()),
                    display_slots_field,
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreboardData| p.teams.clone()),
                    codec::optional_field_of(
                        "Teams",
                        codec::list(PackedPlayerTeam::codec::<Ops>()),
                        Vec::new(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |objectives, scores, display_slots, teams| PackedScoreboardData {
                            objectives,
                            scores,
                            display_slots,
                            teams,
                        },
                    ),
                )
        })
    }

    /// Encode to an NBT `Tag` using `NbtOps`.
    pub fn encode_nbt(&self) -> Result<Tag, String> {
        let codec = Self::codec::<NbtOps>();
        data_result_to_result(codec.encode_start(&NbtOps, self))
    }

    /// Decode from an NBT `Tag` using `NbtOps`.
    pub fn decode_nbt(tag: &Tag) -> Result<Self, String> {
        let codec = Self::codec::<NbtOps>();
        data_result_to_result(codec.parse(&NbtOps, tag))
    }
}

/// Port of `net.minecraft.world.scores.ScoreboardSaveData`.
///
/// Wraps `SavedData` dirty tracking and a `PackedScoreboardData` payload.
#[derive(Clone, Debug, Default)]
pub struct ScoreboardSaveData {
    base: SavedData,
    data: PackedScoreboardData,
}

impl ScoreboardSaveData {
    pub fn new(data: PackedScoreboardData) -> Self {
        Self {
            base: SavedData::default(),
            data,
        }
    }

    pub fn from_scoreboard(scoreboard: &ServerScoreboard) -> Self {
        let data = PackedScoreboardData {
            objectives: scoreboard.pack_objectives(),
            scores: scoreboard.pack_player_scores(),
            display_slots: scoreboard.pack_display_slots(),
            teams: scoreboard.pack_player_teams(),
        };
        let mut save_data = Self::new(data);
        save_data.base.set_dirty_flag(scoreboard.is_dirty());
        save_data
    }

    /// Capture the current state of `scoreboard` into `self` and mark dirty.
    pub fn set_data(&mut self, data: PackedScoreboardData) {
        if data != self.data {
            self.data = data;
            self.base.set_dirty();
        }
    }

    /// Apply this saved data into `scoreboard` in Paper's load order
    /// (objectives -> scores -> display slots -> teams).
    pub fn apply_to(&self, scoreboard: &mut ServerScoreboard) -> Result<(), ScoreboardError> {
        scoreboard.load_packed(
            &self.data.objectives,
            &self.data.scores,
            &self.data.display_slots,
            &self.data.teams,
        )
    }

    pub fn data(&self) -> &PackedScoreboardData {
        &self.data
    }

    pub fn is_dirty(&self) -> bool {
        self.base.is_dirty()
    }

    pub fn set_dirty(&mut self) {
        self.base.set_dirty();
    }

    pub fn set_dirty_flag(&mut self, dirty: bool) {
        self.base.set_dirty_flag(dirty);
    }

    /// Encode to NBT `Tag` (`data/scoreboard.dat` `data` compound).
    pub fn encode_nbt(&self) -> Result<Tag, String> {
        self.data.encode_nbt()
    }

    /// Decode from NBT `Tag`.
    pub fn decode_nbt(tag: &Tag) -> Result<Self, String> {
        let data = PackedScoreboardData::decode_nbt(tag)?;
        Ok(Self::new(data))
    }
}
