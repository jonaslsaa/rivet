//! Port of `net.minecraft.world.scores.{Team, PlayerTeam}` (Paper 26.2).

use crate::scores::{TeamColor, show_text_hover, wrap_in_square_brackets};
use indexmap::IndexSet;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use rivet_serialization::map_codec;
use rivet_serialization::record_builder::{self, RecordCodecBuilder};
use rivet_text::{Component, Style, component_serialization};
use std::sync::Arc;

/// `PlayerTeam.BIT_FRIENDLY_FIRE`.
pub const BIT_FRIENDLY_FIRE: i8 = 1;
/// `PlayerTeam.BIT_SEE_INVISIBLES`.
pub const BIT_SEE_INVISIBLES: i8 = 2;

/// Port of `net.minecraft.world.scores.Team.Visibility`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Visibility {
    Always = 0,
    Never = 1,
    HideForOtherTeams = 2,
    HideForOwnTeam = 3,
}

impl Visibility {
    #[inline]
    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn serialized_name(self) -> &'static str {
        match self {
            Visibility::Always => "always",
            Visibility::Never => "never",
            Visibility::HideForOtherTeams => "hideForOtherTeams",
            Visibility::HideForOwnTeam => "hideForOwnTeam",
        }
    }

    pub const fn from_id(id: i32) -> Option<Visibility> {
        match id {
            0 => Some(Visibility::Always),
            1 => Some(Visibility::Never),
            2 => Some(Visibility::HideForOtherTeams),
            3 => Some(Visibility::HideForOwnTeam),
            _ => None,
        }
    }

    /// `ByIdMap.OutOfBoundsStrategy.ZERO` lookup used by `Team.Visibility.BY_ID`.
    #[inline]
    pub const fn by_id_or_zero(id: i32) -> Visibility {
        match Self::from_id(id) {
            Some(v) => v,
            None => Visibility::Always,
        }
    }

    pub fn from_name(name: &str) -> Option<Visibility> {
        match name {
            "always" => Some(Visibility::Always),
            "never" => Some(Visibility::Never),
            "hideForOtherTeams" => Some(Visibility::HideForOtherTeams),
            "hideForOwnTeam" => Some(Visibility::HideForOwnTeam),
            _ => None,
        }
    }

    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<Visibility, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|s: &String| match Visibility::from_name(s) {
                Some(v) => DataResult::success(v),
                None => DataResult::error(format!("Not a valid team visibility: {s}")),
            }),
            Arc::new(|v: &Visibility| v.serialized_name().to_owned()),
        )
    }
}

impl rivet_serialization::codec::JavaEquals for Visibility {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// Port of `net.minecraft.world.scores.Team.CollisionRule`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CollisionRule {
    Always = 0,
    Never = 1,
    PushOtherTeams = 2,
    PushOwnTeam = 3,
}

impl CollisionRule {
    #[inline]
    pub const fn id(self) -> i32 {
        self as i32
    }

    pub const fn serialized_name(self) -> &'static str {
        match self {
            CollisionRule::Always => "always",
            CollisionRule::Never => "never",
            CollisionRule::PushOtherTeams => "pushOtherTeams",
            CollisionRule::PushOwnTeam => "pushOwnTeam",
        }
    }

    pub const fn from_id(id: i32) -> Option<CollisionRule> {
        match id {
            0 => Some(CollisionRule::Always),
            1 => Some(CollisionRule::Never),
            2 => Some(CollisionRule::PushOtherTeams),
            3 => Some(CollisionRule::PushOwnTeam),
            _ => None,
        }
    }

    /// `ByIdMap.OutOfBoundsStrategy.ZERO` lookup used by `Team.CollisionRule.BY_ID`.
    #[inline]
    pub const fn by_id_or_zero(id: i32) -> CollisionRule {
        match Self::from_id(id) {
            Some(c) => c,
            None => CollisionRule::Always,
        }
    }

    pub fn from_name(name: &str) -> Option<CollisionRule> {
        match name {
            "always" => Some(CollisionRule::Always),
            "never" => Some(CollisionRule::Never),
            "pushOtherTeams" => Some(CollisionRule::PushOtherTeams),
            "pushOwnTeam" => Some(CollisionRule::PushOwnTeam),
            _ => None,
        }
    }

    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<CollisionRule, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|s: &String| match CollisionRule::from_name(s) {
                Some(c) => DataResult::success(c),
                None => DataResult::error(format!("Not a valid collision rule: {s}")),
            }),
            Arc::new(|c: &CollisionRule| c.serialized_name().to_owned()),
        )
    }
}

