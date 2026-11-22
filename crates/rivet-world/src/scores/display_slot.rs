//! Port of `net.minecraft.world.scores.DisplaySlot` (Paper 26.2).

use crate::scores::TeamColor;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// All 19 scoreboard display slots in exact Paper 26.2 declaration / ID order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum DisplaySlot {
    List = 0,
    Sidebar = 1,
    BelowName = 2,
    TeamBlack = 3,
    TeamDarkBlue = 4,
    TeamDarkGreen = 5,
    TeamDarkAqua = 6,
    TeamDarkRed = 7,
    TeamDarkPurple = 8,
    TeamGold = 9,
    TeamGray = 10,
    TeamDarkGray = 11,
    TeamBlue = 12,
    TeamGreen = 13,
    TeamAqua = 14,
    TeamRed = 15,
    TeamLightPurple = 16,
    TeamYellow = 17,
    TeamWhite = 18,
}

impl DisplaySlot {
    /// All 19 display slots in `DisplaySlot.values()` order.
    pub const ALL: [DisplaySlot; 19] = [
        DisplaySlot::List,
        DisplaySlot::Sidebar,
        DisplaySlot::BelowName,
        DisplaySlot::TeamBlack,
        DisplaySlot::TeamDarkBlue,
        DisplaySlot::TeamDarkGreen,
        DisplaySlot::TeamDarkAqua,
        DisplaySlot::TeamDarkRed,
        DisplaySlot::TeamDarkPurple,
        DisplaySlot::TeamGold,
        DisplaySlot::TeamGray,
        DisplaySlot::TeamDarkGray,
        DisplaySlot::TeamBlue,
        DisplaySlot::TeamGreen,
        DisplaySlot::TeamAqua,
        DisplaySlot::TeamRed,
        DisplaySlot::TeamLightPurple,
        DisplaySlot::TeamYellow,
        DisplaySlot::TeamWhite,
    ];

