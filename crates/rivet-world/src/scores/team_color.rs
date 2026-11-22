//! Port of `net.minecraft.world.scores.TeamColor` (Paper 26.2).

use crate::scores::DisplaySlot;
use rivet_core::ChatFormatting;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_text::TextColor;
use std::sync::Arc;

/// The 16 vanilla team colors (`TeamColor.BLACK` .. `TeamColor.WHITE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum TeamColor {
    Black = 0,
    DarkBlue = 1,
    DarkGreen = 2,
    DarkAqua = 3,
    DarkRed = 4,
    DarkPurple = 5,
    Gold = 6,
    Gray = 7,
    DarkGray = 8,
    Blue = 9,
    Green = 10,
    Aqua = 11,
    Red = 12,
    LightPurple = 13,
    Yellow = 14,
    White = 15,
}

impl TeamColor {
    /// All 16 team colors in enum / wire-ID order.
    pub const ALL: [TeamColor; 16] = [
        TeamColor::Black,
        TeamColor::DarkBlue,
        TeamColor::DarkGreen,
        TeamColor::DarkAqua,
        TeamColor::DarkRed,
        TeamColor::DarkPurple,
        TeamColor::Gold,
        TeamColor::Gray,
        TeamColor::DarkGray,
        TeamColor::Blue,
        TeamColor::Green,
        TeamColor::Aqua,
        TeamColor::Red,
        TeamColor::LightPurple,
        TeamColor::Yellow,
        TeamColor::White,
    ];

