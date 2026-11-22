//! World-state, book, firework, attribute-modifier, and miscellaneous leaf item components:
//! - `LodestoneTracker` (`net.minecraft.world.item.component.LodestoneTracker`)
//! - `SeededContainerLoot` (`net.minecraft.world.item.component.SeededContainerLoot`)
//! - `BlockItemStateProperties` (`net.minecraft.world.item.component.BlockItemStateProperties`)
//! - `WritableBookContent` (`net.minecraft.world.item.component.WritableBookContent`)
//! - `WrittenBookContent` (`net.minecraft.world.item.component.WrittenBookContent`)
//! - `FireworkShape`, `FireworkExplosion`, `Fireworks` (`...FireworkExplosion`, `...Fireworks`)
//! - `ItemAttributeModifiers` (`net.minecraft.world.item.component.ItemAttributeModifiers`)
//! - `SulfurCubeContent` (`net.minecraft.world.item.component.SulfurCubeContent`)
//! - `InstrumentComponent` (`net.minecraft.world.item.component.InstrumentComponent`)
//! - `ProvidesTrimMaterial` (`net.minecraft.world.item.component.ProvidesTrimMaterial`)

use crate::enchantment::{EquipmentSlot, EquipmentSlotGroup};
use crate::item::codec_from_fns;
use crate::item::components::core::{Filterable, filterable_codec};
use crate::item::components::{
    decode_identifier, encode_identifier, get_map_field, validate_int_range, validate_string_len,
};
use crate::item::recursive_components::{ItemStackTemplate, item_stack_template_codec};
use indexmap::IndexMap;
use rivet_registry::Identifier;
use rivet_registry::core::{GlobalPos, global_pos_codec};
use rivet_serialization::{Codec, DataResult, DynamicOps};
use rivet_text::component::Component;
use rivet_text::component_serialization;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// LodestoneTracker
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.LodestoneTracker`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LodestoneTracker {
    pub target: Option<GlobalPos>,
    pub tracked: bool,
}

impl LodestoneTracker {
    pub fn new(target: Option<GlobalPos>, tracked: bool) -> Self {
        Self { target, tracked }
    }
}

impl Default for LodestoneTracker {
    fn default() -> Self {
        Self {
            target: None,
            tracked: true,
        }
    }
}

/// `LodestoneTracker.CODEC`.
pub fn lodestone_tracker_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<LodestoneTracker, Ops>>
{
    let gp = global_pos_codec::<Ops>();
    let enc_gp = Arc::clone(&gp);
    let dec_gp = gp;
    codec_from_fns(
        "lodestone_tracker",
        move |v: &LodestoneTracker, ops: &Ops| {
            let mut entries = Vec::new();
            if let Some(target) = &v.target {
                match enc_gp.encode_start(ops, target).result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("target".to_string()),
                        ev,
                    )),
                    None => return DataResult::error("Failed to encode LodestoneTracker.target"),
                }
            }
            if !v.tracked {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("tracked".to_string()),
                    ops.create_boolean(false),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("LodestoneTracker must be a map");
            };
            let target = if let Some(val) = get_map_field(ops, map.as_ref(), "target") {
                match dec_gp.parse(ops, &val).result().cloned() {
                    Some(gp) => Some(gp),
                    None => return DataResult::error("Invalid LodestoneTracker.target"),
                }
            } else {
                None
            };
            let tracked = if let Some(val) = get_map_field(ops, map.as_ref(), "tracked") {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => b,
                    None => return DataResult::error("LodestoneTracker.tracked must be a boolean"),
                }
            } else {
                true
            };
            DataResult::success(LodestoneTracker { target, tracked })
        },
    )
}

// ---------------------------------------------------------------------------
// SeededContainerLoot
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.SeededContainerLoot`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SeededContainerLoot {
    pub loot_table: Identifier,
    pub seed: i64,
}

impl SeededContainerLoot {
    pub fn new(loot_table: Identifier, seed: i64) -> Self {
        Self { loot_table, seed }
    }
}

/// `SeededContainerLoot.CODEC`.
pub fn seeded_container_loot_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SeededContainerLoot, Ops>> {
    codec_from_fns(
        "seeded_container_loot",
        |v: &SeededContainerLoot, ops: &Ops| {
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("loot_table".to_string()),
                encode_identifier(&v.loot_table, ops),
            )];
            if v.seed != 0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("seed".to_string()),
                    ops.create_long(v.seed),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("SeededContainerLoot must be a map");
            };
            let Some(lt_in) = get_map_field(ops, map.as_ref(), "loot_table") else {
                return DataResult::error(
                    "SeededContainerLoot missing required field 'loot_table'",
                );
            };
            let Some(loot_table) = decode_identifier(ops, &lt_in).result().cloned() else {
                return DataResult::error("Invalid SeededContainerLoot.loot_table");
            };
            let seed = if let Some(s_in) = get_map_field(ops, map.as_ref(), "seed") {
                match ops.get_number_value(&s_in).result().map(|n| n.long_value()) {
                    Some(s) => s,
                    None => return DataResult::error("SeededContainerLoot.seed must be a number"),
                }
            } else {
                0
            };
            DataResult::success(SeededContainerLoot { loot_table, seed })
        },
    )
}

