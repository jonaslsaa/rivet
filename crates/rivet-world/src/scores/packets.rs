//! Port of the five MC 26.2 clientbound scoreboard packets:
//! - `ClientboundSetObjectivePacket` (`minecraft:set_objective`, id 106)
//! - `ClientboundSetDisplayObjectivePacket` (`minecraft:set_display_objective`, id 98)
//! - `ClientboundSetScorePacket` (`minecraft:set_score`, id 110)
//! - `ClientboundResetScorePacket` (`minecraft:reset_score`, id 79)
//! - `ClientboundSetPlayerTeamPacket` (`minecraft:set_player_team`, id 109)

use crate::scores::{
    CollisionRule, DisplaySlot, NumberFormat, Objective, PlayerTeam, RenderType, TeamColor,
    Visibility,
};
use rivet_protocol::chat::{
    trusted_component_with_registries, trusted_optional_component_with_registries,
};
use rivet_protocol::codec::{CodecError, StreamDecoder, StreamEncoder};
use rivet_protocol::generated::packets::play::clientbound::PacketType as GeneratedPacketType;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_text::Component;

/// `ClientboundSetObjectivePacket` (`minecraft:set_objective`, protocol id 106).
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundSetObjectivePacket {
    pub objective_name: String,
    pub display_name: Component,
    pub render_type: RenderType,
    pub number_format: Option<NumberFormat>,
    pub method: i8,
}

impl ClientboundSetObjectivePacket {
    pub const METHOD_ADD: i8 = 0;
    pub const METHOD_REMOVE: i8 = 1;
    pub const METHOD_CHANGE: i8 = 2;

    pub fn from_objective(objective: &Objective, method: i8) -> Self {
        Self {
            objective_name: objective.name().to_owned(),
            display_name: objective.display_name().clone(),
            render_type: objective.render_type(),
            number_format: objective.number_format().cloned(),
            method,
        }
    }

    pub const fn protocol_id() -> u32 {
        GeneratedPacketType::SetObjective.id()
    }

    pub fn encode(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_utf(&self.objective_name);
        output.inner_mut().write_byte(self.method);
        if self.method == Self::METHOD_ADD || self.method == Self::METHOD_CHANGE {
            trusted_component_with_registries().encode(output, &self.display_name)?;
            output.inner_mut().write_var_int(self.render_type.id());
            NumberFormat::encode_optional_stream(&self.number_format, output)?;
        }
        Ok(())
    }

    pub fn decode(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let objective_name = input.inner_mut().read_utf();
        let method = input.inner_mut().read_byte();
        if method != Self::METHOD_ADD && method != Self::METHOD_CHANGE {
            Ok(Self {
                objective_name,
                display_name: Component::empty(),
                render_type: RenderType::Integer,
                number_format: None,
                method,
            })
        } else {
            let display_name = trusted_component_with_registries().decode(input)?;
            let render_type_id = input.inner_mut().read_var_int();
            let render_type = RenderType::from_id(render_type_id).ok_or_else(|| {
                CodecError::new(format!("Invalid objective RenderType id: {render_type_id}"))
            })?;
            let number_format = NumberFormat::decode_optional_stream(input)?;
            Ok(Self {
                objective_name,
                display_name,
                render_type,
                number_format,
                method,
            })
        }
    }
}

/// `ClientboundSetDisplayObjectivePacket` (`minecraft:set_display_objective`, protocol id 98).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundSetDisplayObjectivePacket {
    pub slot: DisplaySlot,
    pub objective_name: String,
}

impl ClientboundSetDisplayObjectivePacket {
    pub fn new(slot: DisplaySlot, objective: Option<&Objective>) -> Self {
        Self {
            slot,
            objective_name: objective.map(|o| o.name().to_owned()).unwrap_or_default(),
        }
    }

    pub fn from_name(slot: DisplaySlot, objective_name: Option<&str>) -> Self {
        Self {
            slot,
            objective_name: objective_name.unwrap_or_default().to_owned(),
        }
    }

    /// `getObjectiveName()` — returns `None` when the wire string is empty `""`.
    pub fn objective_name(&self) -> Option<&str> {
        if self.objective_name.is_empty() {
            None
        } else {
            Some(&self.objective_name)
        }
    }

    pub const fn protocol_id() -> u32 {
        GeneratedPacketType::SetDisplayObjective.id()
    }

    pub fn encode(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_var_int(self.slot.id());
        output.inner_mut().write_utf(&self.objective_name);
        Ok(())
    }

