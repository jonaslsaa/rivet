//! Port of `net.minecraft.network.protocol.common.ServerboundClientInformationPacket`
//! (issues #86/#197).
//!
//! Java: `ServerboundClientInformationPacket.java` in `working/Paper`. Wraps the
//! client's [`ClientInformation`] value. `Packet.codec(write, new)` is a
//! passthrough to `ClientInformation`'s own write/read. Registered in
//! configuration serverbound (id 0) and play serverbound (id 14); the listener
//! behavior (`handleClientInformation`) stays deferred with the listener
//! hierarchy (M1.1/#148).

use crate::codec::{StreamCodec, map};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::client_information::ClientInformation;
use crate::protocol::common::packet_types::serverbound_client_information;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;

/// `net.minecraft.network.protocol.common.ServerboundClientInformationPacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerboundClientInformationPacket {
    information: ClientInformation,
}

impl ServerboundClientInformationPacket {
    /// `new ServerboundClientInformationPacket(ClientInformation information)`.
    pub fn new(information: ClientInformation) -> Self {
        ServerboundClientInformationPacket { information }
    }

    /// `ServerboundClientInformationPacket.information()`.
    pub fn information(&self) -> &ClientInformation {
        &self.information
    }

    /// `ServerboundClientInformationPacket.STREAM_CODEC` — `Packet.codec(write,
    /// new)`, i.e. the `ClientInformation` codec both ways.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundClientInformationPacket> {
        map(
            ClientInformation::stream_codec(),
            |c: &ClientInformation| ServerboundClientInformationPacket::new(c.clone()),
            |p: &ServerboundClientInformationPacket| p.information.clone(),
        )
    }
}

impl Packet for ServerboundClientInformationPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_client_information()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    #[test]
    fn round_trips_default_information() {
        let packet = ServerboundClientInformationPacket::new(ClientInformation::create_default());
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundClientInformationPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        assert_eq!(
            ServerboundClientInformationPacket::stream_codec()
                .decode(&mut input)
                .unwrap(),
            packet
        );
        assert_eq!(input.readable_bytes(), 0);
    }

    #[test]
    fn malformed_enum_ordinal_errors_not_panics() {
        // A hostile wire value flows through the packet codec as `Err`, not a
        // panic (Java `ArrayIndexOutOfBoundsException`).
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_var_int(5);
        out.write_bytes(b"en_us");
        out.write_byte(2);
        out.write_var_int(9); // ChatVisiblity ordinal 9
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = ServerboundClientInformationPacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(err.message, "Index 9 out of bounds for length 3");
    }
}
