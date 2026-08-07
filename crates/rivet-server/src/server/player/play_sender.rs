//! The tick thread's play-state outbound helper (Slice A of #101) — encodes a
//! `(packet_id, body)` pair into the wire form `Connection::queue_raw_frame`
//! passes through opaque, then queues it over the connection's bounded outbound
//! channel keyed by [`ConnectionId`].
//!
//! OWNERSHIP §Network: handshake/status/login encode on the tokio side; play
//! packets cross to the tick thread, so the tick thread must produce the
//! compressed VarInt21 frames itself. The wire form mirrors
//! [`Connection::send_packet`](crate::server::network::connection::Connection::send_packet)
//! exactly:
//!
//! `payload = varint(packet_id) ++ body`
//! → when compression is enabled `wire = varint(declaredLen) ++ payload`
//!   (`CompressionEncoder`; below-threshold stays `varint(0) ++ payload`)
//! → `frame = varint21(len(wire)) ++ wire` (`encode_frame`).
//!
//! The compression encoder is owned here (its threshold is the server-global
//! `ServerConfig.compression_threshold`, so one instance serves every
//! connection on the tick thread). The registry-aware play bodies (login's
//! `DIMENSION_TYPE`, set_time's `WORLD_CLOCK`) resolve holders through the
//! [`RegistryAccess`] the buffer carries; the two single-registry accesses this
//! slice needs are built server-side and handed in at construction (the full
//! composite `RegistryAccess` — `LayeredRegistryAccess.createRegistryAccess` —
//! lands with the server bootstrap; OWNERSHIP.md §Registries).

use bytes::{Bytes, BytesMut};

use rivet_protocol::codec::StreamEncoder;
use rivet_protocol::compression_encoder::CompressionEncoder;
use rivet_protocol::friendly_byte_buf::FriendlyByteBuf;
use rivet_protocol::registry_friendly_byte_buf::RegistryFriendlyByteBuf;
use rivet_protocol::varint21_length_field_prepender::encode_frame;
use rivet_registry::RegistryAccess;

use crate::server::network::connection_id::ConnectionId;
use crate::server::network::packet_listener::panic_message;
use crate::server::tick::channels::OutboundEvent;
use crate::server::tick::registry::{ConnectionRegistry, OutboundError};

/// Why a tick-side play send failed.
#[derive(Debug, thiserror::Error)]
pub enum PlaySendError {
    /// The body could not be encoded (a server-side fault — all play bodies are
    /// server-built; Java closes the connection on the netty `EncoderException`).
    #[error("encoding play packet: {0}")]
    Encode(String),
    /// The connection's outbound channel rejected the frame (gone / overflow;
    /// [`ConnectionRegistry::send`] prunes the connection).
    #[error(transparent)]
    Outbound(#[from] OutboundError),
}

impl From<String> for PlaySendError {
    fn from(message: String) -> Self {
        PlaySendError::Encode(message)
    }
}

/// The tick thread's play packet encoder/sender.
pub struct PlaySender {
    /// The server-global compression encoder (threshold from `ServerConfig`).
    compression: CompressionEncoder,
    /// The `DIMENSION_TYPE` registry access (login's `CommonPlayerSpawnInfo`).
    dimension_type_access: RegistryAccess,
    /// The `WORLD_CLOCK` registry access (set_time's clock-update holders).
    world_clock_access: RegistryAccess,
}

impl PlaySender {
    /// `new PlaySender(compressionThreshold, registryAccess)` — a fresh zlib
    /// compressor at the server's threshold plus the two registry accesses the
    /// registry-aware play bodies resolve.
    pub fn new(
        compression_threshold: i32,
        dimension_type_access: RegistryAccess,
        world_clock_access: RegistryAccess,
    ) -> Self {
        PlaySender {
            compression: CompressionEncoder::new(compression_threshold),
            dimension_type_access,
            world_clock_access,
        }
    }

    /// The `DIMENSION_TYPE` access (the buffer's registry access for login).
    pub fn dimension_type_access(&self) -> &RegistryAccess {
        &self.dimension_type_access
    }

    /// The `WORLD_CLOCK` access (the buffer's registry access for set_time).
    pub fn world_clock_access(&self) -> &RegistryAccess {
        &self.world_clock_access
    }

    /// Encode a packet body with a plain-`FriendlyByteBuf` protocol codec (the
    /// `StreamEncoder` half; the packet id is NOT included — the caller passes
    /// it to [`PlaySender::send_packet`]). Same panic/error containment as
    /// [`encode_body`](crate::server::network::server_login_packet_listener::encode_body).
    pub fn encode_body<T, C>(&self, codec: C, value: &T) -> Result<Vec<u8>, String>
    where
        C: StreamEncoder<FriendlyByteBuf, T>,
    {
        let mut out = FriendlyByteBuf::new(BytesMut::new());
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            codec.encode(&mut out, value)
        }))
        .map_err(|payload| {
            format!(
                "encoding {} panicked: {}",
                std::any::type_name::<T>(),
                panic_message(payload)
            )
        })?
        .map_err(|e| format!("encoding {}: {}", std::any::type_name::<T>(), e.message))?;
        Ok(out.into_inner().to_vec())
    }

    /// Encode a packet body with a registry-aware protocol codec over a
    /// `RegistryFriendlyByteBuf` carrying `access` (login → `DIMENSION_TYPE`,
    /// set_time → `WORLD_CLOCK`).
    pub fn encode_registry_body<T, C>(
        &self,
        codec: C,
        value: &T,
        access: &RegistryAccess,
    ) -> Result<Vec<u8>, String>
    where
        C: StreamEncoder<RegistryFriendlyByteBuf, T>,
    {
        let mut out = RegistryFriendlyByteBuf::new(BytesMut::new(), access.clone());
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            codec.encode(&mut out, value)
        }))
        .map_err(|payload| {
            format!(
                "encoding {} panicked: {}",
                std::any::type_name::<T>(),
                panic_message(payload)
            )
        })?
        .map_err(|e| format!("encoding {}: {}", std::any::type_name::<T>(), e.message))?;
        Ok(out.into_inner().to_vec())
    }

    /// `Connection::send_packet`'s wire form, built tick-side: `varint(packet_id)
    /// ++ body`, compressed when enabled, then VarInt21 framed — the opaque
    /// frame `Connection::queue_raw_frame` appends verbatim.
    pub fn encode_frame(&mut self, packet_id: u32, body: &[u8]) -> Result<Bytes, String> {
        let mut payload = Vec::with_capacity(5 + body.len());
        rivet_protocol::var_int::write(&mut payload, packet_id as i32);
        payload.extend_from_slice(body);
        let wire = self.compression.encode(&payload).map_err(|e| e.message)?;
        let frame = encode_frame(&wire).map_err(|e| e.message)?;
        Ok(Bytes::copy_from_slice(&frame))
    }

    /// Encode + frame + queue one play packet for a connection, in order.
    pub fn send_packet(
        &mut self,
        connections: &mut ConnectionRegistry,
        id: ConnectionId,
        packet_id: u32,
        body: &[u8],
    ) -> Result<(), PlaySendError> {
        let frame = self.encode_frame(packet_id, body)?;
        connections
            .send(id, OutboundEvent::Packet { frame })
            .map_err(PlaySendError::Outbound)
    }
}