    pub fn decode(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let slot_id = input.inner_mut().read_var_int();
        let slot = DisplaySlot::from_id(slot_id)
            .ok_or_else(|| CodecError::new(format!("Invalid DisplaySlot id: {slot_id}")))?;
        let objective_name = input.inner_mut().read_utf();
        Ok(Self {
            slot,
            objective_name,
        })
    }
}

/// `ClientboundSetScorePacket` (`minecraft:set_score`, protocol id 110).
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundSetScorePacket {
    pub owner: String,
    pub objective_name: String,
    pub score: i32,
    pub display: Option<Component>,
    pub number_format: Option<NumberFormat>,
}

impl ClientboundSetScorePacket {
    pub const fn protocol_id() -> u32 {
        GeneratedPacketType::SetScore.id()
    }

    pub fn encode(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_utf(&self.owner);
        output.inner_mut().write_utf(&self.objective_name);
        output.inner_mut().write_var_int(self.score);
        trusted_optional_component_with_registries().encode(output, &self.display)?;
        NumberFormat::encode_optional_stream(&self.number_format, output)?;
        Ok(())
    }

    pub fn decode(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let owner = input.inner_mut().read_utf();
        let objective_name = input.inner_mut().read_utf();
        let score = input.inner_mut().read_var_int();
        let display = trusted_optional_component_with_registries().decode(input)?;
        let number_format = NumberFormat::decode_optional_stream(input)?;
        Ok(Self {
            owner,
            objective_name,
            score,
            display,
            number_format,
        })
    }
}

/// `ClientboundResetScorePacket` (`minecraft:reset_score`, protocol id 79).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundResetScorePacket {
    pub owner: String,
    pub objective_name: Option<String>,
}

impl ClientboundResetScorePacket {
    pub const fn protocol_id() -> u32 {
        GeneratedPacketType::ResetScore.id()
    }

    pub fn encode(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_utf(&self.owner);
        match &self.objective_name {
            Some(name) => {
                output.inner_mut().write_boolean(true);
                output.inner_mut().write_utf(name);
            }
            None => {
                output.inner_mut().write_boolean(false);
            }
        }
        Ok(())
    }

    pub fn decode(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let owner = input.inner_mut().read_utf();
        let objective_name = if input.inner_mut().read_boolean() {
            Some(input.inner_mut().read_utf())
        } else {
            None
        };
        Ok(Self {
            owner,
            objective_name,
        })
    }
}

/// `ClientboundSetPlayerTeamPacket.Action`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeamPacketAction {
    Add,
    Remove,
}

/// `ClientboundSetPlayerTeamPacket.Parameters`.
#[derive(Clone, Debug, PartialEq)]
pub struct TeamPacketParameters {
    pub display_name: Component,
    pub player_prefix: Component,
    pub player_suffix: Component,
    pub name_tag_visibility: Visibility,
    pub collision_rule: CollisionRule,
    pub color: Option<TeamColor>,
    pub options: i8,
}

impl TeamPacketParameters {
    pub fn from_team(team: &PlayerTeam, enable_player_collisions: bool) -> Self {
        Self {
            display_name: team.display_name().clone(),
            player_prefix: team.player_prefix().clone(),
            player_suffix: team.player_suffix().clone(),
            name_tag_visibility: team.name_tag_visibility(),
            collision_rule: if enable_player_collisions {
                team.collision_rule()
            } else {
                CollisionRule::Never
            },
            color: team.color(),
            options: team.pack_options(),
        }
    }

    pub fn encode(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        trusted_component_with_registries().encode(output, &self.display_name)?;
        trusted_component_with_registries().encode(output, &self.player_prefix)?;
        trusted_component_with_registries().encode(output, &self.player_suffix)?;
        output
            .inner_mut()
            .write_var_int(self.name_tag_visibility.id());
        output.inner_mut().write_var_int(self.collision_rule.id());
        match self.color {
            Some(color) => {
                output.inner_mut().write_boolean(true);
                output.inner_mut().write_var_int(color.id());
            }
            None => {
                output.inner_mut().write_boolean(false);
            }
        }
        output.inner_mut().write_byte(self.options);
        Ok(())
    }

    pub fn decode(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let display_name = trusted_component_with_registries().decode(input)?;
        let player_prefix = trusted_component_with_registries().decode(input)?;
        let player_suffix = trusted_component_with_registries().decode(input)?;
        let name_tag_visibility = Visibility::by_id_or_zero(input.inner_mut().read_var_int());
        let collision_rule = CollisionRule::by_id_or_zero(input.inner_mut().read_var_int());
        let color = if input.inner_mut().read_boolean() {
            Some(TeamColor::by_id_or_zero(input.inner_mut().read_var_int()))
        } else {
            None
        };
        let options = input.inner_mut().read_byte();
        Ok(Self {
            display_name,
            player_prefix,
            player_suffix,
            name_tag_visibility,
            collision_rule,
            color,
            options,
        })
    }
}

