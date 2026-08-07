//! Port of `net.minecraft.network.protocol.game.ClientboundLoginPacket`
//! (issue #87) — `login` (play clientbound id 49).
//!
//! Java source: `.../network/protocol/game/ClientboundLoginPacket.java`. Wire
//! body over [`RegistryFriendlyByteBuf`] (a `Packet.codec(write,
//! new(RegistryFriendlyByteBuf))`): a big-endian `int` `playerId`, a boolean
//! `hardcore`, a varint-counted collection of `ResourceKey<Level>`s (each an
//! identifier string over `Registries.DIMENSION`), varint `maxPlayers`,
//! `chunkRadius`, `simulationDistance`, booleans `reducedDebugInfo`,
//! `showDeathScreen`, `doLimitedCrafting`, the embedded
//! [`CommonPlayerSpawnInfo`] (field 4 of 12, the only registry-aware member),
//! then booleans `onlineMode`, `enforcesSecureChat`.
//!
//! The captured golden body (`join_clientbound_login.hex`, 113 bytes) carries
//! `playerId 1`, `hardcore false`, the three vanilla levels
//! (`minecraft:overworld`, `minecraft:the_nether`, `minecraft:the_end`),
//! `maxPlayers 20`, `chunkRadius/simulationDistance 4`, `showDeathScreen true`,
//! and the superflat `CommonPlayerSpawnInfo` (dimension-type holder id 0,
//! `minecraft:overworld`, seed `0xC6F218BC089104ED`, `gameType 0`, no previous
//! game type, `isFlat true`, `seaLevel -63`). The seed is the raw capture value
//! (the flat-world seed 42 obfuscated server-side); it is pinned by the fixture,
//! not derived.
//!
//! Java's `levels` field is a `Set<ResourceKey<Level>>` decoded into a
//! `HashSet`; the port carries an ordered [`Vec`] that preserves the wire order
//! (the capture's three levels are already in a fixed order). This makes
//! decode→encode deterministic and byte-exact against the capture — Java's
//! `HashSet` iteration would not be. The server-side construction (issue #101)
//! supplies levels in the order to send.

use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::protocol::game::common_player_spawn_info::CommonPlayerSpawnInfo;
use crate::protocol::game::packet_types::clientbound_login;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::registries;
use rivet_registry::{ResourceKey, registries::Level};

/// `ClientboundLoginPacket` — the join login record.
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundLoginPacket {
    /// `playerId`.
    player_id: i32,
    /// `hardcore`.
    hardcore: bool,
    /// `levels` — the `Set<ResourceKey<Level>>` (identifier strings).
    levels: Vec<ResourceKey<Level>>,
    /// `maxPlayers`.
    max_players: i32,
    /// `chunkRadius`.
    chunk_radius: i32,
    /// `simulationDistance`.
    simulation_distance: i32,
    /// `reducedDebugInfo`.
    reduced_debug_info: bool,
    /// `showDeathScreen`.
    show_death_screen: bool,
    /// `doLimitedCrafting`.
    do_limited_crafting: bool,
    /// `commonPlayerSpawnInfo` — the embedded spawn info.
    common_player_spawn_info: CommonPlayerSpawnInfo,
    /// `onlineMode`.
    online_mode: bool,
    /// `enforcesSecureChat`.
    enforces_secure_chat: bool,
}

