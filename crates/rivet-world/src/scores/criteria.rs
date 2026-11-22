//! Port of `net.minecraft.world.scores.criteria.ObjectiveCriteria` (Paper 26.2).

use crate::scores::TeamColor;
use rivet_serialization::codec::{self, Codec};
use rivet_serialization::data_result::DataResult;
use rivet_serialization::dynamic_ops::DynamicOps;
use std::sync::Arc;

/// `ObjectiveCriteria.RenderType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RenderType {
    Integer = 0,
    Hearts = 1,
}

impl RenderType {
    /// Numeric ordinal (`0` = integer, `1` = hearts) used by `FriendlyByteBuf.writeEnum`.
    #[inline]
    pub const fn id(self) -> i32 {
        self as i32
    }

    /// Serialized string ID (`"integer"` or `"hearts"`).
    pub const fn serialized_name(self) -> &'static str {
        match self {
            RenderType::Integer => "integer",
            RenderType::Hearts => "hearts",
        }
    }

    /// Look up by ordinal.
    pub const fn from_id(id: i32) -> Option<RenderType> {
        match id {
            0 => Some(RenderType::Integer),
            1 => Some(RenderType::Hearts),
            _ => None,
        }
    }

    /// Look up by serialized name (`ObjectiveCriteria.RenderType.byId`).
    pub fn from_name(name: &str) -> Option<RenderType> {
        match name {
            "integer" => Some(RenderType::Integer),
            "hearts" => Some(RenderType::Hearts),
            _ => None,
        }
    }

    /// `ObjectiveCriteria.RenderType.CODEC`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<RenderType, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|s: &String| match RenderType::from_name(s) {
                Some(rt) => DataResult::success(rt),
                None => DataResult::error(format!("Not a valid render type: {s}")),
            }),
            Arc::new(|rt: &RenderType| rt.serialized_name().to_owned()),
        )
    }
}

impl rivet_serialization::codec::JavaEquals for RenderType {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

/// Port of `net.minecraft.world.scores.criteria.ObjectiveCriteria`.
///
/// Built-in criteria match Paper 26.2's `CUSTOM_CRITERIA` table (11 core
/// criteria + 16 `teamkill.<color>` + 16 `killedByTeam.<color>` = 43 custom
/// criteria). Stat-backed criteria (`<stat_type>:<stat_key>`, e.g.
/// `minecraft.custom:minecraft.deaths`) can also be constructed via
/// [`ObjectiveCriteria::stat`] and resolved by [`ObjectiveCriteria::by_name`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ObjectiveCriteria {
    name: String,
    read_only: bool,
    default_render_type: RenderType,
}

impl ObjectiveCriteria {
    /// Construct a custom or stat criteria explicitly.
    pub fn new(name: impl Into<String>, read_only: bool, default_render_type: RenderType) -> Self {
        Self {
            name: name.into(),
            read_only,
            default_render_type,
        }
    }

    pub fn dummy() -> Self {
        Self::new("dummy", false, RenderType::Integer)
    }

    pub fn trigger() -> Self {
        Self::new("trigger", false, RenderType::Integer)
    }

    pub fn death_count() -> Self {
        Self::new("deathCount", false, RenderType::Integer)
    }

    pub fn kill_count_players() -> Self {
        Self::new("playerKillCount", false, RenderType::Integer)
    }

    pub fn kill_count_all() -> Self {
        Self::new("totalKillCount", false, RenderType::Integer)
    }

    pub fn health() -> Self {
        Self::new("health", true, RenderType::Hearts)
    }

    pub fn food() -> Self {
        Self::new("food", true, RenderType::Integer)
    }

    pub fn air() -> Self {
        Self::new("air", true, RenderType::Integer)
    }

    pub fn armor() -> Self {
        Self::new("armor", true, RenderType::Integer)
    }

    pub fn experience() -> Self {
        Self::new("xp", true, RenderType::Integer)
    }

    pub fn level() -> Self {
        Self::new("level", true, RenderType::Integer)
    }

    pub fn team_kill(color: TeamColor) -> Self {
        Self::new(
            format!("teamkill.{}", color.serialized_name()),
            false,
            RenderType::Integer,
        )
    }

    pub fn killed_by_team(color: TeamColor) -> Self {
        Self::new(
            format!("killedByTeam.{}", color.serialized_name()),
            false,
            RenderType::Integer,
        )
    }