impl rivet_serialization::codec::JavaEquals for CollisionRule {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// Port of `net.minecraft.world.scores.PlayerTeam`.
///
/// Uses `IndexSet<String>` for `players` so observable iteration order is
/// deterministic across runs and matches insertion order.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerTeam {
    name: String,
    players: IndexSet<String>,
    display_name: Component,
    player_prefix: Component,
    player_suffix: Component,
    allow_friendly_fire: bool,
    see_friendly_invisibles: bool,
    name_tag_visibility: Visibility,
    death_message_visibility: Visibility,
    color: Option<TeamColor>,
    collision_rule: CollisionRule,
    display_name_style: Style,
}

impl PlayerTeam {
    /// `new PlayerTeam(scoreboard, name)`.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let display_name = Component::literal(&name);
        let display_name_style = Style::EMPTY
            .with_insertion(Some(name.clone()))
            .with_hover_event(Some(show_text_hover(Component::literal(&name))));
        Self {
            name,
            players: IndexSet::new(),
            display_name,
            player_prefix: Component::empty(),
            player_suffix: Component::empty(),
            allow_friendly_fire: true,
            see_friendly_invisibles: true,
            name_tag_visibility: Visibility::Always,
            death_message_visibility: Visibility::Always,
            color: None,
            collision_rule: CollisionRule::Always,
            display_name_style,
        }
    }

    /// `PlayerTeam.pack()`.
    pub fn pack(&self) -> PackedPlayerTeam {
        PackedPlayerTeam {
            name: self.name.clone(),
            display_name: Some(self.display_name.clone()),
            color: self.color,
            allow_friendly_fire: self.allow_friendly_fire,
            see_friendly_invisibles: self.see_friendly_invisibles,
            member_name_prefix: self.player_prefix.clone(),
            member_name_suffix: self.player_suffix.clone(),
            name_tag_visibility: self.name_tag_visibility,
            death_message_visibility: self.death_message_visibility,
            collision_rule: self.collision_rule,
            players: self.players.iter().cloned().collect(),
        }
    }

    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[inline]
    pub fn display_name(&self) -> &Component {
        &self.display_name
    }

    #[inline]
    pub fn player_prefix(&self) -> &Component {
        &self.player_prefix
    }

    #[inline]
    pub fn player_suffix(&self) -> &Component {
        &self.player_suffix
    }

    #[inline]
    pub fn players(&self) -> &IndexSet<String> {
        &self.players
    }

    #[inline]
    pub fn is_allow_friendly_fire(&self) -> bool {
        self.allow_friendly_fire
    }

    #[inline]
    pub fn can_see_friendly_invisibles(&self) -> bool {
        self.see_friendly_invisibles
    }

    #[inline]
    pub fn name_tag_visibility(&self) -> Visibility {
        self.name_tag_visibility
    }

    #[inline]
    pub fn death_message_visibility(&self) -> Visibility {
        self.death_message_visibility
    }

    #[inline]
    pub fn collision_rule(&self) -> CollisionRule {
        self.collision_rule
    }

    #[inline]
    pub fn color(&self) -> Option<TeamColor> {
        self.color
    }

    /// `PlayerTeam.packOptions()`.
    pub fn pack_options(&self) -> i8 {
        let mut result: i8 = 0;
        if self.allow_friendly_fire {
            result |= BIT_FRIENDLY_FIRE;
        }
        if self.see_friendly_invisibles {
            result |= BIT_SEE_INVISIBLES;
        }
        result
    }

    /// `PlayerTeam.getFormattedDisplayName()`.
    pub fn formatted_display_name(&self) -> Component {
        let inner = self
            .display_name
            .copy()
            .with_style(self.display_name_style.clone());
        let wrapped = wrap_in_square_brackets(inner);
        self.apply_color(wrapped)
    }

    /// `PlayerTeam.getFormattedName(Component)`.
    pub fn formatted_name(&self, team_member_name: &Component) -> Component {
        let mut comp = Component::empty();
        comp.append_component(self.player_prefix.clone());
        comp.append_component(team_member_name.clone());
        comp.append_component(self.player_suffix.clone());
        self.apply_color(comp)
    }

    /// `PlayerTeam.formatNameForTeam(@Nullable Team, Component)`.
    pub fn format_name_for_team(team: Option<&PlayerTeam>, name: &Component) -> Component {
        match team {
            Some(t) => t.formatted_name(name),
            None => name.copy(),
        }
    }

    fn apply_color(&self, component: Component) -> Component {
        match self.color {
            Some(c) => component.with_style(Style::EMPTY.with_color(Some(c.text_color()))),
            None => component,
        }
    }

    pub(crate) fn set_display_name_raw(&mut self, display_name: Component) {
        self.display_name = display_name;
    }

    pub(crate) fn set_player_prefix_raw(&mut self, prefix: Option<Component>) {
        self.player_prefix = prefix.unwrap_or_else(Component::empty);
    }

    pub(crate) fn set_player_suffix_raw(&mut self, suffix: Option<Component>) {
        self.player_suffix = suffix.unwrap_or_else(Component::empty);
    }

    pub(crate) fn set_allow_friendly_fire_raw(&mut self, allow: bool) {
        self.allow_friendly_fire = allow;
    }

    pub(crate) fn set_see_friendly_invisibles_raw(&mut self, see: bool) {
        self.see_friendly_invisibles = see;
    }

    pub(crate) fn set_name_tag_visibility_raw(&mut self, visibility: Visibility) {
        self.name_tag_visibility = visibility;
    }

    pub(crate) fn set_death_message_visibility_raw(&mut self, visibility: Visibility) {
        self.death_message_visibility = visibility;
    }

    pub(crate) fn set_collision_rule_raw(&mut self, rule: CollisionRule) {
        self.collision_rule = rule;
    }

    pub(crate) fn set_color_raw(&mut self, color: Option<TeamColor>) {
        self.color = color;
    }

    pub(crate) fn add_player_raw(&mut self, player: String) -> bool {
        self.players.insert(player)
    }

    pub(crate) fn remove_player_raw(&mut self, player: &str) -> bool {
        self.players.shift_remove(player)
    }
}

