//! Display, tooltip, color, lore, custom model data, and map item components:
//! - `TooltipDisplay` (`net.minecraft.world.item.component.TooltipDisplay`)
//! - `ItemLore` (`net.minecraft.world.item.component.ItemLore`)
//! - `CustomModelData` (`net.minecraft.world.item.component.CustomModelData`)
//! - `DyedItemColor` (`net.minecraft.world.item.component.DyedItemColor`)
//! - `MapItemColor` (`net.minecraft.world.item.component.MapItemColor`)
//! - `MapPostProcessing` (`net.minecraft.world.item.component.MapPostProcessing`)
//! - `MapDecorations` / `MapDecorationEntry` (`net.minecraft.world.item.component.MapDecorations`)

use crate::item::codec_from_fns;
use crate::item::component_type::DataComponentTypeId;
use crate::item::components::{
    argb_blue, argb_color, argb_green, argb_opaque, argb_red, decode_identifier, encode_identifier,
    get_map_field, rgb_color_codec,
};
use indexmap::{IndexMap, IndexSet};
use rivet_registry::Identifier;
use rivet_serialization::{Codec, DataResult, DynamicOps};
use rivet_text::component::Component;
use rivet_text::component_serialization;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// TooltipDisplay
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.TooltipDisplay`.
///
/// Uses `IndexSet<DataComponentTypeId>` to preserve insertion order, matching
/// Paper's `ReferenceLinkedOpenHashSet<DataComponentType<?>>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TooltipDisplay {
    pub hide_tooltip: bool,
    pub hidden_components: IndexSet<DataComponentTypeId>,
}

impl TooltipDisplay {
    /// `TooltipDisplay.DEFAULT = new TooltipDisplay(false, ReferenceSortedSets.emptySet())`.
    pub fn default_value() -> Self {
        Self {
            hide_tooltip: false,
            hidden_components: IndexSet::new(),
        }
    }

    pub fn new(hide_tooltip: bool, hidden_components: IndexSet<DataComponentTypeId>) -> Self {
        Self {
            hide_tooltip,
            hidden_components,
        }
    }

    /// `TooltipDisplay.withHidden(DataComponentType<?>, boolean)`.
    pub fn with_hidden(&self, component: DataComponentTypeId, hidden: bool) -> Self {
        if self.hidden_components.contains(&component) == hidden {
            return self.clone();
        }
        let mut next = self.hidden_components.clone();
        if hidden {
            next.insert(component);
        } else {
            next.shift_remove(&component);
        }
        Self {
            hide_tooltip: self.hide_tooltip,
            hidden_components: next,
        }
    }

    /// `TooltipDisplay.shows(DataComponentType<?>)`.
    pub fn shows(&self, component: DataComponentTypeId) -> bool {
        !self.hide_tooltip && !self.hidden_components.contains(&component)
    }
}

impl Default for TooltipDisplay {
    fn default() -> Self {
        Self::default_value()
    }
}

