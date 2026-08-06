//! Connection-state and packet identity types shared across the proxy,
//! normalizer, and fixture layers.

use std::fmt;

use rivet_protocol::generated::packets;

/// The connection state the proxy tracks through the handshake → login →
/// configuration → play progression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Handshake,
    Status,
    Login,
    Configuration,
    Play,
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                State::Handshake => "handshake",
                State::Status => "status",
                State::Login => "login",
                State::Configuration => "configuration",
                State::Play => "play",
            }
        )
    }
}

/// Direction of a packet using the protocol's own `PacketFlow` naming:
/// serverbound packets travel client → server, clientbound packets server → client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    /// Server → client. Protocol flow `clientbound` (the client receives it).
    Clientbound,
    /// Client → server. Protocol flow `serverbound` (the server receives it).
    Serverbound,
}

impl Direction {
    /// The protocol's flow name.
    pub fn flow(self) -> &'static str {
        match self {
            Direction::Clientbound => "clientbound",
            Direction::Serverbound => "serverbound",
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.flow())
    }
}

/// A captured packet as observed by the proxy, before any normalization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedPacket {
    pub state: State,
    pub direction: Direction,
    pub id: i32,
    pub body: Vec<u8>,
}

/// Resolve the canonical `minecraft:` packet name for a (state, flow, id) tuple
/// using the generated protocol-776 packet-ID tables. `None` when the state/flow
/// has no such id (e.g. handshake has no clientbound packets).
pub fn packet_name(state: State, direction: Direction, id: i32) -> Option<&'static str> {
    let table: &[&str] = match (state, direction) {
        (State::Handshake, Direction::Serverbound) => packets::handshake::serverbound::PACKET_BY_ID,
        (State::Handshake, Direction::Clientbound) => return None,
        (State::Status, Direction::Serverbound) => packets::status::serverbound::PACKET_BY_ID,
        (State::Status, Direction::Clientbound) => packets::status::clientbound::PACKET_BY_ID,
        (State::Login, Direction::Serverbound) => packets::login::serverbound::PACKET_BY_ID,
        (State::Login, Direction::Clientbound) => packets::login::clientbound::PACKET_BY_ID,
        (State::Configuration, Direction::Serverbound) => {
            packets::configuration::serverbound::PACKET_BY_ID
        }
        (State::Configuration, Direction::Clientbound) => {
            packets::configuration::clientbound::PACKET_BY_ID
        }
        (State::Play, Direction::Serverbound) => packets::play::serverbound::PACKET_BY_ID,
        (State::Play, Direction::Clientbound) => packets::play::clientbound::PACKET_BY_ID,
    };
    usize::try_from(id).ok().and_then(|i| table.get(i).copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_path_names_resolve() {
        assert_eq!(
            packet_name(State::Handshake, Direction::Serverbound, 0),
            Some("minecraft:intention")
        );
        assert_eq!(
            packet_name(State::Login, Direction::Clientbound, 2),
            Some("minecraft:login_finished")
        );
        assert_eq!(
            packet_name(State::Play, Direction::Clientbound, 45),
            Some("minecraft:level_chunk_with_light")
        );
        assert_eq!(
            packet_name(State::Play, Direction::Clientbound, 49),
            Some("minecraft:login")
        );
        assert_eq!(packet_name(State::Play, Direction::Clientbound, 999), None);
        assert_eq!(
            packet_name(State::Handshake, Direction::Clientbound, 0),
            None
        );
    }
}
