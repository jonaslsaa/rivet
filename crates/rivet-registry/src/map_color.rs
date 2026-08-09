//! `net.minecraft.world.level.material.MapColor` — the material color value
//! surface (issue #228). A `MapColor` is a (id, rgb) pair; every block state
//! carries one via its behavior word's map-color id (`block_state::BlockState
//! ::map_color_id`), and worldgen/lighting surface it as ARGB through
//! [`MapColor::calculate_argb`] / [`get_color_from_packed_id`].
//!
//! Fidelity notes (Paper 26.2 `MapColor.java`):
//! - The 62 constants are generated-equivalent data (ids 0..=61, none holey);
//!   `by_id` maps a hole to `NONE` (`byIdUnsafe` null → `NONE`), and the
//!   guarded `by_id` panics on an out-of-range id like `Preconditions
//!   .checkPositionIndex` (`IndexOutOfBoundsException`).
//! - `calculateARGBColor` special-cases `NONE` (id 0) to return `0` before
//!   scaling; every other color scales its opaque RGB by the brightness
//!   modifier via `ARGB.scaleRGB(int, int)` — `clamp((long)channel * scale /
//!   255L, 0, 255)`, truncating, preserving alpha.
//! - `Brightness` ids/modifiers are {LOW 0/180, NORMAL 1/220, HIGH 2/255,
//!   LOWEST 3/135}; `by_id` is checked (`checkPositionIndex`), `by_id_unchecked`
//!   is not.
//! - `getPackedId`/`getColorFromPackedId` use the low 8 bits of the packed id:
//!   `(id << 2) | (brightness & 3)` packed, and the reverse decode.

/// `net.minecraft.world.level.material.MapColor` — a material color as an
/// `(id, ARGB-rgb)` pair. `Copy`/`Eq` mirror the two `public final int` fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapColor {
    /// The material color id (`MapColor.id`, `0..=63`).
    pub id: u8,
    /// The opaque ARGB rgb (`MapColor.col`; alpha implied 255).
    pub col: i32,
}

