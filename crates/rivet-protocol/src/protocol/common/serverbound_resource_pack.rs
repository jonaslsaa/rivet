//! Port of `net.minecraft.network.protocol.common.ServerboundResourcePackPacket`
//! (issue #86).
//!
//! Java: `ServerboundResourcePackPacket.java` in `working/Paper`. A UUID and an
//! [`Action`] enum by ordinal (`readEnum`/`writeEnum`). `Action::is_terminal` is
//! a per-action method (not a `Packet` flag): `ACCEPTED` and `DOWNLOADED` are
//! non-terminal, every other action is terminal. Registered in play and
//! configuration serverbound.

use crate::codec::{CodecError, StreamCodec, of};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::serverbound_resource_pack;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use rivet_util::mth::Uuid;

/// `ServerboundResourcePackPacket.Action` — the client's resource-pack response.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// `SUCCESSFULLY_LOADED` — terminal.
    SuccessfullyLoaded,
    /// `DECLINED` — terminal.
    Declined,
    /// `FAILED_DOWNLOAD` — terminal.
    FailedDownload,
    /// `ACCEPTED` — non-terminal.
    Accepted,
    /// `DOWNLOADED` — non-terminal.
    Downloaded,
    /// `INVALID_URL` — terminal.
    InvalidUrl,
    /// `FAILED_RELOAD` — terminal.
    FailedReload,
    /// `DISCARDED` — terminal.
    Discarded,
}

impl Action {
    /// The number of constants — the length used by Java's
    /// `values().length` in the out-of-range message.
    const COUNT: i32 = 8;

    /// `Action.isTerminal()` — `this != ACCEPTED && this != DOWNLOADED`.
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Action::Accepted | Action::Downloaded)
    }

    /// `Action.values()[ordinal]` — declaration order is the wire ordinal. An
    /// id outside the 8 constants is `None` — Java's
    /// `ArrayIndexOutOfBoundsException` — and the codec surfaces it as `Err`.
    pub fn from_id(id: i32) -> Option<Action> {
        match id {
            0 => Some(Action::SuccessfullyLoaded),
            1 => Some(Action::Declined),
            2 => Some(Action::FailedDownload),
            3 => Some(Action::Accepted),
            4 => Some(Action::Downloaded),
            5 => Some(Action::InvalidUrl),
            6 => Some(Action::FailedReload),
            7 => Some(Action::Discarded),
            _ => None,
        }
    }

    /// `Action.ordinal()` — the wire ordinal.
    pub fn id(&self) -> i32 {
        *self as i32
    }
}

/// `net.minecraft.network.protocol.common.ServerboundResourcePackPacket`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerboundResourcePackPacket {
    id: Uuid,
    action: Action,
}

impl ServerboundResourcePackPacket {
    /// `new ServerboundResourcePackPacket(UUID id, Action action)`.
    pub fn new(id: Uuid, action: Action) -> Self {
        ServerboundResourcePackPacket { id, action }
    }

    /// `ServerboundResourcePackPacket.id()`.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// `ServerboundResourcePackPacket.action()`.
    pub fn action(&self) -> Action {
        self.action
    }

    /// `ServerboundResourcePackPacket.STREAM_CODEC`.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ServerboundResourcePackPacket> {
        of(
            |output: &mut FriendlyByteBuf, value: &ServerboundResourcePackPacket| {
                output.write_uuid(value.id);
                output.write_enum(Action::id, &value.action);
                Ok(())
            },
            |input: &mut FriendlyByteBuf| {
                let id = input.read_uuid();
                let action_id = input.read_var_int();
                let action = Action::from_id(action_id).ok_or_else(|| {
                    CodecError::new(format!(
                        "Index {action_id} out of bounds for length {}",
                        Action::COUNT
                    ))
                })?;
                Ok(ServerboundResourcePackPacket::new(id, action))
            },
        )
    }
}

impl Packet for ServerboundResourcePackPacket {
    fn packet_type(&self) -> PacketType {
        serverbound_resource_pack()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;

    fn uuid() -> Uuid {
        Uuid { most: 1, least: 2 }
    }

    #[test]
    fn golden_wire_bytes() {
        // UUID, then `Action.DECLINED` ordinal 1 as a varint.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ServerboundResourcePackPacket::stream_codec()
            .encode(
                &mut out,
                &ServerboundResourcePackPacket::new(uuid(), Action::Declined),
            )
            .unwrap();
        let mut expected = vec![];
        expected.extend_from_slice(&1i64.to_be_bytes());
        expected.extend_from_slice(&2i64.to_be_bytes());
        expected.push(1);
        assert_eq!(out.into_inner().to_vec(), expected);
    }

    #[test]
    fn round_trips_every_action() {
        for action in [
            Action::SuccessfullyLoaded,
            Action::Declined,
            Action::FailedDownload,
            Action::Accepted,
            Action::Downloaded,
            Action::InvalidUrl,
            Action::FailedReload,
            Action::Discarded,
        ] {
            let packet = ServerboundResourcePackPacket::new(uuid(), action);
            let mut out = FriendlyByteBuf::new(BytesMut::new());
            ServerboundResourcePackPacket::stream_codec()
                .encode(&mut out, &packet)
                .unwrap();
            let mut input = FriendlyByteBuf::new(out.into_inner());
            assert_eq!(
                ServerboundResourcePackPacket::stream_codec()
                    .decode(&mut input)
                    .unwrap(),
                packet
            );
        }
    }

    #[test]
    fn out_of_range_ordinal_errors() {
        // A hostile wire value: ordinal 8 is beyond `values().length` (8), so
        // Java throws `ArrayIndexOutOfBoundsException`; the codec surfaces it as
        // `Err` instead of coercing to a valid action.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_uuid(uuid());
        out.write_var_int(8);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let err = ServerboundResourcePackPacket::stream_codec()
            .decode(&mut input)
            .unwrap_err();
        assert_eq!(err.message, "Index 8 out of bounds for length 8");
    }

    #[test]
    fn terminal_only_for_accepted_and_downloaded() {
        assert!(Action::SuccessfullyLoaded.is_terminal());
        assert!(Action::Declined.is_terminal());
        assert!(Action::FailedDownload.is_terminal());
        assert!(!Action::Accepted.is_terminal());
        assert!(!Action::Downloaded.is_terminal());
        assert!(Action::InvalidUrl.is_terminal());
        assert!(Action::FailedReload.is_terminal());
        assert!(Action::Discarded.is_terminal());
    }
}