    /// Numeric protocol ID (`0..=15`).
    #[inline]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Serialized lowercase name (`"black"`, `"dark_blue"`, ...).
    pub const fn serialized_name(self) -> &'static str {
        match self {
            TeamColor::Black => "black",
            TeamColor::DarkBlue => "dark_blue",
            TeamColor::DarkGreen => "dark_green",
            TeamColor::DarkAqua => "dark_aqua",
            TeamColor::DarkRed => "dark_red",
            TeamColor::DarkPurple => "dark_purple",
            TeamColor::Gold => "gold",
            TeamColor::Gray => "gray",
            TeamColor::DarkGray => "dark_gray",
            TeamColor::Blue => "blue",
            TeamColor::Green => "green",
            TeamColor::Aqua => "aqua",
            TeamColor::Red => "red",
            TeamColor::LightPurple => "light_purple",
            TeamColor::Yellow => "yellow",
            TeamColor::White => "white",
        }
    }

    /// Corresponding `TextColor` constant.
    pub const fn text_color(self) -> TextColor {
        match self {
            TeamColor::Black => TextColor::BLACK,
            TeamColor::DarkBlue => TextColor::DARK_BLUE,
            TeamColor::DarkGreen => TextColor::DARK_GREEN,
            TeamColor::DarkAqua => TextColor::DARK_AQUA,
            TeamColor::DarkRed => TextColor::DARK_RED,
            TeamColor::DarkPurple => TextColor::DARK_PURPLE,
            TeamColor::Gold => TextColor::GOLD,
            TeamColor::Gray => TextColor::GRAY,
            TeamColor::DarkGray => TextColor::DARK_GRAY,
            TeamColor::Blue => TextColor::BLUE,
            TeamColor::Green => TextColor::GREEN,
            TeamColor::Aqua => TextColor::AQUA,
            TeamColor::Red => TextColor::RED,
            TeamColor::LightPurple => TextColor::LIGHT_PURPLE,
            TeamColor::Yellow => TextColor::YELLOW,
            TeamColor::White => TextColor::WHITE,
        }
    }

    /// Corresponding `ChatFormatting` color variant.
    pub const fn chat_formatting(self) -> ChatFormatting {
        match self {
            TeamColor::Black => ChatFormatting::Black,
            TeamColor::DarkBlue => ChatFormatting::DarkBlue,
            TeamColor::DarkGreen => ChatFormatting::DarkGreen,
            TeamColor::DarkAqua => ChatFormatting::DarkAqua,
            TeamColor::DarkRed => ChatFormatting::DarkRed,
            TeamColor::DarkPurple => ChatFormatting::DarkPurple,
            TeamColor::Gold => ChatFormatting::Gold,
            TeamColor::Gray => ChatFormatting::Gray,
            TeamColor::DarkGray => ChatFormatting::DarkGray,
            TeamColor::Blue => ChatFormatting::Blue,
            TeamColor::Green => ChatFormatting::Green,
            TeamColor::Aqua => ChatFormatting::Aqua,
            TeamColor::Red => ChatFormatting::Red,
            TeamColor::LightPurple => ChatFormatting::LightPurple,
            TeamColor::Yellow => ChatFormatting::Yellow,
            TeamColor::White => ChatFormatting::White,
        }
    }

    /// Convert a `ChatFormatting` color to `TeamColor` (`None` for non-color formats).
    pub const fn from_chat_formatting(formatting: ChatFormatting) -> Option<TeamColor> {
        match formatting {
            ChatFormatting::Black => Some(TeamColor::Black),
            ChatFormatting::DarkBlue => Some(TeamColor::DarkBlue),
            ChatFormatting::DarkGreen => Some(TeamColor::DarkGreen),
            ChatFormatting::DarkAqua => Some(TeamColor::DarkAqua),
            ChatFormatting::DarkRed => Some(TeamColor::DarkRed),
            ChatFormatting::DarkPurple => Some(TeamColor::DarkPurple),
            ChatFormatting::Gold => Some(TeamColor::Gold),
            ChatFormatting::Gray => Some(TeamColor::Gray),
            ChatFormatting::DarkGray => Some(TeamColor::DarkGray),
            ChatFormatting::Blue => Some(TeamColor::Blue),
            ChatFormatting::Green => Some(TeamColor::Green),
            ChatFormatting::Aqua => Some(TeamColor::Aqua),
            ChatFormatting::Red => Some(TeamColor::Red),
            ChatFormatting::LightPurple => Some(TeamColor::LightPurple),
            ChatFormatting::Yellow => Some(TeamColor::Yellow),
            ChatFormatting::White => Some(TeamColor::White),
            _ => None,
        }
    }

    /// Associated team sidebar `DisplaySlot`.
    pub const fn display_slot(self) -> DisplaySlot {
        DisplaySlot::team_color_to_slot(self)
    }

    /// Look up by exact numeric ID (`0..=15`).
    pub const fn from_id(id: i32) -> Option<TeamColor> {
        match id {
            0 => Some(TeamColor::Black),
            1 => Some(TeamColor::DarkBlue),
            2 => Some(TeamColor::DarkGreen),
            3 => Some(TeamColor::DarkAqua),
            4 => Some(TeamColor::DarkRed),
            5 => Some(TeamColor::DarkPurple),
            6 => Some(TeamColor::Gold),
            7 => Some(TeamColor::Gray),
            8 => Some(TeamColor::DarkGray),
            9 => Some(TeamColor::Blue),
            10 => Some(TeamColor::Green),
            11 => Some(TeamColor::Aqua),
            12 => Some(TeamColor::Red),
            13 => Some(TeamColor::LightPurple),
            14 => Some(TeamColor::Yellow),
            15 => Some(TeamColor::White),
            _ => None,
        }
    }

    /// `ByIdMap.OutOfBoundsStrategy.ZERO` lookup used by `TeamColor.BY_ID`.
    #[inline]
    pub const fn by_id_or_zero(id: i32) -> TeamColor {
        match Self::from_id(id) {
            Some(c) => c,
            None => TeamColor::Black,
        }
    }

    /// Look up by serialized name (`"black"`, `"dark_blue"`, ...).
    pub fn from_name(name: &str) -> Option<TeamColor> {
        match name {
            "black" => Some(TeamColor::Black),
            "dark_blue" => Some(TeamColor::DarkBlue),
            "dark_green" => Some(TeamColor::DarkGreen),
            "dark_aqua" => Some(TeamColor::DarkAqua),
            "dark_red" => Some(TeamColor::DarkRed),
            "dark_purple" => Some(TeamColor::DarkPurple),
            "gold" => Some(TeamColor::Gold),
            "gray" => Some(TeamColor::Gray),
            "dark_gray" => Some(TeamColor::DarkGray),
            "blue" => Some(TeamColor::Blue),
            "green" => Some(TeamColor::Green),
            "aqua" => Some(TeamColor::Aqua),
            "red" => Some(TeamColor::Red),
            "light_purple" => Some(TeamColor::LightPurple),
            "yellow" => Some(TeamColor::Yellow),
            "white" => Some(TeamColor::White),
            _ => None,
        }
    }

    /// `TeamColor.CODEC` (`StringRepresentable.fromEnum`).
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TeamColor, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|s: &String| match TeamColor::from_name(s) {
                Some(color) => DataResult::success(color),
                None => DataResult::error(format!("Not a valid team color: {s}")),
            }),
            Arc::new(|c: &TeamColor| c.serialized_name().to_owned()),
        )
    }
}

impl rivet_serialization::codec::JavaEquals for TeamColor {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}
