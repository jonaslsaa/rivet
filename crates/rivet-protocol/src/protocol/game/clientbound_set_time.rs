//! Port of `net.minecraft.network.protocol.game.ClientboundSetTimePacket`
//! (issue #87) — `set_time` (play clientbound id 113).
//!
//! Java source: `.../network/protocol/game/ClientboundSetTimePacket.java`. Wire
//! body over [`RegistryFriendlyByteBuf`]: `ByteBufCodecs.LONG` `gameTime`, then a
//! varint-counted map of `WorldClock.STREAM_CODEC` (`holderRegistry(
//! Registries.WORLD_CLOCK)`) keys to `ClockNetworkState.STREAM_CODEC` values.
//! The captured golden body (`join_clientbound_set_time.hex`, 29 bytes) carries
//! `gameTime 0` and two clock updates `{holder 0, (0, 0.0, 1.0)}`,
//! `{holder 1, (0, 0.0, 1.0)}` — the two clocks the flat world runs (the
//! day/night and weather clocks, holder ids resolved through the `WORLD_CLOCK`
//! registry the connection carries).
//!
//! The `Map<Holder<WorldClock>, ClockNetworkState>` ports as an ordered
//! [`Vec`] of pairs. Java's field is a map and the decode constructor is
//! `HashMap::new`, but `Holder` is a pure-ID value (OWNERSHIP.md §Registries)
//! that carries no `Hash`, and Rust's `HashMap` iteration order is randomized,
//! which would make the golden round trip nondeterministic. The capture carries
//! the two holders in id order, which `Vec` preserves byte-exactly for both
//! encode and decode; the real clock semantics live in `rivet-world` with the
//! clock unit.

use crate::codec::byte_buf_codecs::MAX_INITIAL_COLLECTION_SIZE;
use crate::codec::registry_byte_buf_codecs::{holder_registry, lift};
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, codec};
use crate::protocol::game::clock_network_state::ClockNetworkState;
use crate::protocol::game::packet_types::clientbound_set_time;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use crate::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_registry::holder::Holder;
use rivet_registry::registries::WORLD_CLOCK;
use rivet_registry::registries::WorldClock;

/// `ClientboundSetTimePacket` — the record `(long gameTime, List<(Holder<
/// WorldClock>, ClockNetworkState)> clockUpdates)` (the `Map` field as an
/// ordered pair list, see the module doc).
#[derive(Debug, Clone, PartialEq)]
pub struct ClientboundSetTimePacket {
    /// `gameTime`.
    game_time: i64,
    /// `clockUpdates` — wire-ordered `(holder, state)` pairs.
    clock_updates: Vec<(Holder<WorldClock>, ClockNetworkState)>,
}

impl ClientboundSetTimePacket {
    /// The record's canonical constructor.
    pub fn new(
        game_time: i64,
        clock_updates: Vec<(Holder<WorldClock>, ClockNetworkState)>,
    ) -> Self {
        ClientboundSetTimePacket {
            game_time,
            clock_updates,
        }
    }

    /// `ClientboundSetTimePacket.gameTime()`.
    pub fn game_time(&self) -> i64 {
        self.game_time
    }

    /// `ClientboundSetTimePacket.clockUpdates()` — the wire-ordered pairs.
    pub fn clock_updates(&self) -> &[(Holder<WorldClock>, ClockNetworkState)] {
        &self.clock_updates
    }

    /// `STREAM_CODEC` — `LONG`, then `ByteBufCodecs.map(HashMap::new,
    /// WorldClock.STREAM_CODEC, ClockNetworkState.STREAM_CODEC)` over the
    /// registry-aware buffer.
    pub fn stream_codec() -> StreamCodec<RegistryFriendlyByteBuf, ClientboundSetTimePacket> {
        codec(
            |packet: &ClientboundSetTimePacket, output: &mut RegistryFriendlyByteBuf| {
                output.write_long(packet.game_time);
                output.write_var_int(packet.clock_updates.len() as i32);
                for (holder, state) in &packet.clock_updates {
                    holder_registry(&*WORLD_CLOCK).encode(output, holder)?;
                    lift(ClockNetworkState::stream_codec()).encode(output, state)?;
                }
                Ok(())
            },
            |input: &mut RegistryFriendlyByteBuf| {
                let game_time = input.read_long();
                let count = input.read_var_int();
                // Java `constructor.apply(Math.min(count, 65536))` =
                // `new HashMap<>(...)`, which throws
                // `IllegalArgumentException("Illegal initial capacity: -n")`
                // on a negative count; the loop then decodes `count` pairs.
                let capacity = count.min(MAX_INITIAL_COLLECTION_SIZE);
                if capacity < 0 {
                    panic!("Illegal initial capacity: {capacity}");
                }
                let mut clock_updates = Vec::with_capacity(capacity as usize);
                for _ in 0..count {
                    let holder = holder_registry(&*WORLD_CLOCK).decode(input)?;
                    let state = lift(ClockNetworkState::stream_codec()).decode(input)?;
                    clock_updates.push((holder, state));
                }
                Ok(ClientboundSetTimePacket {
                    game_time,
                    clock_updates,
                })
            },
        )
    }
}

impl Packet for ClientboundSetTimePacket {
    fn packet_type(&self) -> PacketType {
        clientbound_set_time()
    }
}
