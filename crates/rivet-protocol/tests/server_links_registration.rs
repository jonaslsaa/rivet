//! Java-grounded registration tests for `ClientboundServerLinksPacket`
//! (issue #207): the packet is registered at configuration clientbound id 16
//! and play clientbound id 137 (Paper's `ConfigurationProtocols
//! .CLIENTBOUND_TEMPLATE` and `GameProtocols.CLIENTBOUND_TEMPLATE`
//! `addPacket` order), and the real body codec round-trips through the
//! id-dispatch registration machinery.

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets;
use rivet_protocol::generated::protocol::{ConnectionProtocol, PacketFlow};
use rivet_protocol::protocol::clientbound_protocol;
use rivet_protocol::protocol::common::clientbound_server_links::ClientboundServerLinksPacket;
use rivet_protocol::protocol::common::packet_types::clientbound_server_links;
use rivet_protocol::protocol::common::server_links::{KnownLinkType, UntrustedEntry};
use rivet_serialization::Either;
use rivet_text::Component;

fn server_links_codec() -> StreamCodec<FriendlyByteBuf, ClientboundServerLinksPacket> {
    ClientboundServerLinksPacket::stream_codec()
}

fn packet() -> ClientboundServerLinksPacket {
    ClientboundServerLinksPacket::new(vec![
        UntrustedEntry::new(
            Either::left(KnownLinkType::Support),
            "https://support".to_string(),
        ),
        UntrustedEntry::new(
            Either::right(Component::literal("Community")),
            "https://community".to_string(),
        ),
    ])
}

#[test]
fn configuration_clientbound_server_links_is_id_16() {
    // ConfigurationProtocols.CLIENTBOUND_TEMPLATE registers
    // CLIENTBOUND_SERVER_LINKS at id 16; the generated table pins the same
    // addPacket-order fact.
    let template = clientbound_protocol::<ClientboundServerLinksPacket>(
        ConnectionProtocol::Configuration,
        |b| {
            b.add_packet(clientbound_server_links(), server_links_codec());
        },
    );
    assert_eq!(
        template.details().list_packets(),
        &[(clientbound_server_links(), 0)]
    );
    assert_eq!(
        packets::configuration::clientbound::PacketType::ServerLinks.id(),
        16
    );

    let protocol_info = template.bind();
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    protocol_info.codec().encode(&mut out, &packet()).unwrap();
    let mut input = FriendlyByteBuf::new(out.into_inner());
    assert_eq!(protocol_info.codec().decode(&mut input).unwrap(), packet());
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn play_clientbound_server_links_is_id_137() {
    // GameProtocols.CLIENTBOUND_TEMPLATE registers CLIENTBOUND_SERVER_LINKS at
    // id 137.
    let template =
        clientbound_protocol::<ClientboundServerLinksPacket>(ConnectionProtocol::Play, |b| {
            b.add_packet(clientbound_server_links(), server_links_codec());
        });
    assert_eq!(
        template.details().list_packets(),
        &[(clientbound_server_links(), 0)]
    );
    assert_eq!(
        packets::play::clientbound::PacketType::ServerLinks.id(),
        137
    );

    let protocol_info = template.bind();
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    protocol_info.codec().encode(&mut out, &packet()).unwrap();
    let mut input = FriendlyByteBuf::new(out.into_inner());
    assert_eq!(protocol_info.codec().decode(&mut input).unwrap(), packet());
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn packet_type_flow_and_id_are_clientbound_server_links() {
    assert_eq!(clientbound_server_links().flow(), PacketFlow::Clientbound);
    assert_eq!(
        clientbound_server_links().id().to_string(),
        "minecraft:server_links"
    );
}
