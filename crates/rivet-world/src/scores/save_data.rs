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
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock};

struct OrderedDisplaySlotsCodec<Ops: DynamicOps + 'static> {
    key_codec: Arc<dyn Codec<DisplaySlot, Ops>>,
    value_codec: Arc<dyn Codec<String, Ops>>,
    decode_delegate: Arc<dyn Codec<HashMap<DisplaySlot, String>, Ops>>,
}

impl<Ops: DynamicOps + 'static> OrderedDisplaySlotsCodec<Ops> {
    fn new() -> Arc<dyn Codec<BTreeMap<DisplaySlot, String>, Ops>> {
        let key_codec = DisplaySlot::codec::<Ops>();
        let value_codec = codec::string_codec::<Ops>();
        let decode_delegate = codec::unbounded_map(key_codec.clone(), value_codec.clone());
        Arc::new(Self {
            key_codec,
            value_codec,
            decode_delegate,
        })
    }
}

impl<Ops: DynamicOps + 'static> std::fmt::Debug for OrderedDisplaySlotsCodec<Ops> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OrderedDisplaySlotsCodec")
    }
}

impl<Ops: DynamicOps + 'static> rivet_serialization::Decoder<BTreeMap<DisplaySlot, String>, Ops>
    for OrderedDisplaySlotsCodec<Ops>
{
    fn decode(
        &self,
        ops: &Ops,
        input: &Ops::Output,
    ) -> DataResult<(BTreeMap<DisplaySlot, String>, Ops::Output)> {
        self.decode_delegate
            .decode(ops, input)
            .map_owned(|(map, rest)| (map.into_iter().collect(), rest))
    }
}

impl<Ops: DynamicOps + 'static> rivet_serialization::Encoder<BTreeMap<DisplaySlot, String>, Ops>
    for OrderedDisplaySlotsCodec<Ops>
{
    fn encode(
        &self,
        input: &BTreeMap<DisplaySlot, String>,
        ops: &Ops,
        prefix: &Ops::Output,
    ) -> DataResult<Ops::Output> {
        let mut builder = ops.map_builder();
        for (slot, objective_name) in input {
            builder.add_result_result(
                self.key_codec.encode_start(ops, slot),
                self.value_codec.encode_start(ops, objective_name),
            );
        }
        builder.build(Some(prefix.clone()))
    }
}

impl<Ops: DynamicOps + 'static> Codec<BTreeMap<DisplaySlot, String>, Ops>
    for OrderedDisplaySlotsCodec<Ops>
{
}

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
        let display_slots_map_codec = OrderedDisplaySlotsCodec::<Ops>::new();
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

    fn nbt_codec() -> &'static Arc<dyn Codec<PackedScoreboardData, NbtOps>> {
        static CODEC: OnceLock<Arc<dyn Codec<PackedScoreboardData, NbtOps>>> = OnceLock::new();
        CODEC.get_or_init(Self::codec::<NbtOps>)
    }

    /// Encode to an NBT `Tag` using `NbtOps`.
    pub fn encode_nbt(&self) -> Result<Tag, String> {
        data_result_to_result(Self::nbt_codec().encode_start(&NbtOps, self))
    }

    /// Decode from an NBT `Tag` using `NbtOps`.
    pub fn decode_nbt(tag: &Tag) -> Result<Self, String> {
        data_result_to_result(Self::nbt_codec().parse(&NbtOps, tag))
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
