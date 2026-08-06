//! Port of `net.minecraft.network.protocol.common.ClientboundCustomReportDetailsPacket`
//! (issue #86).
//!
//! Java: `ClientboundCustomReportDetailsPacket.java` in `working/Paper`. A
//! `HashMap<String, String>` with keys `stringUtf8(128)`, values
//! `stringUtf8(4096)`, max 32 entries. Registered in play and configuration
//! clientbound.

use crate::codec::byte_buf_codecs;
use crate::codec::{StreamCodec, composite_1};
use crate::friendly_byte_buf::FriendlyByteBuf;
use crate::protocol::common::packet_types::clientbound_custom_report_details;
use crate::protocol::packet::Packet;
use crate::protocol::packet_type::PacketType;
use std::collections::HashMap;

/// `ClientboundCustomReportDetailsPacket.MAX_DETAIL_KEY_LENGTH`.
const MAX_DETAIL_KEY_LENGTH: i32 = 128;
/// `ClientboundCustomReportDetailsPacket.MAX_DETAIL_VALUE_LENGTH`.
const MAX_DETAIL_VALUE_LENGTH: i32 = 4096;
/// `ClientboundCustomReportDetailsPacket.MAX_DETAIL_COUNT`.
const MAX_DETAIL_COUNT: i32 = 32;

/// `net.minecraft.network.protocol.common.ClientboundCustomReportDetailsPacket`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientboundCustomReportDetailsPacket {
    details: HashMap<String, String>,
}

impl ClientboundCustomReportDetailsPacket {
    /// `new ClientboundCustomReportDetailsPacket(Map<String, String> details)`.
    pub fn new(details: HashMap<String, String>) -> Self {
        ClientboundCustomReportDetailsPacket { details }
    }

    /// `ClientboundCustomReportDetailsPacket.details()`.
    pub fn details(&self) -> &HashMap<String, String> {
        &self.details
    }

    /// `ClientboundCustomReportDetailsPacket.DETAILS_STREAM_CODEC`.
    pub fn details_stream_codec() -> StreamCodec<FriendlyByteBuf, HashMap<String, String>> {
        byte_buf_codecs::map(
            |capacity: i32| {
                if capacity < 0 {
                    // Java: `new HashMap<>(-1)` -> `IllegalArgumentException:
                    // Illegal initial capacity: -1` (a hostile count that passes
                    // the max check surfaces here, at the ctor).
                    panic!("Illegal initial capacity: {capacity}");
                }
                HashMap::new()
            },
            byte_buf_codecs::string_utf8(MAX_DETAIL_KEY_LENGTH),
            byte_buf_codecs::string_utf8(MAX_DETAIL_VALUE_LENGTH),
            MAX_DETAIL_COUNT,
        )
    }

    /// `ClientboundCustomReportDetailsPacket.STREAM_CODEC` — a single-field
    /// composite over the details map.
    pub fn stream_codec() -> StreamCodec<FriendlyByteBuf, ClientboundCustomReportDetailsPacket> {
        composite_1(
            Self::details_stream_codec(),
            |p: &ClientboundCustomReportDetailsPacket| p.details.clone(),
            ClientboundCustomReportDetailsPacket::new,
        )
    }
}

impl Packet for ClientboundCustomReportDetailsPacket {
    fn packet_type(&self) -> PacketType {
        clientbound_custom_report_details()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{StreamDecoder, StreamEncoder};
    use bytes::BytesMut;
    use std::panic::catch_unwind;

    fn panic_message<F: FnOnce() -> R, R: std::fmt::Debug>(f: F) -> String {
        let err = catch_unwind(std::panic::AssertUnwindSafe(f))
            .expect_err("expected the closure to panic");
        err.downcast_ref::<String>()
            .cloned()
            .or_else(|| err.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "non-string panic payload".to_string())
    }

    #[test]
    fn round_trips_map() {
        let mut details = HashMap::new();
        details.insert("a".to_string(), "b".to_string());
        details.insert("c".to_string(), "d".to_string());
        let packet = ClientboundCustomReportDetailsPacket::new(details.clone());
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        ClientboundCustomReportDetailsPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap();
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let decoded = ClientboundCustomReportDetailsPacket::stream_codec()
            .decode(&mut input)
            .unwrap();
        assert_eq!(decoded.details(), &details);
    }

    #[test]
    fn oversize_map_errors() {
        // The 32-entry cap errors with Java's `ByteBufCodecs.map` message.
        let mut details = HashMap::new();
        for i in 0..33 {
            details.insert(format!("k{i}"), "v".to_string());
        }
        let packet = ClientboundCustomReportDetailsPacket::new(details);
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        let err = ClientboundCustomReportDetailsPacket::stream_codec()
            .encode(&mut out, &packet)
            .unwrap_err();
        assert_eq!(err.message, "33 elements exceeded max size of: 32");
    }

    #[test]
    fn negative_count_panics_like_java_map_ctor() {
        // A hostile count of -1 passes `readCount` (only `> maxSize` is
        // checked) and reaches the ctor `new HashMap<>(-1)` ->
        // `IllegalArgumentException("Illegal initial capacity: -1")`.
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        out.write_var_int(-1);
        let mut input = FriendlyByteBuf::new(out.into_inner());
        let msg = panic_message(|| {
            let _ = ClientboundCustomReportDetailsPacket::stream_codec().decode(&mut input);
        });
        assert_eq!(msg, "Illegal initial capacity: -1");
    }
}