impl MapColor {
    // The 62 constants in id order (Paper `MapColor.java` lines 9-70). The
    // numeric `col` values are the pinned source-of-truth RGB ints.
    pub const NONE: Self = Self::new(0, 0);
    pub const GRASS: Self = Self::new(1, 8368696);
    pub const SAND: Self = Self::new(2, 16247203);
    pub const WOOL: Self = Self::new(3, 13092807);
    pub const FIRE: Self = Self::new(4, 16711680);
    pub const ICE: Self = Self::new(5, 10526975);
    pub const METAL: Self = Self::new(6, 10987431);
    pub const PLANT: Self = Self::new(7, 31744);
    pub const SNOW: Self = Self::new(8, 16777215);
    pub const CLAY: Self = Self::new(9, 10791096);
    pub const DIRT: Self = Self::new(10, 9923917);
    pub const STONE: Self = Self::new(11, 7368816);
    pub const WATER: Self = Self::new(12, 4210943);
    pub const WOOD: Self = Self::new(13, 9402184);
    pub const QUARTZ: Self = Self::new(14, 16776437);
    pub const COLOR_ORANGE: Self = Self::new(15, 14188339);
    pub const COLOR_MAGENTA: Self = Self::new(16, 11685080);
    pub const COLOR_LIGHT_BLUE: Self = Self::new(17, 6724056);
    pub const COLOR_YELLOW: Self = Self::new(18, 15066419);
    pub const COLOR_LIGHT_GREEN: Self = Self::new(19, 8375321);
    pub const COLOR_PINK: Self = Self::new(20, 15892389);
    pub const COLOR_GRAY: Self = Self::new(21, 5000268);
    pub const COLOR_LIGHT_GRAY: Self = Self::new(22, 10066329);
    pub const COLOR_CYAN: Self = Self::new(23, 5013401);
    pub const COLOR_PURPLE: Self = Self::new(24, 8339378);
    pub const COLOR_BLUE: Self = Self::new(25, 3361970);
    pub const COLOR_BROWN: Self = Self::new(26, 6704179);
    pub const COLOR_GREEN: Self = Self::new(27, 6717235);
    pub const COLOR_RED: Self = Self::new(28, 10040115);
    pub const COLOR_BLACK: Self = Self::new(29, 1644825);
    pub const GOLD: Self = Self::new(30, 16445005);
    pub const DIAMOND: Self = Self::new(31, 6085589);
    pub const LAPIS: Self = Self::new(32, 4882687);
    pub const EMERALD: Self = Self::new(33, 55610);
    pub const PODZOL: Self = Self::new(34, 8476209);
    pub const NETHER: Self = Self::new(35, 7340544);
    pub const TERRACOTTA_WHITE: Self = Self::new(36, 13742497);
    pub const TERRACOTTA_ORANGE: Self = Self::new(37, 10441252);
    pub const TERRACOTTA_MAGENTA: Self = Self::new(38, 9787244);
    pub const TERRACOTTA_LIGHT_BLUE: Self = Self::new(39, 7367818);
    pub const TERRACOTTA_YELLOW: Self = Self::new(40, 12223780);
    pub const TERRACOTTA_LIGHT_GREEN: Self = Self::new(41, 6780213);
    pub const TERRACOTTA_PINK: Self = Self::new(42, 10505550);
    pub const TERRACOTTA_GRAY: Self = Self::new(43, 3746083);
    pub const TERRACOTTA_LIGHT_GRAY: Self = Self::new(44, 8874850);
    pub const TERRACOTTA_CYAN: Self = Self::new(45, 5725276);
    pub const TERRACOTTA_PURPLE: Self = Self::new(46, 8014168);
    pub const TERRACOTTA_BLUE: Self = Self::new(47, 4996700);
    pub const TERRACOTTA_BROWN: Self = Self::new(48, 4993571);
    pub const TERRACOTTA_GREEN: Self = Self::new(49, 5001770);
    pub const TERRACOTTA_RED: Self = Self::new(50, 9321518);
    pub const TERRACOTTA_BLACK: Self = Self::new(51, 2430480);
    pub const CRIMSON_NYLIUM: Self = Self::new(52, 12398641);
    pub const CRIMSON_STEM: Self = Self::new(53, 9715553);
    pub const CRIMSON_HYPHAE: Self = Self::new(54, 6035741);
    pub const WARPED_NYLIUM: Self = Self::new(55, 1474182);
    pub const WARPED_STEM: Self = Self::new(56, 3837580);
    pub const WARPED_HYPHAE: Self = Self::new(57, 5647422);
    pub const WARPED_WART_BLOCK: Self = Self::new(58, 1356933);
    pub const DEEPSLATE: Self = Self::new(59, 6579300);
    pub const RAW_IRON: Self = Self::new(60, 14200723);
    pub const GLOW_LICHEN: Self = Self::new(61, 8365974);

    /// The private `MapColor(int id, int col)` constructor — panics on an id
    /// outside `0..=63` exactly like the Java `IndexOutOfBoundsException`
    /// ("Map colour ID must be between 0 and 63 (inclusive)").
    pub const fn new(id: u8, col: i32) -> Self {
        // A const-fn can't format, so keep the panic string close to Java's.
        if id > 63 {
            panic!("Map colour ID must be between 0 and 63 (inclusive)");
        }
        Self { id, col }
    }

    /// `MapColor.byId(int)` — guarded lookup; a hole or an out-of-range id
    /// behaves per Java: an out-of-range id is a `Preconditions` failure
    /// (panic here, `IndexOutOfBoundsException` there), a hole yields `NONE`.
    pub fn by_id(id: u8) -> Self {
        if (id as usize) >= MATERIAL_COLORS.len() {
            panic!("material id {} out of range", id);
        }
        by_id_unchecked(id)
    }