impl ClientboundLoginPacket {
    /// The record's canonical constructor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        player_id: i32,
        hardcore: bool,
        levels: Vec<ResourceKey<Level>>,
        max_players: i32,
        chunk_radius: i32,
        simulation_distance: i32,
        reduced_debug_info: bool,
        show_death_screen: bool,
        do_limited_crafting: bool,
        common_player_spawn_info: CommonPlayerSpawnInfo,
        online_mode: bool,
        enforces_secure_chat: bool,
    ) -> Self {
        ClientboundLoginPacket {
            player_id,
            hardcore,
            levels,
            max_players,
            chunk_radius,
            simulation_distance,
            reduced_debug_info,
            show_death_screen,
            do_limited_crafting,
            common_player_spawn_info,
            online_mode,
            enforces_secure_chat,
        }
    }

    /// `ClientboundLoginPacket.playerId()`.
    pub fn player_id(&self) -> i32 {
        self.player_id
    }

    /// `ClientboundLoginPacket.hardcore()`.
    pub fn hardcore(&self) -> bool {
        self.hardcore
    }

    /// `ClientboundLoginPacket.levels()`.
    pub fn levels(&self) -> &[ResourceKey<Level>] {
        &self.levels
    }

    /// `ClientboundLoginPacket.maxPlayers()`.
    pub fn max_players(&self) -> i32 {
        self.max_players
    }

    /// `ClientboundLoginPacket.chunkRadius()`.
    pub fn chunk_radius(&self) -> i32 {
        self.chunk_radius
    }

    /// `ClientboundLoginPacket.simulationDistance()`.
    pub fn simulation_distance(&self) -> i32 {
        self.simulation_distance
    }

    /// `ClientboundLoginPacket.reducedDebugInfo()`.
    pub fn reduced_debug_info(&self) -> bool {
        self.reduced_debug_info
    }

    /// `ClientboundLoginPacket.showDeathScreen()`.
    pub fn show_death_screen(&self) -> bool {
        self.show_death_screen
    }

    /// `ClientboundLoginPacket.doLimitedCrafting()`.
    pub fn do_limited_crafting(&self) -> bool {
        self.do_limited_crafting
    }

    /// `ClientboundLoginPacket.commonPlayerSpawnInfo()`.
    pub fn common_player_spawn_info(&self) -> &CommonPlayerSpawnInfo {
        &self.common_player_spawn_info
    }

    /// `ClientboundLoginPacket.onlineMode()`.
    pub fn online_mode(&self) -> bool {
        self.online_mode
    }

    /// `ClientboundLoginPacket.enforcesSecureChat()`.
    pub fn enforces_secure_chat(&self) -> bool {
        self.enforces_secure_chat
    }

    /// `STREAM_CODEC` — `Packet.codec(write, new(RegistryFriendlyByteBuf))`, the
    /// Java field order.
    pub fn stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, ClientboundLoginPacket> {
        codec(
            |packet: &ClientboundLoginPacket, output: &mut RegistryFriendlyByteBuf| {
                output.write_int(packet.player_id);
                output.write_boolean(packet.hardcore);
                output.write_var_int(packet.levels.len() as i32);
                for level in &packet.levels {
                    output.write_resource_key(level);
                }
                output.write_var_int(packet.max_players);
                output.write_var_int(packet.chunk_radius);
                output.write_var_int(packet.simulation_distance);
                output.write_boolean(packet.reduced_debug_info);
                output.write_boolean(packet.show_death_screen);
                output.write_boolean(packet.do_limited_crafting);
                CommonPlayerSpawnInfo::stream_codec()
                    .encode(output, &packet.common_player_spawn_info)?;
                output.write_boolean(packet.online_mode);
                output.write_boolean(packet.enforces_secure_chat);
                Ok(())
            },
            |input: &mut RegistryFriendlyByteBuf| {
                let player_id = input.read_int();
                let hardcore = input.read_boolean();
                let level_count = input.read_var_int();
                let mut levels = Vec::with_capacity(level_count as usize);
                for _ in 0..level_count {
                    levels.push(input.read_resource_key(&*registries::DIMENSION));
                }
                let max_players = input.read_var_int();
                let chunk_radius = input.read_var_int();
                let simulation_distance = input.read_var_int();
                let reduced_debug_info = input.read_boolean();
                let show_death_screen = input.read_boolean();
                let do_limited_crafting = input.read_boolean();
                let common_player_spawn_info =
                    CommonPlayerSpawnInfo::stream_codec().decode(input)?;
                let online_mode = input.read_boolean();
                let enforces_secure_chat = input.read_boolean();
                Ok(ClientboundLoginPacket {
                    player_id,
                    hardcore,
                    levels,
                    max_players,
                    chunk_radius,
                    simulation_distance,
                    reduced_debug_info,
                    show_death_screen,
                    do_limited_crafting,
                    common_player_spawn_info,
                    online_mode,
                    enforces_secure_chat,
                })
            },
        )
    }
}

impl Packet for ClientboundLoginPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_login()
    }
}