/// `TooltipDisplay.CODEC`.
pub fn tooltip_display_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<TooltipDisplay, Ops>> {
    codec_from_fns(
        "tooltip_display",
        |v: &TooltipDisplay, ops: &Ops| {
            let mut entries = Vec::new();
            if v.hide_tooltip {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("hide_tooltip".to_string()),
                    ops.create_boolean(v.hide_tooltip),
                ));
            }
            if !v.hidden_components.is_empty() {
                let mut items = Vec::with_capacity(v.hidden_components.len());
                for c in &v.hidden_components {
                    let Some(id) = c.identifier() else {
                        return DataResult::error(format!(
                            "Unknown DataComponentTypeId {} in TooltipDisplay",
                            c.id()
                        ));
                    };
                    items.push(encode_identifier(&id, ops));
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("hidden_components".to_string()),
                    ops.create_list(items),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("TooltipDisplay must be a map");
            };
            let hide_tooltip = if let Some(val) = get_map_field(ops, map.as_ref(), "hide_tooltip") {
                match ops.get_boolean_value(&val).result() {
                    Some(b) => *b,
                    None => {
                        return DataResult::error("TooltipDisplay.hide_tooltip must be boolean");
                    }
                }
            } else {
                false
            };
            let mut hidden_components = IndexSet::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "hidden_components") {
                let list_res = ops.get_stream(&val);
                let Some(items) = list_res.result() else {
                    return DataResult::error("TooltipDisplay.hidden_components must be a list");
                };
                for item in items {
                    let id = match decode_identifier(ops, item).result().cloned() {
                        Some(id) => id,
                        None => {
                            return DataResult::error(
                                "Invalid component identifier in TooltipDisplay.hidden_components",
                            );
                        }
                    };
                    let Some(ty) = DataComponentTypeId::from_identifier(&id) else {
                        return DataResult::error(format!(
                            "Unknown data component type in TooltipDisplay: {id}"
                        ));
                    };
                    hidden_components.insert(ty);
                }
            }
            DataResult::success(TooltipDisplay {
                hide_tooltip,
                hidden_components,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// ItemLore
// ---------------------------------------------------------------------------

/// `ItemLore.MAX_LINES = 256`.
pub const LORE_MAX_LINES: usize = 256;

/// Port of `net.minecraft.world.item.component.ItemLore`.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemLore {
    lines: Vec<Component>,
}

impl ItemLore {
    /// `ItemLore.EMPTY`.
    pub fn empty() -> Self {
        Self { lines: Vec::new() }
    }

    /// `new ItemLore(List<Component>)` — rejects more than `LORE_MAX_LINES` (256) lines.
    pub fn try_new(lines: Vec<Component>) -> Result<Self, String> {
        if lines.len() > LORE_MAX_LINES {
            return Err(format!(
                "Got {} lines, but maximum is {LORE_MAX_LINES}",
                lines.len()
            ));
        }
        Ok(Self { lines })
    }

    pub fn lines(&self) -> &[Component] {
        &self.lines
    }

    /// `ItemLore.withLineAdded(Component)` — appends a line if under the 256-line limit.
    pub fn with_line_added(&self, line: Component) -> Result<Self, String> {
        let mut next = self.lines.clone();
        next.push(line);
        Self::try_new(next)
    }
}

impl Default for ItemLore {
    fn default() -> Self {
        Self::empty()
    }
}

/// `ItemLore.CODEC` — `ComponentSerialization.CODEC.sizeLimitedListOf(256).xmap(...)`.
pub fn item_lore_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ItemLore, Ops>> {
    let comp_codec = component_serialization::codec::<Ops>();
    let enc_codec = Arc::clone(&comp_codec);
    let dec_codec = comp_codec;
    codec_from_fns(
        "item_lore",
        move |v: &ItemLore, ops: &Ops| {
            if v.lines.len() > LORE_MAX_LINES {
                return DataResult::error(format!(
                    "ItemLore exceeds maximum of {LORE_MAX_LINES} lines: {}",
                    v.lines.len()
                ));
            }
            let mut out = Vec::with_capacity(v.lines.len());
            for line in &v.lines {
                let r = enc_codec.encode_start(ops, line);
                match r.result().cloned() {
                    Some(encoded) => out.push(encoded),
                    None => {
                        return DataResult::error(
                            r.error_ref()
                                .map(|e| e.message().to_string())
                                .unwrap_or_else(|| "Failed to encode ItemLore line".to_string()),
                        );
                    }
                }
            }
            DataResult::success(ops.create_list(out))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let list_res = ops.get_stream(input);
            let Some(items) = list_res.result() else {
                return DataResult::error("ItemLore must be a list of text components");
            };
            if items.len() > LORE_MAX_LINES {
                return DataResult::error(format!(
                    "ItemLore list size {} exceeds maximum {LORE_MAX_LINES}",
                    items.len()
                ));
            }
            let mut lines = Vec::with_capacity(items.len());
            for item in items {
                let r = dec_codec.parse(ops, item);
                match r.result().cloned() {
                    Some(c) => lines.push(c),
                    None => {
                        return DataResult::error(
                            r.error_ref()
                                .map(|e| e.message().to_string())
                                .unwrap_or_else(|| {
                                    "Invalid text component in ItemLore".to_string()
                                }),
                        );
                    }
                }
            }
            DataResult::success(ItemLore { lines })
        },
    )
}

// ---------------------------------------------------------------------------
// CustomModelData
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.CustomModelData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomModelData {
    pub floats: Vec<f32>,
    pub flags: Vec<bool>,
    pub strings: Vec<String>,
    pub colors: Vec<i32>,
}

impl CustomModelData {
    /// `CustomModelData.EMPTY`.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn new(floats: Vec<f32>, flags: Vec<bool>, strings: Vec<String>, colors: Vec<i32>) -> Self {
        Self {
            floats,
            flags,
            strings,
            colors,
        }
    }

    /// `CustomModelData.getFloat(int index)` — returns `None` if `index < 0` or out of bounds.
    pub fn get_float(&self, index: i32) -> Option<f32> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.floats.get(i).copied())
    }

    /// `CustomModelData.getFlag(int index)`.
    pub fn get_flag(&self, index: i32) -> Option<bool> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.flags.get(i).copied())
    }

    /// `CustomModelData.getString(int index)`.
    pub fn get_string(&self, index: i32) -> Option<&str> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.strings.get(i).map(String::as_str))
    }

    /// `CustomModelData.getColor(int index)`.
    pub fn get_color(&self, index: i32) -> Option<i32> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.colors.get(i).copied())
    }
}