    /// Construct a stat-backed criteria (`Stat` extends `ObjectiveCriteria` with
    /// `readOnly = false`, `RenderType.INTEGER`).
    pub fn stat(name: impl Into<String>) -> Self {
        Self::new(name, false, RenderType::Integer)
    }

    /// `ObjectiveCriteria.getName()`.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `ObjectiveCriteria.isReadOnly()`.
    #[inline]
    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// `ObjectiveCriteria.getDefaultRenderType()`.
    #[inline]
    pub fn default_render_type(&self) -> RenderType {
        self.default_render_type
    }

    /// `ObjectiveCriteria.getCustomCriteriaNames()` in deterministic order.
    pub fn custom_criteria_names() -> Vec<String> {
        let mut names = vec![
            "dummy".to_owned(),
            "trigger".to_owned(),
            "deathCount".to_owned(),
            "playerKillCount".to_owned(),
            "totalKillCount".to_owned(),
            "health".to_owned(),
            "food".to_owned(),
            "air".to_owned(),
            "armor".to_owned(),
            "xp".to_owned(),
            "level".to_owned(),
        ];
        for color in TeamColor::ALL {
            names.push(format!("teamkill.{}", color.serialized_name()));
        }
        for color in TeamColor::ALL {
            names.push(format!("killedByTeam.{}", color.serialized_name()));
        }
        names
    }

    /// `ObjectiveCriteria.byName(String)`.
    ///
    /// Resolves all 43 built-in custom criteria. For colon-separated stat names
    /// (`<stat_type>:<stat_key>`), validates that both halves are non-empty
    /// dot-separated identifiers (matching vanilla's `Identifier.bySeparator(..., '.')`
    /// syntax) and returns a writable `RenderType::Integer` criteria.
    pub fn by_name(name: &str) -> Option<ObjectiveCriteria> {
        match name {
            "dummy" => return Some(Self::dummy()),
            "trigger" => return Some(Self::trigger()),
            "deathCount" => return Some(Self::death_count()),
            "playerKillCount" => return Some(Self::kill_count_players()),
            "totalKillCount" => return Some(Self::kill_count_all()),
            "health" => return Some(Self::health()),
            "food" => return Some(Self::food()),
            "air" => return Some(Self::air()),
            "armor" => return Some(Self::armor()),
            "xp" => return Some(Self::experience()),
            "level" => return Some(Self::level()),
            _ => {}
        }

        if let Some(color_name) = name.strip_prefix("teamkill.") {
            return TeamColor::from_name(color_name).map(Self::team_kill);
        }
        if let Some(color_name) = name.strip_prefix("killedByTeam.") {
            return TeamColor::from_name(color_name).map(Self::killed_by_team);
        }

        // Stat criteria format: `<stat_type>:<stat_key>`, where each side uses
        // `.` as namespace/path separator (e.g. `minecraft.custom:minecraft.deaths`).
        if let Some((stat_type, stat_key)) = name.split_once(':')
            && is_valid_dot_separated_id(stat_type)
            && is_valid_dot_separated_id(stat_key)
        {
            return Some(Self::stat(name));
        }

        None
    }

    /// `ObjectiveCriteria.CODEC`.
    pub fn codec<Ops: DynamicOps + 'static>() -> Arc<dyn Codec<ObjectiveCriteria, Ops>> {
        codec::comap_flat_map(
            codec::string_codec::<Ops>(),
            Arc::new(|name: &String| match ObjectiveCriteria::by_name(name) {
                Some(criteria) => DataResult::success(criteria),
                None => DataResult::error(format!("No scoreboard criteria with name: {name}")),
            }),
            Arc::new(|criteria: &ObjectiveCriteria| criteria.name().to_owned()),
        )
    }
}

impl rivet_serialization::codec::JavaEquals for ObjectiveCriteria {
    fn java_equals(&self, other: &Self) -> bool {
        self == other
    }
}

fn is_valid_dot_separated_id(part: &str) -> bool {
    if part.is_empty() {
        return false;
    }
    match part.split_once('.') {
        Some((ns, path)) => {
            !ns.is_empty()
                && !path.is_empty()
                && ns.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-')
                })
                && path.bytes().all(|b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || matches!(b, b'_' | b'-' | b'/' | b'.')
                })
        }
        None => part.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-' | b'/')
        }),
    }
}