// ---------------------------------------------------------------------------
// BlockItemStateProperties
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.BlockItemStateProperties`.
///
/// Uses `IndexMap<String, String>` to preserve deterministic insertion order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockItemStateProperties {
    pub properties: IndexMap<String, String>,
}

impl BlockItemStateProperties {
    pub fn empty() -> Self {
        Self {
            properties: IndexMap::new(),
        }
    }

    pub fn new(properties: IndexMap<String, String>) -> Self {
        Self { properties }
    }

    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    /// `BlockItemStateProperties.with(Property<T>, T)` — copies and inserts `name -> value`.
    pub fn with_property(&self, name: impl Into<String>, value: impl Into<String>) -> Self {
        let mut next = self.properties.clone();
        next.insert(name.into(), value.into());
        Self { properties: next }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.properties.get(name).map(String::as_str)
    }
}

/// `BlockItemStateProperties.CODEC = Codec.unboundedMap(Codec.STRING, Codec.STRING).xmap(...)`.
pub fn block_item_state_properties_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<BlockItemStateProperties, Ops>> {
    codec_from_fns(
        "block_item_state_properties",
        |v: &BlockItemStateProperties, ops: &Ops| {
            let entries: Vec<rivet_serialization::Pair<Ops::Output, Ops::Output>> = v
                .properties
                .iter()
                .map(|(k, val)| {
                    rivet_serialization::Pair::of(
                        ops.create_string(k.clone()),
                        ops.create_string(val.clone()),
                    )
                })
                .collect();
            DataResult::success(ops.create_map(entries))
        },
        |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("BlockItemStateProperties must be a map");
            };
            let mut properties = IndexMap::new();
            for pair in map.entries() {
                let Some(ks) = ops.get_string_value(&pair.first).result().cloned() else {
                    return DataResult::error("BlockItemStateProperties keys must be strings");
                };
                let Some(vs) = ops.get_string_value(&pair.second).result().cloned() else {
                    return DataResult::error("BlockItemStateProperties values must be strings");
                };
                properties.insert(ks, vs);
            }
            DataResult::success(BlockItemStateProperties { properties })
        },
    )
}

// ---------------------------------------------------------------------------
// WritableBookContent & WrittenBookContent
// ---------------------------------------------------------------------------

/// `WritableBookContent.PAGE_EDIT_LENGTH = 1024`.
pub const BOOK_PAGE_EDIT_LENGTH: usize = 1024;
/// `WritableBookContent.MAX_PAGES = 100`.
pub const WRITABLE_BOOK_MAX_PAGES: usize = 100;
/// `WrittenBookContent.PAGE_LENGTH = 32767`.
pub const BOOK_PAGE_LENGTH: usize = 32767;
/// `WrittenBookContent.TITLE_LENGTH = 16`.
pub const BOOK_TITLE_LENGTH: usize = 16;
/// `WrittenBookContent.TITLE_MAX_LENGTH = 32`.
pub const BOOK_TITLE_MAX_LENGTH: usize = 32;
/// `WrittenBookContent.MAX_GENERATION = 3`.
pub const BOOK_MAX_GENERATION: i32 = 3;

/// Port of `net.minecraft.world.item.component.WritableBookContent`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct WritableBookContent {
    pages: Vec<Filterable<String>>,
}

impl WritableBookContent {
    pub fn empty() -> Self {
        Self { pages: Vec::new() }
    }

    /// `new WritableBookContent(List<Filterable<String>>)` — rejects > 100 pages
    /// or any page exceeding 1024 UTF-16 code units.
    pub fn try_new(pages: Vec<Filterable<String>>) -> Result<Self, String> {
        if pages.len() > WRITABLE_BOOK_MAX_PAGES {
            return Err(format!(
                "Got {} pages, but maximum is {WRITABLE_BOOK_MAX_PAGES}",
                pages.len()
            ));
        }
        for page in &pages {
            validate_string_len(
                &page.raw,
                0,
                BOOK_PAGE_EDIT_LENGTH,
                "WritableBookContent.page",
            )?;
            if let Some(f) = &page.filtered {
                validate_string_len(f, 0, BOOK_PAGE_EDIT_LENGTH, "WritableBookContent.page")?;
            }
        }
        Ok(Self { pages })
    }

    pub fn pages(&self) -> &[Filterable<String>] {
        &self.pages
    }

    /// `WritableBookContent.getPages(boolean filtered)`.
    pub fn get_pages(&self, filtered: bool) -> Vec<&str> {
        self.pages
            .iter()
            .map(|p| p.get(filtered).as_str())
            .collect()
    }

    /// `WritableBookContent.withReplacedPages(List<Filterable<String>>)`.
    pub fn with_replaced_pages(&self, pages: Vec<Filterable<String>>) -> Result<Self, String> {
        Self::try_new(pages)
    }
}

/// `WritableBookContent.CODEC`.
pub fn writable_book_content_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<WritableBookContent, Ops>> {
    let page_str_codec = codec_from_fns(
        "writable_book_page_str",
        |s: &String, ops: &Ops| match validate_string_len(
            s,
            0,
            BOOK_PAGE_EDIT_LENGTH,
            "WritableBookContent.page",
        ) {
            Ok(()) => DataResult::success(ops.create_string(s.clone())),
            Err(e) => DataResult::error(e),
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_string_value(input).flat_map(|s| {
                match validate_string_len(&s, 0, BOOK_PAGE_EDIT_LENGTH, "WritableBookContent.page")
                {
                    Ok(()) => DataResult::success(s),
                    Err(e) => DataResult::error(e),
                }
            })
        },
    );
    let page_codec = filterable_codec(page_str_codec);
    let enc_page = Arc::clone(&page_codec);
    let dec_page = page_codec;
    codec_from_fns(
        "writable_book_content",
        move |v: &WritableBookContent, ops: &Ops| {
            let mut entries = Vec::new();
            if !v.pages.is_empty() {
                let mut list = Vec::with_capacity(v.pages.len());
                for p in &v.pages {
                    match enc_page.encode_start(ops, p).result().cloned() {
                        Some(ev) => list.push(ev),
                        None => {
                            return DataResult::error("Failed to encode WritableBookContent.pages");
                        }
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("pages".to_string()),
                    ops.create_list(list),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("WritableBookContent must be a map");
            };
            let mut pages = Vec::new();
            if let Some(val) = get_map_field(ops, map.as_ref(), "pages") {
                let Some(items) = ops.get_stream(&val).result().cloned() else {
                    return DataResult::error("WritableBookContent.pages must be a list");
                };
                if items.len() > WRITABLE_BOOK_MAX_PAGES {
                    return DataResult::error(format!(
                        "WritableBookContent has {} pages, max is {WRITABLE_BOOK_MAX_PAGES}",
                        items.len()
                    ));
                }
                for item in &items {
                    let r = dec_page.parse(ops, item);
                    match r.result().cloned() {
                        Some(p) => pages.push(p),
                        None => {
                            return DataResult::error(
                                r.error_ref()
                                    .map(|e| e.message().to_string())
                                    .unwrap_or_else(|| {
                                        "Invalid page in WritableBookContent".to_string()
                                    }),
                            );
                        }
                    }
                }
            }
            DataResult::success(WritableBookContent { pages })
        },
    )
}

/// Port of `net.minecraft.world.item.component.WrittenBookContent`.
#[derive(Clone, Debug, PartialEq)]
pub struct WrittenBookContent {
    pub title: Filterable<String>,
    pub author: String,
    pub generation: i32,
    pub pages: Vec<Filterable<Component>>,
    pub resolved: bool,
}

impl WrittenBookContent {
    pub fn empty() -> Self {
        Self {
            title: Filterable::pass_through(String::new()),
            author: String::new(),
            generation: 0,
            pages: Vec::new(),
            resolved: true,
        }
    }

    pub fn try_new(
        title: Filterable<String>,
        author: String,
        generation: i32,
        pages: Vec<Filterable<Component>>,
        resolved: bool,
    ) -> Result<Self, String> {
        if !(0..=BOOK_MAX_GENERATION).contains(&generation) {
            return Err(format!(
                "Generation was {generation}, but must be between 0 and {BOOK_MAX_GENERATION}"
            ));
        }
        validate_string_len(
            &title.raw,
            0,
            BOOK_TITLE_MAX_LENGTH,
            "WrittenBookContent.title",
        )?;
        if let Some(f) = &title.filtered {
            validate_string_len(f, 0, BOOK_TITLE_MAX_LENGTH, "WrittenBookContent.title")?;
        }
        Ok(Self {
            title,
            author,
            generation,
            pages,
            resolved,
        })
    }

    /// `WrittenBookContent.tryCraftCopy()` — returns `None` if `generation >= 2`,
    /// otherwise a copy with `generation + 1`.
    pub fn try_craft_copy(&self) -> Option<Self> {
        if self.generation >= 2 {
            None
        } else {
            Some(Self {
                title: self.title.clone(),
                author: self.author.clone(),
                generation: self.generation + 1,
                pages: self.pages.clone(),
                resolved: self.resolved,
            })
        }
    }

    /// `WrittenBookContent.markResolved()`.
    pub fn mark_resolved(&self) -> Self {
        Self {
            title: self.title.clone(),
            author: self.author.clone(),
            generation: self.generation,
            pages: self.pages.clone(),
            resolved: true,
        }
    }

    /// `WrittenBookContent.getPages(boolean filtered)`.
    pub fn get_pages(&self, filtered: bool) -> Vec<&Component> {
        self.pages.iter().map(|p| p.get(filtered)).collect()
    }

    /// `WrittenBookContent.withReplacedPages(List<Filterable<Component>>)`.
    pub fn with_replaced_pages(&self, pages: Vec<Filterable<Component>>) -> Self {
        Self {
            title: self.title.clone(),
            author: self.author.clone(),
            generation: self.generation,
            pages,
            resolved: self.resolved,
        }
    }
}

/// `WrittenBookContent.CODEC`.
pub fn written_book_content_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<WrittenBookContent, Ops>> {
    let title_str_codec = codec_from_fns(
        "written_book_title_str",
        |s: &String, ops: &Ops| match validate_string_len(
            s,
            0,
            BOOK_TITLE_MAX_LENGTH,
            "WrittenBookContent.title",
        ) {
            Ok(()) => DataResult::success(ops.create_string(s.clone())),
            Err(e) => DataResult::error(e),
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_string_value(input).flat_map(|s| {
                match validate_string_len(&s, 0, BOOK_TITLE_MAX_LENGTH, "WrittenBookContent.title")
                {
                    Ok(()) => DataResult::success(s),
                    Err(e) => DataResult::error(e),
                }
            })
        },
    );
    let title_codec = filterable_codec(title_str_codec);
    let page_codec = filterable_codec(component_serialization::codec::<Ops>());
    let enc_title = Arc::clone(&title_codec);
    let dec_title = title_codec;
    let enc_page = Arc::clone(&page_codec);
    let dec_page = page_codec;
    codec_from_fns(
        "written_book_content",
        move |v: &WrittenBookContent, ops: &Ops| {
            let Some(title_val) = enc_title.encode_start(ops, &v.title).result().cloned() else {
                return DataResult::error("Failed to encode WrittenBookContent.title");
            };
            let mut entries = vec![
                rivet_serialization::Pair::of(ops.create_string("title".to_string()), title_val),
                rivet_serialization::Pair::of(
                    ops.create_string("author".to_string()),
                    ops.create_string(v.author.clone()),
                ),
            ];
            if v.generation != 0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("generation".to_string()),
                    ops.create_int(v.generation),
                ));
            }
            if !v.pages.is_empty() {
                let mut list = Vec::with_capacity(v.pages.len());
                for p in &v.pages {
                    match enc_page.encode_start(ops, p).result().cloned() {
                        Some(ev) => list.push(ev),
                        None => {
                            return DataResult::error("Failed to encode WrittenBookContent.pages");
                        }
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("pages".to_string()),
                    ops.create_list(list),
                ));
            }
            if v.resolved {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("resolved".to_string()),
                    ops.create_boolean(true),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("WrittenBookContent must be a map");
            };
            let Some(title_in) = get_map_field(ops, map.as_ref(), "title") else {
                return DataResult::error("WrittenBookContent missing required field 'title'");
            };
            let title_res = dec_title.parse(ops, &title_in);
            let Some(title) = title_res.result().cloned() else {
                return DataResult::error(
                    title_res
                        .error_ref()
                        .map(|e| e.message().to_string())
                        .unwrap_or_else(|| "Invalid WrittenBookContent.title".to_string()),
                );
            };
            let Some(author_in) = get_map_field(ops, map.as_ref(), "author") else {
                return DataResult::error("WrittenBookContent missing required field 'author'");
            };
            let Some(author) = ops.get_string_value(&author_in).result().cloned() else {
                return DataResult::error("WrittenBookContent.author must be a string");
            };
            let generation = if let Some(g_in) = get_map_field(ops, map.as_ref(), "generation") {
                let Some(g) = ops.get_number_value(&g_in).result().map(|n| n.int_value()) else {
                    return DataResult::error("WrittenBookContent.generation must be a number");
                };
                if let Some(err) =
                    validate_int_range(g, 0, BOOK_MAX_GENERATION, "generation").error_ref()
                {
                    return DataResult::error(err.message().to_string());
                }
                g
            } else {
                0
            };
            let mut pages = Vec::new();
            if let Some(p_in) = get_map_field(ops, map.as_ref(), "pages") {
                let Some(items) = ops.get_stream(&p_in).result().cloned() else {
                    return DataResult::error("WrittenBookContent.pages must be a list");
                };
                for item in &items {
                    match dec_page.parse(ops, item).result().cloned() {
                        Some(p) => pages.push(p),
                        None => {
                            return DataResult::error("Invalid page in WrittenBookContent.pages");
                        }
                    }
                }
            }
            let resolved = if let Some(r_in) = get_map_field(ops, map.as_ref(), "resolved") {
                match ops.get_boolean_value(&r_in).result().copied() {
                    Some(b) => b,
                    None => {
                        return DataResult::error("WrittenBookContent.resolved must be a boolean");
                    }
                }
            } else {
                false
            };
            DataResult::success(WrittenBookContent {
                title,
                author,
                generation,
                pages,
                resolved,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// FireworkShape, FireworkExplosion, Fireworks
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.FireworkExplosion.Shape`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum FireworkShape {
    SmallBall = 0,
    LargeBall = 1,
    Star = 2,
    Creeper = 3,
    Burst = 4,
}