/// `ClientboundSetPlayerTeamPacket` (`minecraft:set_player_team`, protocol id 109).
#[derive(Clone, Debug, PartialEq)]
pub struct ClientboundSetPlayerTeamPacket {
    pub method: i8,
    pub name: String,
    pub players: Vec<String>,
    pub parameters: Option<TeamPacketParameters>,
}

impl ClientboundSetPlayerTeamPacket {
    pub const METHOD_ADD: i8 = 0;
    pub const METHOD_REMOVE: i8 = 1;
    pub const METHOD_CHANGE: i8 = 2;
    pub const METHOD_JOIN: i8 = 3;
    pub const METHOD_LEAVE: i8 = 4;

    /// `ClientboundSetPlayerTeamPacket.createAddOrModifyPacket(PlayerTeam, boolean)`.
    pub fn create_add_or_modify(
        team: &PlayerTeam,
        create_new: bool,
        enable_player_collisions: bool,
    ) -> Self {
        Self {
            name: team.name().to_owned(),
            method: if create_new {
                Self::METHOD_ADD
            } else {
                Self::METHOD_CHANGE
            },
            parameters: Some(TeamPacketParameters::from_team(
                team,
                enable_player_collisions,
            )),
            players: if create_new {
                team.players().iter().cloned().collect()
            } else {
                Vec::new()
            },
        }
    }

    /// `ClientboundSetPlayerTeamPacket.createRemovePacket(PlayerTeam)`.
    pub fn create_remove(team_name: impl Into<String>) -> Self {
        Self {
            name: team_name.into(),
            method: Self::METHOD_REMOVE,
            parameters: None,
            players: Vec::new(),
        }
    }

    /// `ClientboundSetPlayerTeamPacket.createPlayerPacket(PlayerTeam, String, Action)`.
    pub fn create_player(
        team_name: impl Into<String>,
        player: impl Into<String>,
        action: TeamPacketAction,
    ) -> Self {
        Self {
            name: team_name.into(),
            method: match action {
                TeamPacketAction::Add => Self::METHOD_JOIN,
                TeamPacketAction::Remove => Self::METHOD_LEAVE,
            },
            parameters: None,
            players: vec![player.into()],
        }
    }

    /// `ClientboundSetPlayerTeamPacket.createMultiplePlayerPacket(PlayerTeam, Collection<String>, Action)`.
    pub fn create_multiple_player(
        team_name: impl Into<String>,
        players: Vec<String>,
        action: TeamPacketAction,
    ) -> Self {
        Self {
            name: team_name.into(),
            method: match action {
                TeamPacketAction::Add => Self::METHOD_JOIN,
                TeamPacketAction::Remove => Self::METHOD_LEAVE,
            },
            parameters: None,
            players,
        }
    }

    pub const fn should_have_parameters(method: i8) -> bool {
        method == Self::METHOD_ADD || method == Self::METHOD_CHANGE
    }

    pub const fn should_have_player_list(method: i8) -> bool {
        method == Self::METHOD_ADD || method == Self::METHOD_JOIN || method == Self::METHOD_LEAVE
    }

    pub const fn player_action(&self) -> Option<TeamPacketAction> {
        match self.method {
            Self::METHOD_ADD | Self::METHOD_JOIN => Some(TeamPacketAction::Add),
            Self::METHOD_LEAVE => Some(TeamPacketAction::Remove),
            _ => None,
        }
    }

    pub const fn team_action(&self) -> Option<TeamPacketAction> {
        match self.method {
            Self::METHOD_ADD => Some(TeamPacketAction::Add),
            Self::METHOD_REMOVE => Some(TeamPacketAction::Remove),
            _ => None,
        }
    }

    pub const fn protocol_id() -> u32 {
        GeneratedPacketType::SetPlayerTeam.id()
    }

    pub fn encode(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_utf(&self.name);
        output.inner_mut().write_byte(self.method);
        if Self::should_have_parameters(self.method) {
            let params = self.parameters.as_ref().ok_or_else(|| {
                CodecError::new(format!(
                    "Parameters not present, but method is {}",
                    self.method
                ))
            })?;
            params.encode(output)?;
        }
        if Self::should_have_player_list(self.method) {
            output.inner_mut().write_var_int(self.players.len() as i32);
            for player in &self.players {
                output.inner_mut().write_utf(player);
            }
        }
        Ok(())
    }

