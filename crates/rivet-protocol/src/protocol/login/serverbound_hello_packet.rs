//! Port of `net.minecraft.network.protocol.login.ServerboundHelloPacket`
//! (issue #99).
//!
//! Java: `ServerboundHelloPacket.java` in `working/Paper`. The login opener:
//! the player's requested name (max 16 UTF-16 code units) and the profile UUID
//! the client believes it is (offline mode: `UUIDUtil.createOfflinePlayerUUID`
//! of the name). Registered at login serverbound id 0 (`LoginProtocols` /
//! generated table).
//!
//! The listener behavior (`handleHello` — the `HELLO → VERIFYING` offline
//! state-machine entry) is deferred with the login state machine (#96); the RSA
//! challenge this packet *can* trigger (`ClientboundHello`/`ServerboundKey`) is
//! the online-auth path (#88), never exercised in M1 offline mode.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, StreamDecoder, StreamEncoder, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::login::packet_types::serverbound_hello;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_util::mth::Uuid;

/// `net.minecraft.network.protocol.login.ServerboundHelloPacket` — the record
/// `(String name, UUID profileId)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerboundHelloPacket {
    name: String,
    profile_id: Uuid,
}

impl ServerboundHelloPacket {
    /// `new ServerboundHelloPacket(String name, UUID profileId)`.
    pub fn new(name: String, profile_id: Uuid) -> Self {
        ServerboundHelloPacket { name, profile_id }
    }

    /// `ServerboundHelloPacket.name()`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// `ServerboundHelloPacket.profileId()`.
    pub fn profile_id(&self) -> Uuid {
        self.profile_id
    }

    /// `ServerboundHelloPacket.STREAM_CODEC` — `Packet.codec(write, new)`:
    /// `writeUtf(name, 16)` then `writeUUID(profileId)`.
    ///
    /// The name goes through `ByteBufCodecs.PLAYER_NAME` (`stringUtf8(16)`):
    /// Java's `readUtf(16)`/`writeUtf(16)` are `Utf8String.read(16)`/`write(16)`
    /// under the hood, and the codec boundary surfaces the over-limit/truncated
    /// cases as `Err` (a hostile wire value closes the connection) instead of
    /// panicking through the raw helper. The 16-unit bound is UTF-16 code units,
    /// not bytes (PORTING.md).
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundHelloPacket> {
        let name_codec = byte_buf_codecs::player_name();
        let name_codec_decode = name_codec.clone();
        of(
            move |output: &mut FriendlyByteBuf, value: &ServerboundHelloPacket| {
                name_codec.encode(output, &value.name)?;
                output.write_uuid(value.profile_id);
                Ok(())
            },
            move |input: &mut FriendlyByteBuf| {
                let name = name_codec_decode.decode(input)?;
                let profile_id = input.read_uuid();
                Ok(ServerboundHelloPacket::new(name, profile_id))
            },
        )
    }
}

impl Packet for ServerboundHelloPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_hello()
    }
}