    /// Numeric protocol ID (`0..=18`).
    #[inline]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Serialized name (`"list"`, `"sidebar"`, `"below_name"`, `"sidebar.team.<color>"`).
    pub const fn serialized_name(self) -> &'static str {
        match self {
            DisplaySlot::List => "list",
            DisplaySlot::Sidebar => "sidebar",
            DisplaySlot::BelowName => "below_name",
            DisplaySlot::TeamBlack => "sidebar.team.black",
            DisplaySlot::TeamDarkBlue => "sidebar.team.dark_blue",
            DisplaySlot::TeamDarkGreen => "sidebar.team.dark_green",
            DisplaySlot::TeamDarkAqua => "sidebar.team.dark_aqua",
            DisplaySlot::TeamDarkRed => "sidebar.team.dark_red",
            DisplaySlot::TeamDarkPurple => "sidebar.team.dark_purple",
            DisplaySlot::TeamGold => "sidebar.team.gold",
            DisplaySlot::TeamGray => "sidebar.team.gray",
            DisplaySlot::TeamDarkGray => "sidebar.team.dark_gray",
            DisplaySlot::TeamBlue => "sidebar.team.blue",
            DisplaySlot::TeamGreen => "sidebar.team.green",
            DisplaySlot::TeamAqua => "sidebar.team.aqua",
            DisplaySlot::TeamRed => "sidebar.team.red",
            DisplaySlot::TeamLightPurple => "sidebar.team.light_purple",
            DisplaySlot::TeamYellow => "sidebar.team.yellow",
            DisplaySlot::TeamWhite => "sidebar.team.white",
        }
    }

    /// Look up by numeric ID (`0..=18`).
    pub const fn from_id(id: i32) -> Option<DisplaySlot> {
        match id {
            0 => Some(DisplaySlot::List),
            1 => Some(DisplaySlot::Sidebar),
            2 => Some(DisplaySlot::BelowName),
            3 => Some(DisplaySlot::TeamBlack),
            4 => Some(DisplaySlot::TeamDarkBlue),
            5 => Some(DisplaySlot::TeamDarkGreen),
            6 => Some(DisplaySlot::TeamDarkAqua),
            7 => Some(DisplaySlot::TeamDarkRed),
            8 => Some(DisplaySlot::TeamDarkPurple),
            9 => Some(DisplaySlot::TeamGold),
            10 => Some(DisplaySlot::TeamGray),
            11 => Some(DisplaySlot::TeamDarkGray),
            12 => Some(DisplaySlot::TeamBlue),
            13 => Some(DisplaySlot::TeamGreen),
            14 => Some(DisplaySlot::TeamAqua),
            15 => Some(DisplaySlot::TeamRed),
            16 => Some(DisplaySlot::TeamLightPurple),
            17 => Some(DisplaySlot::TeamYellow),
            18 => Some(DisplaySlot::TeamWhite),
            _ => None,
        }
    }

    /// Look up by serialized name.
    pub fn from_name(name: &str) -> Option<DisplaySlot> {
        match name {
            "list" => Some(DisplaySlot::List),
            "sidebar" => Some(DisplaySlot::Sidebar),
            "below_name" => Some(DisplaySlot::BelowName),
            "sidebar.team.black" => Some(DisplaySlot::TeamBlack),
            "sidebar.team.dark_blue" => Some(DisplaySlot::TeamDarkBlue),
            "sidebar.team.dark_green" => Some(DisplaySlot::TeamDarkGreen),
            "sidebar.team.dark_aqua" => Some(DisplaySlot::TeamDarkAqua),
            "sidebar.team.dark_red" => Some(DisplaySlot::TeamDarkRed),
            "sidebar.team.dark_purple" => Some(DisplaySlot::TeamDarkPurple),
            "sidebar.team.gold" => Some(DisplaySlot::TeamGold),
            "sidebar.team.gray" => Some(DisplaySlot::TeamGray),
            "sidebar.team.dark_gray" => Some(DisplaySlot::TeamDarkGray),
            "sidebar.team.blue" => Some(DisplaySlot::TeamBlue),
            "sidebar.team.green" => Some(DisplaySlot::TeamGreen),
            "sidebar.team.aqua" => Some(DisplaySlot::TeamAqua),
            "sidebar.team.red" => Some(DisplaySlot::TeamRed),
            "sidebar.team.light_purple" => Some(DisplaySlot::TeamLightPurple),
            "sidebar.team.yellow" => Some(DisplaySlot::TeamYellow),
            "sidebar.team.white" => Some(DisplaySlot::TeamWhite),
            _ => None,
        }
    }

    /// `DisplaySlot.teamColorToSlot(TeamColor)`.
    pub const fn team_color_to_slot(color: TeamColor) -> DisplaySlot {
        match color {
            TeamColor::Black => DisplaySlot::TeamBlack,
            TeamColor::DarkBlue => DisplaySlot::TeamDarkBlue,
            TeamColor::DarkGreen => DisplaySlot::TeamDarkGreen,
            TeamColor::DarkAqua => DisplaySlot::TeamDarkAqua,
            TeamColor::DarkRed => DisplaySlot::TeamDarkRed,
            TeamColor::DarkPurple => DisplaySlot::TeamDarkPurple,
            TeamColor::Gold => DisplaySlot::TeamGold,
            TeamColor::Gray => DisplaySlot::TeamGray,
            TeamColor::DarkGray => DisplaySlot::TeamDarkGray,
            TeamColor::Blue => DisplaySlot::TeamBlue,
            TeamColor::Green => DisplaySlot::TeamGreen,
            TeamColor::Aqua => DisplaySlot::TeamAqua,
            TeamColor::Red => DisplaySlot::TeamRed,
            TeamColor::LightPurple => DisplaySlot::TeamLightPurple,
            TeamColor::Yellow => DisplaySlot::TeamYellow,
            TeamColor::White => DisplaySlot::TeamWhite,
        }
    }

    /// `DisplaySlot.CODEC` (`StringRepresentable.fromEnum`).
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DisplaySlot, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|s: &String| match DisplaySlot::from_name(s) {
                Some(slot) => DataResult::success(slot),
                None => DataResult::error(format!("Not a valid display slot: {s}")),
            }),
            Arc::new(|slot: &DisplaySlot| slot.serialized_name().to_owned()),
        )
    }
}

impl std::fmt::Display for DisplaySlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.serialized_name())
    }
}