    pub fn decode(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let name = input.inner_mut().read_utf();
        let method = input.inner_mut().read_byte();
        let parameters = if Self::should_have_parameters(method) {
            Some(TeamPacketParameters::decode(input)?)
        } else {
            None
        };
        let players = if Self::should_have_player_list(method) {
            let count = input.inner_mut().read_var_int();
            if count < 0 {
                return Err(CodecError::new(format!(
                    "Negative player list length: {count}"
                )));
            }
            let mut list = Vec::with_capacity(count as usize);
            for _ in 0..count {
                list.push(input.inner_mut().read_utf());
            }
            list
        } else {
            Vec::new()
        };
        Ok(Self {
            method,
            name,
            players,
            parameters,
        })
    }
}

/// Any clientbound scoreboard packet emitted by `ServerScoreboard`.
#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum ScoreboardPacket {
    SetObjective(ClientboundSetObjectivePacket),
    SetDisplayObjective(ClientboundSetDisplayObjectivePacket),
    SetScore(ClientboundSetScorePacket),
    ResetScore(ClientboundResetScorePacket),
    SetPlayerTeam(ClientboundSetPlayerTeamPacket),
}

impl ScoreboardPacket {
    /// Vanilla play-clientbound protocol packet ID (MC 26.2, protocol 776).
    pub const fn protocol_id(&self) -> u32 {
        match self {
            ScoreboardPacket::SetObjective(_) => ClientboundSetObjectivePacket::protocol_id(),
            ScoreboardPacket::SetDisplayObjective(_) => {
                ClientboundSetDisplayObjectivePacket::protocol_id()
            }
            ScoreboardPacket::SetScore(_) => ClientboundSetScorePacket::protocol_id(),
            ScoreboardPacket::ResetScore(_) => ClientboundResetScorePacket::protocol_id(),
            ScoreboardPacket::SetPlayerTeam(_) => ClientboundSetPlayerTeamPacket::protocol_id(),
        }
    }

    /// Encode packet payload (without packet-id prefix).
    pub fn encode_payload(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        match self {
            ScoreboardPacket::SetObjective(p) => p.encode(output),
            ScoreboardPacket::SetDisplayObjective(p) => p.encode(output),
            ScoreboardPacket::SetScore(p) => p.encode(output),
            ScoreboardPacket::ResetScore(p) => p.encode(output),
            ScoreboardPacket::SetPlayerTeam(p) => p.encode(output),
        }
    }

    /// Encode packet with leading VarInt protocol packet ID.
    pub fn encode_framed(&self, output: &mut RegistryFriendlyByteBuf) -> Result<(), CodecError> {
        output.inner_mut().write_var_int(self.protocol_id() as i32);
        self.encode_payload(output)
    }

    /// Decode a framed packet (leading VarInt protocol packet ID + payload).
    pub fn decode_framed(input: &mut RegistryFriendlyByteBuf) -> Result<Self, CodecError> {
        let id = input.inner_mut().read_var_int() as u32;
        Self::decode_payload(id, input)
    }

    /// Decode packet payload given its protocol packet ID.
    pub fn decode_payload(
        protocol_id: u32,
        input: &mut RegistryFriendlyByteBuf,
    ) -> Result<Self, CodecError> {
        match GeneratedPacketType::from_id(protocol_id) {
            Some(GeneratedPacketType::SetObjective) => Ok(ScoreboardPacket::SetObjective(
                ClientboundSetObjectivePacket::decode(input)?,
            )),
            Some(GeneratedPacketType::SetDisplayObjective) => {
                Ok(ScoreboardPacket::SetDisplayObjective(
                    ClientboundSetDisplayObjectivePacket::decode(input)?,
                ))
            }
            Some(GeneratedPacketType::SetScore) => Ok(ScoreboardPacket::SetScore(
                ClientboundSetScorePacket::decode(input)?,
            )),
            Some(GeneratedPacketType::ResetScore) => Ok(ScoreboardPacket::ResetScore(
                ClientboundResetScorePacket::decode(input)?,
            )),
            Some(GeneratedPacketType::SetPlayerTeam) => Ok(ScoreboardPacket::SetPlayerTeam(
                ClientboundSetPlayerTeamPacket::decode(input)?,
            )),
            _ => Err(CodecError::new(format!(
                "Not a scoreboard packet id: {protocol_id}"
            ))),
        }
    }
}
