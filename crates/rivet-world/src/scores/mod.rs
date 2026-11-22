//! Authoritative scoreboard, objectives, teams, persistence, and packets
//! (Paper 26.2 `net.minecraft.world.scores.*` and `net.minecraft.server.ServerScoreboard`).

pub mod criteria;
pub mod display_slot;
pub mod number_format;
pub mod objective;
pub mod packets;
pub mod save_data;
pub mod score;
pub mod scoreboard;
pub mod server_scoreboard;
pub mod team;
pub mod team_color;

#[cfg(test)]
mod tests;

pub use criteria::{ObjectiveCriteria, RenderType};
pub use display_slot::DisplaySlot;
pub use number_format::{BLANK_TYPE_ID, FIXED_TYPE_ID, NumberFormat, STYLED_TYPE_ID};
pub use objective::{Objective, PackedObjective};
pub use packets::{
    ClientboundResetScorePacket, ClientboundSetDisplayObjectivePacket,
    ClientboundSetObjectivePacket, ClientboundSetPlayerTeamPacket, ClientboundSetScorePacket,
    ScoreboardPacket, TeamPacketAction, TeamPacketParameters,
};
pub use save_data::{PackedScoreboardData, SCOREBOARD_SAVED_DATA_ID, ScoreboardSaveData};
pub use score::{
    PackedScoreData, PlayerScoreEntry, PlayerScores, ReadOnlyScoreInfo, Score, ScoreHolder,
};
pub use scoreboard::{
    NoopListener, PackedScore, ScoreAccess, Scoreboard, ScoreboardError, ScoreboardListener,
};
pub use server_scoreboard::{ScoreboardEffect, ScoreboardProvider, ServerScoreboard};
pub use team::{
    BIT_FRIENDLY_FIRE, BIT_SEE_INVISIBLES, CollisionRule, PackedPlayerTeam, PlayerTeam, Visibility,
};
pub use team_color::TeamColor;

use rivet_nbt::compound_tag::CompoundTag;
use rivet_nbt::nbt_ops::NbtOps;
use rivet_nbt::tag::Tag;
use rivet_serialization::codec::Codec;
use rivet_serialization::data_result::DataResult;
use rivet_text::contents::translatable::{TranslatableArg, TranslatableContents};
use rivet_text::{Component, ComponentContents, HoverEvent, component_serialization};
use std::sync::{Arc, OnceLock};

pub(crate) fn data_result_to_result<T>(dr: DataResult<T>) -> Result<T, String> {
    if let Some(err) = dr.error_ref() {
        Err(err.message().to_owned())
    } else {
        dr.result_or_partial_silent()
            .ok_or_else(|| "empty DataResult".to_owned())
    }
}

/// Port of `ComponentUtils.wrapInSquareBrackets(Component)`.
pub(crate) fn wrap_in_square_brackets(inner: Component) -> Component {
    Component::create(ComponentContents::Translatable(TranslatableContents::new(
        "chat.square_brackets".to_owned(),
        None,
        vec![TranslatableArg::Component(Box::new(inner))],
    )))
}

/// Build `HoverEvent::ShowText` without needing a private field constructor in `rivet-text`.
pub(crate) fn show_text_hover(value: Component) -> HoverEvent {
    type HoverCodecs = (
        Arc<dyn Codec<Component, NbtOps>>,
        Arc<dyn Codec<HoverEvent, NbtOps>>,
    );
    static CODECS: OnceLock<HoverCodecs> = OnceLock::new();
    let (comp_codec, hover_codec) = CODECS.get_or_init(|| {
        let comp_codec = component_serialization::codec::<NbtOps>();
        let hover_codec = HoverEvent::codec::<NbtOps>(comp_codec.clone());
        (comp_codec, hover_codec)
    });
    let encoded_value = comp_codec
        .encode_start(&NbtOps, &value)
        .result_or_partial_silent()
        .expect("Component literal encodes to NBT");
    let mut map = CompoundTag::new();
    map.put_string("action", "show_text");
    map.put("value".to_owned(), encoded_value);
    hover_codec
        .parse(&NbtOps, &Tag::Compound(map))
        .result_or_partial_silent()
        .expect("ShowText HoverEvent decodes from NBT")
}
