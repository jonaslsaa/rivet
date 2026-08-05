// STUB(mc.nbt.text) — minimal port of `net.minecraft.ChatFormatting` for
// `TextComponentTagVisitor`. Promoted into rivet-core with CrashReport
// (decision 7754455) to break the rivet-server <-> rivet-nbt Cargo cycle; the
// full enum with exact codes belongs to rivet-text.

/// Port of `net.minecraft.ChatFormatting` (enum of format codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChatFormatting {
    Black,
    DarkBlue,
    DarkGreen,
    DarkAqua,
    DarkRed,
    DarkPurple,
    Gold,
    Gray,
    DarkGray,
    Blue,
    Green,
    Aqua,
    Red,
    LightPurple,
    Yellow,
    White,
    Obfuscated,
    Bold,
    Strikethrough,
    Underline,
    Italic,
    Reset,
}

impl ChatFormatting {
    /// `ChatFormatting.PREFIX_CODE`.
    pub const PREFIX_CODE: char = '\u{00a7}';

    /// The formatting code character (e.g. `'0'` for BLACK).
    pub fn code(self) -> char {
        match self {
            ChatFormatting::Black => '0',
            ChatFormatting::DarkBlue => '1',
            ChatFormatting::DarkGreen => '2',
            ChatFormatting::DarkAqua => '3',
            ChatFormatting::DarkRed => '4',
            ChatFormatting::DarkPurple => '5',
            ChatFormatting::Gold => '6',
            ChatFormatting::Gray => '7',
            ChatFormatting::DarkGray => '8',
            ChatFormatting::Blue => '9',
            ChatFormatting::Green => 'a',
            ChatFormatting::Aqua => 'b',
            ChatFormatting::Red => 'c',
            ChatFormatting::LightPurple => 'd',
            ChatFormatting::Yellow => 'e',
            ChatFormatting::White => 'f',
            ChatFormatting::Obfuscated => 'k',
            ChatFormatting::Bold => 'l',
            ChatFormatting::Strikethrough => 'm',
            ChatFormatting::Underline => 'n',
            ChatFormatting::Italic => 'o',
            ChatFormatting::Reset => 'r',
        }
    }
}

impl std::fmt::Display for ChatFormatting {
    /// `ChatFormatting.toString()` = `"§" + code`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", Self::PREFIX_CODE, self.code())
    }
}
