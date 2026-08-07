//! Fuzz target: the configuration-protocol serverbound packet decode paths.
//!
//! Feeds arbitrary bytes through the real registration-built dispatch codec
//! (`serverbound_protocol` + `ProtocolInfoBuilder` + `IdDispatchCodec`) for the
//! two configuration serverbound packets this crate ports:
//! `finish_configuration` (unit body) and `select_known_packs` (a
//! `ByteBufCodecs.list(64)` of `KnownPack` 3-string triples), registered in the
//! target's own dispatch table as ids 0/1 (real protocol ids are 3 and 7 —
//! only the ported subset is registered).
//!
//! `select_known_packs` is the collection-allocation stress surface: an
//! over-limit count returns `Err` (`"{count} elements exceeded max size of: 64"`),
//! a negative count panics faithfully with `Illegal Capacity`, and each element
//! is a bounded string that returns `Err` on oversize/truncation. Every other
//! hostile shape resolves to `Err` or a faithful panic — anything else aborts
//! the fuzzer and writes an artifact.
#![no_main]
use bytes::BytesMut;
use libfuzzer_sys::fuzz_target;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, map};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::configuration::packet_types::{
    serverbound_finish_configuration, serverbound_select_known_packs,
};
use rivet_protocol::protocol::configuration::serverbound_finish_configuration::{
    self, ServerboundFinishConfigurationPacket,
};
use rivet_protocol::protocol::configuration::serverbound_select_known_packs::ServerboundSelectKnownPacks;
use rivet_protocol::protocol::{Packet, PacketType, ProtocolInfoBuilder, serverbound_protocol};
use std::sync::OnceLock;

mod guard;
use guard::guarded;

/// The erased configuration/serverbound packet value (the two bodies registered
/// by `ConfigurationProtocols.SERVERBOUND_TEMPLATE`).
#[derive(Debug, Clone, PartialEq)]
enum ConfigServerbound {
    Finish,
    SelectKnownPacks(ServerboundSelectKnownPacks),
}

impl Packet for ConfigServerbound {
    fn packet_type(&self) -> PacketType {
        match self {
            ConfigServerbound::Finish => serverbound_finish_configuration(),
            ConfigServerbound::SelectKnownPacks(_) => serverbound_select_known_packs(),
        }
    }
}

fn config_serverbound(b: &mut ProtocolInfoBuilder<ConfigServerbound, ()>) {
    b.add_packet(
        serverbound_finish_configuration(),
        map(
            serverbound_finish_configuration::stream_codec(),
            |_: &ServerboundFinishConfigurationPacket| ConfigServerbound::Finish,
            |p: &ConfigServerbound| match p {
                ConfigServerbound::Finish => ServerboundFinishConfigurationPacket,
                _ => unreachable!(),
            },
        ),
    )
    .add_packet(
        serverbound_select_known_packs(),
        map(
            ServerboundSelectKnownPacks::stream_codec(),
            |v: &ServerboundSelectKnownPacks| ConfigServerbound::SelectKnownPacks(v.clone()),
            |p: &ConfigServerbound| match p {
                ConfigServerbound::SelectKnownPacks(v) => v.clone(),
                _ => unreachable!(),
            },
        ),
    );
}

fn dispatch_codec() -> &'static StreamCodec<FriendlyByteBuf, ConfigServerbound> {
    static CODEC: OnceLock<StreamCodec<FriendlyByteBuf, ConfigServerbound>> = OnceLock::new();
    CODEC.get_or_init(|| {
        let template =
            serverbound_protocol::<ConfigServerbound>(ConnectionProtocol::Configuration, |b| {
                config_serverbound(b);
            });
        template.bind().codec().clone()
    })
}

fuzz_target!(|data: &[u8]| {
    if data.len() > guard::MAX_INPUT_LEN {
        return;
    }
    guarded(|| {
        let mut input = FriendlyByteBuf::new(BytesMut::from(data));
        let _ = dispatch_codec().decode(&mut input);
    });
});