    /// `MapColor.calculateARGBColor(Brightness)` — `NONE` maps to 0, else the
    /// opaque rgb scaled by the brightness modifier (`ARGB.scaleRGB(int,int)`:
    /// `(long)channel * modifier / 255L`, truncating).
    pub fn calculate_argb(self, brightness: Brightness) -> i32 {
        if self == Self::NONE {
            0
        } else {
            scale_rgb(argb_opaque(self.col), brightness.modifier())
        }
    }

    /// `MapColor.getColorFromPackedId(int)` — decode the low 8 bits of a packed
    /// color id into ARGB (`byIdUnsafe` on the material id + `Brightness`).
    pub fn get_color_from_packed_id(packed_id: i32) -> i32 {
        let val = packed_id & 0xFF;
        by_id_unchecked((val >> 2) as u8)
            .calculate_argb(Brightness::by_id_unchecked((val & 3) as u8))
    }

    /// `MapColor.getPackedId(Brightness)` — `(byte)(id << 2 | brightness.id & 3)`.
    /// `id <= 63` and the `& 3` mask keep the result within `u8`, so the Java
    /// `(byte)` cast is a no-op here.
    pub fn get_packed_id(self, brightness: Brightness) -> u8 {
        self.id << 2 | brightness.id() & 3
    }
}

/// `MapColor.MATERIAL_COLORS` — id-indexed table. The 62 constants fill ids
/// `0..=61`; the final two slots (62, 63) are empty in Java (`byIdUnsafe`
/// returns `NONE` for a `null` slot), so they are `NONE` here.
static MATERIAL_COLORS: [MapColor; 64] = [
    MapColor::NONE,
    MapColor::GRASS,
    MapColor::SAND,
    MapColor::WOOL,
    MapColor::FIRE,
    MapColor::ICE,
    MapColor::METAL,
    MapColor::PLANT,
    MapColor::SNOW,
    MapColor::CLAY,
    MapColor::DIRT,
    MapColor::STONE,
    MapColor::WATER,
    MapColor::WOOD,
    MapColor::QUARTZ,
    MapColor::COLOR_ORANGE,
    MapColor::COLOR_MAGENTA,
    MapColor::COLOR_LIGHT_BLUE,
    MapColor::COLOR_YELLOW,
    MapColor::COLOR_LIGHT_GREEN,
    MapColor::COLOR_PINK,
    MapColor::COLOR_GRAY,
    MapColor::COLOR_LIGHT_GRAY,
    MapColor::COLOR_CYAN,
    MapColor::COLOR_PURPLE,
    MapColor::COLOR_BLUE,
    MapColor::COLOR_BROWN,
    MapColor::COLOR_GREEN,
    MapColor::COLOR_RED,
    MapColor::COLOR_BLACK,
    MapColor::GOLD,
    MapColor::DIAMOND,
    MapColor::LAPIS,
    MapColor::EMERALD,
    MapColor::PODZOL,
    MapColor::NETHER,
    MapColor::TERRACOTTA_WHITE,
    MapColor::TERRACOTTA_ORANGE,
    MapColor::TERRACOTTA_MAGENTA,
    MapColor::TERRACOTTA_LIGHT_BLUE,
    MapColor::TERRACOTTA_YELLOW,
    MapColor::TERRACOTTA_LIGHT_GREEN,
    MapColor::TERRACOTTA_PINK,
    MapColor::TERRACOTTA_GRAY,
    MapColor::TERRACOTTA_LIGHT_GRAY,
    MapColor::TERRACOTTA_CYAN,
    MapColor::TERRACOTTA_PURPLE,
    MapColor::TERRACOTTA_BLUE,
    MapColor::TERRACOTTA_BROWN,
    MapColor::TERRACOTTA_GREEN,
    MapColor::TERRACOTTA_RED,
    MapColor::TERRACOTTA_BLACK,
    MapColor::CRIMSON_NYLIUM,
    MapColor::CRIMSON_STEM,
    MapColor::CRIMSON_HYPHAE,
    MapColor::WARPED_NYLIUM,
    MapColor::WARPED_STEM,
    MapColor::WARPED_HYPHAE,
    MapColor::WARPED_WART_BLOCK,
    MapColor::DEEPSLATE,
    MapColor::RAW_IRON,
    MapColor::GLOW_LICHEN,
    MapColor::NONE,
    MapColor::NONE,
];