/// `CustomModelData.CODEC`.
pub fn custom_model_data_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<CustomModelData, Ops>>
{
    let color_codec = rgb_color_codec::<Ops>();
    let enc_color = Arc::clone(&color_codec);
    let dec_color = color_codec;
    codec_from_fns(
        "custom_model_data",
        move |v: &CustomModelData, ops: &Ops| {
            let mut entries = Vec::new();
            if !v.floats.is_empty() {
                let list: Vec<Ops::Output> =
                    v.floats.iter().map(|&f| ops.create_float(f)).collect();
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("floats".to_string()),
                    ops.create_list(list),
                ));
            }
            if !v.flags.is_empty() {
                let list: Vec<Ops::Output> =
                    v.flags.iter().map(|&b| ops.create_boolean(b)).collect();
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("flags".to_string()),
                    ops.create_list(list),
                ));
            }
            if !v.strings.is_empty() {
                let list: Vec<Ops::Output> = v
                    .strings
                    .iter()
                    .map(|s| ops.create_string(s.clone()))
                    .collect();
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("strings".to_string()),
                    ops.create_list(list),
                ));
            }
            if !v.colors.is_empty() {
                let mut list = Vec::with_capacity(v.colors.len());
                for &c in &v.colors {
                    let r = enc_color.encode_start(ops, &c);
                    match r.result().cloned() {
                        Some(ev) => list.push(ev),
                        None => return DataResult::error("Failed to encode CustomModelData color"),
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("colors".to_string()),
                    ops.create_list(list),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("CustomModelData must be a map");
            };
            let mut floats = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "floats") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("CustomModelData.floats must be a list");
                };
                for item in &items {
                    match ops.get_number_value(item).result() {
                        Some(n) => floats.push(n.float_value()),
                        None => {
                            return DataResult::error(
                                "CustomModelData.floats elements must be numbers",
                            );
                        }
                    }
                }
            }
            let mut flags = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "flags") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("CustomModelData.flags must be a list");
                };
                for item in &items {
                    match ops.get_boolean_value(item).result() {
                        Some(b) => flags.push(*b),
                        None => {
                            return DataResult::error(
                                "CustomModelData.flags elements must be booleans",
                            );
                        }
                    }
                }
            }
            let mut strings = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "strings") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("CustomModelData.strings must be a list");
                };
                for item in &items {
                    match ops.get_string_value(item).result() {
                        Some(s) => strings.push(s.clone()),
                        None => {
                            return DataResult::error(
                                "CustomModelData.strings elements must be strings",
                            );
                        }
                    }
                }
            }
            let mut colors = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "colors") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("CustomModelData.colors must be a list");
                };
                for item in &items {
                    match dec_color.parse(ops, item).result() {
                        Some(&c) => colors.push(c),
                        None => {
                            return DataResult::error(
                                "Invalid RGB color in CustomModelData.colors",
                            );
                        }
                    }
                }
            }
            DataResult::success(CustomModelData {
                floats,
                flags,
                strings,
                colors,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// DyedItemColor
// ---------------------------------------------------------------------------

/// `DyedItemColor.LEATHER_COLOR = -6265536` (`0xFFA06540`).
pub const LEATHER_COLOR: i32 = -6265536;

/// Port of `net.minecraft.world.item.component.DyedItemColor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DyedItemColor {
    pub rgb: i32,
}

impl DyedItemColor {
    pub const fn new(rgb: i32) -> Self {
        Self { rgb }
    }

    /// `ARGB.opaque(rgb)` — sets alpha to 0xFF (matches `DyedItemColor.getOrDefault`).
    pub fn opaque_rgb(self) -> i32 {
        argb_opaque(self.rgb)
    }

    /// Exact port of `DyedItemColor.applyDyes` color-blending math (Java int/float semantics).
    ///
    /// Takes an optional existing `DyedItemColor` and a slice of dye texture diffuse
    /// ARGB colors (`DyeColor.getTextureDiffuseColor()`), returning the blended
    /// `DyedItemColor` (with 0 alpha channel, matching `ARGB.color(0, red, green, blue)`).
    pub fn apply_dyes(current: Option<Self>, dye_diffuse_colors: &[i32]) -> Option<Self> {
        let mut r_total: i32 = 0;
        let mut g_total: i32 = 0;
        let mut b_total: i32 = 0;
        let mut intensity_total: i32 = 0;
        let mut color_count: i32 = 0;

        if let Some(existing) = current {
            let r = argb_red(existing.rgb);
            let g = argb_green(existing.rgb);
            let b = argb_blue(existing.rgb);
            intensity_total = intensity_total.wrapping_add(r.max(g.max(b)));
            r_total = r_total.wrapping_add(r);
            g_total = g_total.wrapping_add(g);
            b_total = b_total.wrapping_add(b);
            color_count = color_count.wrapping_add(1);
        }

        for &color in dye_diffuse_colors {
            let r = argb_red(color);
            let g = argb_green(color);
            let b = argb_blue(color);
            intensity_total = intensity_total.wrapping_add(r.max(g.max(b)));
            r_total = r_total.wrapping_add(r);
            g_total = g_total.wrapping_add(g);
            b_total = b_total.wrapping_add(b);
            color_count = color_count.wrapping_add(1);
        }

        if color_count == 0 {
            return None;
        }

        let mut red = r_total / color_count;
        let mut green = g_total / color_count;
        let mut blue = b_total / color_count;
        let average_intensity = (intensity_total as f32) / (color_count as f32);
        let result_intensity = red.max(green.max(blue)) as f32;
        red = ((red as f32) * average_intensity / result_intensity) as i32;
        green = ((green as f32) * average_intensity / result_intensity) as i32;
        blue = ((blue as f32) * average_intensity / result_intensity) as i32;
        Some(Self {
            rgb: argb_color(0, red, green, blue),
        })
    }
}

/// `DyedItemColor.CODEC = ExtraCodecs.RGB_COLOR_CODEC.xmap(DyedItemColor::new, DyedItemColor::rgb)`.
pub fn dyed_item_color_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<DyedItemColor, Ops>> {
    let rgb = rgb_color_codec::<Ops>();
    let enc = Arc::clone(&rgb);
    let dec = rgb;
    codec_from_fns(
        "dyed_item_color",
        move |v: &DyedItemColor, ops: &Ops| enc.encode_start(ops, &v.rgb),
        move |ops: &Ops, input: &Ops::Output| dec.parse(ops, input).map_owned(DyedItemColor::new),
    )
}

// ---------------------------------------------------------------------------
// MapItemColor
// ---------------------------------------------------------------------------

/// `MapItemColor.DEFAULT = 4603950` (`0x46402E`).
pub const MAP_ITEM_COLOR_DEFAULT: i32 = 4_603_950;

/// Port of `net.minecraft.world.item.component.MapItemColor`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MapItemColor {
    pub rgb: i32,
}