/// Port of `net.minecraft.world.scores.PlayerTeam.Packed`.
#[derive(Clone, Debug, PartialEq)]
pub struct PackedPlayerTeam {
    pub name: String,
    pub display_name: Option<Component>,
    pub color: Option<TeamColor>,
    pub allow_friendly_fire: bool,
    pub see_friendly_invisibles: bool,
    pub member_name_prefix: Component,
    pub member_name_suffix: Component,
    pub name_tag_visibility: Visibility,
    pub death_message_visibility: Visibility,
    pub collision_rule: CollisionRule,
    pub players: Vec<String>,
}

impl PackedPlayerTeam {
    /// `PlayerTeam.Packed.CODEC`.
    ///
    /// Exact Paper 26.2 field names and defaults (including Paper's lenient
    /// `TeamColor` decode that returns `None` on invalid/missing color):
    /// - `"Name"` (required)
    /// - `"DisplayName"` (optional)
    /// - `"TeamColor"` (lenient optional)
    /// - `"AllowFriendlyFire"` (default `true`)
    /// - `"SeeFriendlyInvisibles"` (default `true`)
    /// - `"MemberNamePrefix"` (default empty component)
    /// - `"MemberNameSuffix"` (default empty component)
    /// - `"NameTagVisibility"` (default `Always`)
    /// - `"DeathMessageVisibility"` (default `Always`)
    /// - `"CollisionRule"` (default `Always`)
    /// - `"Players"` (default empty list)
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<PackedPlayerTeam, Ops>> {
        let comp_codec = component_serialization::codec::<Ops>();
        let lenient_color = codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|s: &String| DataResult::success(TeamColor::from_name(s))),
            Arc::new(|c: &Option<TeamColor>| match c {
                Some(tc) => tc.serialized_name().to_owned(),
                None => String::new(),
            }),
        );
        let optional_comp_default = |field: &str, c: Arc<dyn Codec<Component, Ops>>| {
            map_codec::xmap(
                codec::optional_field(field.to_owned(), c, false),
                Arc::new(|opt: &Option<Component>| opt.clone().unwrap_or_else(Component::empty)),
                Arc::new(|comp: &Component| {
                    if *comp == Component::empty() {
                        None
                    } else {
                        Some(comp.clone())
                    }
                }),
            )
        };

        #[derive(Clone, Debug)]
        struct TeamPart1 {
            name: String,
            display_name: Option<Component>,
            color: Option<TeamColor>,
            allow_friendly_fire: bool,
            see_friendly_invisibles: bool,
            member_name_prefix: Component,
        }

        #[derive(Clone, Debug)]
        struct TeamPart2 {
            member_name_suffix: Component,
            name_tag_visibility: Visibility,
            death_message_visibility: Visibility,
            collision_rule: CollisionRule,
            players: Vec<String>,
        }

        let part1 = record_builder::map_codec::<TeamPart1, Ops>(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart1| p.name.clone()),
                    codec::field_of(codec::string_codec::<Ops>(), "Name".to_owned()),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart1| p.display_name.clone()),
                    codec::optional_field("DisplayName".to_owned(), comp_codec.clone(), false),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart1| p.color.map(Some)),
                    codec::optional_field("TeamColor".to_owned(), lenient_color, false),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart1| p.allow_friendly_fire),
                    codec::optional_field_of("AllowFriendlyFire", codec::bool_codec::<Ops>(), true),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart1| p.see_friendly_invisibles),
                    codec::optional_field_of(
                        "SeeFriendlyInvisibles",
                        codec::bool_codec::<Ops>(),
                        true,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart1| p.member_name_prefix.clone()),
                    optional_comp_default("MemberNamePrefix", comp_codec.clone()),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |name,
                         display_name,
                         color_opt: Option<Option<TeamColor>>,
                         allow_friendly_fire,
                         see_friendly_invisibles,
                         member_name_prefix| TeamPart1 {
                            name,
                            display_name,
                            color: color_opt.flatten(),
                            allow_friendly_fire,
                            see_friendly_invisibles,
                            member_name_prefix,
                        },
                    ),
                )
        });

        let part2 = record_builder::map_codec::<TeamPart2, Ops>(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart2| p.member_name_suffix.clone()),
                    optional_comp_default("MemberNameSuffix", comp_codec),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart2| p.name_tag_visibility),
                    codec::optional_field_of(
                        "NameTagVisibility",
                        Visibility::codec::<Ops>(),
                        Visibility::Always,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart2| p.death_message_visibility),
                    codec::optional_field_of(
                        "DeathMessageVisibility",
                        Visibility::codec::<Ops>(),
                        Visibility::Always,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart2| p.collision_rule),
                    codec::optional_field_of(
                        "CollisionRule",
                        CollisionRule::codec::<Ops>(),
                        CollisionRule::Always,
                    ),
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &TeamPart2| p.players.clone()),
                    codec::optional_field_of(
                        "Players",
                        codec::list(codec::string_codec::<Ops>()),
                        Vec::new(),
                    ),
                ))
                .apply(
                    instance,
                    Arc::new(
                        |member_name_suffix,
                         name_tag_visibility,
                         death_message_visibility,
                         collision_rule,
                         players| TeamPart2 {
                            member_name_suffix,
                            name_tag_visibility,
                            death_message_visibility,
                            collision_rule,
                            players,
                        },
                    ),
                )
        });

        record_builder::create(|instance| {
            instance
                .group(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedPlayerTeam| TeamPart1 {
                        name: p.name.clone(),
                        display_name: p.display_name.clone(),
                        color: p.color,
                        allow_friendly_fire: p.allow_friendly_fire,
                        see_friendly_invisibles: p.see_friendly_invisibles,
                        member_name_prefix: p.member_name_prefix.clone(),
                    }),
                    part1,
                ))
                .and(RecordCodecBuilder::of(
                    Arc::new(|p: &PackedPlayerTeam| TeamPart2 {
                        member_name_suffix: p.member_name_suffix.clone(),
                        name_tag_visibility: p.name_tag_visibility,
                        death_message_visibility: p.death_message_visibility,
                        collision_rule: p.collision_rule,
                        players: p.players.clone(),
                    }),
                    part2,
                ))
                .apply(
                    instance,
                    Arc::new(|p1: TeamPart1, p2: TeamPart2| PackedPlayerTeam {
                        name: p1.name,
                        display_name: p1.display_name,
                        color: p1.color,
                        allow_friendly_fire: p1.allow_friendly_fire,
                        see_friendly_invisibles: p1.see_friendly_invisibles,
                        member_name_prefix: p1.member_name_prefix,
                        member_name_suffix: p2.member_name_suffix,
                        name_tag_visibility: p2.name_tag_visibility,
                        death_message_visibility: p2.death_message_visibility,
                        collision_rule: p2.collision_rule,
                        players: p2.players,
                    }),
                )
        })
    }
}

impl rivet_serialization::codec::JavaEquals for PackedPlayerTeam {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}