/// `MapColor.byIdUnsafe(int)` — `result != null ? result : NONE`.
fn by_id_unchecked(id: u8) -> MapColor {
    MATERIAL_COLORS[id as usize]
}

/// `MapColor.Brightness` — the four brightness modifiers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brightness {
    LOW = 0,
    NORMAL = 1,
    HIGH = 2,
    LOWEST = 3,
}

impl Brightness {
    /// `Brightness.VALUES[id]` — `byId` checks the position index; `by_id_unchecked`
    /// is the private unchecked path.
    pub fn by_id(id: u8) -> Self {
        if (id as usize) >= 4 {
            panic!("brightness id {} out of range", id);
        }
        Self::by_id_unchecked(id)
    }

    /// The private `byIdUnsafe` — indexes the fixed 4-element table.
    pub const fn by_id_unchecked(id: u8) -> Self {
        match id {
            0 => Self::LOW,
            1 => Self::NORMAL,
            2 => Self::HIGH,
            _ => Self::LOWEST,
        }
    }

    /// The brightness `id` field.
    pub const fn id(self) -> u8 {
        self as u8
    }

    /// The brightness `modifier` field.
    pub const fn modifier(self) -> i32 {
        match self {
            Self::LOW => 180,
            Self::NORMAL => 220,
            Self::HIGH => 255,
            Self::LOWEST => 135,
        }
    }
}

// The `ARGB` helpers MapColor depends on. Only `scaleRGB(int,int)` and
// `opaque(int)` (plus the channel extractors it needs) are ported — the rest
// of `net.minecraft.util.ARGB` stays out of scope (rivet-util::mth carries the
// #206 `argb_color` only; see its RivetTodo). These match Paper `ARGB.java`
// exactly: `color(alpha, red, green, blue)` masks each 8-bit channel, and the
// integer scale does `(long)channel * scale / 255L` (truncating) then clamps
// to `0..=255`. `MapColor.calculateARGBColor` passes an `int` modifier, so it
// resolves to this int overload — NOT the `float` `scaleRGB` (which omits the
// `/ 255` normalization and would produce a different ARGB for every color).

/// `ARGB.color(int alpha, int red, int green, int blue)`.
fn argb_color(alpha: i32, red: i32, green: i32, blue: i32) -> i32 {
    (alpha & 0xFF) << 24 | (red & 0xFF) << 16 | (green & 0xFF) << 8 | (blue & 0xFF)
}

/// `ARGB.alpha(int)`.
fn argb_alpha(color: i32) -> i32 {
    (color as u32 >> 24) as i32
}

/// `ARGB.red(int)`.
fn argb_red(color: i32) -> i32 {
    (color >> 16) & 0xFF
}

/// `ARGB.green(int)`.
fn argb_green(color: i32) -> i32 {
    (color >> 8) & 0xFF
}

/// `ARGB.blue(int)`.
fn argb_blue(color: i32) -> i32 {
    color & 0xFF
}

/// `ARGB.opaque(int)` — `color | 0xFF000000` (the alpha byte forced on; the
/// literal's signed bit pattern is `-16777216`).
fn argb_opaque(color: i32) -> i32 {
    color | (0xFF << 24)
}

