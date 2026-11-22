//! Port of `net.minecraft.world.scores.{Score, ScoreHolder, ReadOnlyScoreInfo, PlayerScoreEntry, PlayerScores}` (Paper 26.2).

use crate::scores::{NumberFormat, show_text_hover};
use indexmap::IndexMap;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec::MapCodec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_text::{Component, Style, component_serialization};
use std::sync::Arc;

/// Port of `net.minecraft.world.scores.ScoreHolder`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScoreHolder {
    scoreboard_name: String,
    display_name: Option<Component>,
}

impl ScoreHolder {
    /// `ScoreHolder.WILDCARD_NAME`.
    pub const WILDCARD_NAME: &'static str = "*";

    /// `ScoreHolder.WILDCARD`.
    pub fn wildcard() -> Self {
        Self::for_name_only(Self::WILDCARD_NAME)
    }

    /// `ScoreHolder.forNameOnly(String)`.
    pub fn for_name_only(name: impl Into<String>) -> Self {
        Self {
            scoreboard_name: name.into(),
            display_name: None,
        }
    }

    /// Construct a `ScoreHolder` with a custom display name.
    pub fn with_display_name(name: impl Into<String>, display_name: Component) -> Self {
        Self {
            scoreboard_name: name.into(),
            display_name: Some(display_name),
        }
    }

    /// `ScoreHolder.getScoreboardName()`.
    #[inline]
    pub fn scoreboard_name(&self) -> &str {
        &self.scoreboard_name
    }

    /// `ScoreHolder.getDisplayName()`.
    #[inline]
    pub fn display_name(&self) -> Option<&Component> {
        self.display_name.as_ref()
    }

    /// `ScoreHolder.getFeedbackDisplayName()`.
    pub fn feedback_display_name(&self) -> Component {
        match &self.display_name {
            Some(display) => {
                let hover = show_text_hover(Component::literal(&self.scoreboard_name));
                display
                    .copy()
                    .with_style(Style::EMPTY.with_hover_event(Some(hover)))
            }
            None => Component::literal(&self.scoreboard_name),
        }
    }
}

/// Port of `net.minecraft.world.scores.ReadOnlyScoreInfo`.
#[derive(Clone, Debug, PartialEq)]
pub struct ReadOnlyScoreInfo {
    pub value: i32,
    pub locked: bool,
    pub number_format: Option<NumberFormat>,
}

impl ReadOnlyScoreInfo {
    /// `ReadOnlyScoreInfo.formatValue(NumberFormat)`.
    pub fn format_value(&self, default_format: &NumberFormat) -> Component {
        self.number_format
            .as_ref()
            .unwrap_or(default_format)
            .format(self.value)
    }

    /// `ReadOnlyScoreInfo.safeFormatValue(@Nullable ReadOnlyScoreInfo, NumberFormat)`.
    pub fn safe_format_value(
        score: Option<&ReadOnlyScoreInfo>,
        default_format: &NumberFormat,
    ) -> Component {
        match score {
            Some(info) => info.format_value(default_format),
            None => default_format.format(0),
        }
    }
}

/// Port of `net.minecraft.world.scores.Score`.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    value: i32,
    locked: bool,
    display: Option<Component>,
    number_format: Option<NumberFormat>,
}

impl Default for Score {
    fn default() -> Self {
        Self::new()
    }
}

impl Score {
    /// `new Score()` — defaults to `value = 0`, `locked = true`, `display = null`, `numberFormat = null`.
    pub fn new() -> Self {
        Self {
            value: 0,
            locked: true,
            display: None,
            number_format: None,
        }
    }

    /// `new Score(Score.Packed)`.
    pub fn from_packed(packed: &PackedScoreData) -> Self {
        Self {
            value: packed.value,
            locked: packed.locked,
            display: packed.display.clone(),
            number_format: packed.number_format.clone(),
        }
    }

    /// `Score.pack()`.
    pub fn pack(&self) -> PackedScoreData {
        PackedScoreData {
            value: self.value,
            locked: self.locked,
            display: self.display.clone(),
            number_format: self.number_format.clone(),
        }
    }

    #[inline]
    pub fn value(&self) -> i32 {
        self.value
    }

    #[inline]
    pub fn set_value(&mut self, value: i32) {
        self.value = value;
    }

    #[inline]
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    #[inline]
    pub fn set_locked(&mut self, locked: bool) {
        self.locked = locked;
    }

    #[inline]
    pub fn display(&self) -> Option<&Component> {
        self.display.as_ref()
    }

    #[inline]
    pub fn set_display(&mut self, display: Option<Component>) {
        self.display = display;
    }

    #[inline]
    pub fn number_format(&self) -> Option<&NumberFormat> {
        self.number_format.as_ref()
    }

    #[inline]
    pub fn set_number_format(&mut self, number_format: Option<NumberFormat>) {
        self.number_format = number_format;
    }