impl FireworkShape {
    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::SmallBall => "small_ball",
            Self::LargeBall => "large_ball",
            Self::Star => "star",
            Self::Creeper => "creeper",
            Self::Burst => "burst",
        }
    }

    pub fn from_serialized_name(s: &str) -> Option<Self> {
        match s {
            "small_ball" => Some(Self::SmallBall),
            "large_ball" => Some(Self::LargeBall),
            "star" => Some(Self::Star),
            "creeper" => Some(Self::Creeper),
            "burst" => Some(Self::Burst),
            _ => None,
        }
    }

    pub const fn by_id(id: i32) -> Self {
        match id {
            1 => Self::LargeBall,
            2 => Self::Star,
            3 => Self::Creeper,
            4 => Self::Burst,
            _ => Self::SmallBall,
        }
    }
}

/// `FireworkExplosion.Shape.CODEC`.
pub fn firework_shape_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<FireworkShape, Ops>> {
    codec_from_fns(
        "firework_shape",
        |v: &FireworkShape, ops: &Ops| {
            DataResult::success(ops.create_string(v.serialized_name().to_string()))
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_string_value(input).flat_map(|s| {
                FireworkShape::from_serialized_name(&s)
                    .map(DataResult::success)
                    .unwrap_or_else(|| DataResult::error(format!("Unknown FireworkShape: {s}")))
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.FireworkExplosion`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FireworkExplosion {
    pub shape: FireworkShape,
    pub colors: Vec<i32>,
    pub fade_colors: Vec<i32>,
    pub has_trail: bool,
    pub has_twinkle: bool,
}

impl FireworkExplosion {
    pub fn default_value() -> Self {
        Self {
            shape: FireworkShape::SmallBall,
            colors: Vec::new(),
            fade_colors: Vec::new(),
            has_trail: false,
            has_twinkle: false,
        }
    }

    /// `FireworkExplosion.withFadeColors(IntList)`.
    pub fn with_fade_colors(&self, fade_colors: Vec<i32>) -> Self {
        Self {
            shape: self.shape,
            colors: self.colors.clone(),
            fade_colors,
            has_trail: self.has_trail,
            has_twinkle: self.has_twinkle,
        }
    }
}

impl Default for FireworkExplosion {
    fn default() -> Self {
        Self::default_value()
    }
}

/// `FireworkExplosion.CODEC`.
pub fn firework_explosion_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<FireworkExplosion, Ops>> {
    let sc = firework_shape_codec::<Ops>();
    let enc_s = Arc::clone(&sc);
    let dec_s = sc;
    codec_from_fns(
        "firework_explosion",
        move |v: &FireworkExplosion, ops: &Ops| {
            let Some(shape_val) = enc_s.encode_start(ops, &v.shape).result().cloned() else {
                return DataResult::error("Failed to encode FireworkExplosion.shape");
            };
            let mut entries = vec![rivet_serialization::Pair::of(
                ops.create_string("shape".to_string()),
                shape_val,
            )];
            if !v.colors.is_empty() {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("colors".to_string()),
                    ops.create_int_list(v.colors.clone()),
                ));
            }
            if !v.fade_colors.is_empty() {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("fade_colors".to_string()),
                    ops.create_int_list(v.fade_colors.clone()),
                ));
            }
            if v.has_trail {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("has_trail".to_string()),
                    ops.create_boolean(true),
                ));
            }
            if v.has_twinkle {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("has_twinkle".to_string()),
                    ops.create_boolean(true),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("FireworkExplosion must be a map");
            };
            let Some(s_in) = get_map_field(ops, map.as_ref(), "shape") else {
                return DataResult::error("FireworkExplosion missing required field 'shape'");
            };
            let Some(shape) = dec_s.parse(ops, &s_in).result().copied() else {
                return DataResult::error("Invalid FireworkExplosion.shape");
            };
            let parse_int_list = |field: &str| -> Result<Vec<i32>, String> {
                match get_map_field(ops, map.as_ref(), field) {
                    Some(val) => {
                        if let Some(ints) = ops.get_int_stream(&val).result() {
                            return Ok(ints.clone());
                        }
                        let items = ops.get_stream(&val).result().cloned().ok_or_else(|| {
                            format!("FireworkExplosion.{field} must be an int list")
                        })?;
                        let mut out = Vec::with_capacity(items.len());
                        for item in &items {
                            let n =
                                ops.get_number_value(item)
                                    .result()
                                    .copied()
                                    .ok_or_else(|| {
                                        format!(
                                            "FireworkExplosion.{field} elements must be integers"
                                        )
                                    })?;
                            out.push(n.int_value());
                        }
                        Ok(out)
                    }
                    None => Ok(Vec::new()),
                }
            };
            let colors = match parse_int_list("colors") {
                Ok(c) => c,
                Err(e) => return DataResult::error(e),
            };
            let fade_colors = match parse_int_list("fade_colors") {
                Ok(c) => c,
                Err(e) => return DataResult::error(e),
            };
            let has_trail = if let Some(val) = get_map_field(ops, map.as_ref(), "has_trail") {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => b,
                    None => {
                        return DataResult::error("FireworkExplosion.has_trail must be a boolean");
                    }
                }
            } else {
                false
            };
            let has_twinkle = if let Some(val) = get_map_field(ops, map.as_ref(), "has_twinkle") {
                match ops.get_boolean_value(&val).result().copied() {
                    Some(b) => b,
                    None => {
                        return DataResult::error(
                            "FireworkExplosion.has_twinkle must be a boolean",
                        );
                    }
                }
            } else {
                false
            };
            DataResult::success(FireworkExplosion {
                shape,
                colors,
                fade_colors,
                has_trail,
                has_twinkle,
            })
        },
    )
}

/// `Fireworks.MAX_EXPLOSIONS = 256`.
pub const FIREWORKS_MAX_EXPLOSIONS: usize = 256;

/// Port of `net.minecraft.world.item.component.Fireworks`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fireworks {
    pub flight_duration: u8,
    pub explosions: Vec<FireworkExplosion>,
}

impl Fireworks {
    pub fn try_new(
        flight_duration: u8,
        explosions: Vec<FireworkExplosion>,
    ) -> Result<Self, String> {
        if explosions.len() > FIREWORKS_MAX_EXPLOSIONS {
            return Err(format!(
                "Got {} explosions, but maximum is {FIREWORKS_MAX_EXPLOSIONS}",
                explosions.len()
            ));
        }
        Ok(Self {
            flight_duration,
            explosions,
        })
    }
}

/// `Fireworks.CODEC`.
pub fn fireworks_codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Fireworks, Ops>> {
    let exp = firework_explosion_codec::<Ops>();
    let enc_e = Arc::clone(&exp);
    let dec_e = exp;
    codec_from_fns(
        "fireworks",
        move |v: &Fireworks, ops: &Ops| {
            if v.explosions.len() > FIREWORKS_MAX_EXPLOSIONS {
                return DataResult::error(format!(
                    "Fireworks exceeds maximum of {FIREWORKS_MAX_EXPLOSIONS} explosions: {}",
                    v.explosions.len()
                ));
            }
            let mut entries = Vec::new();
            if v.flight_duration != 0 {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("flight_duration".to_string()),
                    ops.create_byte(v.flight_duration as i8),
                ));
            }
            if !v.explosions.is_empty() {
                let mut list = Vec::with_capacity(v.explosions.len());
                for e in &v.explosions {
                    match enc_e.encode_start(ops, e).result().cloned() {
                        Some(ev) => list.push(ev),
                        None => return DataResult::error("Failed to encode Fireworks.explosions"),
                    }
                }
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("explosions".to_string()),
                    ops.create_list(list),
                ));
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("Fireworks must be a map");
            };
            let flight_duration =
                if let Some(fd_in) = get_map_field(ops, map.as_ref(), "flight_duration") {
                    let n_res = ops.get_number_value(&fd_in);
                    let Some(n) = n_res.result() else {
                        return DataResult::error("Fireworks.flight_duration must be a number");
                    };
                    let raw = n.int_value();
                    // ExtraCodecs.UNSIGNED_BYTE accepts signed byte (-128..=127 -> unsigned 0..=255)
                    // or unsigned integer 0..=255; reject > 255 or < -128.
                    if !(-128..=255).contains(&raw) {
                        return DataResult::error(format!(
                            "Unsigned byte out of range for flight_duration: {raw}"
                        ));
                    }
                    (raw as i8) as u8
                } else {
                    0
                };
            let mut explosions = Vec::new();
            if let Some(ex_in) = get_map_field(ops, map.as_ref(), "explosions") {
                let Some(items) = ops.get_stream(&ex_in).result().cloned() else {
                    return DataResult::error("Fireworks.explosions must be a list");
                };
                if items.len() > FIREWORKS_MAX_EXPLOSIONS {
                    return DataResult::error(format!(
                        "Fireworks has {} explosions, max is {FIREWORKS_MAX_EXPLOSIONS}",
                        items.len()
                    ));
                }
                for item in &items {
                    match dec_e.parse(ops, item).result().cloned() {
                        Some(e) => explosions.push(e),
                        None => return DataResult::error("Invalid entry in Fireworks.explosions"),
                    }
                }
            }
            DataResult::success(Fireworks {
                flight_duration,
                explosions,
            })
        },
    )
}

// ---------------------------------------------------------------------------
// ItemAttributeModifiers
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.entity.ai.attributes.AttributeModifier.Operation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum AttributeOperation {
    AddValue = 0,
    AddMultipliedBase = 1,
    AddMultipliedTotal = 2,
}

impl AttributeOperation {
    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Self::AddValue => "add_value",
            Self::AddMultipliedBase => "add_multiplied_base",
            Self::AddMultipliedTotal => "add_multiplied_total",
        }
    }

    pub fn from_serialized_name(s: &str) -> Option<Self> {
        match s {
            "add_value" => Some(Self::AddValue),
            "add_multiplied_base" => Some(Self::AddMultipliedBase),
            "add_multiplied_total" => Some(Self::AddMultipliedTotal),
            _ => None,
        }
    }
}

/// `AttributeModifier.Operation.CODEC`.
pub fn attribute_operation_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<AttributeOperation, Ops>> {
    codec_from_fns(
        "attribute_operation",
        |v: &AttributeOperation, ops: &Ops| {
            DataResult::success(ops.create_string(v.serialized_name().to_string()))
        },
        |ops: &Ops, input: &Ops::Output| {
            ops.get_string_value(input).flat_map(|s| {
                AttributeOperation::from_serialized_name(&s)
                    .map(DataResult::success)
                    .unwrap_or_else(|| {
                        DataResult::error(format!("Unknown AttributeModifier.Operation: {s}"))
                    })
            })
        },
    )
}

/// Port of `net.minecraft.world.entity.ai.attributes.AttributeModifier`.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeModifierSpec {
    pub id: Identifier,
    pub amount: f64,
    pub operation: AttributeOperation,
}

impl AttributeModifierSpec {
    pub fn new(id: Identifier, amount: f64, operation: AttributeOperation) -> Self {
        Self {
            id,
            amount,
            operation,
        }
    }
}

/// Port of `net.minecraft.world.item.component.ItemAttributeModifiers.Display`.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum AttributeModifierDisplay {
    #[default]
    Default,
    Hidden,
    OverrideText(Box<Component>),
}

/// `ItemAttributeModifiers.Display.CODEC`.
pub fn attribute_modifier_display_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<AttributeModifierDisplay, Ops>> {
    let comp = component_serialization::codec::<Ops>();
    let enc_c = Arc::clone(&comp);
    let dec_c = comp;
    codec_from_fns(
        "attribute_modifier_display",
        move |v: &AttributeModifierDisplay, ops: &Ops| match v {
            AttributeModifierDisplay::Default => {
                DataResult::success(ops.create_map(vec![rivet_serialization::Pair::of(
                    ops.create_string("type".to_string()),
                    ops.create_string("default".to_string()),
                )]))
            }
            AttributeModifierDisplay::Hidden => {
                DataResult::success(ops.create_map(vec![rivet_serialization::Pair::of(
                    ops.create_string("type".to_string()),
                    ops.create_string("hidden".to_string()),
                )]))
            }
            AttributeModifierDisplay::OverrideText(c) => {
                let Some(val) = enc_c.encode_start(ops, c).result().cloned() else {
                    return DataResult::error("Failed to encode Display.OverrideText.value");
                };
                DataResult::success(ops.create_map(vec![
                    rivet_serialization::Pair::of(
                        ops.create_string("type".to_string()),
                        ops.create_string("override".to_string()),
                    ),
                    rivet_serialization::Pair::of(ops.create_string("value".to_string()), val),
                ]))
            }
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("ItemAttributeModifiers.Display must be a map");
            };
            let Some(ty_in) = get_map_field(ops, map.as_ref(), "type") else {
                return DataResult::error("ItemAttributeModifiers.Display missing 'type'");
            };
            let ty_res = ops.get_string_value(&ty_in);
            let Some(ty) = ty_res.result() else {
                return DataResult::error("ItemAttributeModifiers.Display.type must be a string");
            };
            match ty.as_str() {
                "default" => DataResult::success(AttributeModifierDisplay::Default),
                "hidden" => DataResult::success(AttributeModifierDisplay::Hidden),
                "override" => {
                    let Some(val_in) = get_map_field(ops, map.as_ref(), "value") else {
                        return DataResult::error("Display override missing 'value'");
                    };
                    dec_c
                        .parse(ops, &val_in)
                        .map_owned(|c| AttributeModifierDisplay::OverrideText(Box::new(c)))
                }
                other => DataResult::error(format!(
                    "Unknown ItemAttributeModifiers.Display type: {other}"
                )),
            }
        },
    )
}

/// Port of `net.minecraft.world.item.component.ItemAttributeModifiers.Entry`.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeModifierEntry {
    pub attribute: Identifier,
    pub modifier: AttributeModifierSpec,
    pub slot: EquipmentSlotGroup,
    pub display: AttributeModifierDisplay,
}

impl AttributeModifierEntry {
    pub fn new(
        attribute: Identifier,
        modifier: AttributeModifierSpec,
        slot: EquipmentSlotGroup,
    ) -> Self {
        Self {
            attribute,
            modifier,
            slot,
            display: AttributeModifierDisplay::Default,
        }
    }

    pub fn matches(&self, attribute: &Identifier, id: &Identifier) -> bool {
        &self.attribute == attribute && &self.modifier.id == id
    }
}

/// `ItemAttributeModifiers.Entry.CODEC`.
pub fn attribute_modifier_entry_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<AttributeModifierEntry, Ops>> {
    let op_c = attribute_operation_codec::<Ops>();
    let disp_c = attribute_modifier_display_codec::<Ops>();
    let enc_op = Arc::clone(&op_c);
    let dec_op = op_c;
    let enc_disp = Arc::clone(&disp_c);
    let dec_disp = disp_c;
    codec_from_fns(
        "attribute_modifier_entry",
        move |v: &AttributeModifierEntry, ops: &Ops| {
            let Some(op_val) = enc_op
                .encode_start(ops, &v.modifier.operation)
                .result()
                .cloned()
            else {
                return DataResult::error("Failed to encode AttributeModifier.operation");
            };
            let mut entries = vec![
                rivet_serialization::Pair::of(
                    ops.create_string("type".to_string()),
                    encode_identifier(&v.attribute, ops),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("id".to_string()),
                    encode_identifier(&v.modifier.id, ops),
                ),
                rivet_serialization::Pair::of(
                    ops.create_string("amount".to_string()),
                    ops.create_double(v.modifier.amount),
                ),
                rivet_serialization::Pair::of(ops.create_string("operation".to_string()), op_val),
            ];
            if v.slot != EquipmentSlotGroup::Any {
                entries.push(rivet_serialization::Pair::of(
                    ops.create_string("slot".to_string()),
                    ops.create_string(v.slot.serialized_name().to_string()),
                ));
            }
            if v.display != AttributeModifierDisplay::Default {
                match enc_disp.encode_start(ops, &v.display).result().cloned() {
                    Some(ev) => entries.push(rivet_serialization::Pair::of(
                        ops.create_string("display".to_string()),
                        ev,
                    )),
                    None => {
                        return DataResult::error(
                            "Failed to encode AttributeModifierEntry.display",
                        );
                    }
                }
            }
            DataResult::success(ops.create_map(entries))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let map_res = ops.get_map(input);
            let Some(map) = map_res.result() else {
                return DataResult::error("ItemAttributeModifiers.Entry must be a map");
            };
            let Some(attr_in) = get_map_field(ops, map.as_ref(), "type") else {
                return DataResult::error("ItemAttributeModifiers.Entry missing 'type'");
            };
            let Some(attribute) = decode_identifier(ops, &attr_in).result().cloned() else {
                return DataResult::error("Invalid ItemAttributeModifiers.Entry 'type'");
            };
            let Some(id_in) = get_map_field(ops, map.as_ref(), "id") else {
                return DataResult::error("ItemAttributeModifiers.Entry missing 'id'");
            };
            let Some(id) = decode_identifier(ops, &id_in).result().cloned() else {
                return DataResult::error("Invalid ItemAttributeModifiers.Entry 'id'");
            };
            let Some(amt_in) = get_map_field(ops, map.as_ref(), "amount") else {
                return DataResult::error("ItemAttributeModifiers.Entry missing 'amount'");
            };
            let Some(amount) = ops
                .get_number_value(&amt_in)
                .result()
                .map(|n| n.double_value())
            else {
                return DataResult::error("ItemAttributeModifiers.Entry 'amount' must be a number");
            };
            let Some(op_in) = get_map_field(ops, map.as_ref(), "operation") else {
                return DataResult::error("ItemAttributeModifiers.Entry missing 'operation'");
            };
            let Some(operation) = dec_op.parse(ops, &op_in).result().copied() else {
                return DataResult::error("Invalid ItemAttributeModifiers.Entry 'operation'");
            };
            let slot = if let Some(s_in) = get_map_field(ops, map.as_ref(), "slot") {
                let s_res = ops.get_string_value(&s_in);
                let Some(s) = s_res.result() else {
                    return DataResult::error(
                        "ItemAttributeModifiers.Entry 'slot' must be a string",
                    );
                };
                match EquipmentSlotGroup::from_serialized_name(s) {
                    Some(g) => g,
                    None => {
                        return DataResult::error(format!("Unknown EquipmentSlotGroup: {s}"));
                    }
                }
            } else {
                EquipmentSlotGroup::Any
            };
            let display = if let Some(d_in) = get_map_field(ops, map.as_ref(), "display") {
                match dec_disp.parse(ops, &d_in).result().cloned() {
                    Some(d) => d,
                    None => {
                        return DataResult::error("Invalid ItemAttributeModifiers.Entry 'display'");
                    }
                }
            } else {
                AttributeModifierDisplay::Default
            };
            DataResult::success(AttributeModifierEntry {
                attribute,
                modifier: AttributeModifierSpec {
                    id,
                    amount,
                    operation,
                },
                slot,
                display,
            })
        },
    )
}

/// Port of `net.minecraft.world.item.component.ItemAttributeModifiers`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemAttributeModifiers {
    pub modifiers: Vec<AttributeModifierEntry>,
}

impl ItemAttributeModifiers {
    pub fn empty() -> Self {
        Self {
            modifiers: Vec::new(),
        }
    }

    pub fn new(modifiers: Vec<AttributeModifierEntry>) -> Self {
        Self { modifiers }
    }

    /// Exact port of `ItemAttributeModifiers.withModifierAdded`: removes any
    /// existing entry with the same `(attribute, modifier.id)` while preserving
    /// order of remaining entries, then appends the new entry.
    pub fn with_modifier_added(
        &self,
        attribute: Identifier,
        modifier: AttributeModifierSpec,
        slot: EquipmentSlotGroup,
    ) -> Self {
        let mut next: Vec<AttributeModifierEntry> = self
            .modifiers
            .iter()
            .filter(|e| !e.matches(&attribute, &modifier.id))
            .cloned()
            .collect();
        next.push(AttributeModifierEntry::new(attribute, modifier, slot));
        Self { modifiers: next }
    }

    /// Exact port of `ItemAttributeModifiers.compute(Holder<Attribute>, double baseValue, EquipmentSlot slot)`.
    pub fn compute(&self, attribute: &Identifier, base_value: f64, slot: EquipmentSlot) -> f64 {
        let mut result = base_value;
        for entry in &self.modifiers {
            if entry.slot.test(slot) && &entry.attribute == attribute {
                let amount = entry.modifier.amount;
                result += match entry.modifier.operation {
                    AttributeOperation::AddValue => amount,
                    AttributeOperation::AddMultipliedBase => amount * base_value,
                    AttributeOperation::AddMultipliedTotal => amount * result,
                };
            }
        }
        result
    }
}

/// `ItemAttributeModifiers.CODEC = Entry.CODEC.listOf().xmap(...)`.
pub fn item_attribute_modifiers_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ItemAttributeModifiers, Ops>> {
    let ec = attribute_modifier_entry_codec::<Ops>();
    let enc_e = Arc::clone(&ec);
    let dec_e = ec;
    codec_from_fns(
        "item_attribute_modifiers",
        move |v: &ItemAttributeModifiers, ops: &Ops| {
            let mut list = Vec::with_capacity(v.modifiers.len());
            for entry in &v.modifiers {
                match enc_e.encode_start(ops, entry).result().cloned() {
                    Some(ev) => list.push(ev),
                    None => return DataResult::error("Failed to encode ItemAttributeModifiers"),
                }
            }
            DataResult::success(ops.create_list(list))
        },
        move |ops: &Ops, input: &Ops::Output| {
            let Some(items) = ops.get_stream(input).result().cloned() else {
                return DataResult::error("ItemAttributeModifiers must be a list");
            };
            let mut modifiers = Vec::with_capacity(items.len());
            for item in &items {
                match dec_e.parse(ops, item).result().cloned() {
                    Some(e) => modifiers.push(e),
                    None => {
                        return DataResult::error("Invalid entry in ItemAttributeModifiers");
                    }
                }
            }
            DataResult::success(ItemAttributeModifiers { modifiers })
        },
    )
}

// ---------------------------------------------------------------------------
// SulfurCubeContent, InstrumentComponent, ProvidesTrimMaterial
// ---------------------------------------------------------------------------

/// Port of `net.minecraft.world.item.component.SulfurCubeContent`.
#[derive(Clone, Debug, PartialEq)]
pub struct SulfurCubeContent {
    pub absorbed_block_item_stack: ItemStackTemplate,
}

impl SulfurCubeContent {
    pub fn new(absorbed_block_item_stack: ItemStackTemplate) -> Self {
        Self {
            absorbed_block_item_stack,
        }
    }
}

/// `SulfurCubeContent.CODEC`.
pub fn sulfur_cube_content_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<SulfurCubeContent, Ops>> {
    let tpl = item_stack_template_codec::<Ops>();
    let enc = Arc::clone(&tpl);
    let dec = tpl;
    codec_from_fns(
        "sulfur_cube_content",
        move |v: &SulfurCubeContent, ops: &Ops| enc.encode_start(ops, &v.absorbed_block_item_stack),
        move |ops: &Ops, input: &Ops::Output| {
            dec.parse(ops, input).map_owned(SulfurCubeContent::new)
        },
    )
}

/// Port of `net.minecraft.world.item.component.InstrumentComponent`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InstrumentComponent {
    pub instrument: Identifier,
}

impl InstrumentComponent {
    pub fn new(instrument: Identifier) -> Self {
        Self { instrument }
    }
}

/// `InstrumentComponent.CODEC` (registry-reference form).
pub fn instrument_component_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<InstrumentComponent, Ops>> {
    codec_from_fns(
        "instrument_component",
        |v: &InstrumentComponent, ops: &Ops| {
            DataResult::success(encode_identifier(&v.instrument, ops))
        },
        |ops: &Ops, input: &Ops::Output| {
            decode_identifier(ops, input).map_owned(InstrumentComponent::new)
        },
    )
}

/// Port of `net.minecraft.world.item.component.ProvidesTrimMaterial`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProvidesTrimMaterial {
    pub material: Identifier,
}

impl ProvidesTrimMaterial {
    pub fn new(material: Identifier) -> Self {
        Self { material }
    }
}

/// `ProvidesTrimMaterial.CODEC` (registry-reference form).
pub fn provides_trim_material_codec<Ops: DynamicOps + 'static>()
-> Arc<dyn Codec<ProvidesTrimMaterial, Ops>> {
    codec_from_fns(
        "provides_trim_material",
        |v: &ProvidesTrimMaterial, ops: &Ops| {
            DataResult::success(encode_identifier(&v.material, ops))
        },
        |ops: &Ops, input: &Ops::Output| {
            decode_identifier(ops, input).map_owned(ProvidesTrimMaterial::new)
        },
    )
}