/// `ARGB.scaleRGB(int, int)` — `color(alpha, clamp((long)red*scale/255L, 0,
/// 255), ...)` with `Math.clamp` bounding to `0..=255`. This is the overload
/// `MapColor.calculateARGBColor` resolves to: its `brightness.modifier` is an
/// `int`, so the `/ 255L` normalization applies (unlike the `float` overload).
fn scale_rgb(color: i32, scale: i32) -> i32 {
    argb_color(
        argb_alpha(color),
        clamp_channel(((argb_red(color) as i64 * scale as i64) / 255) as i32),
        clamp_channel(((argb_green(color) as i64 * scale as i64) / 255) as i32),
        clamp_channel(((argb_blue(color) as i64 * scale as i64) / 255) as i32),
    )
}

/// `Math.clamp(int, 0, 255)`.
fn clamp_channel(v: i32) -> i32 {
    v.clamp(0, 255)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_paper_ids_and_colors() {
        // Spot-check a spread of ids + the boundary colors; the full table is
        // exercised by `material_colors_table_is_ordered_and_holeless`.
        assert_eq!(MapColor::NONE.id, 0);
        assert_eq!(MapColor::NONE.col, 0);
        assert_eq!(MapColor::STONE.id, 11);
        assert_eq!(MapColor::STONE.col, 7368816);
        assert_eq!(MapColor::WATER.id, 12);
        assert_eq!(MapColor::WATER.col, 4210943);
        assert_eq!(MapColor::PLANT.id, 7);
        assert_eq!(MapColor::PLANT.col, 31744);
        assert_eq!(MapColor::GLOW_LICHEN.id, 61);
        assert_eq!(MapColor::GLOW_LICHEN.col, 8365974);
    }

    #[test]
    fn material_colors_table_is_ordered_and_holeless() {
        // The 62 constants fill ids 0..=61 with no holes (Paper registers every
        // constructor into MATERIAL_COLORS[id]); the remaining two slots are
        // empty (NONE) in vanilla.
        for id in 0..=61u8 {
            assert_eq!(MATERIAL_COLORS[id as usize].id, id);
        }
        // The final two slots are holes -> NONE via byIdUnsafe.
        assert_eq!(by_id_unchecked(62), MapColor::NONE);
        assert_eq!(by_id_unchecked(63), MapColor::NONE);
        // by_id on a hole yields NONE, not a panic.
        assert_eq!(MapColor::by_id(62), MapColor::NONE);
    }

    #[test]
    fn by_id_panics_out_of_range() {
        // Preconditions.checkPositionIndex(id, 64) -> IndexOutOfBoundsException.
        let err = std::panic::catch_unwind(|| MapColor::by_id(64))
            .err()
            .unwrap();
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(msg.contains("64"), "got: {msg}");
    }

    #[test]
    fn new_panics_out_of_range_id() {
        // `const fn new` panics with a `&'static str` literal, so the payload
        // is a `&str`, not a `String`.
        let err = std::panic::catch_unwind(|| MapColor::new(64, 0))
            .err()
            .unwrap();
        let msg = err
            .downcast_ref::<&'static str>()
            .copied()
            .unwrap_or_else(|| {
                err.downcast_ref::<String>()
                    .map(String::as_str)
                    .unwrap_or_default()
            });
        assert!(msg.contains("between 0 and 63"), "got: {msg}");
    }

    #[test]
    fn brightness_ids_and_modifiers() {
        let cases = [
            (Brightness::LOW, 0, 180),
            (Brightness::NORMAL, 1, 220),
            (Brightness::HIGH, 2, 255),
            (Brightness::LOWEST, 3, 135),
        ];
        for (b, id, modifier) in cases {
            assert_eq!(b.id(), id);
            assert_eq!(b.modifier(), modifier);
            assert_eq!(Brightness::by_id_unchecked(id), b);
        }
        assert_eq!(Brightness::by_id(3), Brightness::LOWEST);
    }

    #[test]
    fn brightness_by_id_panics_out_of_range() {
        let err = std::panic::catch_unwind(|| Brightness::by_id(4))
            .err()
            .unwrap();
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        assert!(msg.contains("4"), "got: {msg}");
    }

    #[test]
    fn calculate_argb_none_is_zero() {
        assert_eq!(MapColor::NONE.calculate_argb(Brightness::NORMAL), 0);
    }

    #[test]
    fn calculate_argb_scales_opaque_rgb() {
        // ARGB is a signed Java `int`, so a full-alpha value (high bit set) is
        // written as a `u32` literal cast back to `i32` (bit pattern preserved).
        // GRASS (0x7FB238) at NORMAL (modifier 220) via the int overload:
        // clamp((long)channel * 220 / 255L) -> red 127->109 (0x6D), green
        // 178->153 (0x99), blue 56->48 (0x30) = Paper's 0xFF6D9930 (NOT the
        // float overload's saturated 0xFFFFFFFF).
        assert_eq!(
            MapColor::GRASS.calculate_argb(Brightness::NORMAL),
            0xFF6D_9930u32 as i32
        );
        // A zero channel stays 0.
        assert_eq!(MapColor::NONE.calculate_argb(Brightness::HIGH), 0);
        // Direct scale of an opaque black keeps alpha and clamps nothing.
        assert_eq!(scale_rgb(argb_opaque(0), 255), 0xFF00_0000u32 as i32);
    }

    #[test]
    fn scale_rgb_int_is_channel_times_scale_over_255() {
        // 0x101000: red 0x10, green 0x10, blue 0x00.
        // @ HIGH (255): (16 * 255) / 255 = 16 (identity).
        assert_eq!(
            scale_rgb(argb_opaque(0x10_1000), 255),
            0xFF10_1000u32 as i32
        );
        // @ NORMAL (220): (16 * 220) / 255 = 13 (0x0D), truncating.
        assert_eq!(
            scale_rgb(argb_opaque(0x10_1000), 220),
            0xFF0D_0D00u32 as i32
        );
        // @ LOW (180): (16 * 180) / 255 = 11 (0x0B).
        assert_eq!(
            scale_rgb(argb_opaque(0x10_1000), 180),
            0xFF0B_0B00u32 as i32
        );
        // @ LOWEST (135): (16 * 135) / 255 = 8.
        assert_eq!(
            scale_rgb(argb_opaque(0x10_1000), 135),
            0xFF08_0800u32 as i32
        );
    }

    #[test]
    fn scale_rgb_truncates_like_long_division() {
        // Integer long division, not the float overload: channel 1 @ LOW (180)
        // is (1 * 180) / 255 = 0 (trunc), where the float scale 180.0 would
        // have produced 180.
        assert_eq!(scale_rgb(argb_opaque(0x0001), 180), 0xFF00_0000u32 as i32);
        // channel 2 @ HIGH (255): (2 * 255) / 255 = 2.
        assert_eq!(scale_rgb(argb_opaque(0x0002), 255), 0xFF00_0002u32 as i32);
        // channel 0xC8 (200) @ 128: (200 * 128) / 255 = 100 (0x64).
        assert_eq!(scale_rgb(argb_opaque(0x00_C8), 128), 0xFF00_0064u32 as i32);
    }

    #[test]
    fn packed_id_round_trips() {
        for id in [0u8, 1, 7, 11, 61] {
            let color = MapColor::by_id(id);
            for brightness in [
                Brightness::LOW,
                Brightness::NORMAL,
                Brightness::HIGH,
                Brightness::LOWEST,
            ] {
                let packed = color.get_packed_id(brightness);
                // Low 8 bits only: (id << 2 | brightness & 3) as byte.
                assert_eq!(
                    packed as i32,
                    ((id as i32) << 2 | (brightness.id() as i32) & 3)
                );
            }
        }
        // NONE's packed id decodes back to 0 (NONE is id 0).
        assert_eq!(MapColor::get_color_from_packed_id(0), 0);
    }
}
