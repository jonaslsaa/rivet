//! STUB — `net.minecraft.network.protocol.common.ServerboundClientInformationPacket`.
//!
//! Java: `ServerboundClientInformationPacket.java` in `working/Paper`. The
//! client's `ClientInformation` value (`net.minecraft.server.level.ClientInformation`):
//! language (`stringUtf8(16)`), viewDistance byte, modelCustomisation unsigned
//! byte, chat visibility/arm/particle enums by ordinal, plus the Paper
//! width/height/text-filtering fields.
//!
//! BLOCKED: `ClientInformation` is a `server.level` value type not yet ported
//! (a real client sends this during configuration, so it is an M1 decode
//! requirement — but the body needs the value type first). Discriminator:
//! `packet_types::serverbound_client_information`.