    /// Snapshot as `ReadOnlyScoreInfo`.
    pub fn as_read_only(&self) -> ReadOnlyScoreInfo {
        ReadOnlyScoreInfo {
            value: self.value,
            locked: self.locked,
            number_format: self.number_format.clone(),
        }
    }
}

/// Port of `net.minecraft.world.scores.Score.Packed`.
#[derive(Clone, Debug, PartialEq)]
pub struct PackedScoreData {
    pub value: i32,
    pub locked: bool,
    pub display: Option<Component>,
    pub number_format: Option<NumberFormat>,
}

impl PackedScoreData {
    /// `Score.Packed.MAP_CODEC`.
    ///
    /// Exact Paper field names and defaults:
    /// - `"Score"` (default `0`)
    /// - `"Locked"` (default `false`)
    /// - `"display"` (optional `Component`)
    /// - `"format"` (optional `NumberFormat`)
    pub fn map_codec<Ops: DynamicOps + 'static>() -> Arc<dyn MapCodec<PackedScoreData, Ops>> {
        let comp_codec = component_serialization::codec::<Ops>();
        let nf_codec = NumberFormat::codec::<Ops>();

        record_builder::map_codec(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreData| p.value),
                    codec::optional_field_of("Score", codec::int_codec::<Ops>(), 0),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreData| p.locked),
                    codec::optional_field_of("Locked", codec::bool_codec::<Ops>(), false),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreData| p.display.clone()),
                    codec::optional_field("display".to_owned(), comp_codec, false),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedScoreData| p.number_format.clone()),
                    codec::optional_field("format".to_owned(), nf_codec, false),
                ))
                .apply(
                    instance,
                    Arc::new(|value, locked, display, number_format| PackedScoreData {
                        value,
                        locked,
                        display,
                        number_format,
                    }),
                )
        })
    }
}

/// Port of `net.minecraft.world.scores.PlayerScoreEntry`.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerScoreEntry {
    pub owner: String,
    pub value: i32,
    pub display: Option<Component>,
    pub number_format_override: Option<NumberFormat>,
}

impl PlayerScoreEntry {
    /// `PlayerScoreEntry.isHidden()` (`owner.startsWith("#")`).
    #[inline]
    pub fn is_hidden(&self) -> bool {
        self.owner.starts_with('#')
    }

    /// `PlayerScoreEntry.ownerName()`.
    pub fn owner_name(&self) -> Component {
        match &self.display {
            Some(display) => display.clone(),
            None => Component::literal(&self.owner),
        }
    }

    /// `PlayerScoreEntry.formatValue(NumberFormat)`.
    pub fn format_value(&self, default_format: &NumberFormat) -> Component {
        self.number_format_override
            .as_ref()
            .unwrap_or(default_format)
            .format(self.value)
    }
}

/// Port of `net.minecraft.world.scores.PlayerScores`.
///
/// Uses `IndexMap<String, Score>` (keyed by objective name) so observable
/// iteration order is deterministic across runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerScores {
    scores: IndexMap<String, Score>,
}

impl PlayerScores {
    pub fn new() -> Self {
        Self {
            scores: IndexMap::new(),
        }
    }

    pub fn get(&self, objective_name: &str) -> Option<&Score> {
        self.scores.get(objective_name)
    }

    pub fn get_mut(&mut self, objective_name: &str) -> Option<&mut Score> {
        self.scores.get_mut(objective_name)
    }

    /// `PlayerScores.getOrCreate(Objective, Consumer<Score>)`.
    pub fn get_or_create<F>(&mut self, objective_name: &str, on_new: F) -> &mut Score
    where
        F: FnOnce(&mut Score),
    {
        if !self.scores.contains_key(objective_name) {
            let mut score = Score::new();
            on_new(&mut score);
            self.scores.insert(objective_name.to_owned(), score);
        }
        self.scores
            .get_mut(objective_name)
            .expect("just inserted score")
    }

    /// `PlayerScores.remove(Objective)`.
    pub fn remove(&mut self, objective_name: &str) -> bool {
        self.scores.shift_remove(objective_name).is_some()
    }

    /// `PlayerScores.hasScores()`.
    pub fn has_scores(&self) -> bool {
        !self.scores.is_empty()
    }

    /// `PlayerScores.setScore(Objective, Score)`.
    pub fn set_score(&mut self, objective_name: impl Into<String>, score: Score) {
        self.scores.insert(objective_name.into(), score);
    }

    /// `PlayerScores.listRawScores()`.
    pub fn raw_scores(&self) -> &IndexMap<String, Score> {
        &self.scores
    }

    /// `PlayerScores.listScores()` — objective name -> score value.
    pub fn list_scores(&self) -> IndexMap<String, i32> {
        self.scores
            .iter()
            .map(|(k, v)| (k.clone(), v.value()))
            .collect()
    }
}

/// Keep `Codec` import used cleanly.
#[allow(dead_code)]
fn _codec_marker<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<i32, Ops>> {
    codec::int_codec::<Ops>()
}
