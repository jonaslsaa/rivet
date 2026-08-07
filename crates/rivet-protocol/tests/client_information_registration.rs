//! Java-grounded registration tests for `ServerboundClientInformationPacket`
//! (issue #197): the packet is registered at configuration serverbound id 0 and
//! play serverbound id 14 (Paper's `ConfigurationProtocols.SERVERBOUND_TEMPLATE`
//! and `GameProtocols.SERVERBOUND_TEMPLATE` `addPacket` order), and the real
//! body codec round-trips through the id-dispatch registration machinery.

use bytes::BytesMut;
use rivet_protocol::codec::{StreamCodec, StreamDecoder, StreamEncoder};
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::generated::packets;
use rivet_protocol::generated::protocol::ConnectionProtocol;
use rivet_protocol::protocol::Packet;
use rivet_protocol::protocol::common::chat_visiblity::ChatVisiblity;
use rivet_protocol::protocol::common::client_information::ClientInformation;
use rivet_protocol::protocol::common::humanoid_arm::HumanoidArm;
use rivet_protocol::protocol::common::particle_status::ParticleStatus;
use rivet_protocol::protocol::common::serverbound_client_information::ServerboundClientInformationPacket;
use rivet_protocol::protocol::serverbound_protocol;

fn client_information_codec() -> StreamCodec<FriendlyByteBuf, ServerboundClientInformationPacket> {
    ServerboundClientInformationPacket::stream_codec()
}

fn info() -> ClientInformation {
    ClientInformation::new(
        "en_us".to_string(),
        2,
        ChatVisiblity::Full,
        true,
        0,
        HumanoidArm::Right,
        false,
        false,
        ParticleStatus::All,
    )
}

#[test]
fn configuration_serverbound_client_information_is_id_0() {
    // ConfigurationProtocols.SERVERBOUND_TEMPLATE registers
    // SERVERBOUND_CLIENT_INFORMATION first (id 0), and the generated table pins
    // the same addPacket-order fact.
    let template = serverbound_protocol::<ServerboundClientInformationPacket>(
        ConnectionProtocol::Configuration,
        |b| {
            b.add_packet(
                rivet_protocol::protocol::common::packet_types::serverbound_client_information(),
                client_information_codec(),
            );
        },
    );
    assert_eq!(
        template.details().list_packets(),
        &[(
            rivet_protocol::protocol::common::packet_types::serverbound_client_information(),
            0
        )]
    );
    assert_eq!(
        packets::configuration::serverbound::PacketType::ClientInformation.id(),
        0
    );

    let protocol_info = template.bind();
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    protocol_info
        .codec()
        .encode(&mut out, &ServerboundClientInformationPacket::new(info()))
        .unwrap();
    let mut input = FriendlyByteBuf::new(out.into_inner());
    assert_eq!(
        protocol_info.codec().decode(&mut input).unwrap(),
        ServerboundClientInformationPacket::new(info())
    );
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn play_serverbound_client_information_is_id_14() {
    // GameProtocols.SERVERBOUND_TEMPLATE registers SERVERBOUND_CLIENT_INFORMATION
    // at addPacket index 14 (after ACCEPT_TELEPORTATION..CLIENT_TICK_END), pinned
    // by the generated table. The play registration then round-trips the real
    // body codec through the id-dispatch machinery.
    assert_eq!(
        packets::play::serverbound::PacketType::ClientInformation.id(),
        14
    );
    let template =
        serverbound_protocol::<ServerboundClientInformationPacket>(ConnectionProtocol::Play, |b| {
            b.add_packet(
                rivet_protocol::protocol::common::packet_types::serverbound_client_information(),
                client_information_codec(),
            );
        });
    let protocol_info = template.bind();
    let mut out = FriendlyByteBuf::new(BytesMut::new());
    protocol_info
        .codec()
        .encode(&mut out, &ServerboundClientInformationPacket::new(info()))
        .unwrap();
    let mut input = FriendlyByteBuf::new(out.into_inner());
    assert_eq!(
        protocol_info.codec().decode(&mut input).unwrap(),
        ServerboundClientInformationPacket::new(info())
    );
    assert_eq!(input.readable_bytes(), 0);
}

#[test]
fn packet_type_is_serverbound_client_information() {
    let packet = ServerboundClientInformationPacket::new(info());
    assert_eq!(
        packet.packet_type(),
        rivet_protocol::protocol::common::packet_types::serverbound_client_information()
    );
    assert!(!packet.is_terminal());
    assert!(!packet.is_skippable());
}