impl MapItemColor {
    pub const DEFAULT: Self = Self {
        rgb: MAP_ITEM_COLOR_DEFAULT,
    };

    pub const fn new(rgb: i32) -> Self {
        Self { rgb }
    }
}

impl Default for MapItemColor {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// `MapItemColor.CODEC = Codec.INT.xmap(MapItemColor::new, MapItemColor::rgb)`.
pub fn map_item_color_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<MapItemColor, Ops>> {
    codec_from_fns(
        "map_item_color",
        |v: &MapItemColor, ops: &Ops| DataResult::success(ops.create_int(v.rgb)),
        |ops: &Ops, input: &Ops::Output| {
            ops.get_number_value(input)
                .map_owned(|n| MapItemColor::new(n.int_value()))
        },
    )
}

// ---------------------------------------------------------------------------
// MapPostProcessing
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.MapPostProcessing`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum MapPostProcessing {
    Lock = 0,
    Scale = 1,
}

impl MapPostProcessing {
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// `MapPostProcessing.ID_MAP` with `ByIdMap.OutOfBoundsStrategy.ZERO`.
    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::Scale,
            _ => Self::Lock,
        }
    }
}

// ---------------------------------------------------------------------------
// MapDecorations
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.MapDecorations.Entry`.
#[derive(Clone, Debug, PartialEq)]
pub struct MapDecorationEntry {
    pub decoration_type: Identifier,
    pub x: f64,
    pub z: f64,
    pub rotation: f32,
}

impl MapDecorationEntry {
    pub fn new(decoration_type: Identifier, x: f64, z: f64, rotation: f32) -> Self {
        Self {
            decoration_type,
            x,
            z,
            rotation,
        }
    }
}

/// `MapDecorations.Entry.CODEC`.
pub fn map_decoration_entry_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<MapDecorationEntry, Ops>> {
    codec_from_fns(
        "map_decoration_entry",
        |v: &MapDecorationEntry, ops: &Ops| {
            let entries = vec![
                rivet_serialization::Pair::of(
                    ops.create_string("type".to_string()),
                    encode_identifier(&v.decoration_type, ops),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("x".to_string()),
                    ops.create_double(v.x),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("z".to_string()),
                    ops.create_double(v.z),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("rotation".to_string()),
                    ops.create_float(v.rotation),
                ),
            ];
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("MapDecorations.Entry must be a map");
            };
            let Some(type_in) = get_map_field(ops, map.as_ref(), "type") else {
                return DataResult::error("MapDecorations.Entry missing required field 'type'");
            };
            let decoration_type = match decode_identifier(ops, &type_in).result().cloned() {
                Some(id) => id,
                None => {
                    return DataResult::error("Invalid 'type' identifier in MapDecorations.Entry");
                }
            };
            let Some(x_in) = get_map_field(ops, map.as_ref(), "x") else {
                return DataResult::error("MapDecorations.Entry missing required field 'x'");
            };
            let Some(x) = ops
                .get_number_value(&x_in)
                .result()
                .map(|n| n.double_value())
            else {
                return DataResult::error("MapDecorations.Entry 'x' must be a number");
            };
            let Some(z_in) = get_map_field(ops, map.as_ref(), "z") else {
                return DataResult::error("MapDecorations.Entry missing required field 'z'");
            };
            let Some(z) = ops
                .get_number_value(&z_in)
                .result()
                .map(|n| n.double_value())
            else {
                return DataResult::error("MapDecorations.Entry 'z' must be a number");
            };
            let Some(rot_in) = get_map_field(ops, map.as_ref(), "rotation") else {
                return DataResult::error("MapDecorations.Entry missing required field 'rotation'");
            };
            let Some(rotation) = ops
                .get_number_value(&rot_in)
                .result()
                .map(|n| n.float_value())
            else {
                return DataResult::error("MapDecorations.Entry 'rotation' must be a number");
            };
            DataResult::success(MapDecorationEntry {
                decoration_type,
                x,
                z,
                rotation,
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.MapDecorations`.
///
/// Uses `IndexMap<String, MapDecorationEntry>` to preserve deterministic
/// insertion order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MapDecorations {
    pub decorations: IndexMap<String, MapDecorationEntry>,
}

impl MapDecorations {
    /// `MapDecorations.EMPTY`.
    pub fn empty() -> Self {
        Self {
            decorations: IndexMap::new(),
        }
    }

    pub fn new(decorations: IndexMap<String, MapDecorationEntry>) -> Self {
        Self { decorations }
    }

    /// `MapDecorations.withDecoration(String, Entry)`.
    pub fn with_decoration(&self, id: String, entry: MapDecorationEntry) -> Self {
        let mut next = self.decorations.clone();
        next.insert(id, entry);
        Self { decorations: next }
    }
}

/// `MapDecorations.CODEC = Codec.unboundedMap(Codec.STRING, Entry.CODEC).xmap(...)`.
pub fn map_decorations_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<MapDecorations, Ops>> {
    let entry_codec = map_decoration_entry_codec::<Ops>();
    let enc_entry = Arc::clone(&entry_codec);
    let dec_entry = entry_codec;
    codec_from_fns(
        "map_decorations",
        move |v: &MapDecorations, ops: &Ops| {
            let mut entries = Vec::with_capacity(v.decorations.len());
            for (k, val) in &v.decorations {
                let r = enc_entry.encode_start(ops, val);
                match r.result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string(k.clone()),
                        ev,
                    )),
                    None => {
                        return DataResult::error(
                            r.error_ref()
                                .map(|e| e.message().to_string())
                                .unwrap_or_else(|| {
                                    "Failed to encode MapDecorations entry".to_string()
                                }),
                        );
                    }
                }
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("MapDecorations must be a map");
            };
            let mut decorations = IndexMap::new();
            for pair in map.entries() {
                let Some(key) = ops.get_string_value(&pair.first).result().cloned() else {
                    return DataResult::error("MapDecorations keys must be strings");
                };
                let r = dec_entry.parse(ops, &pair.second);
                match r.result().cloned() {
                    Some(entry) => {
                        decorations.insert(key, entry);
                    }
                    None => {
                        return DataResult::error(
                            r.error_ref()
                                .map(|e| e.message().to_string())
                                .unwrap_or_else(|| "Invalid entry in MapDecorations".to_string()),
                        );
                    }
                }
            }
            DataResult::success(MapDecorations { decorations })
        },
    )
}
